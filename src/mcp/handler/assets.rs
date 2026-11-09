/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPハンドラのassetsを定義するモジュール
//!

use super::{
    AuthContext,
    DatabaseManager,
    DeletePageAssetResponse,
    GetPageAssetDataResponse,
    GetPageAssetMetadataResponse,
    IpAddr,
    ListPageAssetsRequest,
    ListPageAssetsResponse,
    McpError,
    McpHandler,
    McpRequestEnvelope,
    McpToolRequest,
    PageAssetByFileRequest,
    ResolvePageAssetResponse,
    UploadPageAssetRequest,
    UploadPageAssetResponse,
    build_delete_page_asset_audit_record,
    build_get_page_asset_data_audit_record,
    build_get_page_asset_metadata_audit_record,
    build_list_page_assets_audit_record,
    build_resolve_page_asset_audit_record,
    build_upload_page_asset_audit_record,
};

// 可読性確保のため、操作別のimplを子モジュールへ分離する。
impl McpHandler {
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
            crate::mcp::tools::McpToolName::ListPageAssets,
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
            crate::mcp::tools::McpToolName::ResolvePageAsset,
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
            crate::mcp::tools::McpToolName::GetPageAssetMetadata,
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
            crate::mcp::tools::McpToolName::GetPageAssetData,
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
            crate::mcp::tools::McpToolName::UploadPageAsset,
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
            crate::mcp::tools::McpToolName::DeletePageAsset,
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

}
