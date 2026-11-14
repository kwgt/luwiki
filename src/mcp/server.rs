/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! RMCP へ渡す MCP server 実装を定義するモジュール
//!

mod prompts;
mod resources;

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
    ReadResourceRequestParams,
    ReadResourceResult,
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
    McpErrorResponse,
};
use crate::mcp::handler::McpHandler;
use crate::mcp::service::{DEFAULT_RESOURCE_AUTHORITY, McpService};
use crate::mcp::tools::{
    EditPageToolArgs,
    ListPageAssetsToolArgs,
    GetPageSectionToolArgs,
    GetPageTocToolArgs,
    GetPageToolArgs,
    ListPagesToolArgs,
    PageAssetByFileToolArgs,
    RenamePageToolArgs,
    SearchPagesToolArgs,
    UploadPageAssetToolArgs,
    WritePageToolArgs,
    append_page,
    create_page,
    delete_page_asset,
    edit_page,
    get_agent_instructions,
    get_page,
    get_page_asset_data,
    get_page_asset_metadata,
    get_page_section,
    get_page_toc,
    list_pages,
    list_page_assets,
    rename_page,
    resolve_page_asset,
    search_pages,
    update_page,
    upload_page_asset,
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

    /// resource URI authority
    resource_authority: String,
}

impl LuwikiMcpServer {
    ///
    /// 短いblocking状態取得に共有stateを渡す
    ///
    /// # 戻り値
    /// 非同期sleepをまたいでlockしないための共有ハンドル。
    ///
    pub(crate) fn shared_state(&self) -> Arc<RwLock<AppState>> {
        self.state.clone()
    }

    ///
    /// 停止barrierと競合しない状態要求受付を行う
    ///
    /// # 戻り値
    /// 監査終了までのlease又は停止・内部エラー。
    ///
    pub(crate) fn begin_page_state_request(
        &self,
    ) -> Result<
        crate::http_server::app_state::PageStateLease, McpProtocolError,
    > {
        let state = self.state.read().map_err(|_| {
            McpProtocolError::internal_error("internal error", None)
        })?;
        state.begin_page_state_request().ok_or_else(|| {
            McpProtocolError::internal_error("server shutting down", None)
        })
    }

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
        Self::with_resource_authority(
            state,
            DEFAULT_RESOURCE_AUTHORITY.to_string(),
        )
    }

    ///
    /// resource authorityを指定してMCP server実装を生成する
    ///
    /// # 引数
    /// * `state` - HTTP サーバ共有状態
    /// * `resource_authority` - resource URI authority
    ///
    /// # 戻り値
    /// 生成した MCP server 実装を返す。
    ///
    pub(crate) fn with_resource_authority(
        state: Arc<RwLock<AppState>>,
        resource_authority: String,
    ) -> Self {
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
            resource_authority,
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
    /// MCPハンドラを都度構築する
    ///
    /// # 概要
    /// 共有stateの監査/索引設定をサービスへ接続して返す。
    ///
    /// # 戻り値
    /// 現在の共有状態に接続した MCP ハンドラを返す。
    ///
    pub(crate) fn create_handler(&self) -> McpHandler {
        /* 共有stateから監査と同じ索引設定を取得する。 */
        let (audit_sink, fts_config) = self.state.read().ok()
            .map(|state| (
                state.audit_sink(), Some(state.fts_config().clone()),
            ))
            .unwrap_or((None, None));

        /* 公開MCPの保存サービスへ索引同期を接続する。 */
        let mut service = McpService::with_resource_authority(
            self.resource_authority.clone(),
        );
        if let Some(config) = fts_config {
            service = service.with_fts_config(config);
        }
        McpHandler::new(
            McpAuthGateway::new(),
            service,
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
                "internal error",
                None,
            )
        })?;
        if state.is_shutting_down() {
            return Err(McpProtocolError::internal_error(
                "server shutting down", None,
            ));
        }
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
                    "internal error",
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
            .map_err(|_| {
                McpProtocolError::internal_error(
                    "internal error",
                    None,
                )
            })?;
        Ok(CallToolResult::error(vec![content]))
    }

}

#[tool_router]
impl LuwikiMcpServer {
    ///
    /// 標準resourceを読めないクライアントに運用指示を返す。
    ///
    /// # 引数
    /// * `_params` - 空のtool引数
    /// * `context` - MCP要求文脈
    ///
    /// # 戻り値
    /// 運用指示の取得結果。
    ///
    #[tool(
        name = "get_agent_instructions",
        description = "LuWiki MCPの運用指示を取得する。resource読取の代替。",
        annotations(read_only_hint = true, destructive_hint = false,
            idempotent_hint = true, open_world_hint = false)
    )]
    async fn get_agent_instructions_tool(
        &self,
        _params: Parameters<get_agent_instructions::Args>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        get_agent_instructions::execute(self, context)
    }

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
    /// 軽量な最新状態を取得する
    ///
    /// # 引数
    /// * `params` - 絶対path
    /// * `context` - 認証・取消文脈
    ///
    /// # 戻り値
    /// 本文を含まない状態応答。
    ///
    #[tool(
        name = "get_page_state",
        description = "指定pathの最新状態と比較用state_tokenを取得する。",
        annotations(read_only_hint = true, destructive_hint = false,
            idempotent_hint = true, open_world_hint = false)
    )]
    async fn get_page_state_tool(
        &self,
        Parameters(args): Parameters<
            crate::mcp::tools::page_state::GetPageStateToolArgs,
        >,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        crate::mcp::handler::page_state::execute(
            self, args.path,
            crate::mcp::tools::page_state::StateRequest::Get, context,
        ).await
    }

    ///
    /// 基準状態からの変化を期限付きで待つ
    ///
    /// # 引数
    /// * `params` - path・基準token・期限
    /// * `context` - 認証・取消文脈
    ///
    /// # 戻り値
    /// changed又は正常timeout。
    ///
    #[tool(
        name = "wait_page_change",
        description = "取得済みstate_tokenから最大30秒待つ。timeoutは正常結果。同じ基準で再待機する。",
        annotations(read_only_hint = true, destructive_hint = false,
            idempotent_hint = true, open_world_hint = false)
    )]
    async fn wait_page_change_tool(
        &self,
        Parameters(args): Parameters<
            crate::mcp::tools::page_state::WaitPageChangeToolArgs,
        >,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        crate::mcp::handler::page_state::execute(
            self, args.path,
            crate::mcp::tools::page_state::StateRequest::Wait {
                expected_state: args.expected_state,
                timeout_seconds: args.timeout_seconds,
            }, context,
        ).await
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
        description = "指定した path の Markdown 本文を非空の operation 配列で部分編集する。\
            section/anchor は初期本文を参照し、全件成功後に一度保存する。\
            replace_section の見出し変更は title を使う。"
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

    ///
    /// `list_page_assets` の tool 入口
    ///
    #[tool(
        name = "list_page_assets",
        description = "入力 path で指定されたページに紐付く通常アセットのメタ情報一覧を返す。"
    )]
    async fn list_page_assets_tool(
        &self,
        params: Parameters<ListPageAssetsToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        list_page_assets::execute(self, params, context).await
    }

    ///
    /// `resolve_page_asset` の tool 入口
    ///
    #[tool(
        name = "resolve_page_asset",
        description = "入力 path で指定されたページ内で、file_name に一致する通常アセットを解決し、最小識別情報を返す。"
    )]
    async fn resolve_page_asset_tool(
        &self,
        params: Parameters<PageAssetByFileToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        resolve_page_asset::execute(self, params, context).await
    }

    ///
    /// `get_page_asset_metadata` の tool 入口
    ///
    #[tool(
        name = "get_page_asset_metadata",
        description = "入力 path で指定されたページ内で、file_name に一致する通常アセットの詳細メタ情報を返す。"
    )]
    async fn get_page_asset_metadata_tool(
        &self,
        params: Parameters<PageAssetByFileToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        get_page_asset_metadata::execute(self, params, context).await
    }

    ///
    /// `get_page_asset_data` の tool 入口
    ///
    #[tool(
        name = "get_page_asset_data",
        description = "入力 path で指定されたページ内で、file_name に一致する通常アセットのメタ情報と本体を返す。"
    )]
    async fn get_page_asset_data_tool(
        &self,
        params: Parameters<PageAssetByFileToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        get_page_asset_data::execute(self, params, context).await
    }

    ///
    /// `upload_page_asset` の tool 入口
    ///
    #[tool(
        name = "upload_page_asset",
        description = "入力 path で指定されたページへ、新規アセットをアップロードする。"
    )]
    async fn upload_page_asset_tool(
        &self,
        params: Parameters<UploadPageAssetToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        upload_page_asset::execute(self, params, context).await
    }

    ///
    /// `delete_page_asset` の tool 入口
    ///
    #[tool(
        name = "delete_page_asset",
        description = "入力 path で指定されたページ内で、file_name に一致する通常アセットを削除する。"
    )]
    async fn delete_page_asset_tool(
        &self,
        params: Parameters<PageAssetByFileToolArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpProtocolError> {
        delete_page_asset::execute(self, params, context).await
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
                "Use Bearer authentication on every HTTP request. For edit_page.replace_section, do not include the target heading line in content; use title to rename the section; use include_subsection: true when replacing subsections too.",
            )
    }
}

#[cfg(test)]
mod tests;
