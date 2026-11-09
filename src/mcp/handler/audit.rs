/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPハンドラの監査レコード生成と分類を定義するモジュール
//!

use super::{
    AppendServiceResult,
    AuditOperation,
    AuditRecord,
    AuditResult,
    AuthContext,
    DatabaseManager,
    DeletePageAssetResult,
    EditPageResult,
    GetPageAssetDataResult,
    GetPageAssetMetadataResult,
    GetPageResult,
    GetPageSectionResult,
    GetPageTocResult,
    GetPromptServiceResult,
    IpAddr,
    ListPageAssetsResult,
    ListPagesResult,
    ListPromptsServiceResult,
    ListResourcesServiceResult,
    McpError,
    McpRequestEnvelope,
    McpToolRequest,
    ReadResourceServiceResult,
    ResolvePageAssetResult,
    SearchPagesResult,
    UploadPageAssetResult,
    UserId,
    Utc,
    WritePageResult,
    validate_prompt_name,
    validate_resource_path,
    warn,
};

pub(super) fn resolve_audit_user_id(
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

pub(super) fn build_get_page_audit_record(
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

pub(super) fn build_get_page_toc_audit_record(
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

pub(super) fn build_list_pages_audit_record(
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
pub(super) fn build_list_prompts_audit_record(
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
pub(super) fn build_list_prompts_error_audit_record(
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
pub(super) fn build_list_resources_audit_record(
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
pub(super) fn build_list_resources_error_audit_record(
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
pub(super) fn build_read_resource_audit_record(
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
pub(super) fn build_read_resource_error_audit_record(
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
pub(super) fn build_get_prompt_audit_record(
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
pub(super) fn build_get_prompt_error_audit_record(
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

pub(super) fn build_search_pages_audit_record(
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

pub(super) fn build_write_page_audit_record(
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

pub(super) fn build_edit_page_audit_record(
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

pub(super) fn build_append_page_audit_record(
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

pub(super) fn build_rename_page_audit_record(
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

pub(super) fn build_get_page_section_audit_record(
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
pub(super) fn build_list_page_assets_audit_record(
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
pub(super) fn build_resolve_page_asset_audit_record(
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
pub(super) fn build_get_page_asset_metadata_audit_record(
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
pub(super) fn build_get_page_asset_data_audit_record(
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
pub(super) fn build_upload_page_asset_audit_record(
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
pub(super) fn build_delete_page_asset_audit_record(
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

pub(super) fn build_error_audit_record(
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
pub(super) fn normalize_asset_update_audit_result(
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

pub(super) fn audit_result_from_error(error: &McpError) -> AuditResult {
    use crate::mcp::errors::McpErrorCode;

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

