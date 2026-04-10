/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! RMCP へ渡す MCP server 実装を定義するモジュール
//!

use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use rmcp::ErrorData as McpProtocolError;
use rmcp::ServerHandler;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::service::RequestContext;
use rmcp::RoleServer;
use rmcp::{tool, tool_handler, tool_router};

use crate::auth::AuthContext;
use crate::http_server::app_state::AppState;
use crate::mcp::auth::McpAuthGateway;
use crate::mcp::errors::{McpError, McpErrorResponse};
use crate::mcp::handler::McpHandler;
use crate::mcp::service::McpService;
use crate::mcp::tools::{
    EditPageToolArgs,
    GetPageSectionToolArgs,
    GetPageTocToolArgs,
    GetPageToolArgs,
    ListPagesToolArgs,
    RenamePageToolArgs,
    SearchPagesToolArgs,
    WritePageToolArgs,
    append_page,
    create_page,
    edit_page,
    get_page,
    get_page_section,
    get_page_toc,
    list_pages,
    rename_page,
    search_pages,
    update_page,
};

///
/// RMCP へ渡す MCP server 実装の骨格
///
#[derive(Clone)]
pub(crate) struct LuwikiMcpServer {
    /// HTTP サーバ共有状態
    state: Arc<RwLock<AppState>>,

    /// RMCP tool router
    tool_router: ToolRouter<Self>,
}

impl LuwikiMcpServer {
    ///
    /// MCP server 実装を生成する
    ///
    /// # 引数
    /// * `state` - HTTP サーバ共有状態
    ///
    /// # 戻り値
    /// 生成した MCP server 実装を返す。
    ///
    pub(crate) fn new(state: Arc<RwLock<AppState>>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    ///
    /// MCPハンドラを都度構築する
    ///
    /// # 戻り値
    /// 現在の共有状態に接続した MCP ハンドラを返す。
    ///
    pub(crate) fn create_handler(&self) -> McpHandler {
        let audit_sink = self
            .state
            .read()
            .ok()
            .and_then(|state| state.audit_sink());
        McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            audit_sink,
        )
    }

    ///
    /// 共有状態を read lock して処理する
    ///
    /// # 引数
    /// * `f` - read lock 中に実行する処理
    ///
    /// # 戻り値
    /// クロージャの戻り値を返す。
    ///
    pub(crate) fn with_state_read<T, F>(
        &self,
        f: F,
    ) -> Result<T, McpProtocolError>
    where
        F: FnOnce(&AppState) -> Result<T, McpProtocolError>,
    {
        let state = self.state.read().map_err(|_| {
            McpProtocolError::internal_error(
                "failed to lock app state",
                None,
            )
        })?;
        f(&state)
    }

    ///
    /// request context から MCP認証文脈を取得する
    ///
    /// # 引数
    /// * `context` - RMCP request context
    ///
    /// # 戻り値
    /// 認証文脈を返す。存在しない場合は protocol error を返す。
    ///
    pub(crate) fn auth_from_context(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<AuthContext, McpProtocolError> {
        context
            .extensions
            .get::<AuthContext>()
            .cloned()
            .ok_or_else(|| {
                McpProtocolError::internal_error(
                    "missing MCP auth context",
                    None,
                )
            })
    }

    ///
    /// request context から入力元アドレスを取得する
    ///
    /// # 引数
    /// * `context` - RMCP request context
    ///
    /// # 戻り値
    /// 取得できた場合は入力元 IP アドレスを返す。
    ///
    pub(crate) fn address_from_context(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Option<IpAddr> {
        context.extensions.get::<IpAddr>().copied()
    }

    ///
    /// MCP論理エラーを tool error result へ変換する
    ///
    /// # 引数
    /// * `error` - MCP 論理エラー
    ///
    /// # 戻り値
    /// `CallToolResult` へ変換した結果を返す。
    ///
    pub(crate) fn tool_error_result(
        &self,
        error: McpError,
    ) -> Result<CallToolResult, McpProtocolError> {
        let content = Content::json(McpErrorResponse::from(error))
            .map_err(|serialize_error| {
                McpProtocolError::internal_error(
                    format!(
                        "failed to serialize MCP tool error: {serialize_error}"
                    ),
                    None,
                )
            })?;
        Ok(CallToolResult::error(vec![content]))
    }
}

#[tool_router]
impl LuwikiMcpServer {
    ///
    /// `get_page` の tool 入口
    ///
    #[tool(
        name = "get_page",
        description = "指定した path の Markdown 本文全体を取得する。"
    )]
    async fn get_page_tool(
        &self,
        params: Parameters<GetPageToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        get_page::execute(self, params, context).await
    }

    ///
    /// `get_page_toc` の tool 入口
    ///
    #[tool(
        name = "get_page_toc",
        description = "指定した path の見出し構造と各節規模を取得する。"
    )]
    async fn get_page_toc_tool(
        &self,
        params: Parameters<GetPageTocToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        get_page_toc::execute(self, params, context).await
    }

    ///
    /// `list_pages` の tool 入口
    ///
    #[tool(
        name = "list_pages",
        description = "指定した prefix 配下のページ一覧を取得する。"
    )]
    async fn list_pages_tool(
        &self,
        params: Parameters<ListPagesToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        list_pages::execute(self, params, context).await
    }

    ///
    /// `search_pages` の tool 入口
    ///
    #[tool(
        name = "search_pages",
        description = "全文検索または prefix 制約付き検索を実行する。"
    )]
    async fn search_pages_tool(
        &self,
        params: Parameters<SearchPagesToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        search_pages::execute(self, params, context).await
    }

    ///
    /// `create_page` の tool 入口
    ///
    #[tool(
        name = "create_page",
        description = "指定した path に新規ページを作成する。"
    )]
    async fn create_page_tool(
        &self,
        params: Parameters<WritePageToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        create_page::execute(self, params, context).await
    }

    ///
    /// `edit_page` の tool 入口
    ///
    #[tool(
        name = "edit_page",
        description = "指定した path の Markdown 本文を単一操作で部分編集する。"
    )]
    async fn edit_page_tool(
        &self,
        params: Parameters<EditPageToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        edit_page::execute(self, params, context).await
    }

    ///
    /// `update_page` の tool 入口
    ///
    #[tool(
        name = "update_page",
        description = "指定した path のページ本文を上書き更新する。"
    )]
    async fn update_page_tool(
        &self,
        params: Parameters<WritePageToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        update_page::execute(self, params, context).await
    }

    ///
    /// `append_page` の tool 入口
    ///
    #[tool(
        name = "append_page",
        description = "指定した path のページ末尾へ追記する。"
    )]
    async fn append_page_tool(
        &self,
        params: Parameters<WritePageToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        append_page::execute(self, params, context).await
    }

    ///
    /// `rename_page` の tool 入口
    ///
    #[tool(
        name = "rename_page",
        description = "指定した path のページを別 path へ移動する。"
    )]
    async fn rename_page_tool(
        &self,
        params: Parameters<RenamePageToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        rename_page::execute(self, params, context).await
    }

    ///
    /// `get_page_section` の tool 入口
    ///
    #[tool(
        name = "get_page_section",
        description = "指定した path の特定セクション本文を取得する。"
    )]
    async fn get_page_section_tool(
        &self,
        params: Parameters<GetPageSectionToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        get_page_section::execute(self, params, context).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LuwikiMcpServer {
    ///
    /// MCP server 情報を返す
    ///
    /// # 戻り値
    /// 初期化応答で返す server 情報を返す。
    ///
    fn get_info(&self) -> ServerInfo {
        let _ = &self.state;
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "luwiki",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Use Bearer authentication on every HTTP request.",
            )
    }
}
