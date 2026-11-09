/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPツール呼び出し入口と公開エラー写像を定義するモジュール
//!

mod assets;
mod audit;
mod pages;
mod prompts;
mod resources;

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

use self::audit::{
    audit_result_from_error,
    build_append_page_audit_record,
    build_delete_page_asset_audit_record,
    build_edit_page_audit_record,
    build_error_audit_record,
    build_get_page_asset_data_audit_record,
    build_get_page_asset_metadata_audit_record,
    build_get_page_audit_record,
    build_get_page_section_audit_record,
    build_get_page_toc_audit_record,
    build_get_prompt_audit_record,
    build_get_prompt_error_audit_record,
    build_list_page_assets_audit_record,
    build_list_pages_audit_record,
    build_list_prompts_audit_record,
    build_list_prompts_error_audit_record,
    build_list_resources_audit_record,
    build_list_resources_error_audit_record,
    build_read_resource_audit_record,
    build_read_resource_error_audit_record,
    build_rename_page_audit_record,
    build_resolve_page_asset_audit_record,
    build_search_pages_audit_record,
    build_upload_page_asset_audit_record,
    build_write_page_audit_record,
    normalize_asset_update_audit_result,
    resolve_audit_user_id,
};

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

#[cfg(test)]
mod tests;
