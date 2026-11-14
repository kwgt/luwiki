/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! page.*固有のpayload・現在認可・状態照合を扱う
//!

use std::sync::{Arc, RwLock};

use serde::Deserialize;
use serde_json::{Value, json};

use super::app_state::AppState;
use super::websocket::RequestAudit;
use crate::audit::model::{AuditOperation, AuditResult};
use crate::auth::AuthContext;
use crate::database::types::PageId;
use crate::database::{BrowserPageState, StateAccessDenied};
use crate::wiki_path::NormalizedWikiPath;

///
/// 今回の一接続一閲覧対象
///
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageSubscription {
    /// 正規形path
    path: String,
    /// 表示中ID
    page_id: String,
    /// 実際に表示したsourceの基準
    expected_state: String,
}

///
/// 共通接続へ渡す配送済みでない応答
///
pub(super) struct Response {
    /// 一意なtype
    pub(super) kind: &'static str,
    /// 機能側で検証したpayload
    pub(super) payload: Value,
    /// 継続利用が不能か
    pub(super) fatal: bool,
    /// wireへ出さない公開操作の分類
    pub(super) audit_result: Option<AuditResult>,
}

impl Response {
    ///
    /// 固定codeだけで安全なerrorを作る
    ///
    /// # 引数
    /// * `kind` - 共通又は機能error
    /// * `code` - 固定code
    /// * `fatal` - 継続不能か
    ///
    /// # 戻り値
    /// 秘密を含まない応答。
    ///
    pub(super) fn error(
        kind: &'static str,
        code: &'static str,
        fatal: bool,
    ) -> Self {
        Self {
            kind,
            payload: json!({"code":code,"message":code}),
            fatal,
            audit_result: Some(match code {
                "invalid_input" => AuditResult::InvalidInput,
                "unsupported_type" => AuditResult::Unsupported,
                "forbidden" => AuditResult::OperationDenied,
                _ => AuditResult::InternalError,
            }),
        }
    }
}

///
/// ページ機能の状態を共通transportから分離する
///
pub(super) struct PageEvents {
    /// 短いsnapshotを読む共有状態
    state: Arc<RwLock<AppState>>,
    /// 不変主体
    auth: AuthContext,
    /// Basic資格情報の比較値
    credential: Option<String>,
    /// 有効な購読
    subscription: Option<PageSubscription>,
    /// 最後に観測した値
    last: Option<BrowserPageState>,
}

impl PageEvents {
    ///
    /// 接続の認証済み値で機能を準備する
    ///
    /// # 引数
    /// * `state` - 共有状態
    /// * `auth` - 不変主体
    /// * `credential` - Basic比較値
    ///
    /// # 戻り値
    /// 購読開始前の機能。
    ///
    pub(super) fn new(
        state: Arc<RwLock<AppState>>,
        auth: AuthContext,
        credential: Option<String>,
    ) -> Self {
        Self {
            state,
            auth,
            credential,
            subscription: None,
            last: None,
        }
    }

    ///
    /// typeごとのpayload検証と機能処理
    ///
    /// # 引数
    /// * `kind` - 共通parserを通ったtype
    /// * `payload` - object
    /// * `changes` - 初回snapshotより前に登録済みの印
    /// * `audit` - 接続監査
    ///
    /// # 戻り値
    /// 機能応答。順序制御・ACKを追加しない。
    ///
    /// # 概要
    /// subscribe/checkを検証し、状態取得を直列に実行する。
    ///
    pub(super) async fn handle(
        &mut self,
        kind: &str,
        payload: Value,
        changes: &mut tokio::sync::watch::Receiver<u64>,
        audit: &mut RequestAudit,
    ) -> Response {
        match kind {
            "page.subscribe" => {
                /* 必須field・path・基準を検証してから購読する。 */
                if self.subscription.is_some() {
                    return Response::error(
                        "page.error",
                        "invalid_input",
                        false,
                    );
                }
                let Ok(mut value) =
                    serde_json::from_value::<PageSubscription>(payload)
                else {
                    return Response::error(
                        "page.error",
                        "invalid_input",
                        false,
                    );
                };
                let Ok(path) = NormalizedWikiPath::parse(&value.path) else {
                    return Response::error(
                        "page.error",
                        "invalid_input",
                        false,
                    );
                };
                if !crate::page_state::valid_state_token(&value.expected_state)
                    || PageId::from_string(&value.page_id).is_err()
                {
                    return Response::error(
                        "page.error",
                        "invalid_input",
                        false,
                    );
                }
                value.path = path.into_string();
                audit.target(value.path.clone());
                drop(changes.borrow_and_update());
                self.subscription = Some(value);
                self.read("page.state", None).await.unwrap()
            }
            "page.check" => {
                /* payload検証前に要求の監査を開始する。 */
                let mut guard = self.state.read().ok().map(|shared| {
                    RequestAudit::new(
                        &shared,
                        &self.auth,
                        AuditOperation::PageCheck,
                    )
                });
                if let Some(guard) = &mut guard {
                    guard.inherit_address(audit);
                }
                if let Some(subscription) = &self.subscription {
                    if let Some(guard) = &mut guard {
                        guard.inherit_address(audit);
                        guard.target(subscription.path.clone());
                    }
                }
                if !payload.as_object().unwrap().is_empty()
                    || self.subscription.is_none()
                {
                    if let Some(guard) = &mut guard {
                        guard.finish_result(
                            AuditResult::InvalidInput,
                            "invalid_input",
                        );
                    }
                    return Response::error(
                        "page.error",
                        "invalid_input",
                        false,
                    );
                }
                self.read("page.checked", guard.as_mut()).await.unwrap()
            }
            _ => Response::error("error", "unsupported_type", false),
        }
    }

    ///
    /// commit後に現在状態だけを確認する
    ///
    /// # 戻り値
    /// 観測状態に差があれば通知。
    ///
    pub(super) async fn changed(&mut self) -> Option<Response> {
        self.read("page.changed", None).await
    }

    ///
    /// 同じsnapshotの状態を公開応答へ変換する
    ///
    /// # 引数
    /// * `kind` - 応答type
    /// * `audit` - 明示確認の監査
    ///
    /// # 戻り値
    /// 応答、又は通知不要ならNone。
    ///
    /// # 概要
    /// 自発通知だけを同値抑制し、初回・確認には必ず返す。
    ///
    async fn read(
        &mut self,
        kind: &'static str,
        mut audit: Option<&mut RequestAudit>,
    ) -> Option<Response> {
        let subscription = self.subscription.clone()?;
        match snapshot(
            self.state.clone(),
            self.auth.clone(),
            self.credential.clone(),
            subscription,
        )
        .await
        {
            Ok(value) => {
                let unchanged = self.last.as_ref() == Some(&value);
                let payload = serde_json::to_value(&value).unwrap();
                self.last = Some(value);
                if let Some(audit) = &mut audit {
                    audit.finish(true, "state");
                }
                if kind == "page.changed" && unchanged {
                    return None;
                }
                Some(Response {
                    kind,
                    payload,
                    fatal: false,
                    audit_result: None,
                })
            }
            Err((code, result)) => {
                if let Some(audit) = &mut audit {
                    audit.finish_result(result, code);
                }
                let mut response = Response::error("page.error", code, true);
                response.audit_result = Some(result);
                Some(response)
            }
        }
    }
}

///
/// 一回の認可・状態読取だけをblocking threadへ渡す
///
/// # 引数
/// * `state` - 共有状態
/// * `auth` - 不変主体
/// * `credential` - Basic比較値
/// * `subscription` - 購読対象
///
/// # 戻り値
/// 状態又は固定error code。
///
async fn snapshot(
    state: Arc<RwLock<AppState>>,
    auth: AuthContext,
    credential: Option<String>,
    subscription: PageSubscription,
) -> Result<BrowserPageState, (&'static str, AuditResult)> {
    tokio::task::spawn_blocking(move || {
        let shared = state
            .read()
            .map_err(|_| ("internal_error", AuditResult::InternalError))?;
        if shared.is_shutting_down() {
            return Err(("forbidden", AuditResult::OperationDenied));
        }
        let id = PageId::from_string(&subscription.page_id)
            .map_err(|_| ("invalid_input", AuditResult::InvalidInput))?;
        shared
            .db()
            .read_browser_page_state(
                &auth,
                credential.as_deref(),
                &subscription.path,
                &id,
                &subscription.expected_state,
            )
            .map_err(|error| match error.downcast_ref::<StateAccessDenied>() {
                Some(StateAccessDenied::Scope) => {
                    ("forbidden", AuditResult::ScopeDenied)
                }
                Some(StateAccessDenied::Prefix) => {
                    ("forbidden", AuditResult::PathPrefixDenied)
                }
                Some(StateAccessDenied::Authentication) => {
                    ("forbidden", AuditResult::OperationDenied)
                }
                None => ("internal_error", AuditResult::InternalError),
            })
    })
    .await
    .map_err(|_| ("internal_error", AuditResult::InternalError))?
}
