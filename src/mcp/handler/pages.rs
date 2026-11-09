/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPハンドラのpagesを定義するモジュール
//!

use super::{
    AppendPageResponse,
    AuthContext,
    DatabaseManager,
    EditPageRequest,
    EditPageResponse,
    FtsIndexConfig,
    FtsSearchTarget,
    GetPageRequest,
    GetPageResponse,
    GetPageSectionRequest,
    GetPageSectionResponse,
    GetPageTocRequest,
    GetPageTocResponse,
    IpAddr,
    ListPagesRequest,
    ListPagesResponse,
    McpError,
    McpHandler,
    McpRequestEnvelope,
    McpToolRequest,
    RenamePageRequest,
    SearchPagesRequest,
    SearchPagesResponse,
    ServiceEditPageRequest,
    WritePageRequest,
    WritePageResponse,
    build_append_page_audit_record,
    build_edit_page_audit_record,
    build_get_page_audit_record,
    build_get_page_section_audit_record,
    build_get_page_toc_audit_record,
    build_list_pages_audit_record,
    build_rename_page_audit_record,
    build_search_pages_audit_record,
    build_write_page_audit_record,
};

// 可読性確保のため、操作別のimplを子モジュールへ分離する。
impl McpHandler {
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
            crate::mcp::tools::McpToolName::GetPage,
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
            crate::mcp::tools::McpToolName::GetPageToc,
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
            crate::mcp::tools::McpToolName::ListPages,
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
            crate::mcp::tools::McpToolName::SearchPages,
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
        section: crate::mcp::model::McpSectionSelector,
        revision: Option<u64>,
    ) -> Result<GetPageSectionResponse, McpError> {
        let request = McpRequestEnvelope::new(
            crate::mcp::tools::McpToolName::GetPageSection,
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
            crate::mcp::tools::McpToolName::CreatePage,
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
            crate::mcp::tools::McpToolName::UpdatePage,
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
            crate::mcp::tools::McpToolName::EditPage,
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
            crate::mcp::tools::McpToolName::AppendPage,
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
            crate::mcp::tools::McpToolName::RenamePage,
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

}
