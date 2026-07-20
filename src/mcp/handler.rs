/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPツール呼び出し入口と公開エラー写像を定義するモジュール
//!

use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use chrono::Utc;
use tracing::warn;

use crate::audit::AuditSink;
use crate::audit::model::{AuditOperation, AuditRecord, AuditResult};
use crate::auth::AuthContext;
use crate::database::DatabaseManager;
use crate::database::types::UserId;
use crate::fts::{FtsIndexConfig, FtsSearchTarget};
use crate::markdown_source::front_matter::{
    validate_prompt_name,
    validate_resource_path,
};
use crate::mcp::service::{
    AppendServiceResult,
    EditPageResult,
    GetPageResult,
    GetPageSectionResult,
    GetPageTocResult,
    ListPagesResult,
    SearchPagesResult,
    WritePageResult,
};

use super::auth::McpAuthGateway;
use super::errors::McpError;
use super::model::{
    AppendPageResponse,
    EditPageRequest,
    EditPageResponse,
    GetPageRequest,
    GetPageResponse,
    GetPageSectionRequest,
    GetPageSectionResponse,
    GetPageTocRequest,
    GetPageTocResponse,
    GetPromptServiceResult,
    ListPagesRequest,
    ListPagesResponse,
    ListPromptsServiceResult,
    ListResourcesServiceResult,
    McpRequestEnvelope,
    McpResponseEnvelope,
    McpToolRequest,
    McpToolResponse,
    RenamePageRequest,
    ReadResourceServiceResult,
    SearchPagesRequest,
    SearchPagesResponse,
    WritePageRequest,
    WritePageResponse,
};
use super::service::{EditPageRequest as ServiceEditPageRequest, McpService};

///
/// MCPハンドラの骨格
///
#[derive(Debug)]
pub(crate) struct McpHandler {
    /// 認証入口
    auth: McpAuthGateway,

    /// サービス入口
    service: McpService,

    /// 監査ログ投入入口
    audit_sink: Option<Arc<RwLock<AuditSink>>>,
}

impl McpHandler {
    ///
    /// MCPハンドラの生成
    ///
    /// # 戻り値
    /// 生成したハンドラを返す。
    ///
    pub(crate) fn new(
        auth: McpAuthGateway,
        service: McpService,
        audit_sink: Option<Arc<RwLock<AuditSink>>>,
    ) -> Self {
        Self {
            auth,
            service,
            audit_sink,
        }
    }

    ///
    /// MCP認証入口へのアクセサ
    ///
    /// # 戻り値
    /// 認証入口への参照を返す。
    ///
    pub(crate) fn auth(&self) -> &McpAuthGateway {
        &self.auth
    }

    ///
    /// `prompts/list`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// prompt一覧サービス結果を返す。
    ///
    pub(crate) fn handle_list_prompts(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListPromptsServiceResult, McpError> {
        match self.service.list_prompts(auth, db, cursor) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_prompts_audit_record(
                            auth,
                            address,
                            &result,
                            user_id,
                        ),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_prompts_error_audit_record(
                            auth,
                            address,
                            &error,
                            user_id,
                        ),
                    );
                }
                Err(error)
            }
        }
    }

    ///
    /// `resources/list`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// resource一覧サービス結果を返す。
    ///
    pub(crate) fn handle_list_resources(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListResourcesServiceResult, McpError> {
        match self.service.list_resources(auth, db, cursor) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_resources_audit_record(
                            auth,
                            address,
                            &result,
                            user_id,
                        ),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_resources_error_audit_record(
                            auth,
                            address,
                            &error,
                            user_id,
                        ),
                    );
                }
                Err(error)
            }
        }
    }

    ///
    /// `resources/read`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// resource取得サービス結果を返す。
    ///
    pub(crate) fn handle_read_resource(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        uri: &str,
    ) -> Result<ReadResourceServiceResult, McpError> {
        match self.service.read_resource(auth, db, uri) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_read_resource_audit_record(
                            auth,
                            address,
                            uri,
                            &result,
                            user_id,
                            self.service.resource_authority(),
                        ),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_read_resource_error_audit_record(
                            auth,
                            address,
                            uri,
                            &error,
                            user_id,
                            self.service.resource_authority(),
                        ),
                    );
                }
                Err(error)
            }
        }
    }

    ///
    /// `prompts/get`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// prompt取得サービス結果を返す。
    ///
    pub(crate) fn handle_get_prompt(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        name: &str,
        arguments: Option<
            &serde_json::Map<String, serde_json::Value>,
        >,
    ) -> Result<GetPromptServiceResult, McpError> {
        match self.service.get_prompt(auth, db, name, arguments) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_get_prompt_audit_record(
                            auth,
                            address,
                            name,
                            &result,
                            user_id,
                        ),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_get_prompt_error_audit_record(
                            auth,
                            address,
                            name,
                            &error,
                            user_id,
                        ),
                    );
                }
                Err(error)
            }
        }
    }

    ///
    /// 認証済みMCP要求を処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `fts_config` - FTS設定
    /// * `request` - 受理したMCP要求
    ///
    /// # 戻り値
    /// ツール別の応答モデルを返す。
    ///
    pub(crate) fn handle(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        fts_config: &FtsIndexConfig,
        address: Option<IpAddr>,
        request: McpRequestEnvelope,
    ) -> Result<McpResponseEnvelope, McpError> {
        /*
         * ツール別入力をサービス層へディスパッチする
         */
        let tool_name = request.tool_name();
        let response = match request.request() {
            McpToolRequest::GetPage(input) => McpToolResponse::GetPage(
                self.audit_success(
                    db,
                    auth,
                    address,
                    build_get_page_audit_record,
                    &request,
                    self.service
                    .get_page(auth, db, input.path(), input.revision())?
                )?
                .into(),
            ),
            McpToolRequest::GetPageToc(input) => {
                McpToolResponse::GetPageToc(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_get_page_toc_audit_record,
                        &request,
                        self.service
                        .get_page_toc(
                            auth,
                            db,
                            input.path(),
                            input.revision(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::ListPages(input) => {
                McpToolResponse::ListPages(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_list_pages_audit_record,
                        &request,
                        self.service
                        .list_pages(
                            auth,
                            db,
                            input.prefix(),
                            input.limit(),
                            input.cursor(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::SearchPages(input) => {
                McpToolResponse::SearchPages(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_search_pages_audit_record,
                        &request,
                        self.service
                        .search_pages(
                            auth,
                            db,
                            fts_config,
                            input.query(),
                            input.targets(),
                            input.prefix(),
                            input.limit(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::CreatePage(input) => {
                McpToolResponse::CreatePage(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_write_page_audit_record,
                        &request,
                        self.service
                        .create_page(
                            auth,
                            db,
                            input.path(),
                            input.content(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::UpdatePage(input) => {
                McpToolResponse::UpdatePage(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_write_page_audit_record,
                        &request,
                        self.service
                        .update_page(
                            auth,
                            db,
                            input.path(),
                            input.content(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::EditPage(input) => McpToolResponse::EditPage(
                self.audit_success(
                    db,
                    auth,
                    address,
                    build_edit_page_audit_record,
                    &request,
                    self.service
                    .edit_page(
                        auth,
                        db,
                        &ServiceEditPageRequest::from(input.clone()),
                    )?
                )?
                .into(),
            ),
            McpToolRequest::AppendPage(input) => {
                McpToolResponse::AppendPage(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_append_page_audit_record,
                        &request,
                        self.service
                        .append_page(
                            auth,
                            db,
                            input.path(),
                            input.content(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::RenamePage(input) => {
                McpToolResponse::RenamePage(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_rename_page_audit_record,
                        &request,
                        self.service
                        .rename_page(
                            auth,
                            db,
                            input.path(),
                            input.rename_to(),
                        )?
                    )?
                    .into(),
                )
            }
            McpToolRequest::GetPageSection(input) => {
                McpToolResponse::GetPageSection(
                    self.audit_success(
                        db,
                        auth,
                        address,
                        build_get_page_section_audit_record,
                        &request,
                        self.service
                        .get_page_section(
                            auth,
                            db,
                            input.path(),
                            input.section().clone().into(),
                            input.revision(),
                        )?
                    )?
                    .into(),
                )
            }
        };

        Ok(McpResponseEnvelope::new(tool_name, response))
    }

    ///
    /// `get_page` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// `get_page` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_get_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        revision: Option<u64>,
    ) -> Result<GetPageResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::GetPage,
            McpToolRequest::GetPage(GetPageRequest::new(
                path.to_string(),
                revision,
            )),
        );

        /*
         * `get_page` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.get_page(auth, db, path, revision) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_get_page_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `get_page_toc` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// `get_page_toc` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_get_page_toc(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        revision: Option<u64>,
    ) -> Result<GetPageTocResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::GetPageToc,
            McpToolRequest::GetPageToc(GetPageTocRequest::new(
                path.to_string(),
                revision,
            )),
        );

        /*
         * `get_page_toc` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.get_page_toc(auth, db, path, revision)
        {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_get_page_toc_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `list_pages` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `prefix` - 一覧対象 prefix
    /// * `limit` - 最大取得件数
    /// * `cursor` - 継続取得 cursor
    ///
    /// # 戻り値
    /// `list_pages` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_list_pages(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<ListPagesResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::ListPages,
            McpToolRequest::ListPages(ListPagesRequest::new(
                prefix.to_string(),
                limit,
                cursor.map(str::to_string),
            )),
        );

        /*
         * `list_pages` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.list_pages(
            auth,
            db,
            prefix,
            limit,
            cursor,
        ) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_list_pages_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `search_pages` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `fts_config` - FTS 設定
    /// * `address` - 入力元アドレス
    /// * `query` - 全文検索式
    /// * `targets` - 検索対象一覧
    /// * `prefix` - 検索対象 prefix
    /// * `limit` - 最大取得件数
    ///
    /// # 戻り値
    /// `search_pages` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_search_pages(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        fts_config: &FtsIndexConfig,
        address: Option<IpAddr>,
        query: &str,
        targets: &[FtsSearchTarget],
        prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<SearchPagesResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::SearchPages,
            McpToolRequest::SearchPages(SearchPagesRequest::new(
                query.to_string(),
                targets.to_vec(),
                prefix.map(str::to_string),
                limit,
            )),
        );

        /*
         * `search_pages` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.search_pages(
            auth,
            db,
            fts_config,
            query,
            targets,
            prefix,
            limit,
        ) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_search_pages_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `get_page_section` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `section` - セクション指定
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// `get_page_section` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_get_page_section(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        section: super::model::McpSectionSelector,
        revision: Option<u64>,
    ) -> Result<GetPageSectionResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::GetPageSection,
            McpToolRequest::GetPageSection(GetPageSectionRequest::new(
                path.to_string(),
                section.clone(),
                revision,
            )),
        );

        /*
         * `get_page_section` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.get_page_section(
            auth,
            db,
            path,
            section.into(),
            revision,
        ) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_get_page_section_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `create_page` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 作成対象 path
    /// * `content` - 初期 Markdown 本文
    ///
    /// # 戻り値
    /// `create_page` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_create_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        content: &str,
    ) -> Result<WritePageResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::CreatePage,
            McpToolRequest::CreatePage(WritePageRequest::new(
                path.to_string(),
                content.to_string(),
            )),
        );

        /*
         * `create_page` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.create_page(auth, db, path, content) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_write_page_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `update_page` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 更新対象 path
    /// * `content` - 更新後 Markdown 本文
    ///
    /// # 戻り値
    /// `update_page` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_update_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        content: &str,
    ) -> Result<WritePageResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::UpdatePage,
            McpToolRequest::UpdatePage(WritePageRequest::new(
                path.to_string(),
                content.to_string(),
            )),
        );

        /*
         * `update_page` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.update_page(auth, db, path, content) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_write_page_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `edit_page` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `request` - 編集要求
    ///
    /// # 戻り値
    /// `edit_page` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_edit_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        request: EditPageRequest,
    ) -> Result<EditPageResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::EditPage,
            McpToolRequest::EditPage(request),
        );
        let service_request = match request.request() {
            McpToolRequest::EditPage(input) => {
                ServiceEditPageRequest::from(input.clone())
            }
            _ => unreachable!("edit_page request envelope mismatch"),
        };

        /*
         * `edit_page` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.edit_page(auth, db, &service_request) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_edit_page_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `append_page` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 追記対象 path
    /// * `content` - 追記内容
    ///
    /// # 戻り値
    /// `append_page` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_append_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        content: &str,
    ) -> Result<AppendPageResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::AppendPage,
            McpToolRequest::AppendPage(WritePageRequest::new(
                path.to_string(),
                content.to_string(),
            )),
        );

        /*
         * `append_page` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.append_page(auth, db, path, content) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_append_page_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// `rename_page` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - リネーム元 path
    /// * `rename_to` - リネーム先 path
    ///
    /// # 戻り値
    /// `rename_page` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_rename_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        rename_to: &str,
    ) -> Result<WritePageResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::RenamePage,
            McpToolRequest::RenamePage(RenamePageRequest::new(
                path.to_string(),
                rename_to.to_string(),
            )),
        );

        /*
         * `rename_page` を既存 service と監査記録へ橋渡しする
         */
        let result = match self.service.rename_page(
            auth,
            db,
            path,
            rename_to,
        ) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_rename_page_audit_record,
                &request,
                result,
            )?,
            Err(error) => {
                self.record_error(db, auth, address, &request, &error);
                return Err(error);
            }
        };

        Ok(result.into())
    }

    ///
    /// 成功結果を監査ログへ記録しつつ値を返す
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `auth` - 認証文脈
    /// * `address` - 入力元アドレス
    /// * `builder` - 監査レコード生成関数
    /// * `request` - 対象要求
    /// * `result` - サービス結果
    ///
    /// # 戻り値
    /// 受け取ったサービス結果をそのまま返す。
    ///
    fn audit_success<T, F>(
        &self,
        db: &DatabaseManager,
        auth: &AuthContext,
        address: Option<IpAddr>,
        builder: F,
        request: &McpRequestEnvelope,
        result: T,
    ) -> Result<T, McpError>
    where
        F: Fn(
            &AuthContext,
            Option<IpAddr>,
            &McpRequestEnvelope,
            &T,
            UserId,
        ) -> AuditRecord,
    {
        if let Some(user_id) = resolve_audit_user_id(db, auth) {
            let record = builder(auth, address, request, &result, user_id);
            self.try_record_audit(record);
        }

        Ok(result)
    }

    ///
    /// 失敗結果を監査ログへ記録する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `auth` - 認証文脈
    /// * `address` - 入力元アドレス
    /// * `request` - 対象要求
    /// * `error` - サービス失敗
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn record_error(
        &self,
        db: &DatabaseManager,
        auth: &AuthContext,
        address: Option<IpAddr>,
        request: &McpRequestEnvelope,
        error: &McpError,
    ) {
        let Some(user_id) = resolve_audit_user_id(db, auth) else {
            return;
        };

        let record = build_error_audit_record(
            auth,
            address,
            request,
            error,
            user_id,
        );
        self.try_record_audit(record);
    }

    ///
    /// 監査レコードを投入する
    ///
    /// # 引数
    /// * `record` - 投入対象レコード
    ///
    /// # 戻り値
    /// なし
    ///
    fn try_record_audit(&self, record: AuditRecord) {
        let Some(audit_sink) = self.audit_sink.as_ref() else {
            return;
        };

        let mut sink = match audit_sink.write() {
            Ok(sink) => sink,
            Err(_) => {
                warn!("audit sink lock failed");
                return;
            }
        };
        if let Err(err) = sink.record(record) {
            warn!(error = %err, "audit log record failed");
        }
    }
}

fn resolve_audit_user_id(
    db: &DatabaseManager,
    auth: &AuthContext,
) -> Option<UserId> {
    match db.get_user_id_by_name(auth.user_id()) {
        Ok(Some(user_id)) => Some(user_id),
        Ok(None) => {
            warn!(user = auth.user_id(), "audit user id not found");
            None
        }
        Err(err) => {
            warn!(user = auth.user_id(), error = %err, "audit user id lookup failed");
            None
        }
    }
}

fn build_get_page_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &GetPageResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::Get,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        Some(result.revision()),
        None,
    )
}

fn build_get_page_toc_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &GetPageTocResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::Get,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        Some(result.revision()),
        None,
    )
}

fn build_list_pages_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    request: &McpRequestEnvelope,
    _result: &ListPagesResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::List,
        user_id,
        auth,
        address,
        primary_target_path(request),
        None,
        None,
    )
}

///
/// prompts/list成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `result` - prompt一覧結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// prompt一覧成功監査レコードを返す。
///
fn build_list_prompts_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    result: &ListPromptsServiceResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::ListPrompts,
        user_id,
        auth,
        address,
        None,
        None,
        Some(format!(
            "count={} has_more={}",
            result.items().len(),
            result.next_cursor().is_some(),
        )),
    )
}

///
/// prompts/list失敗監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `error` - prompt一覧失敗
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// prompt一覧失敗監査レコードを返す。
///
fn build_list_prompts_error_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    error: &McpError,
    user_id: UserId,
) -> AuditRecord {
    let result = audit_result_from_error(error);
    let summary = match result {
        AuditResult::ScopeDenied => "operation is not allowed",
        AuditResult::InvalidInput => "cursor is invalid",
        _ => "internal error",
    };

    AuditRecord::new(
        AuditOperation::ListPrompts,
        user_id,
        auth.token_id().cloned(),
        address,
        None,
        result,
        Utc::now(),
        Some(summary.to_string()),
        None,
    )
}

///
/// resources/list成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `result` - resource一覧結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// resource一覧成功監査レコードを返す。
///
fn build_list_resources_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    result: &ListResourcesServiceResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::ListResources,
        user_id,
        auth,
        address,
        None,
        None,
        Some(format!(
            "count={} has_more={}",
            result.items().len(),
            result.next_cursor().is_some(),
        )),
    )
}

///
/// resources/list失敗監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `error` - resource一覧失敗
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// resource一覧失敗監査レコードを返す。
///
fn build_list_resources_error_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    error: &McpError,
    user_id: UserId,
) -> AuditRecord {
    let result = audit_result_from_error(error);
    let summary = match result {
        AuditResult::ScopeDenied => "operation is not allowed",
        AuditResult::InvalidInput => "cursor is invalid",
        _ => "internal error",
    };

    AuditRecord::new(
        AuditOperation::ListResources,
        user_id,
        auth.token_id().cloned(),
        address,
        None,
        result,
        Utc::now(),
        Some(summary.to_string()),
        None,
    )
}

///
/// resources/read成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `uri` - resource URI
/// * `result` - resource取得結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// resource取得成功監査レコードを返す。
///
fn build_read_resource_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    uri: &str,
    result: &ReadResourceServiceResult,
    user_id: UserId,
    resource_authority: &str,
) -> AuditRecord {
    build_success_record(
        AuditOperation::ReadResource,
        user_id,
        auth,
        address,
        None,
        result.revision(),
        resource_uri_audit_summary(uri, resource_authority),
    )
}

///
/// resources/read失敗監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `uri` - resource URI
/// * `error` - resource取得失敗
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// resource取得失敗監査レコードを返す。
///
fn build_read_resource_error_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    uri: &str,
    error: &McpError,
    user_id: UserId,
    resource_authority: &str,
) -> AuditRecord {
    AuditRecord::new(
        AuditOperation::ReadResource,
        user_id,
        auth.token_id().cloned(),
        address,
        None,
        audit_result_from_error(error),
        Utc::now(),
        resource_uri_audit_summary(uri, resource_authority),
        None,
    )
}

///
/// prompts/get成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `name` - prompt名
/// * `result` - prompt取得結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// prompt取得成功監査レコードを返す。
///
fn build_get_prompt_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    name: &str,
    result: &GetPromptServiceResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::GetPrompt,
        user_id,
        auth,
        address,
        None,
        Some(result.revision()),
        prompt_name_audit_summary(name),
    )
}

///
/// prompts/get失敗監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `name` - prompt名
/// * `error` - prompt取得失敗
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// prompt取得失敗監査レコードを返す。
///
fn build_get_prompt_error_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    name: &str,
    error: &McpError,
    user_id: UserId,
) -> AuditRecord {
    AuditRecord::new(
        AuditOperation::GetPrompt,
        user_id,
        auth.token_id().cloned(),
        address,
        None,
        audit_result_from_error(error),
        Utc::now(),
        prompt_name_audit_summary(name),
        None,
    )
}

///
/// 監査ログへ記録可能なprompt名summaryを生成する
///
/// # 引数
/// * `name` - prompt名
///
/// # 戻り値
/// prompt名が値制約を満たす場合だけsummaryを返す。
///
fn prompt_name_audit_summary(name: &str) -> Option<String> {
    validate_prompt_name(name)
        .ok()
        .map(|_| format!("name={}", name))
}

///
/// 監査ログへ記録可能なresource URI summaryを生成する
///
/// # 引数
/// * `uri` - resource URI
///
/// # 戻り値
/// URIが値制約を満たす場合だけsummaryを返す。
///
fn resource_uri_audit_summary(
    uri: &str,
    expected_authority: &str,
) -> Option<String> {
    if uri.trim().is_empty()
        || uri.trim() != uri
        || uri.chars().any(char::is_control)
    {
        return None;
    }

    let rest = uri.strip_prefix("luwiki://")?;
    let path_start = rest.find('/')?;
    let authority = &rest[..path_start];
    if authority != expected_authority {
        return None;
    }
    let path = &rest[path_start..];

    if let Some(builtin_id) = path.strip_prefix("/builtin/") {
        if builtin_id.is_empty()
            || builtin_id.contains('/')
            || builtin_id.trim() != builtin_id
            || builtin_id.chars().any(char::is_control)
        {
            return None;
        }

        return Some(format!("uri={}", uri));
    }

    validate_resource_path(path)
        .ok()
        .map(|_| format!("uri={}", uri))
}

fn build_search_pages_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    request: &McpRequestEnvelope,
    _result: &SearchPagesResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::Search,
        user_id,
        auth,
        address,
        primary_target_path(request),
        None,
        None,
    )
}

fn build_write_page_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    request: &McpRequestEnvelope,
    result: &WritePageResult,
    user_id: UserId,
) -> AuditRecord {
    let operation = match request.request() {
        McpToolRequest::CreatePage(_) => AuditOperation::Create,
        McpToolRequest::UpdatePage(_) => AuditOperation::Update,
        _ => AuditOperation::Update,
    };

    build_success_record(
        operation,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        Some(result.revision()),
        Some(result.summary().to_string()),
    )
}

fn build_edit_page_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &EditPageResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::Update,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        Some(result.revision()),
        Some(result.summary().to_string()),
    )
}

fn build_append_page_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &AppendServiceResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::Append,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        Some(result.revision()),
        Some(result.summary().to_string()),
    )
}

fn build_rename_page_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    request: &McpRequestEnvelope,
    result: &WritePageResult,
    user_id: UserId,
) -> AuditRecord {
    let summary = if let McpToolRequest::RenamePage(input) = request.request() {
        Some(format!("rename to {}", input.rename_to()))
    } else {
        Some(result.summary().to_string())
    };

    build_success_record(
        AuditOperation::Rename,
        user_id,
        auth,
        address,
        primary_target_path(request),
        Some(result.revision()),
        summary,
    )
}

fn build_get_page_section_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &GetPageSectionResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::GetSection,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        Some(result.revision()),
        None,
    )
}

fn build_error_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    request: &McpRequestEnvelope,
    error: &McpError,
    user_id: UserId,
) -> AuditRecord {
    let operation = audit_operation(request);
    let result = audit_result_from_error(error);
    let target_path = match result {
        AuditResult::PathPrefixDenied => denied_path_from_error(error)
            .or_else(|| primary_target_path(request)),
        _ => primary_target_path(request),
    };

    AuditRecord::new(
        operation,
        user_id,
        auth.token_id().cloned(),
        address,
        target_path,
        result,
        Utc::now(),
        Some(error.message().to_string()),
        None,
    )
}

fn build_success_record(
    operation: AuditOperation,
    user_id: UserId,
    auth: &AuthContext,
    address: Option<IpAddr>,
    target_path: Option<String>,
    revision: Option<u64>,
    summary: Option<String>,
) -> AuditRecord {
    AuditRecord::new(
        operation,
        user_id,
        auth.token_id().cloned(),
        address,
        target_path,
        AuditResult::Success,
        Utc::now(),
        summary,
        revision,
    )
}

fn audit_operation(request: &McpRequestEnvelope) -> AuditOperation {
    match request.request() {
        McpToolRequest::GetPage(_) => AuditOperation::Get,
        McpToolRequest::GetPageToc(_) => AuditOperation::Get,
        McpToolRequest::ListPages(_) => AuditOperation::List,
        McpToolRequest::SearchPages(_) => AuditOperation::Search,
        McpToolRequest::CreatePage(_) => AuditOperation::Create,
        McpToolRequest::UpdatePage(_) => AuditOperation::Update,
        McpToolRequest::EditPage(_) => AuditOperation::Update,
        McpToolRequest::AppendPage(_) => AuditOperation::Append,
        McpToolRequest::RenamePage(_) => AuditOperation::Rename,
        McpToolRequest::GetPageSection(_) => AuditOperation::GetSection,
    }
}

fn primary_target_path(request: &McpRequestEnvelope) -> Option<String> {
    match request.request() {
        McpToolRequest::GetPage(input) => Some(input.path().to_string()),
        McpToolRequest::GetPageToc(input) => Some(input.path().to_string()),
        McpToolRequest::ListPages(input) => Some(input.prefix().to_string()),
        McpToolRequest::SearchPages(input) => {
            input.prefix().map(str::to_string)
        }
        McpToolRequest::CreatePage(input) => Some(input.path().to_string()),
        McpToolRequest::UpdatePage(input) => Some(input.path().to_string()),
        McpToolRequest::EditPage(input) => Some(input.path().to_string()),
        McpToolRequest::AppendPage(input) => Some(input.path().to_string()),
        McpToolRequest::RenamePage(input) => Some(input.path().to_string()),
        McpToolRequest::GetPageSection(input) => Some(input.path().to_string()),
    }
}

fn denied_path_from_error(error: &McpError) -> Option<String> {
    error
        .message()
        .strip_prefix("path prefix denied: ")
        .map(str::to_string)
}

fn audit_result_from_error(error: &McpError) -> AuditResult {
    use super::errors::McpErrorCode;

    match error.code() {
        McpErrorCode::NotFound => AuditResult::NotFound,
        McpErrorCode::Conflict => AuditResult::Conflict,
        McpErrorCode::InvalidInput => AuditResult::InvalidInput,
        McpErrorCode::NotLatestRevision => AuditResult::Conflict,
        McpErrorCode::InstanceIdNotMatch => AuditResult::Conflict,
        McpErrorCode::Unsupported => AuditResult::Unsupported,
        McpErrorCode::InternalError => AuditResult::InternalError,
        McpErrorCode::Forbidden => {
            if error
                .message()
                .starts_with("required scope denied:")
            {
                AuditResult::ScopeDenied
            } else if error
                .message()
                .starts_with("path prefix denied:")
            {
                AuditResult::PathPrefixDenied
            } else if error
                .message()
                .starts_with("read only denied:")
            {
                AuditResult::ReadOnlyDenied
            } else {
                AuditResult::Unsupported
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, RwLock};

    use tempfile::tempdir;

    use super::*;
    use crate::audit::buffer::AppendAuditBuffer;
    use crate::audit::rotation::{active_log_path, AuditRotationPolicy};
    use crate::audit::writer::{AuditWriter, AuditWriterConfig};
    use crate::auth::AuthUser;
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        PathPrefixSet,
        TokenId,
    };
    use crate::fts::FtsIndexConfig;
    use crate::mcp::errors::{McpErrorCode, McpErrorResponse};
    use crate::mcp::model::{GetPageRequest, WritePageRequest};
    use crate::mcp::service::McpService;
    use crate::mcp::tools::McpToolName;

    fn audit_sink_that_fails_to_write(
        dir: &tempfile::TempDir,
    ) -> Arc<RwLock<AuditSink>> {
        let output_file = dir.path().join("audit-output-file");
        fs::write(&output_file, "not a directory")
            .expect("write audit output file failed");

        Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: output_file,
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )))
    }

    ///
    /// 認可失敗のメッセージから監査結果分類を判定できることを確認する。
    ///
    /// 注記:
    /// scope 不足、path prefix 制約違反、ReadOnly 拒否、
    /// その他 forbidden を個別に与える。
    ///
    #[test]
    fn audit_result_from_error_distinguishes_authorization_failure_kind() {
        let scope_denied = McpError::new(
            McpErrorCode::Forbidden,
            "required scope denied: update",
        );
        let prefix_denied = McpError::new(
            McpErrorCode::Forbidden,
            "path prefix denied: /private",
        );
        let read_only_denied = McpError::new(
            McpErrorCode::Forbidden,
            "read only denied: write operation is not allowed",
        );
        let root_denied = McpError::new(
            McpErrorCode::Forbidden,
            "operation is not allowed for root page",
        );

        assert_eq!(
            audit_result_from_error(&scope_denied),
            AuditResult::ScopeDenied
        );
        assert_eq!(
            audit_result_from_error(&prefix_denied),
            AuditResult::PathPrefixDenied
        );
        assert_eq!(
            audit_result_from_error(&read_only_denied),
            AuditResult::ReadOnlyDenied
        );
        assert_eq!(
            audit_result_from_error(&root_denied),
            AuditResult::Unsupported
        );
    }

    ///
    /// `edit_page` 固有の内容整合性エラーが監査分類で他種別と混同されないことを確認する。
    ///
    #[test]
    fn audit_result_from_error_maps_edit_page_consistency_failures() {
        let not_latest_revision = McpError::new(
            McpErrorCode::NotLatestRevision,
            "revision is not latest",
        );
        let instance_id_not_match = McpError::new(
            McpErrorCode::InstanceIdNotMatch,
            "instance_id does not match latest content",
        );

        assert_eq!(
            audit_result_from_error(&not_latest_revision),
            AuditResult::Conflict
        );
        assert_eq!(
            audit_result_from_error(&instance_id_not_match),
            AuditResult::Conflict
        );
    }

    ///
    /// `McpErrorResponse` が `edit_page` 固有コードを外部文字列へ変換できることを確認する。
    ///
    #[test]
    fn mcp_error_response_preserves_edit_page_error_codes() {
        let not_latest_revision = McpErrorResponse::from(McpError::new(
            McpErrorCode::NotLatestRevision,
            "revision is not latest",
        ));
        let instance_id_not_match = McpErrorResponse::from(McpError::new(
            McpErrorCode::InstanceIdNotMatch,
            "instance_id does not match latest content",
        ));

        assert_eq!(not_latest_revision.code(), "not_latest_revision");
        assert_eq!(
            instance_id_not_match.code(),
            "instance_id_not_match"
        );
    }

    ///
    /// 成功系操作が監査ログへ記録されることを確認する。
    ///
    /// 注記:
    /// `get_page` 成功後に sink を flush し、JSONL の内容を検証する。
    ///
    #[test]
    fn handle_records_success_audit_log() {
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/page", "alice", "# page".to_string())
            .expect("create page failed");

        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            Some(TokenId::new()),
        );
        let request = McpRequestEnvelope::new(
            McpToolName::GetPage,
            McpToolRequest::GetPage(GetPageRequest::new(
                "/mcp/page".to_string(),
                None,
            )),
        );

        let response = handler
            .handle(
                &auth,
                &manager,
                &FtsIndexConfig::new(dir.path().join("fts")),
                None,
                request,
            )
            .expect("handle must succeed");
        assert_eq!(response.tool_name().as_str(), "get_page");

        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");

        assert_eq!(record.operation, AuditOperation::Get);
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path.as_deref(), Some("/mcp/page"));
        assert_eq!(record.revision, Some(1));
        assert_eq!(record.token_id, auth.token_id().cloned());
    }

    ///
    /// 認可失敗が監査ログへ記録されることを確認する。
    ///
    /// 注記:
    /// `update_page` の scope 不足失敗を `record_error()` へ渡し、
    /// `ScopeDenied` 記録を検証する。
    ///
    #[test]
    fn record_error_writes_scope_denied_audit_log() {
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-error.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/page", "alice", "# page".to_string())
            .expect("create page failed");

        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            Some(TokenId::new()),
        );
        let request = McpRequestEnvelope::new(
            McpToolName::UpdatePage,
            McpToolRequest::UpdatePage(WritePageRequest::new(
                "/mcp/page".to_string(),
                "# updated".to_string(),
            )),
        );
        let error = McpService::new()
            .update_page(&auth, &manager, "/mcp/page", "# updated")
            .expect_err("update must fail by scope");

        handler.record_error(&manager, &auth, None, &request, &error);
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");

        assert_eq!(record.operation, AuditOperation::Update);
        assert_eq!(record.result, AuditResult::ScopeDenied);
        assert_eq!(record.target_path.as_deref(), Some("/mcp/page"));
        assert_eq!(
            record.summary.as_deref(),
            Some("required scope denied: update")
        );
        assert_eq!(record.token_id, auth.token_id().cloned());
        assert_eq!(record.revision, None);
    }

    ///
    /// prompts/list成功を専用操作として監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// 秘匿情報を含むpromptを一覧取得し、監査JSONLへ
    /// 件数とhas_more以外が混入しないことを検証する。
    ///
    #[test]
    fn handle_list_prompts_records_success_audit_log() {
        /*
         * promptと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-prompts.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/secret/prompt-path",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: secret-prompt-name\n",
                    "  description: prompt description\n",
                    "  system: secret-system-value\n",
                    "---\n",
                    "secret-body-value",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let address = Some(
            "127.0.0.1"
                .parse()
                .expect("parse audit address failed"),
        );

        /*
         * 一覧成功を監査ログへ記録する
         */
        let result = handler
            .handle_list_prompts(
                &auth,
                &manager,
                address,
                None,
            )
            .expect("list prompts failed");
        assert_eq!(result.items().len(), 1);
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用操作と記録禁止情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");
        assert_eq!(record.operation, AuditOperation::ListPrompts);
        assert_eq!(record.operation.as_str(), "list_prompts");
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path, None);
        assert_eq!(record.revision, None);
        assert_eq!(
            record.summary.as_deref(),
            Some("count=1 has_more=false"),
        );
        assert_eq!(record.token_id, auth.token_id().cloned());
        assert_eq!(record.address, address);
        assert!(!body.contains("secret-prompt-name"));
        assert!(!body.contains("/secret/prompt-path"));
        assert!(!body.contains("secret-system-value"));
        assert!(!body.contains("secret-body-value"));
    }

    ///
    /// prompts/list失敗を専用結果分類で監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// read scope不足を発生させ、scope_deniedと固定summaryを
    /// 検証する。
    ///
    #[test]
    fn handle_list_prompts_records_failure_audit_log() {
        /*
         * scope不足の認証文脈と監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-prompts-error.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );

        /*
         * scope不足失敗を監査ログへ記録する
         */
        let error = handler
            .handle_list_prompts(
                &auth,
                &manager,
                None,
                Some("secret-cursor"),
            )
            .expect_err("scope denial expected");
        assert_eq!(error.code(), McpErrorCode::Forbidden);
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用操作と固定失敗情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");
        assert_eq!(record.operation, AuditOperation::ListPrompts);
        assert_eq!(record.result, AuditResult::ScopeDenied);
        assert_eq!(record.target_path, None);
        assert_eq!(record.revision, None);
        assert_eq!(
            record.summary.as_deref(),
            Some("operation is not allowed"),
        );
        assert!(!body.contains("secret-cursor"));
        assert!(!body.contains("required scope denied: read"));
    }

    ///
    /// prompts/get成功を専用操作として監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// system、本文、引数値を含むpromptを取得し、
    /// 監査JSONLへ許可情報だけが
    /// 記録されることを検証する。
    ///
    #[test]
    fn handle_get_prompt_records_success_audit_log() {
        /*
         * promptと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-get-prompt.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/secret/get-prompt-path",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: audit-get-prompt\n",
                    "  description: secret-description\n",
                    "  system: secret-system {{@target}}\n",
                    "  arguments:\n",
                    "    - name: target\n",
                    "      description: target description\n",
                    "      required: true\n",
                    "---\n",
                    "secret-body {{@target}}",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let address = Some(
            "127.0.0.1"
                .parse()
                .expect("parse audit address failed"),
        );
        let arguments = serde_json::Map::from_iter([(
            "target".to_string(),
            serde_json::Value::String(
                "secret-argument-value".to_string(),
            ),
        )]);

        /*
         * prompt取得成功を監査ログへ記録する
         */
        let result = handler
            .handle_get_prompt(
                &auth,
                &manager,
                address,
                "audit-get-prompt",
                Some(&arguments),
            )
            .expect("get prompt failed");
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用操作、revision、記録禁止情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");
        assert_eq!(record.operation, AuditOperation::GetPrompt);
        assert_eq!(record.operation.as_str(), "get_prompt");
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path, None);
        assert_eq!(record.revision, Some(result.revision()));
        assert_eq!(
            record.summary.as_deref(),
            Some("name=audit-get-prompt"),
        );
        assert_eq!(record.token_id, auth.token_id().cloned());
        assert_eq!(record.address, address);
        assert!(!body.contains("/secret/get-prompt-path"));
        assert!(!body.contains("secret-description"));
        assert!(!body.contains("secret-system"));
        assert!(!body.contains("secret-body"));
        assert!(!body.contains("secret-argument-value"));
    }

    ///
    /// prompts/get失敗を専用結果分類で監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// 不存在、引数不正、scope不足、
    /// 内部不整合を発生させ、
    /// 固定分類と記録禁止情報を検証する。
    ///
    #[test]
    fn handle_get_prompt_records_failure_audit_logs() {
        /*
         * 失敗状態を作るpromptと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-get-error.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/secret/get-error-path",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: audit-failure\n",
                    "  description: secret-description\n",
                    "  system: secret-system\n",
                    "  arguments:\n",
                    "    - name: target\n",
                    "      description: target description\n",
                    "      required: true\n",
                    "---\n",
                    "secret-body {{@target}}",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(
                    1024 * 1024,
                ),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let read = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let append_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let arguments = serde_json::Map::from_iter([(
            "unknown".to_string(),
            serde_json::Value::String(
                "secret-argument-value".to_string(),
            ),
        )]);

        /*
         * 不存在、引数不正、scope不足を記録する
         */
        let not_found = handler
            .handle_get_prompt(
                &read,
                &manager,
                None,
                "missing-prompt",
                None,
            )
            .expect_err("not found expected");
        assert_eq!(not_found.code(), McpErrorCode::NotFound);
        let invalid_name = handler
            .handle_get_prompt(
                &read,
                &manager,
                None,
                " invalid-name",
                None,
            )
            .expect_err("invalid name must be hidden");
        assert_eq!(invalid_name.code(), McpErrorCode::NotFound);
        let invalid = handler
            .handle_get_prompt(
                &read,
                &manager,
                None,
                "audit-failure",
                Some(&arguments),
            )
            .expect_err("invalid input expected");
        assert_eq!(invalid.code(), McpErrorCode::InvalidInput);
        let forbidden = handler
            .handle_get_prompt(
                &append_only,
                &manager,
                None,
                "audit-failure",
                None,
            )
            .expect_err("scope denial expected");
        assert_eq!(forbidden.code(), McpErrorCode::Forbidden);

        /*
         * latest source不整合を記録する
         */
        manager
            .replace_latest_page_source_for_prompt_rebuild_test(
                &page_id,
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: secret-changed-name\n",
                    "  description: changed\n",
                    "---\n",
                    "secret-internal-body",
                )
                .to_string(),
            )
            .expect("replace latest source failed");
        let internal = handler
            .handle_get_prompt(
                &read,
                &manager,
                None,
                "audit-failure",
                None,
            )
            .expect_err("internal error expected");
        assert_eq!(internal.code(), McpErrorCode::InternalError);
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 失敗分類、summary、記録禁止情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let records = body
            .lines()
            .map(|line| {
                serde_json::from_str::<AuditRecord>(line)
                    .expect("decode audit record failed")
            })
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 5);
        assert_eq!(records[0].result, AuditResult::NotFound);
        assert_eq!(records[1].result, AuditResult::NotFound);
        assert_eq!(records[2].result, AuditResult::InvalidInput);
        assert_eq!(records[3].result, AuditResult::ScopeDenied);
        assert_eq!(records[4].result, AuditResult::InternalError);
        for record in &records {
            assert_eq!(record.operation, AuditOperation::GetPrompt);
            assert_eq!(record.target_path, None);
            assert_eq!(record.revision, None);
        }
        assert_eq!(
            records[0].summary.as_deref(),
            Some("name=missing-prompt"),
        );
        assert_eq!(records[1].summary, None);
        for record in &records[2..] {
            assert_eq!(
                record.summary.as_deref(),
                Some("name=audit-failure"),
            );
        }
        assert!(!body.contains("/secret/get-error-path"));
        assert!(!body.contains("secret-description"));
        assert!(!body.contains("secret-system"));
        assert!(!body.contains("secret-body"));
        assert!(!body.contains("secret-argument-value"));
        assert!(!body.contains("secret-changed-name"));
        assert!(!body.contains("secret-internal-body"));
        assert!(!body.contains(&page_id.to_string()));
    }

    ///
    /// resources/list成功を専用操作として監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// 秘匿情報を含むresourceを一覧取得し、監査JSONLへ
    /// 件数とhas_more以外が混入しないことを検証する。
    ///
    #[test]
    fn handle_list_resources_records_success_audit_log() {
        /*
         * resourceと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-resources.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/secret/resource-path",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_path: /docs/secret-resource\n",
                    "  name: secret-resource-name\n",
                    "  description: secret resource description\n",
                    "  mime_type: text/plain\n",
                    "---\n",
                    "secret-resource-body",
                )
                .to_string(),
            )
            .expect("create resource failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let address = Some(
            "127.0.0.1"
                .parse()
                .expect("parse audit address failed"),
        );

        /*
         * 一覧成功を監査ログへ記録する
         */
        let result = handler
            .handle_list_resources(
                &auth,
                &manager,
                address,
                None,
            )
            .expect("list resources failed");
        assert!(result.items().len() >= 1);
        let expected_summary = format!(
            "count={} has_more=false",
            result.items().len(),
        );
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用操作と記録禁止情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");
        assert_eq!(record.operation, AuditOperation::ListResources);
        assert_eq!(record.operation.as_str(), "list_resources");
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path, None);
        assert_eq!(record.revision, None);
        assert_eq!(
            record.summary.as_deref(),
            Some(expected_summary.as_str()),
        );
        assert_eq!(record.token_id, auth.token_id().cloned());
        assert_eq!(record.address, address);
        assert!(!body.contains("docs/secret-resource"));
        assert!(!body.contains("secret-resource-name"));
        assert!(!body.contains("/secret/resource-path"));
        assert!(!body.contains("secret resource description"));
        assert!(!body.contains("secret-resource-body"));
    }

    ///
    /// resources/list失敗を専用結果分類で監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// read scope不足を発生させ、scope_deniedと固定summaryを
    /// 検証する。
    ///
    #[test]
    fn handle_list_resources_records_failure_audit_log() {
        /*
         * scope不足の認証文脈と監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-resources-error.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );

        /*
         * scope不足失敗を監査ログへ記録する
         */
        let error = handler
            .handle_list_resources(
                &auth,
                &manager,
                None,
                Some("secret-cursor"),
            )
            .expect_err("scope denial expected");
        assert_eq!(error.code(), McpErrorCode::Forbidden);
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用操作と固定失敗情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");
        assert_eq!(record.operation, AuditOperation::ListResources);
        assert_eq!(record.result, AuditResult::ScopeDenied);
        assert_eq!(record.target_path, None);
        assert_eq!(record.revision, None);
        assert_eq!(
            record.summary.as_deref(),
            Some("operation is not allowed"),
        );
        assert!(!body.contains("secret-cursor"));
        assert!(!body.contains("required scope denied: read"));
    }

    ///
    /// resources/read成功を専用操作として監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// front matterと本文を含むresourceを取得し、
    /// 監査JSONLへ許可情報だけが記録されることを検証する。
    ///
    #[test]
    fn handle_read_resource_records_success_audit_log() {
        /*
         * resourceと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-read-resource.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/secret/read-resource-path",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_path: /docs/audit-read\n",
                    "  name: audit-read-resource\n",
                    "  description: secret read description\n",
                    "  mime_type: text/plain\n",
                    "---\n",
                    "secret-read-body",
                )
                .to_string(),
            )
            .expect("create resource failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(1024),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let address = Some(
            "127.0.0.1"
                .parse()
                .expect("parse audit address failed"),
        );
        let uri = "luwiki://local.luwiki/docs/audit-read";

        /*
         * resource取得成功を監査ログへ記録する
         */
        let result = handler
            .handle_read_resource(
                &auth,
                &manager,
                address,
                uri,
            )
            .expect("read resource failed");
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用操作、revision、記録禁止情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let record: AuditRecord = serde_json::from_str(
            body.lines().next().expect("record line missing"),
        )
        .expect("decode audit record failed");
        assert_eq!(record.operation, AuditOperation::ReadResource);
        assert_eq!(record.operation.as_str(), "read_resource");
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path, None);
        assert_eq!(record.revision, result.revision());
        assert_eq!(
            record.summary.as_deref(),
            Some("uri=luwiki://local.luwiki/docs/audit-read"),
        );
        assert_eq!(record.token_id, auth.token_id().cloned());
        assert_eq!(record.address, address);
        assert!(!body.contains("/secret/read-resource-path"));
        assert!(!body.contains("audit-read-resource"));
        assert!(!body.contains("secret read description"));
        assert!(!body.contains("secret-read-body"));
    }

    ///
    /// resources/read失敗を専用結果分類で監査記録することを
    /// 確認する。
    ///
    /// # 注記
    /// 不存在、URI不正、scope不足、内部不整合を発生させ、
    /// 固定分類と記録禁止情報を検証する。
    ///
    #[test]
    fn handle_read_resource_records_failure_audit_logs() {
        /*
         * 失敗状態を作るresourceと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-read-error.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/secret/read-error-path",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_path: /docs/audit-failure\n",
                    "  name: audit-failure-resource\n",
                    "  description: secret failure description\n",
                    "  mime_type: text/plain\n",
                    "---\n",
                    "secret-failure-body",
                )
                .to_string(),
            )
            .expect("create resource failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(
                    1024 * 1024,
                ),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let read = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let append_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );

        /*
         * 不存在、URI不正、scope不足を記録する
         */
        let missing_uri = "luwiki://local.luwiki/docs/missing";
        let not_found = handler
            .handle_read_resource(&read, &manager, None, missing_uri)
            .expect_err("not found expected");
        assert_eq!(not_found.code(), McpErrorCode::NotFound);
        let invalid = handler
            .handle_read_resource(
                &read,
                &manager,
                None,
                "luwiki://local.luwiki/docs//bad-secret-uri",
            )
            .expect_err("invalid input expected");
        assert_eq!(invalid.code(), McpErrorCode::InvalidInput);
        let failure_uri =
            "luwiki://local.luwiki/docs/audit-failure";
        let forbidden = handler
            .handle_read_resource(
                &append_only,
                &manager,
                None,
                failure_uri,
            )
            .expect_err("scope denial expected");
        assert_eq!(forbidden.code(), McpErrorCode::Forbidden);

        /*
         * latest source不整合を記録する
         */
        manager
            .replace_latest_page_source_for_resource_rebuild_test(
                &page_id,
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_path: /docs/secret-changed\n",
                    "  name: changed-resource\n",
                    "  description: changed secret description\n",
                    "---\n",
                    "secret-internal-resource-body",
                )
                .to_string(),
            )
            .expect("replace latest source failed");
        let internal = handler
            .handle_read_resource(&read, &manager, None, failure_uri)
            .expect_err("internal error expected");
        assert_eq!(internal.code(), McpErrorCode::InternalError);
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 失敗分類、summary、記録禁止情報を確認する
         */
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        let records = body
            .lines()
            .map(|line| {
                serde_json::from_str::<AuditRecord>(line)
                    .expect("decode audit record failed")
            })
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].result, AuditResult::NotFound);
        assert_eq!(records[1].result, AuditResult::InvalidInput);
        assert_eq!(records[2].result, AuditResult::ScopeDenied);
        assert_eq!(records[3].result, AuditResult::InternalError);
        for record in &records {
            assert_eq!(record.operation, AuditOperation::ReadResource);
            assert_eq!(record.target_path, None);
            assert_eq!(record.revision, None);
        }
        assert_eq!(
            records[0].summary.as_deref(),
            Some("uri=luwiki://local.luwiki/docs/missing"),
        );
        assert_eq!(records[1].summary, None);
        for record in &records[2..] {
            assert_eq!(
                record.summary.as_deref(),
                Some("uri=luwiki://local.luwiki/docs/audit-failure"),
            );
        }
        assert!(!body.contains(" bad-secret-uri"));
        assert!(!body.contains("/secret/read-error-path"));
        assert!(!body.contains("audit-failure-resource"));
        assert!(!body.contains("secret failure description"));
        assert!(!body.contains("secret-failure-body"));
        assert!(!body.contains("docs/secret-changed"));
        assert!(!body.contains("changed-resource"));
        assert!(!body.contains("changed secret description"));
        assert!(!body.contains("secret-internal-resource-body"));
        assert!(!body.contains(&page_id.to_string()));
    }

    ///
    /// resources/listが監査ログ書き込み失敗で結果を
    /// 変えないことを確認する。
    ///
    /// # 注記
    /// audit writerの出力先を通常ファイルにして書き込み失敗を
    /// 発生させ、成功系と失敗系の結果が維持されることを検証する。
    ///
    #[test]
    fn handle_list_resources_ignores_audit_record_failure() {
        /*
         * 書き込み失敗する監査sinkと認証文脈を準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-list-audit-fail.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(audit_sink_that_fails_to_write(&dir)),
        );
        let read = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );
        let append_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );

        /*
         * 成功結果と本来の失敗結果が監査ログ失敗で変わらない
         */
        let result = handler
            .handle_list_resources(&read, &manager, None, None)
            .expect("list resources should succeed");
        assert!(!result.items().is_empty());

        let error = handler
            .handle_list_resources(
                &append_only,
                &manager,
                None,
                None,
            )
            .expect_err("scope denial expected");
        assert_eq!(error.code(), McpErrorCode::Forbidden);
    }

    ///
    /// resources/readが監査ログ書き込み失敗で結果を
    /// 変えないことを確認する。
    ///
    /// # 注記
    /// audit writerの出力先を通常ファイルにして書き込み失敗を
    /// 発生させ、成功系と失敗系の結果が維持されることを検証する。
    ///
    #[test]
    fn handle_read_resource_ignores_audit_record_failure() {
        /*
         * 書き込み失敗する監査sinkとresourceを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-read-audit-fail.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/audit-fail/read-resource",
                "alice",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: resource\n",
                    "  resource_path: /docs/audit-fail\n",
                    "  name: audit-fail-resource\n",
                    "  description: audit fail resource\n",
                    "---\n",
                    "audit fail body",
                )
                .to_string(),
            )
            .expect("create resource failed");
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(audit_sink_that_fails_to_write(&dir)),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            Some(TokenId::new()),
        );

        /*
         * 成功結果と本来の失敗結果が監査ログ失敗で変わらない
         */
        let result = handler
            .handle_read_resource(
                &auth,
                &manager,
                None,
                "luwiki://local.luwiki/docs/audit-fail",
            )
            .expect("read resource should succeed");
        assert_eq!(result.text(), "audit fail body");

        let error = handler
            .handle_read_resource(
                &auth,
                &manager,
                None,
                "luwiki://local.luwiki/docs/missing",
            )
            .expect_err("not found expected");
        assert_eq!(error.code(), McpErrorCode::NotFound);
    }
}
