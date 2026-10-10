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
    DeletePageAssetResult,
    EditPageResult,
    GetPageAssetDataResult,
    GetPageAssetMetadataResult,
    GetPageResult,
    GetPageSectionResult,
    GetPageTocResult,
    ListPageAssetsResult,
    ListPagesResult,
    ResolvePageAssetResult,
    SearchPagesResult,
    UploadPageAssetResult,
    WritePageResult,
};

use super::auth::McpAuthGateway;
use super::errors::McpError;
use super::model::{
    AppendPageResponse,
    DeletePageAssetResponse,
    EditPageRequest,
    EditPageResponse,
    GetPageAssetDataResponse,
    GetPageAssetMetadataResponse,
    GetPageRequest,
    GetPageResponse,
    GetPageSectionRequest,
    GetPageSectionResponse,
    GetPageTocRequest,
    GetPageTocResponse,
    ListPageAssetsRequest,
    ListPageAssetsResponse,
    GetPromptServiceResult,
    ListPagesRequest,
    ListPagesResponse,
    ListPromptsServiceResult,
    ListResourcesServiceResult,
    McpRequestEnvelope,
    McpToolRequest,
    PageAssetByFileRequest,
    RenamePageRequest,
    ResolvePageAssetResponse,
    ReadResourceServiceResult,
    SearchPagesRequest,
    SearchPagesResponse,
    UploadPageAssetRequest,
    UploadPageAssetResponse,
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
    /// `list_page_assets` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    ///
    /// # 戻り値
    /// `list_page_assets` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_list_page_assets(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
    ) -> Result<ListPageAssetsResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::ListPageAssets,
            McpToolRequest::ListPageAssets(ListPageAssetsRequest::new(
                path.to_string(),
            )),
        );

        let result = match self.service.list_page_assets(auth, db, path) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_list_page_assets_audit_record,
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
    /// `resolve_page_asset` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// `resolve_page_asset` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_resolve_page_asset(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        file_name: &str,
    ) -> Result<ResolvePageAssetResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::ResolvePageAsset,
            McpToolRequest::ResolvePageAsset(PageAssetByFileRequest::new(
                path.to_string(),
                file_name.to_string(),
            )),
        );

        let result = match self
            .service
            .resolve_page_asset(auth, db, path, file_name)
        {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_resolve_page_asset_audit_record,
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
    /// `get_page_asset_metadata` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// `get_page_asset_metadata` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_get_page_asset_metadata(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        file_name: &str,
    ) -> Result<GetPageAssetMetadataResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::GetPageAssetMetadata,
            McpToolRequest::GetPageAssetMetadata(
                PageAssetByFileRequest::new(
                    path.to_string(),
                    file_name.to_string(),
                ),
            ),
        );

        let result = match self
            .service
            .get_page_asset_metadata(auth, db, path, file_name)
        {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_get_page_asset_metadata_audit_record,
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
    /// `get_page_asset_data` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// `get_page_asset_data` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_get_page_asset_data(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        file_name: &str,
    ) -> Result<GetPageAssetDataResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::GetPageAssetData,
            McpToolRequest::GetPageAssetData(PageAssetByFileRequest::new(
                path.to_string(),
                file_name.to_string(),
            )),
        );

        let result = match self
            .service
            .get_page_asset_data(auth, db, path, file_name)
        {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_get_page_asset_data_audit_record,
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
    /// `upload_page_asset` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `file_name` - アップロードするアセットファイル名
    /// * `mime_type` - アップロードするアセットの MIME 種別
    /// * `data_base64` - base64 エンコード済みアセット本体
    ///
    /// # 戻り値
    /// `upload_page_asset` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_upload_page_asset(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        file_name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<UploadPageAssetResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::UploadPageAsset,
            McpToolRequest::UploadPageAsset(UploadPageAssetRequest::new(
                path.to_string(),
                file_name.to_string(),
                mime_type.to_string(),
                data_base64.to_string(),
            )),
        );

        let result = match self.service.upload_page_asset(
            auth,
            db,
            path,
            file_name,
            mime_type,
            data_base64,
        ) {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_upload_page_asset_audit_record,
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
    /// `delete_page_asset` を tool 単位入口として処理する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元アドレス
    /// * `path` - 対象ページ path
    /// * `file_name` - 削除するアセットファイル名
    ///
    /// # 戻り値
    /// `delete_page_asset` の公開応答モデルを返す。
    ///
    pub(crate) fn handle_delete_page_asset(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        path: &str,
        file_name: &str,
    ) -> Result<DeletePageAssetResponse, McpError> {
        let request = McpRequestEnvelope::new(
            super::tools::McpToolName::DeletePageAsset,
            McpToolRequest::DeletePageAsset(PageAssetByFileRequest::new(
                path.to_string(),
                file_name.to_string(),
            )),
        );

        let result = match self
            .service
            .delete_page_asset(auth, db, path, file_name)
        {
            Ok(result) => self.audit_success(
                db,
                auth,
                address,
                build_delete_page_asset_audit_record,
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
        if sink.record(record).is_err() {
            warn!(failure = "record", "audit log operation failed");
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
            warn!(failure = "not_found", "audit user id resolution failed");
            None
        }
        Err(_) => {
            warn!(failure = "lookup", "audit user id resolution failed");
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

///
/// list_page_assets成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `_request` - 対象要求
/// * `result` - アセット一覧結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// アセット一覧成功監査レコードを返す。
///
fn build_list_page_assets_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &ListPageAssetsResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::ListPageAssets,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        None,
        Some(format!("count={}", result.assets().len())),
    )
}

///
/// resolve_page_asset成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `_request` - 対象要求
/// * `result` - アセット解決結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// アセット解決成功監査レコードを返す。
///
fn build_resolve_page_asset_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &ResolvePageAssetResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::ResolvePageAsset,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        None,
        Some(page_asset_audit_summary(
            result.asset().asset_id(),
            result.asset().file_name(),
            result.asset().mime_type(),
            result.asset().size(),
        )),
    )
}

///
/// get_page_asset_metadata成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `_request` - 対象要求
/// * `result` - アセットmetadata取得結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// アセットmetadata取得成功監査レコードを返す。
///
fn build_get_page_asset_metadata_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &GetPageAssetMetadataResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::GetPageAssetMetadata,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        None,
        Some(page_asset_audit_summary(
            result.asset().asset_id(),
            result.asset().file_name(),
            result.asset().mime_type(),
            result.asset().size(),
        )),
    )
}

///
/// get_page_asset_data成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `_request` - 対象要求
/// * `result` - アセット本体取得結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// アセット本体取得成功監査レコードを返す。
///
fn build_get_page_asset_data_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &GetPageAssetDataResult,
    user_id: UserId,
) -> AuditRecord {
    build_success_record(
        AuditOperation::GetPageAssetData,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        None,
        Some(page_asset_audit_summary(
            result.asset().asset_id(),
            result.asset().file_name(),
            result.asset().mime_type(),
            result.asset().size(),
        )),
    )
}

///
/// upload_page_asset成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `_request` - 対象要求
/// * `result` - アセットアップロード結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// アセットアップロード成功監査レコードを返す。
///
fn build_upload_page_asset_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &UploadPageAssetResult,
    user_id: UserId,
) -> AuditRecord {
    /*
     * 公開済み結果から安全な監査項目を構築
     */
    let asset = result.asset();

    build_success_record(
        AuditOperation::UploadPageAsset,
        user_id,
        auth,
        address,
        Some(asset.path().to_string()),
        None,
        Some(format!(
            "asset_id={} file_name={} mime_type={} size={} summary={}",
            asset.asset_id(),
            asset.file_name(),
            asset.mime_type(),
            asset.size(),
            result.summary(),
        )),
    )
}

///
/// delete_page_asset成功監査レコードを生成する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `address` - 入力元IP address
/// * `_request` - 対象要求
/// * `result` - アセット削除結果
/// * `user_id` - 操作主体ユーザID
///
/// # 戻り値
/// アセット削除成功監査レコードを返す。
///
fn build_delete_page_asset_audit_record(
    auth: &AuthContext,
    address: Option<IpAddr>,
    _request: &McpRequestEnvelope,
    result: &DeletePageAssetResult,
    user_id: UserId,
) -> AuditRecord {
    /*
     * 公開済み結果から安全な監査項目を構築
     */
    build_success_record(
        AuditOperation::DeletePageAsset,
        user_id,
        auth,
        address,
        Some(result.path().to_string()),
        None,
        Some(format!(
            "asset_id={} file_name={} summary={}",
            result.asset_id(),
            result.file_name(),
            result.summary(),
        )),
    )
}

///
/// アセットmetadataの監査summaryを生成する
///
/// # 引数
/// * `asset_id` - アセットID
/// * `file_name` - アセットファイル名
/// * `mime_type` - MIME種別
/// * `size` - raw binary size
///
/// # 戻り値
/// 監査summaryを返す。
///
fn page_asset_audit_summary(
    asset_id: &str,
    file_name: &str,
    mime_type: &str,
    size: u64,
) -> String {
    format!(
        "asset_id={} file_name={} mime_type={} size={}",
        asset_id,
        file_name,
        mime_type,
        size,
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
    let result = normalize_asset_update_audit_result(
        operation,
        audit_result_from_error(error),
    );
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
        McpToolRequest::ListPageAssets(_) => AuditOperation::ListPageAssets,
        McpToolRequest::ResolvePageAsset(_) => {
            AuditOperation::ResolvePageAsset
        }
        McpToolRequest::GetPageAssetMetadata(_) => {
            AuditOperation::GetPageAssetMetadata
        }
        McpToolRequest::GetPageAssetData(_) => {
            AuditOperation::GetPageAssetData
        }
        McpToolRequest::UploadPageAsset(_) => {
            AuditOperation::UploadPageAsset
        }
        McpToolRequest::DeletePageAsset(_) => {
            AuditOperation::DeletePageAsset
        }
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
        McpToolRequest::ListPageAssets(input) => Some(input.path().to_string()),
        McpToolRequest::ResolvePageAsset(input) => Some(input.path().to_string()),
        McpToolRequest::GetPageAssetMetadata(input) => {
            Some(input.path().to_string())
        }
        McpToolRequest::GetPageAssetData(input) => Some(input.path().to_string()),
        McpToolRequest::UploadPageAsset(input) => {
            Some(input.path().to_string())
        }
        McpToolRequest::DeletePageAsset(input) => {
            Some(input.path().to_string())
        }
    }
}

fn denied_path_from_error(error: &McpError) -> Option<String> {
    error
        .message()
        .strip_prefix("path prefix denied: ")
        .map(str::to_string)
}

///
/// 更新系アセット操作の監査結果分類を
/// 設計上の分類へ正規化する
///
/// # 引数
/// * `operation` - 監査対象操作
/// * `result` - 共通エラー分類から得た監査結果
///
/// # 戻り値
/// 更新系アセット操作の ReadOnly 拒否だけを
/// scope 拒否へ変換した結果を返す。
///
fn normalize_asset_update_audit_result(
    operation: AuditOperation,
    result: AuditResult,
) -> AuditResult {
    match (operation, result) {
        (
            AuditOperation::UploadPageAsset
            | AuditOperation::DeletePageAsset,
            AuditResult::ReadOnlyDenied,
        ) => AuditResult::ScopeDenied,
        (_, result) => result,
    }
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
    use std::fmt::Write as _;
    use std::fs;
    use std::sync::{Arc, Mutex, RwLock};

    use tempfile::tempdir;
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

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
        UserAttribute,
        UserAttributeSet,
    };
    use crate::mcp::errors::{McpErrorCode, McpErrorResponse};
    use crate::mcp::model::WritePageRequest;
    use crate::mcp::service::McpService;
    use crate::mcp::tools::McpToolName;

    ///
    /// テスト中のtracing eventを収集するsubscriber
    ///
    #[derive(Clone, Default)]
    struct RecordingSubscriber {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingSubscriber {
        ///
        /// 収集済みeventを1つの文字列として返す
        ///
        /// # 戻り値
        /// 収集済みeventを改行区切りで返す。
        ///
        fn output(&self) -> String {
            self.events
                .lock()
                .expect("lock recorded tracing events failed")
                .join("\n")
        }
    }

    impl Subscriber for RecordingSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut output = String::new();
            let _ = write!(output, "{} ", event.metadata().name());
            event.record(&mut EventVisitor(&mut output));
            self.events
                .lock()
                .expect("lock tracing events for record failed")
                .push(output);
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    ///
    /// tracing event fieldをテスト用文字列へ変換するvisitor
    ///
    struct EventVisitor<'a>(&'a mut String);

    impl Visit for EventVisitor<'_> {
        fn record_debug(
            &mut self,
            field: &Field,
            value: &dyn std::fmt::Debug,
        ) {
            let _ = write!(self.0, "{}={value:?};", field.name());
        }
    }

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

    fn read_audit_records(dir: &tempfile::TempDir) -> Vec<AuditRecord> {
        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");

        body.lines()
            .map(|line| {
                serde_json::from_str::<AuditRecord>(line)
                    .expect("decode audit record failed")
            })
            .collect()
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
    /// 更新系アセット操作だけが ReadOnly 拒否を
    /// scope 拒否へ正規化することを確認する。
    ///
    /// 注記:
    /// upload、delete、既存のページ更新操作へ
    /// 同じ監査結果分類を与える。
    ///
    #[test]
    fn asset_update_read_only_denial_is_normalized_to_scope_denial() {
        for operation in [
            AuditOperation::UploadPageAsset,
            AuditOperation::DeletePageAsset,
        ] {
            assert_eq!(
                normalize_asset_update_audit_result(
                    operation,
                    AuditResult::ReadOnlyDenied,
                ),
                AuditResult::ScopeDenied
            );
        }

        assert_eq!(
            normalize_asset_update_audit_result(
                AuditOperation::Update,
                AuditResult::ReadOnlyDenied,
            ),
            AuditResult::ReadOnlyDenied
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
    /// handlerからのアセット入力でも禁止表記を拒否する
    ///
    /// # 概要
    /// 正常なページが存在してもraw入力不正を別の失敗と混同しない。
    ///
    /// # 注記
    /// 末尾/等を渡しInvalidInputと元ページの保持を確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn handle_asset_path_rejects_invalid_raw_input() {
        /*
         * 正常なページとhandlerを準備する
         */
        let dir = tempdir().unwrap();
        let manager = DatabaseManager::open(
            dir.path().join("path-input.redb"), dir.path().join("assets"),
        ).unwrap();
        manager.add_user("alice", "pass", None).unwrap();
        let page_id = manager.create_page(
            "/mcp/assets", "alice", "body".to_string(),
        ).unwrap();
        let handler = McpHandler::new(
            McpAuthGateway::new(), McpService::new(), None,
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]), None,
        );

        /*
         * 入力不正の分類と保存済みページの無変更を確認する
         */
        for path in [
            "/mcp/assets/", "/mcp//assets", "/mcp/../../assets",
            "/mcp/asset s",
        ] {
            let error = handler.handle_list_page_assets(
                &auth, &manager, None, path,
            ).unwrap_err();
            assert_eq!(error.code(), McpErrorCode::InvalidInput);
        }
        assert_eq!(manager.get_page_id_by_path("/mcp/assets").unwrap(),
            Some(page_id));
    }

    ///
    /// 参照系アセット一覧の成功監査ログを確認する。
    ///
    /// # 注記
    /// テスト用assetを作成して `handle_list_page_assets()` を実行し、
    /// 専用operation、正規化済みtarget_path、revisionなし、件数summaryを検証する。
    ///
    #[test]
    fn handle_list_page_assets_records_success_audit_log() {
        /*
         * ページ、アセット、監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-assets.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        manager
            .create_asset(
                &page_id,
                "asset.bin",
                "application/octet-stream",
                "alice",
                b"secret-payload",
            )
            .expect("create asset failed");
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

        /*
         * list_page_assets成功監査ログを検証する
         */
        handler
            .handle_list_page_assets(
                &auth,
                &manager,
                None,
                "/mcp/./assets",
            )
            .expect("list page assets must succeed");
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

        assert_eq!(record.operation, AuditOperation::ListPageAssets);
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path.as_deref(), Some("/mcp/assets"));
        assert_eq!(record.revision, None);
        assert_eq!(record.summary.as_deref(), Some("count=1"));
        assert_eq!(record.token_id, auth.token_id().cloned());
    }

    ///
    /// アセット本体取得の成功監査summaryに本体情報が混入しないことを確認する。
    ///
    /// # 注記
    /// `handle_get_page_asset_data()` を実行し、summary が metadata のみに限定され、
    /// raw bytes相当文字列やbase64文字列を含まないことを検証する。
    ///
    #[test]
    fn handle_get_page_asset_data_records_safe_success_summary() {
        /*
         * ページ、アセット、監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-asset-data.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "asset.bin",
                "application/octet-stream",
                "alice",
                b"secret-payload",
            )
            .expect("create asset failed");
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

        /*
         * get_page_asset_data成功監査ログを検証する
         */
        handler
            .handle_get_page_asset_data(
                &auth,
                &manager,
                None,
                "/mcp/./assets",
                "asset.bin",
            )
            .expect("get page asset data must succeed");
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
        let summary = record.summary.as_deref().expect("summary missing");

        assert_eq!(record.operation, AuditOperation::GetPageAssetData);
        assert_eq!(record.result, AuditResult::Success);
        assert_eq!(record.target_path.as_deref(), Some("/mcp/assets"));
        assert_eq!(record.revision, None);
        assert!(summary.contains(&format!("asset_id={}", asset_id)));
        assert!(summary.contains("file_name=asset.bin"));
        assert!(summary.contains("mime_type=application/octet-stream"));
        assert!(summary.contains("size=14"));
        assert!(!summary.contains("secret-payload"));
        assert!(!summary.contains("c2VjcmV0LXBheWxvYWQ="));
    }

    ///
    /// 参照系アセット失敗が専用operationとして監査されることを確認する。
    ///
    /// # 注記
    /// 不正な file_name で `handle_resolve_page_asset()` を失敗させ、
    /// summary が公開message相当のみであることを検証する。
    ///
    #[test]
    fn handle_reference_asset_failure_records_dedicated_operation() {
        /*
         * ページと監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-asset-error.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
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

        /*
         * resolve_page_asset失敗監査ログを検証する
         */
        let error = handler
            .handle_resolve_page_asset(
                &auth,
                &manager,
                None,
                "/mcp/assets",
                "dir/secret.bin",
            )
            .expect_err("invalid file_name must fail");
        assert_eq!(error.code(), McpErrorCode::InvalidInput);
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

        assert_eq!(record.operation, AuditOperation::ResolvePageAsset);
        assert_eq!(record.result, AuditResult::InvalidInput);
        assert_eq!(record.target_path.as_deref(), Some("/mcp/assets"));
        assert_eq!(record.revision, None);
        assert_eq!(record.summary.as_deref(), Some("file_name is invalid"));
    }

    ///
    /// アセット本体サイズ超過が競合として監査されることを確認する。
    ///
    /// # 注記
    /// 10MiBを超えるassetで `handle_get_page_asset_data()` を失敗させ、
    /// 監査resultとsummaryが仕様どおりになることを検証する。
    ///
    #[test]
    fn handle_get_page_asset_data_oversize_records_conflict() {
        /*
         * ページ、大きなアセット、監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-asset-large.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let data = vec![0_u8; 10 * 1024 * 1024 + 1];
        manager
            .create_asset(
                &page_id,
                "large.bin",
                "application/octet-stream",
                "alice",
                &data,
            )
            .expect("create asset failed");
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

        /*
         * サイズ超過失敗監査ログを検証する
         */
        let error = handler
            .handle_get_page_asset_data(
                &auth,
                &manager,
                None,
                "/mcp/assets",
                "large.bin",
            )
            .expect_err("oversized asset must fail");
        assert_eq!(error.code(), McpErrorCode::Conflict);
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

        assert_eq!(record.operation, AuditOperation::GetPageAssetData);
        assert_eq!(record.result, AuditResult::Conflict);
        assert_eq!(record.target_path.as_deref(), Some("/mcp/assets"));
        assert_eq!(record.revision, None);
        assert_eq!(record.summary.as_deref(), Some("asset data too large"));
    }

    ///
    /// 参照系アセット4 handler の成功監査ログを横断確認する。
    ///
    #[test]
    fn handle_reference_asset_methods_record_safe_success_audit_logs() {
        /*
         * ページ、アセット、監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-asset-success-all.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "asset.bin",
                "application/octet-stream",
                "alice",
                b"secret-payload",
            )
            .expect("create asset failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(4096),
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

        /*
         * 参照系4 handler を成功させる
         */
        handler
            .handle_list_page_assets(
                &auth,
                &manager,
                None,
                "/mcp/./assets",
            )
            .expect("list page assets must succeed");
        handler
            .handle_resolve_page_asset(
                &auth,
                &manager,
                None,
                "/mcp/./assets",
                "asset.bin",
            )
            .expect("resolve page asset must succeed");
        handler
            .handle_get_page_asset_metadata(
                &auth,
                &manager,
                None,
                "/mcp/./assets",
                "asset.bin",
            )
            .expect("get page asset metadata must succeed");
        handler
            .handle_get_page_asset_data(
                &auth,
                &manager,
                None,
                "/mcp/./assets",
                "asset.bin",
            )
            .expect("get page asset data must succeed");
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 専用operation、成功result、正規化済みpath、revisionなし、秘匿summaryを検証する
         */
        let records = read_audit_records(&dir);
        assert_eq!(records.len(), 4);
        assert_eq!(
            records
                .iter()
                .map(|record| record.operation)
                .collect::<Vec<_>>(),
            vec![
                AuditOperation::ListPageAssets,
                AuditOperation::ResolvePageAsset,
                AuditOperation::GetPageAssetMetadata,
                AuditOperation::GetPageAssetData,
            ],
        );

        for record in &records {
            assert_eq!(record.result, AuditResult::Success);
            assert_eq!(record.target_path.as_deref(), Some("/mcp/assets"));
            assert_eq!(record.revision, None);
            assert_eq!(record.token_id, auth.token_id().cloned());
        }

        assert_eq!(records[0].summary.as_deref(), Some("count=1"));
        for record in records.iter().skip(1) {
            let summary = record.summary.as_deref().expect("summary missing");
            assert!(summary.contains(&format!("asset_id={}", asset_id)));
            assert!(summary.contains("file_name=asset.bin"));
            assert!(summary.contains("mime_type=application/octet-stream"));
            assert!(summary.contains("size=14"));
            assert!(!summary.contains("data_base64"));
            assert!(!summary.contains("secret-payload"));
            assert!(!summary.contains("c2VjcmV0LXBheWxvYWQ="));
            assert!(!summary.contains("Authorization"));
            assert!(!summary.contains("Bearer"));
            assert!(!summary.contains("handler-asset-success-all.redb"));
            assert!(!summary.contains("assets/"));
        }
    }

    ///
    /// 参照系アセット handler 失敗監査ログの分類と秘匿を横断確認する。
    ///
    #[test]
    fn handle_reference_asset_failures_record_safe_audit_results() {
        /*
         * 失敗ケース用のページ、アセット、監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-asset-failures.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let oversized_data = vec![0_u8; 10 * 1024 * 1024 + 1];
        manager
            .create_asset(
                &page_id,
                "large.bin",
                "application/octet-stream",
                "alice",
                &oversized_data,
            )
            .expect("create large asset failed");
        let missing_file_asset_id = manager
            .create_asset(
                &page_id,
                "missing-file.bin",
                "application/octet-stream",
                "alice",
                b"secret-payload",
            )
            .expect("create missing file asset failed");
        fs::remove_file(manager.asset_file_path(&missing_file_asset_id))
            .expect("remove asset file failed");

        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(4096),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let read_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            Some(TokenId::new()),
        );
        let no_read_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            Some(TokenId::new()),
        );
        let outside_prefix_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/allowed"]),
            Some(TokenId::new()),
        );

        /*
         * 代表的な失敗分類を発生させる
         */
        assert_eq!(
            handler
                .handle_list_page_assets(
                    &no_read_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                )
                .expect_err("scope denied must fail")
                .code(),
            McpErrorCode::Forbidden,
        );
        assert_eq!(
            handler
                .handle_resolve_page_asset(
                    &outside_prefix_auth,
                    &manager,
                    None,
                    "/secret/assets",
                    "asset.bin",
                )
                .expect_err("path prefix denied must fail")
                .code(),
            McpErrorCode::Forbidden,
        );
        assert_eq!(
            handler
                .handle_get_page_asset_metadata(
                    &read_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "dir/secret.bin",
                )
                .expect_err("invalid input must fail")
                .code(),
            McpErrorCode::InvalidInput,
        );
        assert_eq!(
            handler
                .handle_resolve_page_asset(
                    &read_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "missing.bin",
                )
                .expect_err("missing asset must fail")
                .code(),
            McpErrorCode::NotFound,
        );
        assert_eq!(
            handler
                .handle_get_page_asset_data(
                    &read_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "large.bin",
                )
                .expect_err("oversized asset must fail")
                .code(),
            McpErrorCode::Conflict,
        );
        assert_eq!(
            handler
                .handle_get_page_asset_data(
                    &read_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "missing-file.bin",
                )
                .expect_err("missing asset file must fail")
                .code(),
            McpErrorCode::InternalError,
        );
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 失敗時も専用operationと固定summaryで監査されることを検証する
         */
        let records = read_audit_records(&dir);
        assert_eq!(records.len(), 6);
        let expected = [
            (
                AuditOperation::ListPageAssets,
                AuditResult::ScopeDenied,
                Some("/mcp/assets"),
                "required scope denied: read",
            ),
            (
                AuditOperation::ResolvePageAsset,
                AuditResult::PathPrefixDenied,
                Some("/secret/assets"),
                "path prefix denied: /secret/assets",
            ),
            (
                AuditOperation::GetPageAssetMetadata,
                AuditResult::InvalidInput,
                Some("/mcp/assets"),
                "file_name is invalid",
            ),
            (
                AuditOperation::ResolvePageAsset,
                AuditResult::NotFound,
                Some("/mcp/assets"),
                "asset not found",
            ),
            (
                AuditOperation::GetPageAssetData,
                AuditResult::Conflict,
                Some("/mcp/assets"),
                "asset data too large",
            ),
            (
                AuditOperation::GetPageAssetData,
                AuditResult::InternalError,
                Some("/mcp/assets"),
                "asset read failed",
            ),
        ];

        for (record, expected) in records.iter().zip(expected.iter()) {
            assert_eq!(record.operation, expected.0);
            assert_eq!(record.result, expected.1);
            assert_eq!(record.target_path.as_deref(), expected.2);
            assert_eq!(record.revision, None);
            assert_eq!(record.summary.as_deref(), Some(expected.3));
            let summary = record.summary.as_deref().expect("summary missing");
            assert!(!summary.contains("data_base64"));
            assert!(!summary.contains("secret-payload"));
            assert!(!summary.contains("c2VjcmV0LXBheWxvYWQ="));
            assert!(!summary.contains("Authorization"));
            assert!(!summary.contains("Bearer"));
            assert!(!summary.contains("handler-asset-failures.redb"));
            assert!(!summary.contains("assets/"));
        }
    }

    ///
    /// 更新系アセットhandlerの成功応答と監査ログを確認する。
    ///
    /// # 注記
    /// uploadとdeleteを実行し、公開response、永続化operation、
    /// 許可済みsummary、秘匿情報非記録を横断確認する。
    ///
    #[test]
    fn handle_update_asset_methods_record_safe_success_audit_logs() {
        /*
         * ページ、監査sink、認証文脈を準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-update-asset-success.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(4096),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/mcp"]),
            Some(TokenId::new()),
        );
        let address = Some("127.0.0.1".parse().expect("parse address failed"));

        /*
         * uploadとdeleteの公開responseを検証する
         */
        let upload = handler
            .handle_upload_page_asset(
                &auth,
                &manager,
                address,
                "/mcp/./assets",
                "secret.bin",
                "application/octet-stream",
                "c2VjcmV0LXBheWxvYWQ=",
            )
            .expect("upload page asset failed");
        assert!(!sink
            .read()
            .expect("lock sink failed")
            .has_pending_appends());
        let upload_json = serde_json::to_value(&upload)
            .expect("encode upload response failed");
        let asset_id = upload_json["asset"]["asset_id"]
            .as_str()
            .expect("upload asset id missing")
            .to_string();

        assert_eq!(upload_json["asset"]["path"], "/mcp/assets");
        assert_eq!(upload_json["asset"]["file_name"], "secret.bin");
        assert_eq!(
            upload_json["asset"]["mime_type"],
            "application/octet-stream",
        );
        assert_eq!(upload_json["asset"]["size"], 14);
        assert_eq!(upload_json["summary"], "asset uploaded");
        assert!(upload_json.get("data_base64").is_none());

        let deleted = handler
            .handle_delete_page_asset(
                &auth,
                &manager,
                address,
                "/mcp/./assets",
                "secret.bin",
            )
            .expect("delete page asset failed");
        assert!(!sink
            .read()
            .expect("lock sink failed")
            .has_pending_appends());
        let delete_json = serde_json::to_value(&deleted)
            .expect("encode delete response failed");

        assert_eq!(delete_json["asset_id"], asset_id);
        assert_eq!(delete_json["path"], "/mcp/assets");
        assert_eq!(delete_json["file_name"], "secret.bin");
        assert_eq!(delete_json["summary"], "asset deleted");
        assert_eq!(delete_json.as_object().map(|value| value.len()), Some(4));
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * 成功監査の共通項目と許可済みsummaryを検証する
         */
        let records = read_audit_records(&dir);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].operation, AuditOperation::UploadPageAsset);
        assert_eq!(records[1].operation, AuditOperation::DeletePageAsset);

        for record in &records {
            assert_eq!(record.result, AuditResult::Success);
            assert_eq!(record.target_path.as_deref(), Some("/mcp/assets"));
            assert_eq!(record.revision, None);
            assert_eq!(record.token_id, auth.token_id().cloned());
            assert_eq!(record.address, address);
        }

        assert_eq!(
            records[0].summary.as_deref(),
            Some(format!(
                concat!(
                    "asset_id={} file_name=secret.bin ",
                    "mime_type=application/octet-stream size=14 ",
                    "summary=asset uploaded",
                ),
                asset_id,
            )
            .as_str()),
        );
        assert_eq!(
            records[1].summary.as_deref(),
            Some(format!(
                concat!(
                    "asset_id={} file_name=secret.bin ",
                    "summary=asset deleted",
                ),
                asset_id,
            )
            .as_str()),
        );

        /*
         * JSONL永続化名と秘匿情報非記録を検証する
         */
        let bytes = fs::read(active_log_path(dir.path()))
            .expect("read active log failed");
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(bytes.ends_with(b"\n"));
        assert!(!bytes.contains(&b'\r'));
        let body = String::from_utf8(bytes)
            .expect("audit log must be UTF-8");
        let values = body
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .expect("decode audit JSON failed")
            })
            .collect::<Vec<_>>();

        assert_eq!(values[0]["operation"], "upload_page_asset");
        assert_eq!(values[1]["operation"], "delete_page_asset");
        assert_eq!(values[0]["result"], "success");
        assert_eq!(values[1]["result"], "success");
        assert_eq!(values.len(), 2);
        assert!(!body.contains("data_base64"));
        assert!(!body.contains("c2VjcmV0LXBheWxvYWQ="));
        assert!(!body.contains("secret-payload"));
        assert!(!body.contains("Authorization"));
        assert!(!body.contains("Bearer"));
        assert!(!body.contains("lock_token"));
    }

    ///
    /// 更新系アセットhandlerの失敗監査分類と秘匿を確認する。
    ///
    /// # 注記
    /// 認可、競合、入力、対象不存在、内部失敗を発生させ、
    /// operation、result、固定summaryを横断確認する。
    ///
    #[test]
    fn handle_update_asset_failures_record_expected_audit_results() {
        /*
         * 通常ページ、ロック中ページ、監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-update-asset-failures.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let locked_page_id = manager
            .create_page(
                "/mcp/locked-assets",
                "alice",
                "# locked".to_string(),
            )
            .expect("create locked page failed");
        let locked_asset_id = manager
            .create_asset(
                &locked_page_id,
                "locked.bin",
                "application/octet-stream",
                "alice",
                b"locked",
            )
            .expect("create locked asset failed");
        manager
            .acquire_page_lock(&locked_page_id, "alice")
            .expect("acquire page lock failed");
        let sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().to_path_buf(),
                rotation_policy: AuditRotationPolicy::new(8192),
            }),
        )));
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(sink.clone()),
        );

        /*
         * 認可条件別の認証文脈を準備する
         */
        let read_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
            Some(TokenId::new()),
        );
        let prefix_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/allowed"]),
            Some(TokenId::new()),
        );
        let read_only_auth = AuthContext::new_with_attributes(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            Some(TokenId::new()),
            None,
        );
        let write_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            Some(TokenId::new()),
        );

        /*
         * 認可、競合、入力、対象不存在を発生させる
         */
        assert_eq!(
            handler
                .handle_upload_page_asset(
                    &read_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "scope.bin",
                    "application/octet-stream",
                    "c2NvcGU=",
                )
                .expect_err("scope denied upload must fail")
                .code(),
            McpErrorCode::Forbidden,
        );
        assert_eq!(
            handler
                .handle_delete_page_asset(
                    &prefix_auth,
                    &manager,
                    None,
                    "/private/assets",
                    "missing.bin",
                )
                .expect_err("prefix denied delete must fail")
                .code(),
            McpErrorCode::Forbidden,
        );
        assert_eq!(
            handler
                .handle_upload_page_asset(
                    &read_only_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "read-only.bin",
                    "application/octet-stream",
                    "cmVhZC1vbmx5",
                )
                .expect_err("read only upload must fail")
                .code(),
            McpErrorCode::Forbidden,
        );
        assert_eq!(
            handler
                .handle_delete_page_asset(
                    &write_auth,
                    &manager,
                    None,
                    "/mcp/locked-assets",
                    "locked.bin",
                )
                .expect_err("locked delete must fail")
                .code(),
            McpErrorCode::Conflict,
        );
        assert_eq!(
            {
                let error = handler
                    .handle_upload_page_asset(
                        &write_auth,
                        &manager,
                        None,
                        "/mcp/assets",
                        "invalid.bin",
                        "application/octet-stream",
                        "private:not-base64",
                    )
                    .expect_err("invalid upload must fail");
                assert_eq!(error.message(), "data_base64 is invalid");
                let response = McpErrorResponse::from(error.clone());
                let response_json = serde_json::to_string(&response)
                    .expect("encode public error failed");
                assert_eq!(response.code(), "invalid_input");
                assert_eq!(response.message(), "data_base64 is invalid");
                assert!(!response_json.contains("private:not-base64"));
                error.code()
            },
            McpErrorCode::InvalidInput,
        );
        assert_eq!(
            handler
                .handle_delete_page_asset(
                    &write_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "missing.bin",
                )
                .expect_err("missing delete must fail")
                .code(),
            McpErrorCode::NotFound,
        );

        /*
         * 再現不能なDB内部失敗を固定公開errorとして記録する
         */
        let internal_request = McpRequestEnvelope::new(
            McpToolName::UploadPageAsset,
            McpToolRequest::UploadPageAsset(
                UploadPageAssetRequest::new(
                    "/mcp/assets".to_string(),
                    "internal.bin".to_string(),
                    "application/octet-stream".to_string(),
                    "private-internal-payload".to_string(),
                ),
            ),
        );
        handler.record_error(
            &manager,
            &write_auth,
            None,
            &internal_request,
            &McpError::new(
                McpErrorCode::InternalError,
                "asset create failed",
            ),
        );
        assert!(!sink
            .read()
            .expect("lock sink failed")
            .has_pending_appends());
        sink.write()
            .expect("lock sink failed")
            .flush()
            .expect("flush sink failed");

        /*
         * operation、result、path、固定summaryを検証する
         */
        let records = read_audit_records(&dir);
        assert_eq!(records.len(), 7);
        let expected = [
            (
                AuditOperation::UploadPageAsset,
                AuditResult::ScopeDenied,
                Some("/mcp/assets"),
                "required scope denied: update",
            ),
            (
                AuditOperation::DeletePageAsset,
                AuditResult::PathPrefixDenied,
                Some("/private/assets"),
                "path prefix denied: /private/assets",
            ),
            (
                AuditOperation::UploadPageAsset,
                AuditResult::ScopeDenied,
                Some("/mcp/assets"),
                "read only denied: write operation is not allowed",
            ),
            (
                AuditOperation::DeletePageAsset,
                AuditResult::Conflict,
                Some("/mcp/locked-assets"),
                "page is locked",
            ),
            (
                AuditOperation::UploadPageAsset,
                AuditResult::InvalidInput,
                Some("/mcp/assets"),
                "data_base64 is invalid",
            ),
            (
                AuditOperation::DeletePageAsset,
                AuditResult::NotFound,
                Some("/mcp/assets"),
                "asset not found",
            ),
            (
                AuditOperation::UploadPageAsset,
                AuditResult::InternalError,
                Some("/mcp/assets"),
                "asset create failed",
            ),
        ];

        for (record, expected) in records.iter().zip(expected.iter()) {
            assert_eq!(record.operation, expected.0);
            assert_eq!(record.result, expected.1);
            assert_eq!(record.target_path.as_deref(), expected.2);
            assert_eq!(record.revision, None);
            assert_eq!(record.summary.as_deref(), Some(expected.3));
        }

        /*
         * 失敗監査とerror summaryの秘匿を検証する
         */
        let bytes = fs::read(active_log_path(dir.path()))
            .expect("read active log failed");
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(bytes.ends_with(b"\n"));
        assert!(!bytes.contains(&b'\r'));
        let body = String::from_utf8(bytes)
            .expect("audit log must be UTF-8");

        assert!(!body.contains("private:not-base64"));
        assert!(!body.contains("private-internal-payload"));
        assert!(!body.contains("c2NvcGU="));
        assert!(!body.contains("cmVhZC1vbmx5"));
        assert!(!body.contains("Authorization"));
        assert!(!body.contains("Bearer"));
        assert!(!body.contains("lock_token"));
        assert!(!body.contains("handler-update-asset-failures.redb"));
        assert!(!body.contains("assets/"));
        assert!(!body.contains(&locked_page_id.to_string()));
        assert!(!body.contains(&locked_asset_id.to_string()));
        assert!(!body.contains("locked.bin"));
        assert!(!body.contains(&db_path.display().to_string()));
        assert!(!body.contains(&asset_path.display().to_string()));

        let values = body
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .expect("decode audit JSON failed")
            })
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 7);
        assert_eq!(values[4]["operation"], "upload_page_asset");
        assert_eq!(values[4]["result"], "invalid_input");
        assert_eq!(values[5]["operation"], "delete_page_asset");
        assert_eq!(values[5]["result"], "not_found");
        assert_eq!(values[6]["operation"], "upload_page_asset");
        assert_eq!(values[6]["result"], "internal_error");
    }

    ///
    /// 更新系アセットhandlerが監査失敗を業務応答へ反映しないことを
    /// 確認する。
    ///
    /// # 注記
    /// ユーザID解決失敗、sink lock失敗、writer書込み失敗を発生させ、
    /// 成功応答と本来の失敗応答が維持されることを検証する。
    ///
    #[test]
    fn handle_update_asset_methods_ignore_audit_failures() {
        /*
         * ページと認証文脈を準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("handler-asset-audit-fail.redb");
        let asset_path = dir.path().join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let write_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            Some(TokenId::new()),
        );

        /*
         * writer失敗時も成功結果と本来の失敗結果を維持する
         */
        let writer_failure_handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(audit_sink_that_fails_to_write(&dir)),
        );
        let upload = writer_failure_handler
            .handle_upload_page_asset(
                &write_auth,
                &manager,
                None,
                "/mcp/assets",
                "writer-failure.bin",
                "application/octet-stream",
                "d3JpdGVyLWZhaWx1cmU=",
            )
            .expect("audit write failure must not fail upload");
        let upload_json = serde_json::to_value(&upload)
            .expect("encode upload response failed");
        assert_eq!(upload_json["summary"], "asset uploaded");

        let error = writer_failure_handler
            .handle_delete_page_asset(
                &write_auth,
                &manager,
                None,
                "/mcp/assets",
                "missing.bin",
            )
            .expect_err("missing asset must remain not found");
        assert_eq!(error.code(), McpErrorCode::NotFound);
        assert_eq!(error.message(), "asset not found");

        /*
         * sink lock失敗時も成功結果を維持する
         */
        let poisoned_sink = Arc::new(RwLock::new(AuditSink::new(
            AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().join("poisoned-audit"),
                rotation_policy: AuditRotationPolicy::new(4096),
            }),
        )));
        let sink_for_thread = poisoned_sink.clone();
        let poison_result = std::thread::spawn(move || {
            let _guard = sink_for_thread
                .write()
                .expect("lock sink before poisoning");
            panic!("poison audit sink lock");
        })
        .join();
        assert!(poison_result.is_err());

        let lock_failure_handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(poisoned_sink),
        );
        let upload = lock_failure_handler
            .handle_upload_page_asset(
                &write_auth,
                &manager,
                None,
                "/mcp/assets",
                "lock-failure.bin",
                "application/octet-stream",
                "bG9jay1mYWlsdXJl",
            )
            .expect("audit lock failure must not fail upload");
        let upload_json = serde_json::to_value(&upload)
            .expect("encode upload response failed");
        assert_eq!(upload_json["summary"], "asset uploaded");

        /*
         * 監査ユーザID解決失敗時も本来の入力エラーを維持する
         */
        let unknown_user_auth = AuthContext::new(
            AuthUser::new("unknown-audit-user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            Some(TokenId::new()),
        );
        let handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            None,
        );
        let error = handler
            .handle_upload_page_asset(
                &unknown_user_auth,
                &manager,
                None,
                "/mcp/assets",
                "invalid.bin",
                "application/octet-stream",
                "not-valid-base64",
            )
            .expect_err("invalid payload must remain invalid input");
        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert_eq!(error.message(), "data_base64 is invalid");
    }

    ///
    /// 監査失敗時のtracing出力が
    /// 内部詳細を秘匿することを確認する。
    ///
    /// # 注記
    /// writer失敗と監査ユーザ解決失敗を発生させ、
    /// 固定eventだけが記録されることと、
    /// 元の業務応答が維持されることを検証する。
    ///
    #[test]
    fn audit_failure_tracing_redacts_internal_details() {
        const SECRET_USER: &str = "secret-audit-user";
        const SECRET_PAYLOAD: &str = "c2VjcmV0LXRyYWNpbmctcGF5bG9hZA==";

        /*
         * ページ、認証文脈、失敗する監査sinkを準備する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("secret-handler-database.redb");
        let asset_path = dir.path().join("secret-assets-path");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let write_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            Some(TokenId::new()),
        );
        let unknown_user_auth = AuthContext::new(
            AuthUser::new(SECRET_USER.to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            Some(TokenId::new()),
        );
        let writer_failure_handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            Some(audit_sink_that_fails_to_write(&dir)),
        );
        let no_sink_handler = McpHandler::new(
            McpAuthGateway::new(),
            McpService::new(),
            None,
        );
        let subscriber = RecordingSubscriber::default();
        let captured = subscriber.clone();

        /*
         * tracing捕捉中に監査失敗を発生させる
         */
        tracing::subscriber::with_default(subscriber, || {
            let upload = writer_failure_handler
                .handle_upload_page_asset(
                    &write_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "writer-failure.bin",
                    "application/octet-stream",
                    SECRET_PAYLOAD,
                )
                .expect("audit write failure must not fail upload");
            let upload = serde_json::to_value(upload)
                .expect("serialize upload response failed");
            assert_eq!(upload["summary"], "asset uploaded");

            let error = no_sink_handler
                .handle_upload_page_asset(
                    &unknown_user_auth,
                    &manager,
                    None,
                    "/mcp/assets",
                    "invalid.bin",
                    "application/octet-stream",
                    "not-valid-base64",
                )
                .expect_err("invalid payload must remain invalid input");
            assert_eq!(error.code(), McpErrorCode::InvalidInput);
            assert_eq!(error.message(), "data_base64 is invalid");
        });

        /*
         * 固定eventと秘匿結果を検証する
         */
        let output = captured.output();
        assert!(output.contains("audit log operation failed"));
        assert!(output.contains("audit user id resolution failed"));
        for secret in [
            SECRET_USER.to_string(),
            SECRET_PAYLOAD.to_string(),
            "secret-handler-database.redb".to_string(),
            "secret-assets-path".to_string(),
            "audit-output-file".to_string(),
            db_path.display().to_string(),
            asset_path.display().to_string(),
        ] {
            assert!(!output.contains(&secret));
        }
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
