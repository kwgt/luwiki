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
use rmcp::model::{
    CallToolResult,
    Content,
    GetPromptRequestParams,
    GetPromptResult,
    Implementation,
    ListPromptsResult,
    ListResourcesResult,
    PaginatedRequestParams,
    Prompt,
    PromptArgument,
    PromptMessage,
    PromptMessageRole,
    RawResource,
    ReadResourceRequestParams,
    ReadResourceResult,
    Resource,
    ResourceContents,
    ServerCapabilities,
    ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::RoleServer;
use rmcp::{tool, tool_handler, tool_router};

use crate::auth::AuthContext;
use crate::http_server::app_state::AppState;
use crate::mcp::auth::McpAuthGateway;
use crate::mcp::errors::{
    McpError,
    McpErrorCode,
    McpErrorResponse,
};
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

    /// prompts公開前提が整っているか
    prompts_ready: bool,

    /// resources公開前提が整っているか
    resources_ready: bool,
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
        let prompts_ready = state
            .read()
            .ok()
            .and_then(|state| {
                state
                    .db()
                    .is_mcp_primitive_name_index_ready()
                    .ok()
            })
            .unwrap_or(false);
        let resources_ready = state
            .read()
            .ok()
            .and_then(|state| {
                state
                    .db()
                    .is_resource_uri_index_ready()
                    .ok()
            })
            .unwrap_or(false);
        Self {
            state,
            tool_router: Self::tool_router(),
            prompts_ready,
            resources_ready,
        }
    }

    ///
    /// MCP capabilityを構築する
    ///
    /// # 戻り値
    /// 現在の実装状態とDB readinessに対応するcapabilityを返す。
    ///
    fn build_capabilities(&self) -> ServerCapabilities {
        match (self.prompts_ready, self.resources_ready) {
            (true, true) => ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
            (true, false) => ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
            (false, true) => ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            (false, false) => {
                ServerCapabilities::builder().enable_tools().build()
            }
        }
    }

    ///
    /// 認証済み文脈からrmcp resource一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のresource一覧結果を返す。
    ///
    fn list_resources_for_auth(
        &self,
        auth: &AuthContext,
        cursor: Option<&str>,
    ) -> Result<ListResourcesResult, McpProtocolError> {
        self.list_resources_for_auth_at(auth, None, cursor)
    }

    ///
    /// 認証済み文脈と入力元からrmcp resource一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のresource一覧結果を返す。
    ///
    fn list_resources_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListResourcesResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok(handler.handle_list_resources(
                auth,
                state.db(),
                address,
                cursor,
            ))
        })?;
        let result =
            result.map_err(Self::list_resource_protocol_error)?;
        let resources = result
            .items()
            .iter()
            .map(|item| {
                let raw = RawResource::new(item.uri(), item.name())
                    .with_description(item.description())
                    .with_mime_type(item.mime_type());

                Resource::new(raw, None)
            })
            .collect();
        let mut response =
            ListResourcesResult::default();
        response.resources = resources;
        response.next_cursor =
            result.next_cursor().map(str::to_string);

        Ok(response)
    }

    ///
    /// resource論理エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - resource一覧の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn list_resource_protocol_error(
        error: McpError,
    ) -> McpProtocolError {
        match error.code() {
            McpErrorCode::Forbidden => {
                McpProtocolError::invalid_request(
                    "operation is not allowed",
                    Some(serde_json::json!({
                        "code": "forbidden",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    "cursor is invalid",
                    Some(serde_json::json!({
                        "code": "invalid_input",
                    })),
                )
            }
            _ => McpProtocolError::internal_error(
                "internal error",
                Some(serde_json::json!({
                    "code": "internal_error",
                })),
            ),
        }
    }

    ///
    /// 認証済み文脈からrmcp resource取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// rmcp標準のresource取得結果を返す。
    ///
    fn read_resource_for_auth(
        &self,
        auth: &AuthContext,
        uri: &str,
    ) -> Result<ReadResourceResult, McpProtocolError> {
        self.read_resource_for_auth_at(auth, None, uri)
    }

    ///
    /// 認証済み文脈と入力元からrmcp resource取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// rmcp標準のresource取得結果を返す。
    ///
    fn read_resource_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        uri: &str,
    ) -> Result<ReadResourceResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok::<_, McpProtocolError>(handler.handle_read_resource(
                auth,
                state.db(),
                address,
                uri,
            ))
        })?;
        let result =
            result.map_err(Self::read_resource_protocol_error)?;
        let _revision = result.revision();
        let contents = ResourceContents::text(
            result.text(),
            result.uri(),
        )
        .with_mime_type(result.mime_type());

        Ok(ReadResourceResult::new(vec![contents]))
    }

    ///
    /// resource取得エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - resource取得の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn read_resource_protocol_error(
        error: McpError,
    ) -> McpProtocolError {
        match error.code() {
            McpErrorCode::Forbidden => {
                McpProtocolError::invalid_request(
                    "operation is not allowed",
                    Some(serde_json::json!({
                        "code": "forbidden",
                    })),
                )
            }
            McpErrorCode::NotFound => {
                McpProtocolError::invalid_params(
                    "resource not found",
                    Some(serde_json::json!({
                        "code": "not_found",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    "resource uri is invalid",
                    Some(serde_json::json!({
                        "code": "invalid_input",
                    })),
                )
            }
            _ => McpProtocolError::internal_error(
                "internal error",
                Some(serde_json::json!({
                    "code": "internal_error",
                })),
            ),
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

    ///
    /// 認証済み文脈からrmcp prompt一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のprompt一覧結果を返す。
    ///
    fn list_prompts_for_auth(
        &self,
        auth: &AuthContext,
        cursor: Option<&str>,
    ) -> Result<ListPromptsResult, McpProtocolError> {
        self.list_prompts_for_auth_at(auth, None, cursor)
    }

    ///
    /// 認証済み文脈と入力元からrmcp prompt一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のprompt一覧結果を返す。
    ///
    fn list_prompts_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListPromptsResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok(handler.handle_list_prompts(
                auth,
                state.db(),
                address,
                cursor,
            ))
        })?;
        let result =
            result.map_err(Self::list_prompt_protocol_error)?;
        let prompts = result
            .items()
            .iter()
            .map(|item| {
                let arguments = item
                    .arguments()
                    .iter()
                    .map(|argument| {
                        let required = argument.required();
                        let mapped = PromptArgument::new(
                            argument.name(),
                        )
                        .with_description(argument.description());
                        match required {
                            Some(required) => {
                                mapped.with_required(required)
                            }
                            None => mapped,
                        }
                    })
                    .collect::<Vec<_>>();
                let arguments = if arguments.is_empty() {
                    None
                } else {
                    Some(arguments)
                };
                Prompt::new(
                    item.name(),
                    Some(item.description()),
                    arguments,
                )
            })
            .collect();
        let mut response =
            ListPromptsResult::with_all_items(prompts);
        response.next_cursor =
            result.next_cursor().map(str::to_string);

        Ok(response)
    }

    ///
    /// prompt論理エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - prompt一覧の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn list_prompt_protocol_error(
        error: McpError,
    ) -> McpProtocolError {
        match error.code() {
            McpErrorCode::Forbidden => {
                McpProtocolError::invalid_request(
                    "operation is not allowed",
                    Some(serde_json::json!({
                        "code": "forbidden",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    "cursor is invalid",
                    Some(serde_json::json!({
                        "code": "invalid_input",
                    })),
                )
            }
            _ => McpProtocolError::internal_error(
                "internal error",
                Some(serde_json::json!({
                    "code": "internal_error",
                })),
            ),
        }
    }

    ///
    /// 認証済み文脈からrmcp prompt取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// rmcp標準のprompt取得結果を返す。
    ///
    fn get_prompt_for_auth(
        &self,
        auth: &AuthContext,
        name: &str,
        arguments: Option<
            &serde_json::Map<String, serde_json::Value>,
        >,
    ) -> Result<GetPromptResult, McpProtocolError> {
        self.get_prompt_for_auth_at(
            auth,
            None,
            name,
            arguments,
        )
    }

    ///
    /// 認証済み文脈と入力元から
    /// rmcp prompt取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// rmcp標準のprompt取得結果を返す。
    ///
    fn get_prompt_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        name: &str,
        arguments: Option<
            &serde_json::Map<String, serde_json::Value>,
        >,
    ) -> Result<GetPromptResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok(handler.handle_get_prompt(
                auth,
                state.db(),
                address,
                name,
                arguments,
            ))
        })?;
        let result =
            result.map_err(Self::get_prompt_protocol_error)?;
        let message = PromptMessage::new_text(
            PromptMessageRole::User,
            result.message(),
        );

        Ok(GetPromptResult::new(vec![message])
            .with_description(result.description()))
    }

    ///
    /// prompt取得エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - prompt取得の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn get_prompt_protocol_error(
        error: McpError,
    ) -> McpProtocolError {
        match error.code() {
            McpErrorCode::Forbidden => {
                McpProtocolError::invalid_request(
                    "operation is not allowed",
                    Some(serde_json::json!({
                        "code": "forbidden",
                    })),
                )
            }
            McpErrorCode::NotFound => {
                McpProtocolError::invalid_params(
                    "prompt not found",
                    Some(serde_json::json!({
                        "code": "not_found",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    error.message().to_string(),
                    Some(serde_json::json!({
                        "code": "invalid_input",
                    })),
                )
            }
            _ => McpProtocolError::internal_error(
                "internal error",
                Some(serde_json::json!({
                    "code": "internal_error",
                })),
            ),
        }
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
    /// MCP標準の`prompts/get`を処理する
    ///
    /// # 引数
    /// * `request` - prompt取得要求
    /// * `context` - RMCP request context
    ///
    /// # 戻り値
    /// prompt取得結果を返す。
    ///
    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpProtocolError> {
        let auth = self.auth_from_context(&context)?;
        let address = self.address_from_context(&context);
        self.get_prompt_for_auth_at(
            &auth,
            address,
            &request.name,
            request.arguments.as_ref(),
        )
    }

    ///
    /// MCP標準の`prompts/list`を処理する
    ///
    /// # 引数
    /// * `request` - ページング要求
    /// * `context` - RMCP request context
    ///
    /// # 戻り値
    /// prompt一覧結果を返す。
    ///
    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpProtocolError> {
        let auth = self.auth_from_context(&context)?;
        let address = self.address_from_context(&context);
        let cursor = request
            .as_ref()
            .and_then(|request| request.cursor.as_deref());
        self.list_prompts_for_auth_at(
            &auth,
            address,
            cursor,
        )
    }

    ///
    /// MCP標準の`resources/list`を処理する
    ///
    /// # 引数
    /// * `request` - ページング要求
    /// * `context` - RMCP request context
    ///
    /// # 戻り値
    /// resource一覧結果を返す。
    ///
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpProtocolError> {
        let auth = self.auth_from_context(&context)?;
        let address = self.address_from_context(&context);
        let cursor = request
            .as_ref()
            .and_then(|request| request.cursor.as_deref());
        self.list_resources_for_auth_at(
            &auth,
            address,
            cursor,
        )
    }

    ///
    /// MCP標準の`resources/read`を処理する
    ///
    /// # 引数
    /// * `request` - resource取得要求
    /// * `context` - RMCP request context
    ///
    /// # 戻り値
    /// resource取得結果を返す。
    ///
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpProtocolError> {
        let auth = self.auth_from_context(&context)?;
        let address = self.address_from_context(&context);
        self.read_resource_for_auth_at(
            &auth,
            address,
            &request.uri,
        )
    }

    ///
    /// MCP server 情報を返す
    ///
    /// # 戻り値
    /// 初期化応答で返す server 情報を返す。
    ///
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(self.build_capabilities())
            .with_server_info(Implementation::new(
                "luwiki",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Use Bearer authentication on every HTTP request.",
            )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use rmcp::ErrorData as McpProtocolError;
    use rmcp::ServerHandler;
    use rmcp::model::{
        ErrorCode,
        PromptMessageContent,
        PromptMessageRole,
        ResourceContents,
    };
    use tempfile::tempdir;

    use super::LuwikiMcpServer;
    use crate::auth::{AuthContext, AuthUser};
    use crate::cmd_args::FrontendConfig;
    use crate::database::DatabaseManager;
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        McpPrimitiveKind,
        PageId,
        PathPrefixSet,
        PromptCandidateEntry,
        ResourceCandidateEntry,
        UserAttribute,
        UserAttributeSet,
    };
    use crate::fts::FtsIndexConfig;
    use crate::http_server::app_state::AppState;

    ///
    /// prompts/listテスト用のpromptページソースを生成する
    ///
    /// # 引数
    /// * `name` - prompt名
    ///
    /// # 戻り値
    /// prompt front matterと本文を含むソースを返す。
    ///
    fn prompt_source_for_list(name: &str) -> String {
        format!(
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: {}\n",
                "  description: {} description\n",
                "---\n",
                "本文",
            ),
            name,
            name,
        )
    }

    ///
    /// resources/listテスト用のresourceページソースを生成する
    ///
    /// # 引数
    /// * `resource_id` - resource識別子
    /// * `name` - resource名
    ///
    /// # 戻り値
    /// resource front matterと本文を含むソースを返す。
    ///
    fn resource_source_for_list(resource_id: &str, name: &str) -> String {
        format!(
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: resource\n",
                "  resource_id: {}\n",
                "  name: {}\n",
                "  description: {} description\n",
                "---\n",
                "本文",
            ),
            resource_id,
            name,
            name,
        )
    }

    ///
    /// resources/listテスト用のMIME type付きresourceページソースを生成する
    ///
    /// # 引数
    /// * `resource_id` - resource識別子
    /// * `name` - resource名
    /// * `mime_type` - MIME type
    ///
    /// # 戻り値
    /// resource front matterと本文を含むソースを返す。
    ///
    fn resource_source_for_list_with_mime_type(
        resource_id: &str,
        name: &str,
        mime_type: &str,
    ) -> String {
        format!(
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: resource\n",
                "  resource_id: {}\n",
                "  name: {}\n",
                "  description: {} description\n",
                "  mime_type: {}\n",
                "---\n",
                "本文",
            ),
            resource_id,
            name,
            name,
            mime_type,
        )
    }

    ///
    /// rmcp標準のprompts/get経路が名前索引から
    /// 最新prompt本文を取得することを確認する。
    ///
    /// # 注記
    /// 候補テーブルを欠損させ、path prefix範囲外のpromptを
    /// 名前索引と最新ソースだけから取得する。
    ///
    #[test]
    fn mcp_server_get_prompt_resolves_latest_source_by_name() {
        /*
         * path制約外のprompt正本と共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/private/prompts/get",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: get-prompt\n",
                    "  description: prompt description\n",
                    "---\n",
                    "\n# Heading\n\n",
                    "{{macro}}\n",
                    "---\n",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        manager
            .remove_prompt_candidate_by_page_id(&page_id)
            .expect("remove prompt candidate failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );

        /*
         * rmcp取得結果のdescriptionとraw Markdownを確認する
         */
        let result = server
            .get_prompt_for_auth(&auth, "get-prompt", None)
            .expect("get prompt failed");
        assert_eq!(
            result.description.as_deref(),
            Some("prompt description"),
        );
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, PromptMessageRole::User);
        assert_eq!(
            result.messages[0].content,
            PromptMessageContent::Text {
                text: concat!(
                    "\n# Heading\n\n",
                    "{{macro}}\n",
                    "---\n",
                )
                .to_string(),
            },
        );
    }

    ///
    /// system未指定時のprompts/getが本文だけを
    /// 単一User text messageへ変換することを確認する。
    ///
    /// # 注記
    /// 本文の先頭・末尾空白と改行を保持し、
    /// descriptionへsystem相当の情報を
    /// 追加しないことを検証する。
    ///
    #[test]
    fn mcp_server_get_prompt_maps_body_without_system() {
        /*
         * system未指定のprompt正本を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/no-system",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: no-system\n",
                    "  description: body description\n",
                    "---\n",
                    "\n  Body {{macro}}  \n\n",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 本文だけの単一User text messageを確認する
         */
        let result = server
            .get_prompt_for_auth(&auth, "no-system", None)
            .expect("get prompt failed");
        assert_eq!(
            result.description.as_deref(),
            Some("body description"),
        );
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, PromptMessageRole::User);
        assert_eq!(
            result.messages[0].content,
            PromptMessageContent::Text {
                text: "\n  Body {{macro}}  \n\n".to_string(),
            },
        );
    }

    ///
    /// prompts/getの垂直接続が未展開本文を
    /// 公開しないことを確認する。
    ///
    /// # 注記
    /// system、required、optional、エスケープを含むpromptを
    /// 取得し、単一User messageへの最小変換を検証する。
    ///
    #[test]
    fn mcp_server_get_prompt_returns_expanded_user_message() {
        /*
         * 引数付きprompt正本と要求引数を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/arguments",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: argument-prompt\n",
                    "  description: argument description\n",
                    "  system: \"System {{@target}}\\n\"\n",
                    "  arguments:\n",
                    "    - name: target\n",
                    "      description: target value\n",
                    "      required: true\n",
                    "    - name: optional\n",
                    "      description: optional value\n",
                    "---\n",
                    "\nBody {{@target}} {{@optional}} ",
                    "{{@@literal}}",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let arguments = serde_json::Map::from_iter([(
            "target".to_string(),
            serde_json::Value::String(
                "{{@optional}}".to_string(),
            ),
        )]);

        /*
         * 一回展開後の単一User messageを確認する
         */
        let result = server
            .get_prompt_for_auth(
                &auth,
                "argument-prompt",
                Some(&arguments),
            )
            .expect("get argument prompt failed");
        assert_eq!(
            result.description.as_deref(),
            Some("argument description"),
        );
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, PromptMessageRole::User);
        assert_eq!(
            result.messages[0].content,
            PromptMessageContent::Text {
                text: concat!(
                    "System {{@optional}}\n",
                    "\n\n",
                    "\nBody {{@optional}}  {{@literal}}",
                )
                .to_string(),
            },
        );
    }

    ///
    /// prompts/getの引数不正を固定InvalidParamsへ
    /// 変換することを確認する。
    ///
    /// # 注記
    /// 必須不足、未知引数、型不正の公開messageと論理codeを
    /// 検証し、引数値が含まれないことを確認する。
    ///
    #[test]
    fn mcp_server_get_prompt_maps_argument_errors() {
        for message in [
            "required prompt argument is missing: target",
            "unknown prompt argument: unknown",
            "prompt argument must be a string: target",
        ] {
            let error = LuwikiMcpServer::get_prompt_protocol_error(
                crate::mcp::errors::McpError::new(
                    crate::mcp::errors::McpErrorCode::InvalidInput,
                    message,
                ),
            );
            assert_prompt_protocol_error(
                &error,
                ErrorCode::INVALID_PARAMS,
                message,
                "invalid_input",
            );
            let serialized = serde_json::to_string(&error)
                .expect("serialize protocol error failed");
            assert!(!serialized.contains("secret-argument-value"));
        }
    }

    ///
    /// prompts/getがread scopeを要求し、ReadOnly属性を
    /// 許可することを確認する。
    ///
    /// # 注記
    /// append-only、read、ReadOnly付きreadの各認証文脈で
    /// 同じpromptを取得する。
    ///
    #[test]
    fn mcp_server_get_prompt_requires_read_scope() {
        /*
         * prompt正本と認証文脈を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/scope",
                "alice",
                prompt_source_for_list("scope-prompt"),
            )
            .expect("create prompt failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let append_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let read = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let read_only = AuthContext::new_with_attributes(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
        );

        /*
         * scope不足拒否とread系の許可を確認する
         */
        let error = server
            .get_prompt_for_auth(
                &append_only,
                "scope-prompt",
                None,
            )
            .expect_err("append-only must be rejected");
        assert_prompt_protocol_error(
            &error,
            ErrorCode::INVALID_REQUEST,
            "operation is not allowed",
            "forbidden",
        );
        assert!(server
            .get_prompt_for_auth(&read, "scope-prompt", None)
            .is_ok());
        assert!(server
            .get_prompt_for_auth(
                &read_only,
                "scope-prompt",
                None,
            )
            .is_ok());
    }

    ///
    /// prompts/getが非公開状態と正本不整合を
    /// 固定protocol errorへ変換することを確認する。
    ///
    /// # 注記
    /// soft deleteと名前索引・latest source不一致を順に作り、
    /// 公開エラーへ内部情報が混入しないことを検証する。
    ///
    #[test]
    fn mcp_server_get_prompt_maps_state_and_consistency_errors() {
        /*
         * 状態遷移用のprompt正本を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/secret/prompt-path",
                "alice",
                prompt_source_for_list("state-prompt"),
            )
            .expect("create prompt failed");
        manager
            .delete_page_by_id(&page_id)
            .expect("soft delete prompt failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state.clone());
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * soft deleteをprompt不存在へ変換する
         */
        let not_found = server
            .get_prompt_for_auth(&auth, "state-prompt", None)
            .expect_err("deleted prompt must not be returned");
        assert_prompt_protocol_error(
            &not_found,
            ErrorCode::INVALID_PARAMS,
            "prompt not found",
            "not_found",
        );

        /*
         * undelete後にlatest sourceだけを不整合化する
         */
        {
            let guard = state.read().expect("lock state failed");
            guard
                .db()
                .undelete_page_by_id(
                    &page_id,
                    "/secret/prompt-path",
                    false,
                )
                .expect("undelete prompt failed");
            guard
                .db()
                .replace_latest_page_source_for_prompt_rebuild_test(
                    &page_id,
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: changed-name\n",
                        "  description: changed\n",
                        "---\n",
                        "secret-body",
                    )
                    .to_string(),
                )
                .expect("replace latest source failed");
        }
        let internal = server
            .get_prompt_for_auth(&auth, "state-prompt", None)
            .expect_err("name mismatch must fail");
        assert_prompt_protocol_error(
            &internal,
            ErrorCode::INTERNAL_ERROR,
            "internal error",
            "internal_error",
        );
        let serialized = serde_json::to_string(&internal)
            .expect("serialize protocol error failed");
        assert!(!serialized.contains("/secret/prompt-path"));
        assert!(!serialized.contains("secret-body"));
        assert!(!serialized.contains(&page_id.to_string()));
    }

    ///
    /// prompts/getが不存在、draft、hard deleteを
    /// 同じ秘匿済みエラーへ変換することを確認する。
    ///
    /// # 注記
    /// 名前索引に存在しない要求、不正な名前、draftを指す
    /// 不整合索引、hard delete後の要求を順に検証する。
    ///
    #[test]
    fn mcp_server_get_prompt_hides_unavailable_states() {
        /*
         * hard delete対象とdraftを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let deleted_id = manager
            .create_page(
                "/secret/hard-delete",
                "alice",
                prompt_source_for_list("hard-deleted"),
            )
            .expect("create prompt failed");
        manager
            .delete_page_by_id(&deleted_id)
            .expect("soft delete prompt failed");
        manager
            .delete_page_by_id_hard(&deleted_id)
            .expect("hard delete prompt failed");
        let draft_id = manager
            .create_draft_page("/secret/draft", "alice")
            .expect("create draft failed")
            .0;
        manager
            .set_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "draft-prompt",
                Some(&draft_id),
            )
            .expect("insert draft name index failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 各非公開状態を同じnot_foundへ変換する
         */
        for name in [
            "missing-prompt",
            " invalid-name",
            "draft-prompt",
            "hard-deleted",
        ] {
            let error = server
                .get_prompt_for_auth(&auth, name, None)
                .expect_err("unavailable prompt must fail");
            assert_prompt_protocol_error(
                &error,
                ErrorCode::INVALID_PARAMS,
                "prompt not found",
                "not_found",
            );
            let serialized = serde_json::to_string(&error)
                .expect("serialize protocol error failed");
            assert!(!serialized.contains("/secret/"));
            assert!(!serialized.contains(&draft_id.to_string()));
            assert!(!serialized.contains(&deleted_id.to_string()));
            assert!(!serialized.contains("hard delete"));
            assert!(!serialized.contains("draft"));
        }
    }

    ///
    /// prompts/getの内部不整合が正本、名前索引、
    /// prompt候補を変更しないことを確認する。
    ///
    /// # 注記
    /// front matter不正、用途不一致、名前不一致、
    /// 未宣言placeholderをlatest sourceへ順に投入する。
    ///
    #[test]
    fn mcp_server_get_prompt_preserves_source_on_consistency_error() {
        /*
         * 不整合化するprompt正本と共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/secret/consistency",
                "alice",
                prompt_source_for_list("stable-prompt"),
            )
            .expect("create prompt failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state.clone());
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let corrupted_sources = [
            concat!(
                "---\n",
                "mcp: [invalid\n",
                "---\n",
                "secret-invalid-body",
            ),
            concat!(
                "---\n",
                "wiki:\n",
                "  tags: []\n",
                "---\n",
                "secret-normal-body",
            ),
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: changed-prompt\n",
                "  description: changed\n",
                "---\n",
                "secret-name-body",
            ),
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: stable-prompt\n",
                "  description: stable\n",
                "---\n",
                "secret {{@undeclared}} body",
            ),
        ];

        /*
         * 各不整合で固定errorとDB状態の非変更を確認する
         */
        for source in corrupted_sources {
            let guard = state.read().expect("lock state failed");
            guard
                .db()
                .replace_latest_page_source_for_prompt_rebuild_test(
                    &page_id,
                    source.to_string(),
                )
                .expect("replace latest source failed");
            let before_source = guard
                .db()
                .get_prompt_source_by_name("stable-prompt")
                .expect("get source failed")
                .expect("prompt source missing");
            let before_candidate = guard
                .db()
                .get_prompt_candidate_by_page_id(&page_id)
                .expect("get candidate failed");
            let before_owner = guard
                .db()
                .get_mcp_primitive_name_owner_for_test(
                    McpPrimitiveKind::Prompt,
                    "stable-prompt",
                )
                .expect("get name owner failed");
            drop(guard);

            let error = server
                .get_prompt_for_auth(
                    &auth,
                    "stable-prompt",
                    None,
                )
                .expect_err("consistency error expected");
            assert_prompt_protocol_error(
                &error,
                ErrorCode::INTERNAL_ERROR,
                "internal error",
                "internal_error",
            );
            let serialized = serde_json::to_string(&error)
                .expect("serialize protocol error failed");
            assert!(!serialized.contains("secret"));
            assert!(!serialized.contains("undeclared"));
            assert!(!serialized.contains("/secret/consistency"));
            assert!(!serialized.contains(&page_id.to_string()));

            let guard = state.read().expect("lock state failed");
            let after_source = guard
                .db()
                .get_prompt_source_by_name("stable-prompt")
                .expect("get source failed")
                .expect("prompt source missing");
            let after_candidate = guard
                .db()
                .get_prompt_candidate_by_page_id(&page_id)
                .expect("get candidate failed");
            let after_owner = guard
                .db()
                .get_mcp_primitive_name_owner_for_test(
                    McpPrimitiveKind::Prompt,
                    "stable-prompt",
                )
                .expect("get name owner failed");
            assert_eq!(
                after_source.revision(),
                before_source.revision(),
            );
            assert_eq!(
                after_source.source(),
                before_source.source(),
            );
            assert_eq!(after_candidate, before_candidate);
            assert_eq!(after_owner, before_owner);
        }
    }

    ///
    /// 名前索引未構築時にprompts capabilityを
    /// 公開しないことを確認する。
    ///
    #[test]
    fn mcp_server_does_not_publish_prompts_when_name_index_is_not_ready() {
        /*
         * 構築済みマーカーを削除した共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .set_mcp_primitive_name_state_for_test(None)
            .expect("remove readiness marker failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));

        /*
         * server readinessとcapabilityを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let info = server.get_info();

        assert!(!server.prompts_ready);
        assert!(info.capabilities.prompts.is_none());
        assert!(info.capabilities.tools.is_some());
    }

    ///
    /// 名前索引構築済みの場合にprompts capabilityを
    /// 公開することを確認する。
    ///
    /// # 注記
    /// 通常初期化したDBからserver情報を生成し、
    /// promptsとlistChangedおよびtoolsを検証する。
    ///
    #[test]
    fn mcp_server_publishes_prompts_when_name_index_is_ready() {
        /*
         * 名前索引構築済みの共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));

        /*
         * prompts capabilityと通知非対応を確認する
         */
        let server = LuwikiMcpServer::new(state);
        let info = server.get_info();
        let prompts = info
            .capabilities
            .prompts
            .expect("prompts capability missing");

        assert!(server.prompts_ready);
        assert_eq!(prompts.list_changed, None);
        assert!(info.capabilities.tools.is_some());
    }

    ///
    /// resource URI索引未構築時にresources capabilityを
    /// 公開しないことを確認する。
    ///
    #[test]
    fn mcp_server_does_not_publish_resources_when_uri_index_is_not_ready() {
        /*
         * resource URI索引未構築の共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));

        /*
         * server readinessとcapabilityを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let info = server.get_info();

        assert!(!server.resources_ready);
        assert!(info.capabilities.resources.is_none());
        assert!(info.capabilities.tools.is_some());
    }

    ///
    /// resource URI索引構築済みの場合にresources capabilityを
    /// 公開することを確認する。
    ///
    /// # 注記
    /// resource再構成でreadinessを立てたDBからserver情報を生成し、
    /// resourcesとlistChangedおよびtoolsを検証する。
    ///
    #[test]
    fn mcp_server_publishes_resources_when_uri_index_is_ready() {
        /*
         * resource URI索引構築済みの共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));

        /*
         * resources capabilityと通知非対応を確認する
         */
        let server = LuwikiMcpServer::new(state);
        let info = server.get_info();
        let resources = info
            .capabilities
            .resources
            .expect("resources capability missing");

        assert!(server.resources_ready);
        assert_eq!(resources.list_changed, None);
        assert!(info.capabilities.tools.is_some());
    }

    ///
    /// tools、prompts、resources capabilityが
    /// 同時に公開されることを確認する。
    ///
    /// # 注記
    /// prompt名前索引とresource URI索引がreadyな共有状態から
    /// server情報を生成し、capability共存と通知非対応を検証する。
    ///
    #[test]
    fn mcp_server_publishes_tools_prompts_and_resources_together() {
        /*
         * prompts/resources readinessが揃った共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));

        /*
         * 3 capabilityの共存とlistChanged非宣言を確認する
         */
        let server = LuwikiMcpServer::new(state);
        let info = server.get_info();
        let prompts = info
            .capabilities
            .prompts
            .expect("prompts capability missing");
        let resources = info
            .capabilities
            .resources
            .expect("resources capability missing");

        assert!(server.prompts_ready);
        assert!(server.resources_ready);
        assert!(info.capabilities.tools.is_some());
        assert_eq!(prompts.list_changed, None);
        assert_eq!(resources.list_changed, None);
    }

    ///
    /// rmcp標準のprompts/list handlerが
    /// prompt一覧処理へ接続されることを確認する。
    ///
    /// # 注記
    /// promptページを持つ共有状態とread scopeを準備し、
    /// handler共通処理からrmcp一覧結果を取得する。
    ///
    #[test]
    fn mcp_server_list_prompts_connects_standard_handler() {
        /*
         * prompt候補を持つ共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/list",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: list-prompt\n",
                    "  description: 一覧接続\n",
                    "---\n",
                    "本文",
                )
                .to_string(),
            )
            .expect("create prompt page failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 標準handlerと同じ一覧処理の接続を確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_prompts_for_auth(&auth, None)
            .expect("list prompts failed");

        assert_eq!(result.prompts.len(), 1);
        assert_eq!(result.prompts[0].name, "list-prompt");
        assert_eq!(
            result.prompts[0].description.as_deref(),
            Some("一覧接続"),
        );
        assert_eq!(result.next_cursor, None);
    }

    ///
    /// rmcp標準のresources/list handlerが
    /// resource一覧処理へ接続されることを確認する。
    ///
    /// # 注記
    /// resourceページを持つ共有状態とread scopeを準備し、
    /// handler共通処理からrmcp一覧結果を取得する。
    ///
    #[test]
    fn mcp_server_list_resources_connects_standard_handler() {
        /*
         * resource候補を持つ共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/resources/list",
                "user",
                resource_source_for_list("docs/list", "list-resource"),
            )
            .expect("create resource page failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 標準handlerと同じ一覧処理の接続を確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_resources_for_auth(&auth, None)
            .expect("list resources failed");
        let uris = result
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();

        assert!(server.resources_ready);
        assert!(uris.contains(
            &"luwiki://local.luwiki/page/docs/list"
        ));
        assert_eq!(result.next_cursor, None);
    }

    ///
    /// resources/listがread scopeを要求し、
    /// ReadOnly属性では拒否しないことを確認する。
    ///
    /// # 注記
    /// append scope、read scope、ReadOnly付きread scopeで
    /// 同じ一覧処理を実行する。
    ///
    #[test]
    fn mcp_server_list_resources_requires_read_scope_and_allows_read_only() {
        /*
         * 空のresource一覧を持つ共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);

        /*
         * read scopeなしの要求が拒否されることを確認する
         */
        let append_only = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let error = server
            .list_resources_for_auth(&append_only, None)
            .expect_err("append-only scope must be denied");
        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        assert_eq!(error.message, "operation is not allowed");
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("code"))
                .and_then(serde_json::Value::as_str),
            Some("forbidden"),
        );

        /*
         * read scopeとReadOnly属性が許可されることを確認する
         */
        let read = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        assert!(server.list_resources_for_auth(&read, None).is_ok());

        let mut attributes = UserAttributeSet::new();
        attributes.insert(UserAttribute::ReadOnly);
        let read_only = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            attributes,
            None,
        );
        assert!(server
            .list_resources_for_auth(&read_only, None)
            .is_ok());
    }

    ///
    /// resources/listがページ由来resourceへ
    /// path prefix制約を適用することを確認する。
    ///
    /// # 注記
    /// 許可prefix内外のresourceページを準備し、
    /// 固定組み込みresourceと許可済みページ由来resourceだけを返す。
    ///
    #[test]
    fn mcp_server_list_resources_applies_page_path_prefix() {
        /*
         * path制約内外のresourceページを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/allowed/resource",
                "user",
                resource_source_for_list(
                    "docs/allowed",
                    "allowed-resource",
                ),
            )
            .expect("create allowed resource failed");
        manager
            .create_page(
                "/blocked/resource",
                "user",
                resource_source_for_list(
                    "docs/blocked",
                    "blocked-resource",
                ),
            )
            .expect("create blocked resource failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );

        /*
         * path prefix制約でページ由来resourceだけ絞られることを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_resources_for_auth(&auth, None)
            .expect("list resources failed");
        let uris = result
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();

        assert!(uris.contains(
            &"luwiki://local.luwiki/builtin/front-matter-spec"
        ));
        assert!(uris.contains(
            &"luwiki://local.luwiki/page/docs/allowed"
        ));
        assert!(!uris.contains(
            &"luwiki://local.luwiki/page/docs/blocked"
        ));
    }

    ///
    /// resources/listが公開不能なページ由来候補を
    /// 一覧へ出さないことを確認する。
    ///
    /// # 注記
    /// soft delete、draft、orphan候補を直接混入させ、
    /// 固定組み込みresourceと公開可能なページ由来resourceだけを返す。
    ///
    #[test]
    fn mcp_server_list_resources_filters_unavailable_page_candidates() {
        /*
         * 公開可否が異なるresource候補を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let _visible_id = manager
            .create_page(
                "/resources/visible",
                "user",
                resource_source_for_list(
                    "docs/visible",
                    "visible-resource",
                ),
            )
            .expect("create visible resource failed");
        let deleted_id = manager
            .create_page(
                "/resources/deleted",
                "user",
                resource_source_for_list(
                    "docs/deleted",
                    "deleted-resource",
                ),
            )
            .expect("create deleted resource failed");
        manager
            .delete_page_by_id(&deleted_id)
            .expect("soft delete resource failed");
        let draft_id = manager
            .create_draft_page("/resources/draft", "user")
            .expect("create draft failed")
            .0;
        let orphan_id = PageId::new();
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        manager
            .insert_resource_candidate_for_test(
                &draft_id,
                &ResourceCandidateEntry::new(
                    "docs/draft".to_string(),
                    "draft-resource".to_string(),
                    "draft-resource description".to_string(),
                    None,
                ),
            )
            .expect("insert draft candidate failed");
        manager
            .insert_resource_candidate_for_test(
                &orphan_id,
                &ResourceCandidateEntry::new(
                    "docs/orphan".to_string(),
                    "orphan-resource".to_string(),
                    "orphan-resource description".to_string(),
                    None,
                ),
            )
            .expect("insert orphan candidate failed");

        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 公開可能なresourceだけが一覧へ出ることを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_resources_for_auth(&auth, None)
            .expect("list resources failed");
        let uris = result
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();
        let names = result
            .resources
            .iter()
            .map(|resource| resource.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            uris,
            vec![
                "luwiki://local.luwiki/builtin/front-matter-spec",
                "luwiki://local.luwiki/builtin/mcp-prompt-spec",
                "luwiki://local.luwiki/page/docs/visible",
            ],
        );
        assert_eq!(result.next_cursor, None);
        assert!(names.contains(&"visible-resource"));
        assert!(!names.contains(&"deleted-resource"));
        assert!(!names.contains(&"draft-resource"));
        assert!(!names.contains(&"orphan-resource"));
    }

    ///
    /// resources/listが固定組み込みresourceとページ由来resourceを
    /// 同じrmcp Resource一覧へマッピングすることを確認する。
    ///
    /// # 注記
    /// URI昇順の合流結果とM4初期版の公開フィールドを検証する。
    ///
    #[test]
    fn mcp_server_list_resources_maps_builtin_and_page_resources() {
        /*
         * ページ由来resourceを持つ共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/resources/mapping",
                "user",
                resource_source_for_list_with_mime_type(
                    "docs/mapping",
                    "mapped-resource",
                    "application/json",
                ),
            )
            .expect("create resource page failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 固定組み込みresourceとページ由来resourceの
         * 合流およびfield mappingを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_resources_for_auth(&auth, None)
            .expect("list resources failed");
        assert_eq!(result.resources.len(), 3);
        assert_eq!(result.next_cursor, None);

        let front_matter_spec = &result.resources[0];
        assert_eq!(
            front_matter_spec.uri,
            "luwiki://local.luwiki/builtin/front-matter-spec",
        );
        assert_eq!(
            front_matter_spec.name,
            "Front Matter Specification",
        );
        assert_eq!(
            front_matter_spec.description.as_deref(),
            Some("LuWiki front matter specification"),
        );
        assert_eq!(
            front_matter_spec.mime_type.as_deref(),
            Some("text/markdown"),
        );
        assert_eq!(front_matter_spec.title, None);
        assert_eq!(front_matter_spec.size, None);
        assert_eq!(front_matter_spec.icons, None);
        assert_eq!(front_matter_spec.meta, None);
        assert_eq!(front_matter_spec.annotations, None);

        let prompt_spec = &result.resources[1];
        assert_eq!(
            prompt_spec.uri,
            "luwiki://local.luwiki/builtin/mcp-prompt-spec",
        );
        assert_eq!(prompt_spec.name, "MCP Prompt Specification");
        assert_eq!(
            prompt_spec.description.as_deref(),
            Some("LuWiki MCP prompt specification"),
        );
        assert_eq!(
            prompt_spec.mime_type.as_deref(),
            Some("text/markdown"),
        );
        assert_eq!(prompt_spec.title, None);
        assert_eq!(prompt_spec.size, None);
        assert_eq!(prompt_spec.icons, None);
        assert_eq!(prompt_spec.meta, None);
        assert_eq!(prompt_spec.annotations, None);

        let page_resource = &result.resources[2];
        assert_eq!(
            page_resource.uri,
            "luwiki://local.luwiki/page/docs/mapping",
        );
        assert_eq!(page_resource.name, "mapped-resource");
        assert_eq!(
            page_resource.description.as_deref(),
            Some("mapped-resource description"),
        );
        assert_eq!(
            page_resource.mime_type.as_deref(),
            Some("application/json"),
        );
        assert_eq!(page_resource.title, None);
        assert_eq!(page_resource.size, None);
        assert_eq!(page_resource.icons, None);
        assert_eq!(page_resource.meta, None);
        assert_eq!(page_resource.annotations, None);
    }

    ///
    /// resources/readが固定組み込みresource本文を返すことを
    /// 確認する。
    ///
    /// # 注記
    /// 固定組み込みresourceをURIから取得し、text contentsと
    /// MIME typeを検証する。
    ///
    #[test]
    fn mcp_server_read_resource_returns_builtin_contents() {
        /*
         * resource基盤がreadyな共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/private"]),
            None,
        );

        /*
         * 固定組み込みresourceはpath prefix制約なしで取得できる
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .read_resource_for_auth(
                &auth,
                "luwiki://local.luwiki/builtin/front-matter-spec",
            )
            .expect("read builtin resource failed");
        assert_eq!(result.contents.len(), 1);

        match &result.contents[0] {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                meta,
            } => {
                assert_eq!(
                    uri,
                    "luwiki://local.luwiki/builtin/front-matter-spec",
                );
                assert_eq!(mime_type.as_deref(), Some("text/markdown"));
                assert!(text.contains("# Front Matter 基本仕様"));
                assert_eq!(meta, &None);
            }
            ResourceContents::BlobResourceContents { .. } => {
                panic!("builtin resource must be text contents");
            }
        }

        /*
         * 固定組み込みresourceは候補テーブルに依存せず一覧にも残る
         */
        let listed = server
            .list_resources_for_auth(&auth, None)
            .expect("list builtin resources failed");
        let listed_uris = listed
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            listed_uris,
            vec![
                "luwiki://local.luwiki/builtin/front-matter-spec",
                "luwiki://local.luwiki/builtin/mcp-prompt-spec",
            ],
        );
        let prompt_spec = server
            .read_resource_for_auth(
                &auth,
                "luwiki://local.luwiki/builtin/mcp-prompt-spec",
            )
            .expect("read builtin prompt spec failed");
        assert_eq!(prompt_spec.contents.len(), 1);
    }

    ///
    /// resources/readがread scopeを要求し、
    /// ReadOnly属性では拒否しないことを確認する。
    ///
    /// # 注記
    /// append scope、read scope、ReadOnly付きread scopeで
    /// 固定組み込みresourceの取得を実行する。
    ///
    #[test]
    fn mcp_server_read_resource_requires_read_scope_and_allows_read_only() {
        /*
         * resource基盤がreadyな共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let uri = "luwiki://local.luwiki/builtin/front-matter-spec";

        /*
         * read scopeなしの要求が拒否されることを確認する
         */
        let append_only = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let error = server
            .read_resource_for_auth(&append_only, uri)
            .expect_err("append-only scope must be denied");
        assert_resource_protocol_error(
            &error,
            ErrorCode::INVALID_REQUEST,
            "operation is not allowed",
            "forbidden",
        );

        /*
         * read scopeとReadOnly属性が許可されることを確認する
         */
        let read = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        assert!(server.read_resource_for_auth(&read, uri).is_ok());

        let read_only = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
        );
        assert!(server
            .read_resource_for_auth(&read_only, uri)
            .is_ok());
    }

    ///
    /// resources/readがdraft resourceを秘匿することを確認する。
    ///
    /// # 注記
    /// draftページを指すURI索引と候補を直接混入させ、
    /// read時には不存在と同じ公開エラーへ変換する。
    ///
    #[test]
    fn mcp_server_read_resource_hides_draft_resource() {
        /*
         * draftを指すresource URI索引を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let draft_id = manager
            .create_draft_page("/secret/draft-resource", "user")
            .expect("create draft failed")
            .0;
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        manager
            .insert_resource_candidate_for_test(
                &draft_id,
                &ResourceCandidateEntry::new(
                    "docs/draft-read".to_string(),
                    "draft-read-resource".to_string(),
                    "draft read description".to_string(),
                    None,
                ),
            )
            .expect("insert draft resource candidate failed");
        manager
            .set_resource_uri_owner_for_test(
                "docs/draft-read",
                Some(&draft_id),
            )
            .expect("insert draft resource uri owner failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * draft resourceは秘匿済みnot foundへ寄せる
         */
        let server = LuwikiMcpServer::new(state);
        let error = server
            .read_resource_for_auth(
                &auth,
                "luwiki://local.luwiki/page/docs/draft-read",
            )
            .expect_err("draft resource must be hidden");
        assert_resource_protocol_error(
            &error,
            ErrorCode::INVALID_PARAMS,
            "resource not found",
            "not_found",
        );
        let serialized = serde_json::to_string(&error)
            .expect("serialize protocol error failed");
        assert!(!serialized.contains("/secret/draft-resource"));
        assert!(!serialized.contains("docs/draft-read"));
        assert!(!serialized.contains("draft-read-resource"));
        assert!(!serialized.contains("draft read description"));
        assert!(!serialized.contains(&draft_id.to_string()));
    }

    ///
    /// resources/readがページ由来resource本文を返すことを
    /// 確認する。
    ///
    /// # 注記
    /// URI索引から最新ページソースを解決し、front matter除去後の
    /// raw Markdown本文、MIME type、path prefix制約を検証する。
    ///
    #[test]
    fn mcp_server_read_resource_returns_page_contents() {
        /*
         * ページ由来resourceを持つ共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/allowed/read-resource",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_id: docs/read\n",
                    "  name: read-resource\n",
                    "  description: read resource description\n",
                    "  mime_type: text/plain\n",
                    "---\n",
                    "# Resource Body\n",
                    "\n",
                    "本文\n",
                )
                .to_string(),
            )
            .expect("create resource page failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let allowed = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );
        let blocked = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/blocked"]),
            None,
        );

        /*
         * 許可prefix内ではfront matter除去後本文を取得できる
         */
        let result = server
            .read_resource_for_auth(
                &allowed,
                "luwiki://local.luwiki/page/docs/read",
            )
            .expect("read page resource failed");
        assert_eq!(result.contents.len(), 1);
        match &result.contents[0] {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                meta,
            } => {
                assert_eq!(uri, "luwiki://local.luwiki/page/docs/read");
                assert_eq!(mime_type.as_deref(), Some("text/plain"));
                assert_eq!(text, "# Resource Body\n\n本文\n");
                assert_eq!(meta, &None);
            }
            ResourceContents::BlobResourceContents { .. } => {
                panic!("page resource must be text contents");
            }
        }

        /*
         * path prefix制約外では秘匿優先のnot foundへ寄せる
         */
        let error = server
            .read_resource_for_auth(
                &blocked,
                "luwiki://local.luwiki/page/docs/read",
            )
            .expect_err("blocked resource must be hidden");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "resource not found");
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("code"))
                .and_then(serde_json::Value::as_str),
            Some("not_found"),
        );
    }

    ///
    /// resources/readが状態とURIエラーを
    /// 固定protocol errorへ変換することを確認する。
    ///
    /// # 注記
    /// soft delete、hard delete、URI形式不正、authority不一致を
    /// 外部仕様どおり秘匿済みエラーへ変換する。
    ///
    #[test]
    fn mcp_server_read_resource_maps_state_and_uri_errors() {
        /*
         * 状態遷移用のresource正本を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let soft_deleted_id = manager
            .create_page(
                "/secret/soft-resource",
                "alice",
                resource_source_for_list("docs/soft", "soft-resource"),
            )
            .expect("create soft deleted resource failed");
        manager
            .delete_page_by_id(&soft_deleted_id)
            .expect("soft delete resource failed");
        let hard_deleted_id = manager
            .create_page(
                "/secret/hard-resource",
                "alice",
                resource_source_for_list("docs/hard", "hard-resource"),
            )
            .expect("create hard deleted resource failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        manager
            .delete_page_by_id(&hard_deleted_id)
            .expect("soft delete hard resource failed");
        manager
            .delete_page_by_id_hard(&hard_deleted_id)
            .expect("hard delete resource failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 非公開状態と不存在は同じnot_foundへ寄せる
         */
        for uri in [
            "luwiki://local.luwiki/page/docs/soft",
            "luwiki://local.luwiki/page/docs/hard",
            "luwiki://other.luwiki/page/docs/soft",
            "luwiki://local.luwiki/page/docs/missing",
        ] {
            let error = server
                .read_resource_for_auth(&auth, uri)
                .expect_err("unavailable resource must fail");
            assert_resource_protocol_error(
                &error,
                ErrorCode::INVALID_PARAMS,
                "resource not found",
                "not_found",
            );
            let serialized = serde_json::to_string(&error)
                .expect("serialize protocol error failed");
            assert!(!serialized.contains(uri));
            assert!(!serialized.contains("/secret/"));
            assert!(!serialized.contains(&soft_deleted_id.to_string()));
            assert!(!serialized.contains(&hard_deleted_id.to_string()));
            assert!(!serialized.contains("soft delete"));
            assert!(!serialized.contains("hard delete"));
        }

        /*
         * URI形式不正はinvalid_inputへ変換する
         */
        let invalid = server
            .read_resource_for_auth(&auth, "luwiki://local.luwiki/page/ bad")
            .expect_err("invalid resource uri must fail");
        assert_resource_protocol_error(
            &invalid,
            ErrorCode::INVALID_PARAMS,
            "resource uri is invalid",
            "invalid_input",
        );
        let serialized_invalid = serde_json::to_string(&invalid)
            .expect("serialize invalid uri error failed");
        assert!(!serialized_invalid.contains(" bad"));
    }

    ///
    /// resources/readがlatest source不整合を
    /// internal errorへ変換することを確認する。
    ///
    /// # 注記
    /// URI索引を維持したままlatest sourceのresource_idだけを変え、
    /// 本文、path、page IDが公開エラーへ混入しないことを検証する。
    ///
    #[test]
    fn mcp_server_read_resource_maps_consistency_error() {
        /*
         * 不整合化するresource正本と共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/secret/inconsistent-resource",
                "alice",
                resource_source_for_list(
                    "docs/original",
                    "inconsistent-resource",
                ),
            )
            .expect("create inconsistent resource failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        manager
            .replace_latest_page_source_for_resource_rebuild_test(
                &page_id,
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_id: docs/changed\n",
                    "  name: changed-resource\n",
                    "  description: changed resource description\n",
                    "---\n",
                    "secret-body",
                )
                .to_string(),
            )
            .expect("replace latest source failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * latest source再検証の不整合は内部エラーへ寄せる
         */
        let error = server
            .read_resource_for_auth(
                &auth,
                "luwiki://local.luwiki/page/docs/original",
            )
            .expect_err("resource_id mismatch must fail");
        assert_resource_protocol_error(
            &error,
            ErrorCode::INTERNAL_ERROR,
            "internal error",
            "internal_error",
        );
        let serialized = serde_json::to_string(&error)
            .expect("serialize protocol error failed");
        assert!(!serialized.contains("/secret/inconsistent-resource"));
        assert!(!serialized.contains("docs/changed"));
        assert!(!serialized.contains("changed-resource"));
        assert!(!serialized.contains("changed resource description"));
        assert!(!serialized.contains("secret-body"));
        assert!(!serialized.contains(&page_id.to_string()));
    }

    ///
    /// resources/listがcase-sensitiveなURI順と
    /// cursor境界を使用することを確認する。
    ///
    /// # 注記
    /// 固定組み込みresource、大小文字、Unicodeを含むページ由来resourceで
    /// Rustの文字列比較順を検証する。
    ///
    #[test]
    fn mcp_server_list_resources_uses_uri_order() {
        /*
         * 比較規則を判別できるresourceを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        for (path, resource_id, name) in [
            ("/resources/zeta", "docs/zeta", "zeta"),
            ("/resources/alpha-upper", "Docs/alpha", "alpha-upper"),
            ("/resources/japanese", "docs/日本", "japanese"),
        ] {
            manager
                .create_page(
                    path,
                    "user",
                    resource_source_for_list(resource_id, name),
                )
                .expect("create resource page failed");
        }
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);

        /*
         * 全件順序と実在しないcursor境界を確認する
         */
        let all = server
            .list_resources_for_auth(&auth, None)
            .expect("list all resources failed");
        let uris = all
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            uris,
            vec![
                "luwiki://local.luwiki/builtin/front-matter-spec",
                "luwiki://local.luwiki/builtin/mcp-prompt-spec",
                "luwiki://local.luwiki/page/Docs/alpha",
                "luwiki://local.luwiki/page/docs/zeta",
                "luwiki://local.luwiki/page/docs/日本",
            ],
        );

        let after_missing = server
            .list_resources_for_auth(
                &auth,
                Some("luwiki://local.luwiki/page/docs/a"),
            )
            .expect("list after missing cursor failed");
        let uris = after_missing
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            uris,
            vec![
                "luwiki://local.luwiki/page/docs/zeta",
                "luwiki://local.luwiki/page/docs/日本",
            ],
        );
        assert_eq!(after_missing.next_cursor, None);
    }

    ///
    /// resources/listが50件単位で
    /// ページングすることを確認する。
    ///
    /// # 注記
    /// 固定組み込み2件とページ由来52件を作成し、
    /// 先頭50件と残り4件のcursorを検証する。
    ///
    #[test]
    fn mcp_server_list_resources_pages_by_fifty() {
        /*
         * URI順が明確な52件のページ由来resourceを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        for index in 0..52 {
            let resource_id = format!("docs/resource-{:03}", index);
            let name = format!("resource-{:03}", index);
            manager
                .create_page(
                    format!("/resources/{}", index),
                    "user",
                    resource_source_for_list(&resource_id, &name),
                )
                .expect("create resource page failed");
        }
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);

        /*
         * 先頭50件と残り4件をcursorで継続取得する
         */
        let first = server
            .list_resources_for_auth(&auth, None)
            .expect("list first page failed");
        assert_eq!(first.resources.len(), 50);
        assert_eq!(
            first.resources[0].uri,
            "luwiki://local.luwiki/builtin/front-matter-spec",
        );
        assert_eq!(
            first.resources[1].uri,
            "luwiki://local.luwiki/builtin/mcp-prompt-spec",
        );
        assert_eq!(
            first.resources[49].uri,
            "luwiki://local.luwiki/page/docs/resource-047",
        );
        assert_eq!(
            first.next_cursor.as_deref(),
            Some("luwiki://local.luwiki/page/docs/resource-047"),
        );

        let second = server
            .list_resources_for_auth(
                &auth,
                first.next_cursor.as_deref(),
            )
            .expect("list second page failed");
        let uris = second
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            uris,
            vec![
                "luwiki://local.luwiki/page/docs/resource-048",
                "luwiki://local.luwiki/page/docs/resource-049",
                "luwiki://local.luwiki/page/docs/resource-050",
                "luwiki://local.luwiki/page/docs/resource-051",
            ],
        );
        assert_eq!(second.next_cursor, None);
    }

    ///
    /// resources/listが不正cursorを拒否することを確認する。
    ///
    /// # 注記
    /// resource URIとして不正な境界値と、最大URIより後ろの
    /// 正常な境界値を検証する。
    ///
    #[test]
    fn mcp_server_list_resources_rejects_invalid_cursor() {
        /*
         * cursor検証用の共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .rebuild_resource_candidates()
            .expect("rebuild resource candidates failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);
        let too_long = format!(
            "luwiki://local.luwiki/page/{}",
            "x".repeat(513),
        );

        /*
         * resource URIとして不正なcursorを拒否する
         */
        for cursor in [
            "",
            " leading",
            "trailing ",
            "\n",
            "http://local.luwiki/page/docs/a",
            "luwiki://other/page/docs/a",
            "luwiki://local.luwiki/other/docs/a",
            "luwiki://local.luwiki/page/",
            "luwiki://local.luwiki/page/builtin/spec",
            "luwiki://local.luwiki/builtin/",
            "luwiki://local.luwiki/builtin/a/b",
        ] {
            let error = server
                .list_resources_for_auth(&auth, Some(cursor))
                .expect_err("invalid cursor must be rejected");
            assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
            assert_eq!(error.message, "cursor is invalid");
        }
        let error = server
            .list_resources_for_auth(&auth, Some(&too_long))
            .expect_err("too long cursor must be rejected");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "cursor is invalid");

        /*
         * 最大URIより後ろの正常cursorは空一覧として処理する
         */
        let result = server
            .list_resources_for_auth(
                &auth,
                Some("luwiki://local.luwiki/page/zzzz"),
            )
            .expect("valid missing cursor failed");
        assert!(result.resources.is_empty());
        assert_eq!(result.next_cursor, None);
    }

    ///
    /// resources/listが論理エラーを固定protocol errorへ
    /// 変換することを確認する。
    ///
    /// # 注記
    /// scope不足、cursor不正、候補重複を発生させ、
    /// code・message・dataを検証する。
    ///
    #[test]
    fn mcp_server_list_resources_maps_protocol_errors() {
        /*
         * 内部不整合を発生可能なresource候補を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/resources/first",
                "user",
                resource_source_for_list("docs/first", "first-resource"),
            )
            .expect("create first resource failed");
        let second_id = manager
            .create_page(
                "/resources/second",
                "user",
                resource_source_for_list("docs/second", "second-resource"),
            )
            .expect("create second resource failed");
        manager
            .insert_resource_candidate_for_test(
                &second_id,
                &ResourceCandidateEntry::new(
                    "docs/first".to_string(),
                    "duplicate-resource".to_string(),
                    "secret duplicate description".to_string(),
                    None,
                ),
            )
            .expect("insert duplicate candidate failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let read = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * scope不足とcursor不正の固定公開形式を確認する
         */
        let append_only = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let forbidden = server
            .list_resources_for_auth(&append_only, None)
            .expect_err("scope denial expected");
        assert_resource_protocol_error(
            &forbidden,
            ErrorCode::INVALID_REQUEST,
            "operation is not allowed",
            "forbidden",
        );

        let invalid = server
            .list_resources_for_auth(&read, Some("secret-cursor"))
            .expect_err("invalid cursor expected");
        assert_resource_protocol_error(
            &invalid,
            ErrorCode::INVALID_PARAMS,
            "cursor is invalid",
            "invalid_input",
        );
        let serialized_invalid = serde_json::to_string(&invalid)
            .expect("serialize invalid cursor error failed");
        assert!(!serialized_invalid.contains("secret-cursor"));

        /*
         * 候補不整合の固定internal error形式を確認する
         */
        let internal = server
            .list_resources_for_auth(&read, None)
            .expect_err("internal inconsistency expected");
        assert_resource_protocol_error(
            &internal,
            ErrorCode::INTERNAL_ERROR,
            "internal error",
            "internal_error",
        );
        let serialized = serde_json::to_string(&internal)
            .expect("serialize protocol error failed");
        assert!(!serialized.contains("/resources/"));
        assert!(!serialized.contains("docs/first"));
        assert!(!serialized.contains("duplicate-resource"));
        assert!(!serialized.contains("secret duplicate description"));
        assert!(!serialized.contains("duplicate resource"));
    }

    ///
    /// prompts/listが決定済みのprompt公開フィールドへ
    /// マッピングすることを確認する。
    ///
    /// # 注記
    /// 全属性を持つpromptと引数未指定promptを列挙し、
    /// rmcp型の全公開フィールドを検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_maps_decided_prompt_fields() {
        /*
         * field mapping用のprompt候補を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/arguments",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: arguments-prompt\n",
                    "  description: 引数あり\n",
                    "  system: 非公開system\n",
                    "  arguments:\n",
                    "    - name: first\n",
                    "      description: 第一引数\n",
                    "    - name: second\n",
                    "      description: 第二引数\n",
                    "      required: false\n",
                    "    - name: third\n",
                    "      description: 第三引数\n",
                    "      required: true\n",
                    "---\n",
                    "{{@first}} {{@second}} {{@third}}",
                )
                .to_string(),
            )
            .expect("create arguments prompt failed");
        manager
            .create_page(
                "/prompts/no-arguments",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: no-arguments\n",
                    "  description: 引数なし\n",
                    "---\n",
                    "本文",
                )
                .to_string(),
            )
            .expect("create no arguments prompt failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * rmcp promptとargumentの全公開フィールドを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_prompts_for_auth(&auth, None)
            .expect("list prompts failed");
        assert_eq!(result.prompts.len(), 2);
        assert_eq!(result.meta, None);
        assert_eq!(result.next_cursor, None);

        let prompt = &result.prompts[0];
        assert_eq!(prompt.name, "arguments-prompt");
        assert_eq!(prompt.description.as_deref(), Some("引数あり"));
        assert_eq!(prompt.title, None);
        assert_eq!(prompt.icons, None);
        assert_eq!(prompt.meta, None);
        let arguments = prompt
            .arguments
            .as_ref()
            .expect("prompt arguments missing");
        assert_eq!(arguments.len(), 3);
        assert_eq!(arguments[0].name, "first");
        assert_eq!(
            arguments[0].description.as_deref(),
            Some("第一引数"),
        );
        assert_eq!(arguments[0].required, None);
        assert_eq!(arguments[0].title, None);
        assert_eq!(arguments[1].name, "second");
        assert_eq!(
            arguments[1].description.as_deref(),
            Some("第二引数"),
        );
        assert_eq!(arguments[1].required, Some(false));
        assert_eq!(arguments[1].title, None);
        assert_eq!(arguments[2].name, "third");
        assert_eq!(
            arguments[2].description.as_deref(),
            Some("第三引数"),
        );
        assert_eq!(arguments[2].required, Some(true));
        assert_eq!(arguments[2].title, None);

        let no_arguments = &result.prompts[1];
        assert_eq!(no_arguments.name, "no-arguments");
        assert_eq!(
            no_arguments.description.as_deref(),
            Some("引数なし"),
        );
        assert_eq!(no_arguments.arguments, None);
        assert_eq!(no_arguments.title, None);
        assert_eq!(no_arguments.icons, None);
        assert_eq!(no_arguments.meta, None);
    }

    ///
    /// prompts/listがread scopeを要求し、
    /// ReadOnly属性では拒否しないことを確認する。
    ///
    /// # 注記
    /// append scope、read scope、ReadOnly付きread scopeで
    /// 同じ一覧処理を実行する。
    ///
    #[test]
    fn mcp_server_list_prompts_requires_read_scope_and_allows_read_only() {
        /*
         * 空のprompt一覧を持つ共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);

        /*
         * read scopeなしの要求が拒否されることを確認する
         */
        let append_only = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let error = server
            .list_prompts_for_auth(&append_only, None)
            .expect_err("append-only scope must be denied");
        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        assert_eq!(error.message, "operation is not allowed");
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("code"))
                .and_then(serde_json::Value::as_str),
            Some("forbidden"),
        );

        /*
         * read scopeとReadOnly属性が許可されることを確認する
         */
        let read = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        assert!(server.list_prompts_for_auth(&read, None).is_ok());

        let mut attributes = UserAttributeSet::new();
        attributes.insert(UserAttribute::ReadOnly);
        let read_only = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            attributes,
            None,
        );
        assert!(server
            .list_prompts_for_auth(&read_only, None)
            .is_ok());
    }

    ///
    /// prompts/listがページ用path prefixを
    /// 適用しないことを確認する。
    ///
    /// # 注記
    /// 許可prefix外のページから作成したpromptを、
    /// read scopeだけで一覧取得する。
    ///
    #[test]
    fn mcp_server_list_prompts_ignores_page_path_prefix() {
        /*
         * path制約外のpromptページを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/private/prompts/page",
                "user",
                prompt_source_for_list("outside-prefix"),
            )
            .expect("create prompt page failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let mut prefixes = PathPrefixSet::new();
        prefixes.insert("/allowed".to_string());
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            prefixes,
            None,
        );

        /*
         * path制約外のpromptが
         * 名前で公開されることを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_prompts_for_auth(&auth, None)
            .expect("list prompts failed");

        assert_eq!(result.prompts.len(), 1);
        assert_eq!(result.prompts[0].name, "outside-prefix");
        assert_eq!(result.prompts[0].meta, None);
    }

    ///
    /// prompts/listが非公開・取得不能ページを
    /// 除外することを確認する。
    ///
    /// # 注記
    /// 通常、soft delete、draft、orphan候補を準備し、
    /// 最新ページ状態との合流結果を検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_filters_unavailable_pages() {
        /*
         * ページ状態が異なるprompt候補を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/visible",
                "user",
                prompt_source_for_list("visible"),
            )
            .expect("create visible prompt failed");
        let deleted_id = manager
            .create_page(
                "/prompts/deleted",
                "user",
                prompt_source_for_list("deleted"),
            )
            .expect("create deleted prompt failed");
        manager
            .delete_page_by_id(&deleted_id)
            .expect("soft delete prompt failed");
        let draft_id = manager
            .create_draft_page("/prompts/draft", "user")
            .expect("create draft failed")
            .0;
        manager
            .insert_prompt_candidate_for_test(
                &draft_id,
                &PromptCandidateEntry::new(
                    "draft".to_string(),
                    "draft description".to_string(),
                    None,
                    Vec::new(),
                ),
            )
            .expect("insert draft candidate failed");
        manager
            .insert_prompt_candidate_for_test(
                &PageId::new(),
                &PromptCandidateEntry::new(
                    "orphan".to_string(),
                    "orphan description".to_string(),
                    None,
                    Vec::new(),
                ),
            )
            .expect("insert orphan candidate failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 公開可能な通常promptだけが残ることを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_prompts_for_auth(&auth, None)
            .expect("list prompts failed");

        assert_eq!(result.prompts.len(), 1);
        assert_eq!(result.prompts[0].name, "visible");
    }

    ///
    /// prompts/listがcase-sensitiveな名前順と
    /// cursor境界を使用することを確認する。
    ///
    /// # 注記
    /// 大文字、小文字、Unicode名と実在しないcursorで
    /// Rustの文字列比較順を検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_uses_case_sensitive_name_order() {
        /*
         * 比較規則を判別できるprompt名を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        for name in ["日本", "alpha", "Alpha"] {
            manager
                .create_page(
                    format!("/prompts/{}", name),
                    "user",
                    prompt_source_for_list(name),
                )
                .expect("create prompt page failed");
        }
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);

        /*
         * 全件順序と実在しないcursor境界を確認する
         */
        let all = server
            .list_prompts_for_auth(&auth, None)
            .expect("list all prompts failed");
        let names = all
            .prompts
            .iter()
            .map(|prompt| prompt.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Alpha", "alpha", "日本"]);

        let after_missing = server
            .list_prompts_for_auth(&auth, Some("Alphaz"))
            .expect("list after missing cursor failed");
        let names = after_missing
            .prompts
            .iter()
            .map(|prompt| prompt.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha", "日本"]);
    }

    ///
    /// prompts/listが50件単位で
    /// ページングすることを確認する。
    ///
    /// # 注記
    /// 52件を作成し、先頭50件と残り2件のcursorを検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_pages_by_fifty() {
        /*
         * 名前順が明確な52件のpromptを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        for index in 0..52 {
            let name = format!("prompt-{:03}", index);
            manager
                .create_page(
                    format!("/prompts/{}", index),
                    "user",
                    prompt_source_for_list(&name),
                )
                .expect("create prompt page failed");
        }
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);

        /*
         * 先頭50件と残り2件をcursorで継続取得する
         */
        let first = server
            .list_prompts_for_auth(&auth, None)
            .expect("list first page failed");
        assert_eq!(first.prompts.len(), 50);
        assert_eq!(first.prompts[0].name, "prompt-000");
        assert_eq!(first.prompts[49].name, "prompt-049");
        assert_eq!(
            first.next_cursor.as_deref(),
            Some("prompt-049"),
        );

        let second = server
            .list_prompts_for_auth(
                &auth,
                first.next_cursor.as_deref(),
            )
            .expect("list second page failed");
        assert_eq!(second.prompts.len(), 2);
        assert_eq!(second.prompts[0].name, "prompt-050");
        assert_eq!(second.prompts[1].name, "prompt-051");
        assert_eq!(second.next_cursor, None);
    }

    ///
    /// prompts/listが不正cursorを拒否することを確認する。
    ///
    /// # 注記
    /// prompt名として不正な境界値と、最大名より後ろの
    /// 正常な境界値を検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_rejects_invalid_cursor() {
        /*
         * cursor検証用の空一覧を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);
        let too_long = "x".repeat(129);

        /*
         * prompt名として不正なcursorを拒否する
         */
        for cursor in ["", " leading", "trailing ", "\n"] {
            let error = server
                .list_prompts_for_auth(&auth, Some(cursor))
                .expect_err("invalid cursor must be rejected");
            assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        }
        let error = server
            .list_prompts_for_auth(&auth, Some(&too_long))
            .expect_err("too long cursor must be rejected");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);

        /*
         * 実在しない正常cursorは空一覧として処理する
         */
        let result = server
            .list_prompts_for_auth(&auth, Some("zzzz"))
            .expect("valid missing cursor failed");
        assert!(result.prompts.is_empty());
        assert_eq!(result.next_cursor, None);
    }

    ///
    /// prompts/listが候補なしを正常な空一覧として返すことを
    /// 確認する。
    ///
    /// # 注記
    /// cursor未指定で空DBを一覧取得し、rmcp結果を検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_returns_empty_list() {
        /*
         * prompt候補を持たない共有状態を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 空一覧が正常結果として返ることを確認する
         */
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_prompts_for_auth(&auth, None)
            .expect("list empty prompts failed");

        assert!(result.prompts.is_empty());
        assert_eq!(result.next_cursor, None);
        assert_eq!(result.meta, None);
    }

    ///
    /// prompts/listが論理エラーを固定protocol errorへ
    /// 変換することを確認する。
    ///
    /// # 注記
    /// scope不足、cursor不正、候補重複を発生させ、
    /// code・message・dataを検証する。
    ///
    #[test]
    fn mcp_server_list_prompts_maps_protocol_errors() {
        /*
         * 内部不整合を発生可能なprompt候補を準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/first",
                "user",
                prompt_source_for_list("first"),
            )
            .expect("create first prompt failed");
        let second_id = manager
            .create_page(
                "/prompts/second",
                "user",
                prompt_source_for_list("second"),
            )
            .expect("create second prompt failed");
        manager
            .insert_prompt_candidate_for_test(
                &second_id,
                &PromptCandidateEntry::new(
                    "first".to_string(),
                    "duplicate".to_string(),
                    None,
                    Vec::new(),
                ),
            )
            .expect("insert duplicate candidate failed");
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let server = LuwikiMcpServer::new(state);
        let read = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * scope不足とcursor不正の固定公開形式を確認する
         */
        let append_only = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let forbidden = server
            .list_prompts_for_auth(&append_only, None)
            .expect_err("scope denial expected");
        assert_prompt_protocol_error(
            &forbidden,
            ErrorCode::INVALID_REQUEST,
            "operation is not allowed",
            "forbidden",
        );

        let invalid = server
            .list_prompts_for_auth(&read, Some(""))
            .expect_err("invalid cursor expected");
        assert_prompt_protocol_error(
            &invalid,
            ErrorCode::INVALID_PARAMS,
            "cursor is invalid",
            "invalid_input",
        );

        /*
         * 候補不整合の固定internal error形式を確認する
         */
        let internal = server
            .list_prompts_for_auth(&read, None)
            .expect_err("internal inconsistency expected");
        assert_prompt_protocol_error(
            &internal,
            ErrorCode::INTERNAL_ERROR,
            "internal error",
            "internal_error",
        );
        let serialized =
            serde_json::to_string(&internal).expect("serialize error failed");
        assert!(!serialized.contains("/prompts/"));
        assert!(!serialized.contains("duplicate MCP primitive name"));
    }

    ///
    /// prompt protocol errorの固定公開形式を確認する
    ///
    /// # 引数
    /// * `error` - 検証対象error
    /// * `code` - 期待するJSON-RPC code
    /// * `message` - 期待する公開message
    /// * `logical_code` - 期待するLuWiki論理code
    ///
    /// # 戻り値
    /// なし
    ///
    fn assert_prompt_protocol_error(
        error: &McpProtocolError,
        code: ErrorCode,
        message: &str,
        logical_code: &str,
    ) {
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("code"))
                .and_then(serde_json::Value::as_str),
            Some(logical_code),
        );
    }

    ///
    /// resources/read protocol errorを検証する
    ///
    /// # 引数
    /// * `error` - 検証対象エラー
    /// * `code` - 期待するJSON-RPC error code
    /// * `message` - 期待する公開message
    /// * `logical_code` - 期待するLuWiki論理code
    ///
    /// # 戻り値
    /// なし
    ///
    fn assert_resource_protocol_error(
        error: &McpProtocolError,
        code: ErrorCode,
        message: &str,
        logical_code: &str,
    ) {
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("code"))
                .and_then(serde_json::Value::as_str),
            Some(logical_code),
        );
    }

    ///
    /// 再構成したprompt候補をprompts/listから
    /// 取得できることを確認する。
    ///
    /// # 注記
    /// 候補と名前索引を欠損させ、latest sourceから
    /// 再構成した後にrmcp一覧結果を検証する。
    ///
    #[test]
    fn rebuilt_prompt_candidates_are_visible_to_prompts_list() {
        /*
         * prompt正本と欠損した派生データを準備する
         */
        let dir = tempdir().expect("create tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_path = dir.path().join("assets");
        let index_path = dir.path().join("fts");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open database failed");
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/prompts/rebuilt",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: rebuilt-prompt\n",
                    "  description: 再構成prompt\n",
                    "  arguments:\n",
                    "    - name: target\n",
                    "      description: 対象\n",
                    "      required: true\n",
                    "---\n",
                    "{{@target}}",
                )
                .to_string(),
            )
            .expect("create prompt page failed");
        manager
            .remove_prompt_candidate_by_page_id(&page_id)
            .expect("remove prompt candidate failed");
        manager
            .set_mcp_primitive_name_owner_for_test(
                crate::database::types::McpPrimitiveKind::Prompt,
                "rebuilt-prompt",
                None,
            )
            .expect("remove prompt name failed");
        assert!(manager
            .list_prompt_candidates()
            .expect("list candidates before rebuild failed")
            .is_empty());

        /*
         * latest sourceから候補と名前索引を再構成する
         */
        let count = manager
            .rebuild_prompt_candidates()
            .expect("rebuild prompt candidates failed");
        assert_eq!(count, 1);
        assert_eq!(
            manager
                .get_mcp_primitive_name_owner_for_test(
                    crate::database::types::McpPrimitiveKind::Prompt,
                    "rebuilt-prompt",
                )
                .expect("get rebuilt name owner failed"),
            Some(page_id),
        );
        assert!(manager
            .is_mcp_primitive_name_index_ready()
            .expect("get rebuilt readiness failed"));

        /*
         * MCP prompts/list公開結果を確認する
         */
        let state = Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_path),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        )));
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let server = LuwikiMcpServer::new(state);
        let result = server
            .list_prompts_for_auth(&auth, None)
            .expect("list rebuilt prompts failed");

        assert_eq!(result.prompts.len(), 1);
        let prompt = &result.prompts[0];
        assert_eq!(prompt.name, "rebuilt-prompt");
        assert_eq!(
            prompt.description.as_deref(),
            Some("再構成prompt"),
        );
        let arguments = prompt
            .arguments
            .as_ref()
            .expect("rebuilt arguments missing");
        assert_eq!(arguments.len(), 1);
        assert_eq!(arguments[0].name, "target");
        assert_eq!(arguments[0].description.as_deref(), Some("対象"));
        assert_eq!(arguments[0].required, Some(true));
    }
}
