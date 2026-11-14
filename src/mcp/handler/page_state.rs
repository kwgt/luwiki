/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 状態toolの非同期制御と要求単位の監査を定義する
//!

use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use rmcp::{ErrorData, RoleServer};
use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use serde::Serialize;

use super::McpHandler;
use crate::audit::AuditSink;
use crate::audit::model::{AuditOperation, AuditRecord, AuditResult};
use crate::auth::AuthContext;
use crate::http_server::app_state::AppState;
use crate::mcp::errors::{McpError, McpErrorCode};
use crate::mcp::model::page_state::{
    GetPageStateResponse, PageState, WaitPageChangeResponse,
};
use crate::mcp::server::LuwikiMcpServer;
use crate::mcp::service::{McpService, page_state};
use crate::mcp::tools::page_state::StateRequest;

#[cfg(test)]
static POLLS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(
    std::collections::HashMap::new(),
));

///
/// 隔離試験で各tokenのsnapshot完了回数を取得する
///
/// # 引数
/// * `token_id` - 試験用主体の管理ID
///
/// # 戻り値
/// 完了したsnapshot数。製品buildでは保持しない。
///
#[cfg(test)]
pub(crate) fn poll_count_for_test(
    token_id: &crate::database::types::TokenId,
) -> usize {
    *POLLS.lock().unwrap().get(&token_id.to_string()).unwrap_or(&0)
}

///
/// 正常応答の選択
///
#[derive(Serialize)]
#[serde(untagged)]
enum StateOutput {
    /// 状態取得
    Get(GetPageStateResponse),
    /// 待機結果
    Wait(WaitPageChangeResponse),
}

///
/// future破棄時も一度だけ要求結果を記録するguard
///
struct StateAudit {
    /// 最終監査投入が終わるまで停止処理を待たせる
    _lease: crate::http_server::app_state::PageStateLease,
    /// 既存の監査投入先
    sink: Option<Arc<RwLock<AuditSink>>>,
    /// 不変主体と正規化pathだけを保持する記録
    record: Option<AuditRecord>,
    /// 認証・入力を含む処理開始時刻
    started: Instant,
    /// 非機密な固定終了理由
    reason: &'static str,
}

// 状態toolの監査を専用子モジュールに分離する。
impl McpHandler {
    ///
    /// 認証時の不変主体を使って要求監査を開始する
    ///
    /// # 引数
    /// * `auth` - 入口の認証文脈
    /// * `address` - 接続元
    /// * `operation` - 新toolの操作種別
    /// * `lease` - 停止gateを通過した受付
    ///
    /// # 戻り値
    /// 未完了ならcancel扱いで記録するguard。
    ///
    /// # 概要
    /// 入口の不変主体と受付leaseを監査の終了まで保持する。
    ///
    fn state_audit(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        operation: AuditOperation,
        lease: crate::http_server::app_state::PageStateLease,
    ) -> StateAudit {
        StateAudit {
            _lease: lease,
            sink: self.audit_sink.clone(),
            record: auth.actor_id().map(|actor| AuditRecord::new(
                operation, actor.clone(), auth.token_id().cloned(),
                address, None, AuditResult::OperationDenied,
                Utc::now(), None, None,
            )),
            started: Instant::now(),
            reason: "cancelled",
        }
    }
}

impl StateAudit {
    ///
    /// エラーを既存の監査分類へ写像する
    ///
    /// # 引数
    /// * `error` - 秘匿情報を含まない公開エラー
    ///
    /// # 戻り値
    /// なし。記録はDropで一度だけ投入する。
    ///
    fn failed(&mut self, error: &McpError) {
        /* 停止・認証利用不能と通常の業務失敗を分類する。 */
        self.reason = if error.message() == "server shutting down" {
            "shutdown"
        } else {
            "error"
        };
        if let Some(record) = &mut self.record {
            record.result = if self.reason == "shutdown"
                || error.message() == "authentication no longer valid"
            {
                AuditResult::OperationDenied
            } else {
                super::audit_result_from_error(error)
            };
        }
    }

    ///
    /// 成功した結果だけを監査へ反映する
    ///
    /// # 引数
    /// * `output` - 公開応答
    ///
    /// # 戻り値
    /// なし。本文や比較tokenを記録へ複製しない。
    ///
    fn succeeded(&mut self, output: &StateOutput) {
        /* 成功時の固定理由と実際のrevisionだけを抽出する。 */
        let (reason, revision) = match output {
            StateOutput::Get(value) => ("state", Some(value.state.revision)),
            StateOutput::Wait(value) => (
                value.change.unwrap_or("timeout"),
                value.state.as_ref().map(|state| state.revision),
            ),
        };
        self.reason = reason;
        if let Some(record) = &mut self.record {
            record.result = AuditResult::Success;
            record.revision = revision;
        }
    }
}

impl Drop for StateAudit {
    ///
    /// 受付・終了のいずれでも公開要求の記録を一度だけ投入する
    ///
    /// # 戻り値
    /// なし。監査失敗は業務結果を変更しない。
    ///
    /// # 概要
    /// 記録を一度だけ取り出して投入し、その後fieldのleaseを解放する。
    ///
    fn drop(&mut self) {
        /* 不変主体と固定理由だけで監査を完成させる。 */
        let Some(mut record) = self.record.take() else { return };
        record.summary = Some(serde_json::json!({
            "reason": self.reason,
            "elapsed_ms": self.started.elapsed().as_millis() as u64,
        }).to_string());
        let Some(sink) = &self.sink else { return };

        /* 既存sinkと同じbest-effort方針で投入する。 */
        match sink.write() {
            Ok(mut sink) => {
                if sink.record(record).is_err() {
                    tracing::warn!("page state audit recording failed");
                }
            }
            Err(_) => tracing::warn!("page state audit lock failed"),
        }
    }
}

///
/// 新状態toolを処理する
///
/// # 引数
/// * `server` - 共有状態を持つserver
/// * `path` - 入力path
/// * `request` - 状態取得又は変更待ち
/// * `context` - 要求取消と認証文脈
///
/// # 戻り値
/// tool応答又はprotocol上の取消・内部失敗。
///
/// # 概要
/// 監査guardをasync future内に保持し、dropと正常終了を一つにする。
///
pub(crate) async fn execute(
    server: &LuwikiMcpServer,
    path: String,
    request: StateRequest,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    /* 主体と監査を確定し、入力pathを検証する。 */
    let auth = server.auth_from_context(&context)?;
    let operation = match &request {
        StateRequest::Get => AuditOperation::GetPageState,
        StateRequest::Wait { .. } => AuditOperation::WaitPageChange,
    };
    let handler = server.create_handler();
    let lease = server.begin_page_state_request()?;
    let mut audit = handler.state_audit(
        &auth, server.address_from_context(&context), operation, lease,
    );
    let normalized = McpService::new().validate_and_normalize_path(&path);
    let path = match normalized {
        Ok(path) => path,
        Err(_) => {
            let error = McpError::new(
                McpErrorCode::InvalidInput, "path is invalid",
            );
            audit.failed(&error);
            return server.tool_error_result(error);
        }
    };
    if let Some(record) = &mut audit.record {
        record.target_path = Some(path.clone());
    }

    /* 取消時にrun futureを破棄し、非同期sleepを残さない。 */
    let outcome = tokio::select! {
        biased;
        _ = context.ct.cancelled() => {
            return Err(ErrorData::internal_error("request cancelled", None));
        }
        result = run(
            server.shared_state(), auth, path, request, audit.started,
        ) => result,
    };
    match outcome {
        Ok(output) => {
            let content = Content::json(&output).map_err(|_| {
                ErrorData::internal_error("internal error", None)
            })?;
            audit.succeeded(&output);
            Ok(CallToolResult::success(vec![content]))
        }
        Err(error) => {
            audit.failed(&error);
            server.tool_error_result(error)
        }
    }
}

///
/// 一回の短いblocking取得を実行する
///
/// # 引数
/// * `state` - 共有状態
/// * `auth` - 認証主体
/// * `path` - 正規化済みpath
///
/// # 戻り値
/// 公開状態又は固定エラー。
///
/// # 概要
/// blocking taskへ一回の短い取得だけを渡し、lockを保持して待機しない。
///
async fn read(
    state: Arc<RwLock<AppState>>,
    auth: AuthContext,
    path: String,
) -> Result<Option<PageState>, McpError> {
    tokio::task::spawn_blocking(move || {
        let state = state.read().map_err(|_| McpError::new(
            McpErrorCode::InternalError, "page state lookup failed",
        ))?;
        if state.is_shutting_down() {
            return Err(McpError::new(
                McpErrorCode::InternalError, "server shutting down",
            ));
        }
        let result = page_state::read_state(state.db(), &auth, &path);
        #[cfg(test)]
        if let Some(token_id) = auth.token_id() {
            let mut counts = POLLS.lock().unwrap();
            *counts.entry(token_id.to_string()).or_default() += 1;
        }
        result
    }).await.map_err(|_| McpError::new(
        McpErrorCode::InternalError, "page state lookup failed",
    ))?
}

///
/// 基準比較と期限付きの非同期待機を実行する
///
/// # 引数
/// * `state` - 共有状態
/// * `auth` - 入口の不変主体
/// * `path` - 正規化済みpath
/// * `request` - 入力種別
/// * `started` - 要求処理の開始時刻
///
/// # 戻り値
/// 認可済みsnapshotに対応する正常応答又はエラー。
///
/// # 概要
/// 各取得後にlockとtxnを解放し、最終snapshotの変化を期限より優先する。
///
async fn run(
    state: Arc<RwLock<AppState>>,
    auth: AuthContext,
    path: String,
    request: StateRequest,
    started: Instant,
) -> Result<StateOutput, McpError> {
    /* 待機条件を検証し、monotonicな期限を設定する。 */
    let (expected, seconds) = match request {
        StateRequest::Get => (None, 0),
        StateRequest::Wait { expected_state, timeout_seconds } => {
            let seconds = timeout_seconds.as_u64().filter(|s| {
                (1..=30).contains(s)
            }).ok_or_else(|| McpError::new(
                McpErrorCode::InvalidInput, "timeout_seconds must be 1..30",
            ))?;
            if !page_state::valid_state_token(&expected_state) {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput, "expected_state is invalid",
                ));
            }
            (Some(expected_state), seconds)
        }
    };
    let deadline = tokio::time::Instant::from_std(started)
        + Duration::from_secs(seconds);

    /* 同一snapshotの再確認を非同期sleepと交互に行う。 */
    loop {
        let current = read(state.clone(), auth.clone(), path.clone()).await?;
        let Some(expected) = expected.as_ref() else {
            return current.map(|value| StateOutput::Get(GetPageStateResponse {
                path, state: value,
            })).ok_or_else(|| McpError::new(
                McpErrorCode::NotFound, "page not available",
            ));
        };
        let change = match &current {
            None => Some("unavailable"),
            Some(value) if value.state_token == *expected => None,
            Some(value) if value.state_token[3..67] != expected[3..67] => {
                Some("replaced")
            }
            Some(_) => Some("updated"),
        };
        let now = tokio::time::Instant::now();
        if change.is_some() || now >= deadline {
            return Ok(StateOutput::Wait(WaitPageChangeResponse {
                path,
                status: if change.is_some() { "changed" } else { "timeout" },
                change,
                state: current,
            }));
        }
        tokio::time::sleep(
            (deadline - now).min(Duration::from_millis(500)),
        ).await;
    }
}
