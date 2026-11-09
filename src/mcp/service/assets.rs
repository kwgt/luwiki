/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPサービスのassets処理を定義するモジュール
//!

use base64::Engine;

use super::{
    AssetInfo,
    AuthContext,
    DatabaseManager,
    DeletePageAssetResult,
    GetPageAssetDataResult,
    GetPageAssetMetadataResult,
    ListPageAssetsResult,
    MAX_MCP_ASSET_DATA_BASE64_SIZE,
    MAX_MCP_ASSET_DATA_SIZE,
    McpError,
    McpErrorCode,
    McpOperation,
    McpService,
    PageAccessPolicy,
    PageAssetMetadata,
    ResolvePageAssetResult,
    ResolvedPage,
    UNKNOWN_ASSET_USERNAME,
    UploadPageAssetResult,
    UserId,
    format_mcp_timestamp,
    map_asset_boundary_error,
    map_asset_snapshot_error,
    map_delete_asset_db_error,
    map_upload_asset_db_error,
};

// 可読性確保のため、業務別のimplを子モジュールへ分離する。
impl McpService {
    ///
    /// アセットファイル名を検証する
    ///
    /// # 引数
    /// * `file_name` - 検証対象ファイル名
    ///
    /// # 戻り値
    /// 有効な場合は `Ok(())` を返す。
    ///
    pub(super) fn validate_asset_file_name(
        &self,
        file_name: &str,
    ) -> Result<(), McpError> {
        if file_name.is_empty()
            || file_name.contains('/')
            || file_name.contains('\\')
        {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "file_name is invalid",
            ));
        }

        Ok(())
    }

    ///
    /// アセットのMIME種別を検証する
    ///
    /// # 引数
    /// * `mime_type` - 検証対象のMIME種別
    ///
    /// # 戻り値
    /// 空文字でない場合は `Ok(())` を返す。
    ///
    pub(super) fn validate_asset_mime_type(
        &self,
        mime_type: &str,
    ) -> Result<(), McpError> {
        if mime_type.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "mime_type is invalid",
            ));
        }

        Ok(())
    }

    ///
    /// アセット本体のbase64入力をデコードする
    ///
    /// # 概要
    /// 入力長とデコード後サイズを検証し、
    /// 標準base64としてデコードする。
    ///
    /// # 引数
    /// * `data_base64` - base64エンコード済みアセット本体
    ///
    /// # 戻り値
    /// デコード済みアセット本体を返す。
    ///
    pub(super) fn decode_asset_data_base64(
        &self,
        data_base64: &str,
    ) -> Result<Vec<u8>, McpError> {
        /*
         * 空入力とデコード前サイズの検証
         */
        if data_base64.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "data_base64 is invalid",
            ));
        }
        if data_base64.len() > MAX_MCP_ASSET_DATA_BASE64_SIZE {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "asset data too large",
            ));
        }

        /*
         * 標準base64としてデコード
         */
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .map_err(|_| {
                McpError::new(
                    McpErrorCode::InvalidInput,
                    "data_base64 is invalid",
                )
            })?;

        /*
         * デコード後サイズの検証
         */
        if data.len() as u64 > MAX_MCP_ASSET_DATA_SIZE {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "asset data too large",
            ));
        }

        Ok(data)
    }

    ///
    /// アセット登録ユーザ名を解決する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `user_id` - 登録ユーザID
    ///
    /// # 戻り値
    /// ユーザ名を返す。ユーザ欠落時は代替名を返す。
    ///
    fn resolve_asset_username(
        &self,
        db: &DatabaseManager,
        user_id: &UserId,
    ) -> Result<String, McpError> {
        match db.get_user_name_by_id(user_id) {
            Ok(Some(name)) => Ok(name),
            Ok(None) => Ok(UNKNOWN_ASSET_USERNAME.to_string()),
            Err(_) => Err(McpError::new(
                McpErrorCode::InternalError,
                "user lookup failed",
            )),
        }
    }

    ///
    /// DBアセット情報をMCP公開用metadataへ変換する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `normalized_path` - 正規化済み対象ページ path
    /// * `asset_info` - DBアセット情報
    ///
    /// # 戻り値
    /// MCP公開用metadataを返す。
    ///
    fn build_page_asset_metadata(
        &self,
        db: &DatabaseManager,
        normalized_path: &str,
        asset_info: &AssetInfo,
    ) -> Result<PageAssetMetadata, McpError> {
        let username = self.resolve_asset_username(db, &asset_info.user())?;

        Ok(PageAssetMetadata::new(
            asset_info.id().to_string(),
            normalized_path.to_string(),
            asset_info.file_name(),
            asset_info.mime(),
            asset_info.size(),
            format_mcp_timestamp(asset_info.timestamp()),
            username,
            asset_info.instance_id().map(|id| id.to_string()),
        ))
    }

    ///
    /// 同snapshot/commitのrecordから公開metadataを生成する
    ///
    /// # 引数
    /// * `path` - 認可済み実owner path
    /// * `record` - metadataと同時点のusername
    ///
    /// # 戻り値
    /// 既存MCP metadata形式。
    ///
    fn asset_metadata_from_record(
        &self, path: &str, record: &crate::database::AssetRecord,
    ) -> PageAssetMetadata {
        let info = &record.info;
        PageAssetMetadata::new(info.id().to_string(), path.to_string(),
            info.file_name(), info.mime(), info.size(),
            format_mcp_timestamp(info.timestamp()), record.user_name.clone(),
            info.instance_id().map(|id| id.to_string()))
    }

    ///
    /// 固定期待page/fileを同snapshotで再認可して解決する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - DB
    /// * `resolved` - read時点の期待ID
    /// * `file_name` - 既存file名規則の入力
    /// * `mode` - bytesの必要性/既存上限
    ///
    /// # 戻り値
    /// 現在対象の同snapshot metadata/bytes。
    ///
    fn read_mcp_asset_snapshot(
        &self, auth: &AuthContext, db: &DatabaseManager,
        resolved: &ResolvedPage, file_name: &str,
        mode: crate::database::AssetReadMode,
    ) -> Result<crate::database::AuthorizedAssetRead, McpError> {
        self.validate_asset_file_name(file_name)?;
        db.read_asset_authorized(crate::database::AssetSelector::File {
            page: crate::database::AssetPageSelector::Id(&resolved.page_id()),
            file_name,
        }, &PageAccessPolicy::new(auth.path_prefixes()), mode)
            .map_err(|err| map_asset_snapshot_error(err, "asset lookup failed"))
    }

    ///
    /// ページとファイル名から通常アセットmetadataを解決する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `resolved` - 解決済みページ
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// MCP公開用metadataを返す。
    ///
    pub(super) fn resolve_page_asset_by_file(
        &self,
        db: &DatabaseManager,
        resolved: &ResolvedPage,
        file_name: &str,
    ) -> Result<PageAssetMetadata, McpError> {
        self.validate_asset_file_name(file_name)?;

        let resolved_page_id = resolved.page_id();
        let asset_id = match db.get_asset_id_by_page_file(
            &resolved_page_id,
            file_name,
        ) {
            Ok(Some(asset_id)) => asset_id,
            Ok(None) => {
                match db.has_deleted_asset_by_page_file(
                    &resolved_page_id,
                    file_name,
                ) {
                    Ok(_) => {
                        return Err(McpError::new(
                            McpErrorCode::NotFound,
                            "asset not found",
                        ));
                    }
                    Err(_) => {
                        return Err(McpError::new(
                            McpErrorCode::InternalError,
                            "asset lookup failed",
                        ));
                    }
                }
            }
            Err(_) => {
                return Err(McpError::new(
                    McpErrorCode::InternalError,
                    "asset lookup failed",
                ));
            }
        };

        let asset_info = match db.get_asset_info_by_id(&asset_id) {
            Ok(Some(info)) => info,
            Ok(None) => {
                return Err(McpError::new(
                    McpErrorCode::NotFound,
                    "asset not found",
                ));
            }
            Err(_) => {
                return Err(McpError::new(
                    McpErrorCode::InternalError,
                    "asset lookup failed",
                ));
            }
        };

        if asset_info.deleted()
            || asset_info.is_zombie()
            || asset_info.page_id().as_ref() != Some(&resolved_page_id)
            || asset_info.file_name() != file_name
        {
            return Err(McpError::new(
                McpErrorCode::NotFound,
                "asset not found",
            ));
        }

        self.build_page_asset_metadata(
            db,
            resolved.normalized_path(),
            &asset_info,
        )
    }

    ///
    /// 認可済みの通常ページを解決する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `operation` - 操作種別
    /// * `raw_path` - 対象ページ path
    ///
    /// # 戻り値
    /// 解決済みページを返す。
    ///
    pub(super) fn resolve_authorized_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        operation: McpOperation,
        raw_path: &str,
    ) -> Result<ResolvedPage, McpError> {
        let normalized_path =
            self.ensure_authorized_path(auth, operation, raw_path)?;
        self.resolve_page_by_path(db, &normalized_path)
    }

    ///
    /// `list_page_assets` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象ページ path
    ///
    /// # 戻り値
    /// ページに紐付く公開可能なアセットmetadata一覧を返す。
    ///
    pub(crate) fn list_page_assets(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
    ) -> Result<ListPageAssetsResult, McpError> {
        let resolved = self.resolve_authorized_page(
            auth,
            db,
            McpOperation::ListPageAssets,
            raw_path,
        )?;
        let (owner, records) = db.list_assets_authorized(
            crate::database::AssetPageSelector::Id(&resolved.page_id()),
            &PageAccessPolicy::new(auth.path_prefixes()),
        ).map_err(|err| map_asset_snapshot_error(err, "asset lookup failed"))?;
        let path = owner.path();
        let assets = records.iter().map(|record| {
            self.asset_metadata_from_record(&path, record)
        }).collect();
        Ok(ListPageAssetsResult::new(path, assets))
    }

    ///
    /// `resolve_page_asset` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// 解決されたアセットmetadataを返す。
    ///
    pub(crate) fn resolve_page_asset(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        file_name: &str,
    ) -> Result<ResolvePageAssetResult, McpError> {
        let resolved = self.resolve_authorized_page(
            auth,
            db,
            McpOperation::ResolvePageAsset,
            raw_path,
        )?;
        let snapshot = self.read_mcp_asset_snapshot(auth, db, &resolved,
            file_name, crate::database::AssetReadMode::Metadata)?;
        let path = snapshot.owner.path();
        let asset = self.asset_metadata_from_record(&path, &snapshot.record);
        Ok(ResolvePageAssetResult::new(path, asset))
    }

    ///
    /// `get_page_asset_metadata` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// アセットmetadataを返す。
    ///
    pub(crate) fn get_page_asset_metadata(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        file_name: &str,
    ) -> Result<GetPageAssetMetadataResult, McpError> {
        let resolved = self.resolve_authorized_page(
            auth,
            db,
            McpOperation::GetPageAssetMetadata,
            raw_path,
        )?;
        let snapshot = self.read_mcp_asset_snapshot(auth, db, &resolved,
            file_name, crate::database::AssetReadMode::Metadata)?;
        let path = snapshot.owner.path();
        let asset = self.asset_metadata_from_record(&path, &snapshot.record);
        Ok(GetPageAssetMetadataResult::new(path, asset))
    }

    ///
    /// `get_page_asset_data` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// アセットmetadataとbase64エンコード済み本体を返す。
    ///
    pub(crate) fn get_page_asset_data(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        file_name: &str,
    ) -> Result<GetPageAssetDataResult, McpError> {
        let resolved = self.resolve_authorized_page(
            auth,
            db,
            McpOperation::GetPageAssetData,
            raw_path,
        )?;
        let snapshot = self.read_mcp_asset_snapshot(auth, db, &resolved,
            file_name, crate::database::AssetReadMode::Data {
                max_bytes: Some(MAX_MCP_ASSET_DATA_SIZE),
            })?;
        let path = snapshot.owner.path();
        let asset = self.asset_metadata_from_record(&path, &snapshot.record);
        let data = snapshot.data.ok_or_else(|| {
            McpError::new(McpErrorCode::InternalError, "asset read failed")
        })?;
        let data_base64 =
            base64::engine::general_purpose::STANDARD.encode(&data);
        Ok(GetPageAssetDataResult::new(path, asset, data_base64))
    }

    ///
    /// `upload_page_asset` を実行する
    ///
    /// # 概要
    /// 対象ページの認可とロック状態を確認する。
    /// 入力されたアセットをDBと
    /// ファイルシステムへ保存し、
    /// 公開metadataを返す。
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象ページ path
    /// * `file_name` - アセットファイル名
    /// * `mime_type` - MIME 種別
    /// * `data_base64` - base64 エンコード済みアセット本体
    ///
    /// # 戻り値
    /// アップロードしたアセットの
    /// 公開metadataと要約を返す。
    ///
    pub(crate) fn upload_page_asset(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        file_name: &str,
        mime_type: &str,
        data_base64: &str,
    ) -> Result<UploadPageAssetResult, McpError> {
        /*
         * 対象ページの認可とロック状態の確認
         */
        let resolved = self
            .resolve_authorized_page(
                auth,
                db,
                McpOperation::UploadPageAsset,
                raw_path,
            )
            .map_err(|err| {
                map_asset_boundary_error(err, "page lookup failed")
            })?;
        let page_id = resolved.page_id();

        /*
         * 入力値の検証とアセット本体のデコード
         */
        self.validate_asset_file_name(file_name)?;
        self.validate_asset_mime_type(mime_type)?;
        let data = self.decode_asset_data_base64(data_base64)?;

        /*
         * アセットの作成と公開metadataの取得
         */
        let access = crate::database::MutationAccess {
            policy: PageAccessPolicy::new(auth.path_prefixes()),
            lock: crate::database::MutationLock::Unlocked,
            allow_draft: false, allow_deleted: false, cleanup_draft: false,
            expected_version: None,
        };
        let result = db
            .create_asset_authorized(
                crate::database::AssetPageSelector::Id(&page_id),
                file_name,
                mime_type,
                auth.user_id(),
                &data,
                &access,
            )
            .map_err(map_upload_asset_db_error)?;
        let asset = self
            .asset_metadata_from_record(&result.outcome.path, &result.value);
        Ok(UploadPageAssetResult::new(
            asset,
            "asset uploaded".to_string(),
        ))
    }

    ///
    /// `delete_page_asset` を実行する
    ///
    /// # 概要
    /// 対象ページの認可とロック状態を確認する。
    /// 通常アセットを解決し、DB上で論理削除する。
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象ページ path
    /// * `file_name` - アセットファイル名
    ///
    /// # 戻り値
    /// 削除したアセットの識別情報と要約を返す。
    ///
    pub(crate) fn delete_page_asset(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        file_name: &str,
    ) -> Result<DeletePageAssetResult, McpError> {
        /*
         * 対象ページの認可とロック状態の確認
         */
        let resolved = self
            .resolve_authorized_page(
                auth,
                db,
                McpOperation::DeletePageAsset,
                raw_path,
            )
            .map_err(|err| {
                map_asset_boundary_error(err, "page lookup failed")
            })?;
        self.validate_asset_file_name(file_name)?;
        let access = crate::database::MutationAccess {
            policy: PageAccessPolicy::new(auth.path_prefixes()),
            lock: crate::database::MutationLock::Unlocked,
            allow_draft: false, allow_deleted: false, cleanup_draft: false,
            expected_version: None,
        };
        let result = db
            .delete_asset_authorized(
                crate::database::AssetSelector::File {
                    page: crate::database::AssetPageSelector::Id(
                        &resolved.page_id(),
                    ),
                    file_name,
                },
                &access,
            )
            .map_err(map_delete_asset_db_error)?;
        Ok(DeletePageAssetResult::new(
            result.value.info.id().to_string(),
            result.outcome.path,
            result.value.info.file_name(),
            "asset deleted".to_string(),
        ))
    }

}

