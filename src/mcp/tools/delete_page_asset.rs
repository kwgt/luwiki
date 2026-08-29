/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! `delete_page_asset` ツールの入口を定義するモジュール
//!

use rmcp::ErrorData as McpProtocolError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use rmcp::RoleServer;

use crate::mcp::server::LuwikiMcpServer;
use crate::mcp::tools::PageAssetByFileToolArgs;

///
/// `delete_page_asset` を実行する
///
/// # 引数
/// * `server` - MCP server 実装
/// * `params` - tool 呼び出し引数
/// * `context` - RMCP request context
///
/// # 戻り値
/// `delete_page_asset` の tool result を返す。
///
pub(crate) async fn execute(
    server: &LuwikiMcpServer,
    Parameters(args): Parameters<PageAssetByFileToolArgs>,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, McpProtocolError> {
    let auth = server.auth_from_context(&context)?;
    let address = server.address_from_context(&context);
    let handler = server.create_handler();

    /*
     * 既存 handler / service へ `delete_page_asset` を橋渡しする
     */
    let result = server.with_state_read(|state| {
        Ok(handler.handle_delete_page_asset(
            &auth,
            state.db(),
            address,
            &args.path,
            &args.file_name,
        ))
    })?;

    match result {
        Ok(response) => {
            let content = Content::json(response).map_err(|_| {
                McpProtocolError::internal_error(
                    "internal error",
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![content]))
        }
        Err(error) => server.tool_error_result(error),
    }
}
