/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 状態snapshotの公開変換と比較値を定義する
//!

pub(crate) use crate::page_state::valid_state_token;

use crate::auth::AuthContext;
use crate::database::DatabaseManager;
use crate::database::StateAccessDenied;
use crate::mcp::errors::{McpError, McpErrorCode};
use crate::mcp::model::page_state::PageState;

///
/// 認可付きsnapshotを公開状態へ変換する
///
/// # 引数
/// * `db` - DB
/// * `auth` - 認証済み主体
/// * `path` - 正規化済みpath
///
/// # 戻り値
/// 公開状態、取得不能ならNone、又は固定の公開エラー。
///
/// # 概要
/// 内部IDをhashへだけ使用し、旧sourceの識別子欠落には本文を用いる。
///
pub(crate) fn read_state(
    db: &DatabaseManager,
    auth: &AuthContext,
    path: &str,
) -> Result<Option<PageState>, McpError> {
    /* DBの拒否種別を固定の公開エラーに写像する。 */
    let snapshot = db.read_mcp_page_state(auth, path).map_err(|err| {
        match err.downcast_ref::<StateAccessDenied>() {
            Some(StateAccessDenied::Authentication) => McpError::new(
                McpErrorCode::Forbidden, "authentication no longer valid",
            ),
            Some(StateAccessDenied::Scope) => McpError::new(
                McpErrorCode::Forbidden, "required scope denied: read",
            ),
            Some(StateAccessDenied::Prefix) => McpError::new(
                McpErrorCode::Forbidden,
                "path prefix denied: outside allowed paths",
            ),
            None => McpError::new(
                McpErrorCode::InternalError, "page state lookup failed",
            ),
        }
    })?;
    let Some(snapshot) = snapshot else { return Ok(None) };

    let instance_id = snapshot.source.instance_id().map(|id| id.to_string());
    let token = crate::page_state::state_token(
        path, &snapshot.page_id, snapshot.revision, &snapshot.source,
    );
    Ok(Some(PageState {
        revision: snapshot.revision,
        instance_id,
        state_token: token,
    }))
}
