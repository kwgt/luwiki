/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! `search_pages` ツールの入口を定義するモジュール
//!

use rmcp::ErrorData as McpProtocolError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::service::RequestContext;
use rmcp::RoleServer;

use crate::mcp::server::LuwikiMcpServer;
use crate::mcp::tools::SearchPagesToolArgs;

///
/// `search_pages` を実行する
///
/// # 引数
/// * `server` - MCP server 実装
/// * `params` - tool 呼び出し引数
/// * `context` - RMCP request context
///
/// # 戻り値
/// `search_pages` の tool result を返す。
///
pub(crate) async fn execute(
    server: &LuwikiMcpServer,
    Parameters(args): Parameters<SearchPagesToolArgs>,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, McpProtocolError> {
    let auth = server.auth_from_context(&context)?;
    let address = server.address_from_context(&context);
    let handler = server.create_handler();

    /*
     * 既存 handler / service へ `search_pages` を橋渡しする
     */
    let result = server.with_state_read(|state| {
        let targets = args
            .target
            .iter()
            .map(|target| target.to_fts_search_target())
            .collect::<Vec<_>>();
        Ok(handler.handle_search_pages(
            &auth,
            state.db(),
            state.fts_config(),
            address,
            &args.query,
            &targets,
            args.prefix.as_deref(),
            args.limit,
        ))
    })?;

    match result {
        Ok(response) => {
            let content = Content::json(response).map_err(|error| {
                McpProtocolError::internal_error(
                    format!(
                        "failed to serialize search_pages response: {error}"
                    ),
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![content]))
        }
        Err(error) => server.tool_error_result(error),
    }
}
