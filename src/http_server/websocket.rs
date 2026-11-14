/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 共通/wsとページ通知の接続・認証・終了境界
//!

use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use actix_ws::{AggregatedMessage, Session};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use super::app_state::{AppState, PageStateLease};
use crate::audit::{
    AuditSink,
    model::{AuditOperation, AuditRecord, AuditResult},
};
use crate::auth::{AuthContext, BasicCredential};

///
/// 共通の2項目だけを受け付けるpacket
///
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    /// 機能・操作名
    r#type: String,
    /// 機能が検証するobject
    payload: Value,
}

///
/// 終了時に一度だけ記録し、その後leaseを解放する
///
pub(super) struct RequestAudit {
    /// 終了監査後まで保持するlease
    _lease: Option<PageStateLease>,
    /// 終了監査とlease解放より後まで保持する接続枠
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
    /// 共通sink
    sink: Option<Arc<RwLock<AuditSink>>>,
    /// 非機密な要求の記録
    record: Option<AuditRecord>,
    /// 経過時間の起点
    started: Instant,
    /// 固定終了理由
    reason: &'static str,
}

impl RequestAudit {
    ///
    /// 入口の主体で監査を開始する
    ///
    /// # 引数
    /// * `state` - 共有状態
    /// * `auth` - 入口の主体
    /// * `operation` - 固定操作
    ///
    /// # 戻り値
    /// drop時に一度だけ記録するguard。
    ///
    pub(super) fn new(
        state: &AppState,
        auth: &AuthContext,
        operation: AuditOperation,
    ) -> Self {
        Self {
            _lease: None,
            _permit: None,
            sink: state.audit_sink(),
            record: auth.actor_id().map(|actor| {
                AuditRecord::new(
                    operation,
                    actor.clone(),
                    auth.token_id().cloned(),
                    None,
                    None,
                    AuditResult::OperationDenied,
                    Utc::now(),
                    None,
                    None,
                )
            }),
            started: Instant::now(),
            reason: "cancelled",
        }
    }

    ///
    /// 検証後の対象pathだけを記録する
    ///
    /// # 引数
    /// * `path` - 正規化・検証済みの対象
    ///
    /// # 戻り値
    /// なし。
    ///
    pub(super) fn target(&mut self, path: String) {
        if let Some(record) = &mut self.record {
            record.target_path = Some(path);
        }
    }

    ///
    /// 接続元を記録へ引き継ぐ
    ///
    /// # 引数
    /// * `address` - HTTPで得た接続元
    ///
    /// # 戻り値
    /// なし。
    ///
    pub(super) fn address(&mut self, address: Option<IpAddr>) {
        if let Some(record) = &mut self.record {
            record.address = address;
        }
    }

    ///
    /// 同じ接続の確認要求へ接続元を引き継ぐ
    ///
    /// # 引数
    /// * `parent` - 接続監査
    ///
    /// # 戻り値
    /// なし。
    ///
    pub(super) fn inherit_address(&mut self, parent: &Self) {
        self.address(parent.record.as_ref().and_then(|record| record.address));
    }

    ///
    /// 公開操作の結果分類を保持する
    ///
    /// # 引数
    /// * `result` - 固定分類
    /// * `reason` - 非機密な固定理由
    ///
    /// # 戻り値
    /// なし。
    ///
    pub(super) fn finish_result(
        &mut self,
        result: AuditResult,
        reason: &'static str,
    ) {
        self.reason = reason;
        if let Some(record) = &mut self.record {
            record.result = result;
        }
    }

    ///
    /// 非機密な結果を確定する
    ///
    /// # 引数
    /// * `success` - 成功したか
    /// * `reason` - 固定理由
    ///
    /// # 戻り値
    /// なし。
    ///
    pub(super) fn finish(&mut self, success: bool, reason: &'static str) {
        self.reason = reason;
        if let Some(record) = &mut self.record {
            record.result = if success {
                AuditResult::Success
            } else {
                AuditResult::OperationDenied
            };
        }
    }
}

impl Drop for RequestAudit {
    ///
    /// 記録の所有権を取り出し一度だけ投入する
    ///
    /// # 戻り値
    /// なし。sink失敗を応答へ逆流させない。
    ///
    fn drop(&mut self) {
        let Some(mut record) = self.record.take() else {
            return;
        };
        record.summary = Some(
            json!({
                "reason": self.reason,
                "elapsed_ms": self.started.elapsed().as_millis() as u64,
            })
            .to_string(),
        );
        if let Some(sink) = &self.sink {
            match sink.write() {
                Ok(mut sink) => {
                    if sink.record(record).is_err() {
                        tracing::warn!("websocket audit recording failed");
                    }
                }
                Err(_) => tracing::warn!("websocket audit lock failed"),
            }
        }
    }
}

///
/// RESTのscopeとは独立した認証済み共通入口
///
/// # 戻り値
/// /wsのresource。
///
pub(super) fn endpoint() -> impl actix_web::dev::HttpServiceFactory {
    web::resource("/ws")
        .wrap(actix_web::middleware::from_fn(
            crate::rest_api::append_bearer_expire_header,
        ))
        .wrap(actix_web_httpauth::middleware::HttpAuthentication::with_fn(
            crate::rest_api::validate_authorization,
        ))
        .route(web::get().to(upgrade))
}

///
/// Originが同一originを表しているか確認する
///
/// # 引数
/// * `req` - 認証済みHTTP要求
///
/// # 戻り値
/// scheme・authorityが一致すればtrue。
///
fn same_origin(req: &HttpRequest) -> bool {
    let Some(origin) = req.headers().get("origin") else {
        return false;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let connection = req.connection_info();
    origin == format!("{}://{}", connection.scheme(), connection.host())
}

///
/// 認証・Origin・上限・停止gateを通してからUpgradeする
///
/// # 引数
/// * `req` - HTTP要求
/// * `body` - frame stream
/// * `state` - 共有状態
///
/// # 戻り値
/// Upgrade又は拒否。
///
/// # 概要
/// 接続の寿命を監査guardとpermitで保持する。
///
async fn upgrade(
    req: HttpRequest,
    body: web::Payload,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> actix_web::Result<HttpResponse> {
    let auth = match crate::rest_api::require_request_scope(
        &req,
        crate::database::types::BearerScope::Read,
    ) {
        Ok(auth) => auth,
        Err(response) => {
            if let Some(auth) = req.extensions().get::<AuthContext>() {
                if let Ok(shared) = state.read() {
                    let mut audit = RequestAudit::new(
                        &shared,
                        auth,
                        AuditOperation::WebSocket,
                    );
                    audit.address(req.peer_addr().map(|address| address.ip()));
                    audit.finish_result(AuditResult::ScopeDenied, "scope");
                }
            }
            return Ok(response);
        }
    };
    let credential = req
        .extensions()
        .get::<BasicCredential>()
        .map(|value| value.0.clone());

    /* gateと同じguardで接続を登録する。 */
    let (mut audit, changes, stop, limit) = {
        let shared = state.read().map_err(|_| {
            actix_web::error::ErrorInternalServerError("state unavailable")
        })?;
        let mut audit =
            RequestAudit::new(&shared, &auth, AuditOperation::WebSocket);
        audit.address(req.peer_addr().map(|address| address.ip()));
        if !same_origin(&req) {
            audit.finish(false, "origin");
            return Ok(HttpResponse::Forbidden().finish());
        }
        let Some(lease) = shared.begin_page_state_request() else {
            audit.finish(false, "shutdown");
            return Ok(HttpResponse::ServiceUnavailable().finish());
        };
        audit._lease = Some(lease);
        let permit = match shared.page_events_slots.clone().try_acquire_owned()
        {
            Ok(permit) => permit,
            Err(_) => {
                audit.finish(false, "limit");
                return Ok(HttpResponse::ServiceUnavailable().finish());
            }
        };
        audit._permit = Some(permit);
        (
            audit,
            shared.db().subscribe_page_changes(),
            shared.page_events_shutdown(),
            shared.asset_limit_size() as usize,
        )
    };
    let (response, session, stream) = match actix_ws::handle(&req, body) {
        Ok(value) => value,
        Err(error) => {
            audit.finish_result(AuditResult::InvalidInput, "handshake");
            return Err(error);
        }
    };
    let shared = state.get_ref().clone();
    actix_web::rt::spawn(async move {
        let result = serve(
            shared, auth, credential, session, stream, changes, stop,
            &mut audit, limit,
        )
        .await;
        if result && audit.reason == "cancelled" {
            audit.finish(true, "closed");
        }
    });
    Ok(response)
}

///
/// 応答を上限付きで送信する
///
/// # 引数
/// * `session` - 接続
/// * `kind` - type
/// * `payload` - 検証済みの公開値
///
/// # 戻り値
/// 送信成功ならtrue。
///
async fn send(session: &mut Session, kind: &str, payload: Value) -> bool {
    tokio::time::timeout(
        Duration::from_secs(5),
        session.text(json!({"type":kind,"payload":payload}).to_string()),
    )
    .await
    .is_ok_and(|value| value.is_ok())
}

///
/// 入力packetを解析し、2項目のobjectを検証する
///
/// # 引数
/// * `text` - 一つのJSON message
///
/// # 戻り値
/// 検証済み共通packet。
///
fn packet(text: &str) -> Result<Packet, &'static str> {
    let value: Packet =
        serde_json::from_str(text).map_err(|_| "invalid_input")?;
    if value.r#type.is_empty() || !value.payload.is_object() {
        return Err("invalid_input");
    }
    Ok(value)
}

///
/// 直列に一接続の入力・commit起床・終了を処理する
///
/// # 引数
/// * `state` - 共有状態
/// * `auth` - 不変主体
/// * `credential` - Basic比較値
/// * `session` - 接続
/// * `stream` - 入力
/// * `changes` - 確定後の印
/// * `stop` - 停止signal
/// * `audit` - 接続監査
/// * `limit` - 既存payload上限
///
/// # 戻り値
/// 通常切断ならtrue。
///
/// # 概要
/// read snapshotをawait中や入力待ちへ持ち越さない。
///
async fn serve(
    state: Arc<RwLock<AppState>>,
    auth: AuthContext,
    credential: Option<String>,
    mut session: Session,
    stream: actix_ws::MessageStream,
    mut changes: tokio::sync::watch::Receiver<u64>,
    mut stop: tokio::sync::watch::Receiver<bool>,
    audit: &mut RequestAudit,
    limit: usize,
) -> bool {
    let mut stream = stream
        .max_frame_size(limit)
        .aggregate_continuations()
        .max_continuation_size(limit);
    let mut page = super::page_events::PageEvents::new(
        state.clone(),
        auth.clone(),
        credential.clone(),
    );
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    let mut ping_at = Instant::now();
    let mut pong_pending: Option<Instant> = None;
    let mut normal = false;
    loop {
        /* 全入力と状態取得を一つのloopで順序付ける。 */
        let mut response = None;
        tokio::select! {
            _ = stop.changed() => { audit.finish(false, "shutdown"); break; }
            input = stream.recv() => {
                match input {
                    None | Some(Ok(AggregatedMessage::Close(_))) => {
                        normal = true; break;
                    }
                    Some(Ok(AggregatedMessage::Ping(bytes))) => {
                        if !tokio::time::timeout(Duration::from_secs(5), session.pong(&bytes))
                            .await.is_ok_and(|value| value.is_ok()) { break; }
                    }
                    Some(Ok(AggregatedMessage::Pong(_))) => { pong_pending = None; }
                    Some(Ok(AggregatedMessage::Text(text))) => {
                        response = Some(match packet(&text) {
                            Ok(value) if value.r#type.starts_with("page.") => {
                                page.handle(&value.r#type, value.payload,
                                    &mut changes, audit).await
                            }
                            Ok(_) => super::page_events::Response::error(
                                "error", "unsupported_type", false,
                            ),
                            Err(code) => super::page_events::Response::error(
                                "error", code, false,
                            ),
                        });
                    }
                    Some(Err(_)) | Some(Ok(AggregatedMessage::Binary(_))) => break,
                }
            }
            changed = changes.changed() => {
                if changed.is_err() { break; }
                drop(changes.borrow_and_update());
                response = page.changed().await;
            }
            _ = heartbeat.tick() => {
                if pong_pending.is_some_and(|at| at.elapsed() >= Duration::from_secs(10)) { break; }
                if ping_at.elapsed() >= Duration::from_secs(15) {
                    let shared = state.clone(); let auth = auth.clone();
                    let credential = credential.clone();
                    let valid = tokio::task::spawn_blocking(move || {
                        shared.read().ok().is_some_and(|state| {
                            state.db().check_page_event_identity(&auth, credential.as_deref()).is_ok()
                        })
                    }).await.unwrap_or(false);
                    if !valid {
                        audit.finish(false, "authentication");
                        if !send(&mut session,"page.error",json!({"code":"forbidden","message":"authentication no longer valid"})).await { break; }
                        break;
                    }
                    if !tokio::time::timeout(Duration::from_secs(5), session.ping(b""))
                        .await.is_ok_and(|value| value.is_ok()) { break; }
                    ping_at = Instant::now(); pong_pending = Some(Instant::now());
                }
            }
        }
        if let Some(response) = response {
            if let Some(result) = response.audit_result {
                audit.finish_result(result, "message_error");
            }
            if !send(&mut session, response.kind, response.payload).await {
                break;
            }
            if response.fatal {
                break;
            }
        }
    }

    let _ =
        tokio::time::timeout(Duration::from_secs(1), session.close(None)).await;
    normal
}

#[cfg(test)]
mod tests {
    use super::*;

    ///
    /// 必須2項目とobject payload以外を拒否する
    ///
    /// # 注記
    /// 型不正・旧channel/version・不足項目を同じparserへ渡す。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[test]
    fn common_packet_has_only_type_and_payload() {
        assert!(packet(r#"{"type":"page.check","payload":{}}"#).is_ok());
        for value in [
            r#"{"type":"page.check","payload":{},"channel":"page_changes"}"#,
            r#"{"type":"page.check","payload":{},"version":1}"#,
            r#"{"type":"page.check","payload":null}"#,
            r#"{"type":"page.check"}"#,
            r#"{"type":"","payload":{}}"#,
        ] {
            assert!(packet(value).is_err());
        }
    }

    ///
    /// same-originだけをUpgrade候補にする
    ///
    /// # 注記
    /// 欠落・null・別originと正常originを比較する。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[test]
    fn websocket_origin_is_required() {
        for origin in [None, Some("null"), Some("http://other.test")] {
            let mut request =
                actix_web::test::TestRequest::get().uri("http://localhost/ws");
            if let Some(origin) = origin {
                request = request.insert_header(("origin", origin));
            }
            assert!(!same_origin(&request.to_http_request()));
        }
        let request = actix_web::test::TestRequest::get()
            .uri("http://localhost/ws")
            .insert_header(("origin", "http://localhost"));
        assert!(same_origin(&request.to_http_request()));
    }

    ///
    /// 応答bodyが失われた送信を成功扱いせず終了へ返す
    ///
    /// # 注記
    /// Upgrade応答の受信側を破棄し、実sessionの送信失敗を確認する。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[actix_web::test]
    async fn websocket_send_failure_returns_false() {
        let request = actix_web::test::TestRequest::get()
            .insert_header(("upgrade", "websocket"))
            .insert_header(("connection", "upgrade"))
            .insert_header(("sec-websocket-version", "13"))
            .insert_header((
                "sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==",
            ))
            .to_http_request();
        let mut payload = actix_web::dev::Payload::None;
        let payload = <web::Payload as actix_web::FromRequest>::from_request(
            &request, &mut payload,
        ).await.unwrap();
        let (response, mut session, _stream) = actix_ws::handle(
            &request, payload,
        ).unwrap();
        drop(response);
        assert!(!send(&mut session, "error", json!({
            "code":"internal_error", "message":"internal_error",
        })).await);
    }
}
