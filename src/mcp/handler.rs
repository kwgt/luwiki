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
use crate::fts::FtsIndexConfig;
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
    ListPagesRequest,
    ListPagesResponse,
    McpRequestEnvelope,
    McpResponseEnvelope,
    McpToolRequest,
    McpToolResponse,
    RenamePageRequest,
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
        prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<SearchPagesResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::SearchPages,
            McpToolRequest::SearchPages(SearchPagesRequest::new(
                query.to_string(),
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
}
