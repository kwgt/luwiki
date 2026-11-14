/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 固定運用指示の代替取得ツールを定義するモジュール。
//!

use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer};
use serde::Deserialize;
use serde_json::json;

use crate::mcp::server::LuwikiMcpServer;

///
/// 引数の省略または空オブジェクトを受け付ける。
///
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {}

///
/// 代替ツールの取得結果を既存のtool応答形式で返す。
///
/// # 引数
/// * `server` - MCPサーバ
/// * `context` - MCP要求文脈
///
/// # 戻り値
/// URI・MIME型・本文を含むtool応答、またはエラー。
///
/// # 概要
/// ハンドラの結果をJSONのテキストコンテンツへ変換する。
///
pub(crate) fn execute(
    server: &LuwikiMcpServer,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    /*
     * 認証文脈を引き継いで固定本文を取得する
     */
    let auth = server.auth_from_context(&context)?;
    let address = server.address_from_context(&context);
    let handler = server.create_handler();
    let result = server.with_state_read(|state| {
        Ok(handler.handle_get_agent_instructions(&auth, state.db(), address))
    })?;

    /*
     * 既存toolと同じ成功・失敗形式へ変換する
     */
    match result {
        Ok(contents) => {
            let content = Content::json(json!({
                "uri": contents.uri(),
                "mime_type": contents.mime_type(),
                "content": contents.text(),
            })).map_err(|_| ErrorData::internal_error("internal error", None))?;
            Ok(CallToolResult::success(vec![content]))
        }
        Err(error) => server.tool_error_result(error),
    }
}
