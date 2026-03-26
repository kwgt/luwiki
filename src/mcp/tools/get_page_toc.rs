/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! `get_page_toc` ツールの入口を定義するモジュール
//!

use rmcp::ErrorData as McpProtocolError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use rmcp::RoleServer;

use crate::mcp::server::LuwikiMcpServer;
use crate::mcp::tools::GetPageTocToolArgs;

///
/// `get_page_toc` を実行する
///
/// # 引数
/// * `server` - MCP server 実装
/// * `params` - tool 呼び出し引数
/// * `context` - RMCP request context
///
/// # 戻り値
/// `get_page_toc` の tool result を返す。
///
pub(crate) async fn execute(
    server: &LuwikiMcpServer,
    Parameters(args): Parameters<GetPageTocToolArgs>,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, McpProtocolError> {
    let auth = server.auth_from_context(&context)?;
    let address = server.address_from_context(&context);
    let handler = server.create_handler();

    /*
     * 既存 handler / service へ `get_page_toc` を橋渡しする
     */
    let result = server.with_state_read(|state| {
        Ok(handler.handle_get_page_toc(
            &auth,
            state.db(),
            address,
            &args.path,
            args.revision,
        ))
    })?;

    match result {
        Ok(response) => {
            let content = Content::json(response).map_err(|error| {
                McpProtocolError::internal_error(
                    format!(
                        "failed to serialize get_page_toc response: {error}"
                    ),
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![content]))
        }
        Err(error) => server.tool_error_result(error),
    }
}
