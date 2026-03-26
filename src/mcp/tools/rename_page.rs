/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! `rename_page` ツールの入口を定義するモジュール
//!

use rmcp::ErrorData as McpProtocolError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use rmcp::RoleServer;

use crate::mcp::server::LuwikiMcpServer;
use crate::mcp::tools::RenamePageToolArgs;

///
/// `rename_page` を実行する
///
/// # 引数
/// * `server` - MCP server 実装
/// * `params` - tool 呼び出し引数
/// * `context` - RMCP request context
///
/// # 戻り値
/// `rename_page` の tool result を返す。
///
pub(crate) async fn execute(
    server: &LuwikiMcpServer,
    Parameters(args): Parameters<RenamePageToolArgs>,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, McpProtocolError> {
    let auth = server.auth_from_context(&context)?;
    let address = server.address_from_context(&context);
    let handler = server.create_handler();

    /*
     * 既存 handler / service へ `rename_page` を橋渡しする
     */
    let result = server.with_state_read(|state| {
        Ok(handler.handle_rename_page(
            &auth,
            state.db(),
            address,
            &args.path,
            &args.rename_to,
        ))
    })?;

    match result {
        Ok(response) => {
            let content = Content::json(response).map_err(|error| {
                McpProtocolError::internal_error(
                    format!(
                        "failed to serialize rename_page response: {error}"
                    ),
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![content]))
        }
        Err(error) => server.tool_error_result(error),
    }
}
