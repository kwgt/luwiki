/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP pathベースサービス層の認可補助を定義するモジュール
//!

mod assets;
mod pages;
mod prompts;
mod resources;
mod sections;
mod types;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Error;
#[cfg(test)]
use base64::Engine;
use chrono::{DateTime, Local};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use serde_json::{Map as JsonObject, Value as JsonValue};

use crate::auth::{
    AuthContext, PageAccessPolicy, ScopeAuthorizationError, authorize_scope,
};
use crate::database::{
    AppendPageRequest,
    AppendPageResult,
    DatabaseManager,
    DbError,
    PageListEntry,
    ResourceSourceLookupResult,
};
use crate::database::resource_list::{
    DEFAULT_RESOURCE_MIME_TYPE,
    builtin_resource_contents,
    builtin_resource_list_entries,
    merge_resource_list_entries,
    page_resource_list_entry,
    page_resource_uri,
};
use crate::database::types::{
    AssetInfo,
    AssetId,
    BearerScope,
    PageId,
    PageIndex,
    TokenId,
    UserId,
};
use crate::fts::{self, FtsIndexConfig, FtsSearchTarget};
use crate::markdown_source::front_matter::{
    PromptPageFrontMatter,
    ResourceAclDefaultAction,
    ResourceAclFrontMatter,
    extract_front_matter,
    is_valid_prompt_argument_name,
    parse_front_matter,
    validate_prompt_name,
    validate_resource_path,
    validate_resource_path_shape,
};

use crate::wiki_path::{NormalizedWikiPath, path_matches_prefix};

use super::errors::{McpError, McpErrorCode};
use super::model::{
    GetPromptServiceResult,
    ListResourcesServiceResult,
    ListPromptsServiceResult,
    PromptListArgument,
    PromptListItem,
    ReadResourceServiceResult,
    ResourceListItem,
};

// 型と対応implを子モジュールへ集約し、既存の参照経路を再公開する。
pub(crate) use self::types::{
    AppendServiceResult,
    DeletePageAssetResult,
    EditPageInsertSectionPlacement,
    EditPageOperation,
    EditPageReplaceTextOccurrence,
    EditPageRequest,
    EditPageResult,
    GetPageAssetDataResult,
    GetPageAssetMetadataResult,
    GetPageResult,
    GetPageSectionResult,
    GetPageTocResult,
    ListPageAssetsResult,
    ListPageItem,
    ListPagesResult,
    McpOperation,
    PageAssetMetadata,
    PrefixFilterMode,
    ResolvePageAssetResult,
    ResolvedPage,
    ResolvedPrefixRequest,
    SearchPageItem,
    SearchPagesResult,
    SectionSelector,
    TocSection,
    UploadPageAssetResult,
    WritePageResult,
};

/// `list_pages` の既定件数
const DEFAULT_LIST_LIMIT: usize = 50;

/// `prompts/list`の既定件数
const DEFAULT_PROMPT_LIST_LIMIT: usize = 50;

/// `resources/list`の既定件数
const DEFAULT_RESOURCE_LIST_LIMIT: usize = 50;

/// resource URI authority の既定値
pub(crate) const DEFAULT_RESOURCE_AUTHORITY: &str = "local.luwiki";

/// `search_pages` の既定件数
const DEFAULT_SEARCH_LIMIT: usize = 20;

/// `list_pages` / `search_pages` の上限件数
const MAX_PAGE_RESULT_LIMIT: usize = 100;

/// `append` 競合待機の上限時間 (ミリ秒)
const APPEND_WAIT_TIMEOUT_MS: u64 = 1_000;

/// `append` 競合待機のポーリング間隔 (ミリ秒)
const APPEND_WAIT_INTERVAL_MS: u64 = 50;

/// アセット metadata のユーザ名欠落時代替値
const UNKNOWN_ASSET_USERNAME: &str = "unknown";

/// MCPで返却するアセットデータの最大サイズ
const MAX_MCP_ASSET_DATA_SIZE: u64 = 10 * 1024 * 1024;

/// MCPアセットuploadで受け付けるbase64入力の最大サイズ
const MAX_MCP_ASSET_DATA_BASE64_SIZE: usize = 13_981_016;

///
/// resource ACL の対象operation
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceAclOperation {
    /// resources/list
    List,

    /// resources/read
    Read,
}

///
/// Markdown 解析中の heading 情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedHeading {
    /// 動的 section ID
    id: String,

    /// 見出し文字列
    title: String,

    /// 見出しレベル
    level: u32,

    /// 文書順番号
    ordinal: u32,

    /// 親 section ID
    parent_id: Option<String>,

    /// 見出し開始位置
    heading_start: usize,

    /// セクション本文開始位置
    content_start: usize,

    /// セクション本文終了位置
    content_end: usize,
}

///
/// MCPサービス層
///
#[derive(Clone, Debug)]
pub(crate) struct McpService {
    /// resource URI authority
    resource_authority: String,
}

impl McpService {
    ///
    /// MCPサービス層の生成
    ///
    /// # 戻り値
    /// 生成したサービス層オブジェクトを返す。
    ///
    pub(crate) fn new() -> Self {
        Self::with_resource_authority(DEFAULT_RESOURCE_AUTHORITY.to_string())
    }

    ///
    /// resource authorityを指定してMCPサービス層を生成する
    ///
    /// # 引数
    /// * `resource_authority` - resource URI authority
    ///
    /// # 戻り値
    /// 生成したサービス層オブジェクトを返す。
    ///
    pub(crate) fn with_resource_authority(
        resource_authority: String,
    ) -> Self {
        Self {
            resource_authority,
        }
    }

    ///
    /// resource URI authorityを返す
    ///
    /// # 戻り値
    /// resource URI authorityを返す。
    ///
    pub(crate) fn resource_authority(&self) -> &str {
        &self.resource_authority
    }

    ///
    /// MCP 用の path 妥当性検証と正規化を行う
    ///
    /// # 引数
    /// * `raw_path` - クライアントから受け取った path
    ///
    /// # 戻り値
    /// 正規化済み絶対 path を返す。
    ///
    pub(crate) fn validate_and_normalize_path(
        &self,
        raw_path: &str,
    ) -> Result<String, McpError> {
        NormalizedWikiPath::parse(raw_path)
            .map(NormalizedWikiPath::into_string)
            .map_err(|err| {
                McpError::new(McpErrorCode::InvalidInput, err.to_string())
            })
    }

    ///
    /// 操作種別ごとの required scope を判定する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `operation` - 判定対象の操作種別
    ///
    /// # 戻り値
    /// required scope を満たす場合は `Ok(())` を返す。
    ///
    pub(crate) fn ensure_operation_scope(
        &self,
        auth: &AuthContext,
        operation: McpOperation,
    ) -> Result<(), McpError> {
        let required = operation.required_scope();
        authorize_scope(auth, required).map_err(|err| {
            if err.downcast_ref::<ScopeAuthorizationError>()
                == Some(&ScopeAuthorizationError::ReadOnlyDenied)
            {
                return McpError::forbidden_read_only();
            }
            McpError::new(
                McpErrorCode::Forbidden,
                format!("required scope denied: {}", required.as_str()),
            )
        })
    }

    ///
    /// 単一 path に対する path prefix 制約を判定する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `target_path` - 判定対象の正規化済み絶対 path
    ///
    /// # 戻り値
    /// 制約を満たす場合は `Ok(())` を返す。
    ///
    pub(crate) fn ensure_path_prefix_allowed(
        &self,
        auth: &AuthContext,
        target_path: &str,
    ) -> Result<(), McpError> {
        if self.is_path_prefix_allowed(auth, target_path) {
            return Ok(());
        }

        Err(McpError::new(
            McpErrorCode::Forbidden,
            format!("path prefix denied: {}", target_path),
        ))
    }

    ///
    /// 複数 path に対する path prefix 制約を判定する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `target_paths` - 判定対象 path 群
    ///
    /// # 戻り値
    /// すべての path が許可される場合は `Ok(())` を返す。
    ///
    pub(crate) fn ensure_all_path_prefixes_allowed<'a, I>(
        &self,
        auth: &AuthContext,
        target_paths: I,
    ) -> Result<(), McpError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        for target_path in target_paths {
            self.ensure_path_prefix_allowed(auth, target_path)?;
        }

        Ok(())
    }

    ///
    /// rename 用の複合認可判定を行う
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `current_path` - 移動元 current path
    /// * `rename_to` - 移動先 path
    ///
    /// # 戻り値
    /// 移動元 / 移動先の双方が許可される場合は `Ok(())` を返す。
    ///
    pub(crate) fn ensure_rename_authorized(
        &self,
        auth: &AuthContext,
        current_path: &str,
        rename_to: &str,
    ) -> Result<(), McpError> {
        self.ensure_operation_scope(auth, McpOperation::RenamePage)?;
        self.ensure_all_path_prefixes_allowed(auth, [current_path, rename_to])
    }

    ///
    /// current path からページ実体を解決する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `raw_path` - クライアントから受け取った path
    ///
    /// # 戻り値
    /// current path を解決できた場合は `ResolvedPage` を返す。
    ///
    /// # 概要
    /// 入力を共通解決し、従来のMCP状態制約と保存パスの整合を確認する。
    ///
    pub(crate) fn resolve_page_by_path(
        &self,
        db: &DatabaseManager,
        raw_path: &str,
    ) -> Result<ResolvedPage, McpError> {
        /*
         * path の妥当性検証と正規化
         */
        let normalized_path = self.validate_and_normalize_path(raw_path)?;

        /*
         * current path からページIDと index を解決
         */
        let page_id = match db.get_page_id_by_path(&normalized_path) {
            Ok(Some(page_id)) => page_id,
            Ok(None) => {
                return Err(McpError::new(
                    McpErrorCode::NotFound,
                    format!("page not found: {}", normalized_path),
                ));
            }
            Err(err) => {
                return Err(McpError::new(
                    McpErrorCode::InternalError,
                    format!("page id resolution failed: {}", err),
                ));
            }
        };
        let page_index = match db.get_page_index_by_id(&page_id) {
            Ok(Some(page_index)) => page_index,
            Ok(None) => {
                return Err(McpError::new(
                    McpErrorCode::InternalError,
                    format!("page index not found: {}", page_id),
                ));
            }
            Err(err) => {
                return Err(McpError::new(
                    McpErrorCode::InternalError,
                    format!("page index resolution failed: {}", err),
                ));
            }
        };

        /*
         * current path 状態の整合確認
         */
        match page_index.current_path() {
            Some(_) => {}
            None => {
                return Err(McpError::new(
                    McpErrorCode::NotFound,
                    format!(
                        "deleted page is not available: {}",
                        normalized_path
                    ),
                ));
            }
        };
        let current_path = page_index.authorization_path().map_err(|err| {
            McpError::new(McpErrorCode::InternalError, err.to_string())
        })?;
        if current_path.as_str() != normalized_path {
            return Err(McpError::new(
                McpErrorCode::InternalError,
                format!(
                    "current path mismatch: {} != {}",
                    current_path.as_str(),
                    normalized_path
                ),
            ));
        }
        if page_index.is_draft() {
            return Err(McpError::new(
                McpErrorCode::Conflict,
                format!("draft page is not supported: {}", normalized_path),
            ));
        }

        Ok(ResolvedPage::new(
            normalized_path,
            page_id,
            page_index.clone(),
            Some(page_index.latest()),
        ))
    }

    ///
    /// 初期版対象外の deleted ページ参照要求を拒否する
    ///
    /// # 引数
    /// * `raw_path` - 対象 path
    ///
    /// # 戻り値
    /// 常に `unsupported` を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn get_deleted_page(
        &self,
        raw_path: &str,
    ) -> Result<GetPageResult, McpError> {
        self.validate_and_normalize_path(raw_path)?;

        Err(McpError::new(
            McpErrorCode::Unsupported,
            "deleted page access is not supported",
        ))
    }

    ///
    /// 初期版対象外の restore 要求を拒否する
    ///
    /// # 引数
    /// * `raw_path` - 復旧対象 path
    /// * `raw_restore_to` - 復旧先 path
    ///
    /// # 戻り値
    /// 常に `unsupported` を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn restore_page(
        &self,
        raw_path: &str,
        raw_restore_to: &str,
    ) -> Result<WritePageResult, McpError> {
        self.validate_and_normalize_path(raw_path)?;
        self.validate_and_normalize_path(raw_restore_to)?;

        Err(McpError::new(
            McpErrorCode::Unsupported,
            "restore is not supported",
        ))
    }

    ///
    /// 初期版対象外の asset 操作要求を拒否する
    ///
    /// # 引数
    /// * `raw_path` - 対象ページ path
    ///
    /// # 戻り値
    /// 常に `unsupported` を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn asset_operation(
        &self,
        raw_path: &str,
    ) -> Result<(), McpError> {
        self.validate_and_normalize_path(raw_path)?;

        Err(McpError::new(
            McpErrorCode::Unsupported,
            "asset operation is not supported",
        ))
    }

    ///
    /// 初期版対象外の lock 操作要求を拒否する
    ///
    /// # 引数
    /// * `raw_path` - 対象ページ path
    ///
    /// # 戻り値
    /// 常に `unsupported` を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn lock_operation(
        &self,
        raw_path: &str,
    ) -> Result<(), McpError> {
        self.validate_and_normalize_path(raw_path)?;

        Err(McpError::new(
            McpErrorCode::Unsupported,
            "lock operation is not supported",
        ))
    }

    ///
    /// list / search 用の要求 prefix を解決する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `raw_prefix` - 正規化前の要求 prefix
    ///
    /// # 戻り値
    /// 解決済み要求 prefix を返す。
    ///
    pub(crate) fn resolve_prefix_request(
        &self,
        auth: &AuthContext,
        operation: McpOperation,
        raw_prefix: Option<&str>,
    ) -> Result<ResolvedPrefixRequest, McpError> {
        match raw_prefix {
            Some(prefix) => {
                let normalized_prefix =
                    self.validate_and_normalize_path(prefix)?;

                self.ensure_operation_scope(auth, operation)?;
                self.ensure_path_prefix_allowed(auth, &normalized_prefix)?;

                Ok(ResolvedPrefixRequest::new(
                    Some(normalized_prefix.clone()),
                    PrefixFilterMode::DescendantsOf(normalized_prefix),
                ))
            }
            None => Ok(ResolvedPrefixRequest::new(
                None,
                PrefixFilterMode::NoPrefix,
            )),
        }
    }

    ///
    /// list 用の要求 prefix を解決する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `raw_prefix` - 正規化前の要求 prefix
    ///
    /// # 戻り値
    /// list 用の解決済み要求 prefix を返す。
    ///
    pub(crate) fn resolve_list_prefix_request(
        &self,
        auth: &AuthContext,
        raw_prefix: Option<&str>,
    ) -> Result<ResolvedPrefixRequest, McpError> {
        self.resolve_prefix_request(auth, McpOperation::ListPages, raw_prefix)
    }

    ///
    /// search 用の要求 prefix を解決する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `raw_prefix` - 正規化前の要求 prefix
    ///
    /// # 戻り値
    /// search 用の解決済み要求 prefix を返す。
    ///
    pub(crate) fn resolve_search_prefix_request(
        &self,
        auth: &AuthContext,
        raw_prefix: Option<&str>,
    ) -> Result<ResolvedPrefixRequest, McpError> {
        self.resolve_prefix_request(
            auth,
            McpOperation::SearchPages,
            raw_prefix,
        )
    }

    ///
    /// 結果 path 群に後段フィルタを適用する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `paths` - current path 群
    ///
    /// # 戻り値
    /// path prefix 制約内の path だけを返す。
    ///
    pub(crate) fn filter_authorized_paths<'a, I>(
        &self,
        auth: &AuthContext,
        paths: I,
    ) -> Vec<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        paths.into_iter()
            .filter(|path| self.is_path_prefix_allowed(auth, path))
            .collect()
    }

    ///
    /// 要求 prefix 配下に属する path だけを返す
    ///
    /// # 引数
    /// * `resolved_prefix` - 解決済み要求 prefix
    /// * `paths` - current path 群
    ///
    /// # 戻り値
    /// 要求 prefix 配下に属する path だけを返す。
    ///
    pub(crate) fn filter_paths_by_request_prefix<'a, I>(
        &self,
        resolved_prefix: &ResolvedPrefixRequest,
        paths: I,
    ) -> Vec<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        match resolved_prefix.filter_mode() {
            PrefixFilterMode::NoPrefix => paths.into_iter().collect(),
            PrefixFilterMode::DescendantsOf(prefix) => paths
                .into_iter()
                .filter(|path| path_matches_prefix(path, prefix))
                .collect(),
        }
    }

    ///
    /// list / search 結果に複合後段フィルタを適用する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `resolved_prefix` - 解決済み要求 prefix
    /// * `paths` - current path 群
    ///
    /// # 戻り値
    /// path prefix 制約および要求 prefix 条件を満たす path だけを返す。
    ///
    pub(crate) fn filter_paths_for_prefix_request<'a, I>(
        &self,
        auth: &AuthContext,
        resolved_prefix: &ResolvedPrefixRequest,
        paths: I,
    ) -> Vec<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let authorized = self.filter_authorized_paths(auth, paths);
        self.filter_paths_by_request_prefix(resolved_prefix, authorized)
    }

    ///
    /// path prefix 制約に合致するかを返す
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `target_path` - 判定対象の正規化済み絶対 path
    ///
    /// # 戻り値
    /// 許可される場合は `true` を返す。
    ///
    pub(crate) fn is_path_prefix_allowed(
        &self,
        auth: &AuthContext,
        target_path: &str,
    ) -> bool {
        PageAccessPolicy::new(auth.path_prefixes()).allows(target_path)
    }

    ///
    /// 単一 path に対する認可付き正規化を行う
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `operation` - 操作種別
    /// * `raw_path` - 対象 path
    ///
    /// # 戻り値
    /// 認可済み正規化 path を返す。
    ///
    fn ensure_authorized_path(
        &self,
        auth: &AuthContext,
        operation: McpOperation,
        raw_path: &str,
    ) -> Result<String, McpError> {
        let normalized_path = self.validate_and_normalize_path(raw_path)?;
        self.ensure_operation_scope(auth, operation)?;
        self.ensure_path_prefix_allowed(auth, &normalized_path)?;
        Ok(normalized_path)
    }

    ///
    /// revision と page source を解決する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `resolved` - 解決済みページ
    /// * `revision` - 要求 revision
    ///
    /// # 戻り値
    /// `(revision, instance_id, source)` を返す。
    ///
    fn resolve_revision_source(
        &self,
        db: &DatabaseManager,
        resolved: &ResolvedPage,
        revision: Option<u64>,
    ) -> Result<(u64, String, String), McpError> {
        let revision = match revision {
            Some(0) => {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    "revision must be greater than zero",
                ));
            }
            Some(revision) => revision,
            None => resolved.latest_revision().ok_or_else(|| {
                McpError::new(
                    McpErrorCode::Conflict,
                    "draft page does not have latest revision",
                )
            })?,
        };
        let source = db
            .get_page_source(&resolved.page_id(), revision)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("page source lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::NotFound,
                    format!(
                        "revision not found: {}/{}",
                        resolved.normalized_path(),
                        revision
                    ),
                )
            })?;

        let instance_id = source.instance_id().ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "page source instance_id is missing",
            )
        })?;

        Ok((revision, instance_id.to_string(), source.source()))
    }

    ///
    /// 対象ページがロックされていないことを確認する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `page_id` - 確認対象ページID
    ///
    /// # 戻り値
    /// ロックされていない場合は `Ok(())` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    fn ensure_page_not_locked(
        &self,
        db: &DatabaseManager,
        page_id: &PageId,
    ) -> Result<(), McpError> {
        /*
         * ロック状態の確認
         */
        let lock_info = db.get_page_lock_info(page_id).map_err(|err| {
            McpError::new(
                McpErrorCode::InternalError,
                format!("lock lookup failed: {}", err),
            )
        })?;
        if lock_info.is_some() {
            return Err(McpError::new(
                McpErrorCode::Conflict,
                "page is locked",
            ));
        }

        Ok(())
    }

    ///
    /// update 系保存経路で本文全体を保存する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `resolved` - 解決済みページ情報
    /// * `user_id` - 保存主体ユーザID
    /// * `content` - 保存する Markdown 本文全体
    ///
    /// # 戻り値
    /// `(revision, instance_id)` を返す。
    ///
    /// # 注記
    /// `update_page` と `edit_page` はともに本 helper を通し、
    /// DB の `put_page(..., amend = false)` を使う update 系保存経路を再利用する。
    ///
    fn save_updated_page(
        &self, auth: &AuthContext, db: &DatabaseManager,
        resolved: &ResolvedPage, user_name: &str, content: String,
        expected_version: Option<(u64, &str)>,
    ) -> Result<(String, u64, String), McpError> {
        let access = crate::database::MutationAccess {
            policy: PageAccessPolicy::new(auth.path_prefixes()),
            lock: crate::database::MutationLock::Unlocked,
            allow_draft: false, allow_deleted: false, cleanup_draft: false,
            expected_version,
        };
        let result = db
            .put_page_authorized(
                &resolved.page_id(),
                user_name,
                content,
                false,
                &access,
            )
            .map_err(map_update_db_error)?;
        let revision = result.outcome.revision.ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "saved page revision missing",
            )
        })?;
        let instance = result.outcome.instance_id.ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "saved page source instance_id is missing",
            )
        })?;
        Ok((result.outcome.path, revision, instance))
    }

    ///
    /// 認証済みユーザ名から内部 `UserId` を解決する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `user_name` - 認証済みユーザ名
    ///
    /// # 戻り値
    /// 解決した `UserId` を返す。
    ///
    fn resolve_auth_user_id(
        &self,
        db: &DatabaseManager,
        user_name: &str,
    ) -> Result<UserId, McpError> {
        db.get_user_id_by_name(user_name)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("user lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    "user resolution failed",
                )
            })
    }

    ///
    /// `append` 競合確認状態を取得する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// 競合確認状態を返す。
    ///
    fn get_append_conflict_state(
        &self,
        auth: &AuthContext, db: &DatabaseManager,
        page_id: &PageId,
    ) -> Result<crate::database::AppendConflictState, McpError> {
        db.get_authorized_append_conflict_state(page_id,
            &PageAccessPolicy::new(auth.path_prefixes()))
            .map_err(|err| {
                if let Some(mapped) = map_shared_mutation_error(&err,
                    "append conflict state lookup failed") { return mapped; }
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("append conflict state lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(McpErrorCode::NotFound, "page not found")
            })
    }

    ///
    /// `append` の基準となる最新本文を解決する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `page_id` - 対象ページID
    /// * `revision` - 基準 revision
    ///
    /// # 戻り値
    /// 基準本文を返す。
    ///
    fn resolve_append_base_source(
        &self,
        db: &DatabaseManager,
        page_id: &PageId,
        revision: u64,
    ) -> Result<String, McpError> {
        db.get_page_source(page_id, revision)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("page source lookup failed: {}", err),
                )
            })?
            .map(|source| source.source())
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    "page source not found",
                )
            })
    }

    ///
    /// `append` の待機上限を超えたかを判定する
    ///
    /// # 引数
    /// * `started_at` - 待機開始時刻
    ///
    /// # 戻り値
    /// 上限を超えた場合は `true` を返す。
    ///
    fn append_wait_timed_out(&self, started_at: Instant) -> bool {
        started_at.elapsed()
            >= Duration::from_millis(APPEND_WAIT_TIMEOUT_MS)
    }

    ///
    /// `append` 結果モデルを組み立てる
    ///
    /// # 引数
    /// * `path` - current path
    /// * `result` - DB 保存結果
    ///
    /// # 戻り値
    /// MCP サービス層向け結果を返す。
    ///
    fn build_append_result(
        &self,
        db: &DatabaseManager,
        page_id: &PageId,
        path: &str,
        result: AppendPageResult,
    ) -> Result<AppendServiceResult, McpError> {
        let summary = if result.amended() {
            "page appended (amended)"
        } else {
            "page appended"
        };
        let instance_id = self.lookup_saved_instance_id(
            db,
            page_id,
            result.revision(),
            "appended",
        )?;

        Ok(AppendServiceResult::new(
            path.to_string(),
            result.revision(),
            instance_id,
            summary.to_string(),
            result.amended(),
        ))
    }

    ///
    /// 保存済み revision の instance_id を取得する
    ///
    /// # 引数
    /// * `db` - データベースマネージャ
    /// * `page_id` - 対象ページID
    /// * `revision` - 取得対象 revision
    /// * `action` - 失敗文言用の操作名
    ///
    /// # 戻り値
    /// 保存済み page source の instance_id を返す。
    ///
    fn lookup_saved_instance_id(
        &self,
        db: &DatabaseManager,
        page_id: &PageId,
        revision: u64,
        action: &str,
    ) -> Result<String, McpError> {
        let page_source = db
            .get_page_source(page_id, revision)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("{action} page source lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("{action} page source not found"),
                )
            })?;

        page_source.instance_id().map(|id| id.to_string()).ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                format!("{action} page source instance_id is missing"),
            )
        })
    }

    ///
    /// `append` で再試行可能な競合かを返す
    ///
    /// # 引数
    /// * `err` - DB 失敗
    ///
    /// # 戻り値
    /// 再試行可能な競合なら `true` を返す。
    ///
    fn is_retryable_append_conflict(&self, err: &Error) -> bool {
        if err
            .downcast_ref::<crate::database::MutationOutcome>()
            .is_some_and(|outcome| {
                outcome.committed && outcome.post_processing_failure
            })
        {
            return false;
        }
        err.downcast_ref::<DbError>().is_some_and(|db_err| {
            matches!(db_err, DbError::PageLocked | DbError::RevisionConflict)
        })
    }

    ///
    /// 一覧項目が MCP `list_pages` の返却対象かを返す
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `entry` - 一覧項目
    ///
    /// # 戻り値
    /// 返却対象の場合は `true` を返す。
    ///
    fn entry_visible_for_list(
        &self,
        auth: &AuthContext,
        entry: &PageListEntry,
    ) -> bool {
        !entry.deleted()
            && !entry.is_draft()
            && self.is_path_prefix_allowed(auth, &entry.path())
    }

    ///
    /// 件数上限を解決する
    ///
    /// # 引数
    /// * `limit` - 入力件数上限
    /// * `default_limit` - 既定件数
    ///
    /// # 戻り値
    /// 解決済み件数上限を返す。
    ///
    fn resolve_limit(
        &self,
        limit: Option<usize>,
        default_limit: usize,
    ) -> Result<usize, McpError> {
        let limit = limit.unwrap_or(default_limit);
        if !(1..=MAX_PAGE_RESULT_LIMIT).contains(&limit) {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "limit must be between 1 and 100",
            ));
        }

        Ok(limit)
    }

    ///
    /// 検索結果を `(page_id, revision)` 単位でマージする
    ///
    /// # 引数
    /// * `merged` - マージ先
    /// * `results` - 追加結果
    ///
    /// # 戻り値
    /// なし
    ///
    fn merge_search_results(
        &self,
        merged: &mut HashMap<(PageId, u64), fts::FtsSearchResult>,
        results: Vec<fts::FtsSearchResult>,
    ) {
        for result in results {
            let key = (result.page_id(), result.revision());
            let replace = match merged.get(&key) {
                Some(existing) => result.score() > existing.score(),
                None => true,
            };
            if replace {
                merged.insert(key, result);
            }
        }
    }

    ///
    /// 要求 prefix 条件に合致するかを返す
    ///
    /// # 引数
    /// * `resolved_prefix` - 解決済み要求 prefix
    /// * `target_path` - 判定対象 path
    ///
    /// # 戻り値
    /// 合致する場合は `true` を返す。
    ///
    fn matches_requested_prefix(
        &self,
        resolved_prefix: &ResolvedPrefixRequest,
        target_path: &str,
    ) -> bool {
        match resolved_prefix.filter_mode() {
            PrefixFilterMode::NoPrefix => true,
            PrefixFilterMode::DescendantsOf(prefix) => {
                path_matches_prefix(target_path, prefix)
            }
        }
    }

}

///
/// MCP 応答用の日時文字列へ変換する
///
/// # 引数
/// * `timestamp` - 変換対象日時
///
/// # 戻り値
/// 秒精度の日時文字列を返す。
///
fn format_mcp_timestamp(timestamp: DateTime<Local>) -> String {
    timestamp.format("%Y-%m-%dT%H:%M:%S").to_string()
}

///
/// FTS 検索失敗を MCP エラーへ写像する
///
/// # 引数
/// * `err` - FTS 失敗
///
/// # 戻り値
/// 写像した MCP エラーを返す。
///
fn map_search_error(err: Error) -> McpError {
    if err
        .downcast_ref::<tantivy::query::QueryParserError>()
        .is_some()
    {
        return McpError::new(
            McpErrorCode::InvalidInput,
            "query is invalid",
        );
    }

    McpError::new(
        McpErrorCode::InternalError,
        format!("search failed: {}", err),
    )
}

///
/// 同snapshot assetエラーを既存MCP固定形式へ変換する
///
/// # 引数
/// * `err` - DB/FS境界エラー
/// * `internal` - 既存操作固有内部失敗文言
///
/// # 概要
/// 共有commit/認可条件とasset固有の固定公開エラーを照合する。
///
/// # 戻り値
/// 情報秘匿を維持したMCPエラー。
///
fn map_asset_snapshot_error(err: Error, internal: &str) -> McpError {
    /*
     * 共有境界と操作固有条件を既存MCP形式へ写像する
     */
    if let Some(mapped) = map_shared_mutation_error(&err, internal) {
        return mapped;
    }
    if err
        .downcast_ref::<crate::database::AssetBodyReadFailure>()
        .is_some()
    {
        return McpError::new(McpErrorCode::InternalError, "asset read failed");
    }
    if err.downcast_ref::<crate::database::AssetDataTooLarge>().is_some() {
        return McpError::new(McpErrorCode::Conflict, "asset data too large");
    }
    if err
        .downcast_ref::<crate::database::AssetLookupMismatch>()
        .is_some()
        || err
            .downcast_ref::<crate::database::AssetLookupMetadataMissing>()
            .is_some()
    {
        return McpError::new(McpErrorCode::NotFound, "asset not found");
    }
    match err.downcast_ref::<DbError>() {
        Some(DbError::PageNotFound | DbError::PageDeleted) => McpError::new(
            McpErrorCode::NotFound, "page not found"),
        Some(DbError::AssetNotFound | DbError::AssetDeleted) => McpError::new(
            McpErrorCode::NotFound, "asset not found"),
        _ => McpError::new(McpErrorCode::InternalError, internal),
    }
}

///
/// DBのアセット作成失敗をMCPエラーへ写像する
///
/// # 引数
/// * `err` - DBまたはファイルシステムの失敗
///
/// # 戻り値
/// 公開可能な固定MCPエラーへ写像した結果を返す。
///
fn map_upload_asset_db_error(err: Error) -> McpError {
    if let Some(mapped) = map_shared_mutation_error(&err, "asset create failed")
    {
        return mapped;
    }
    match err.downcast_ref::<DbError>() {
        Some(DbError::PageNotFound) => {
            McpError::new(McpErrorCode::NotFound, "page not found")
        }
        Some(DbError::UserNotFound) => McpError::new(
            McpErrorCode::InternalError,
            "user resolution failed",
        ),
        Some(DbError::AssetAlreadyExists) => McpError::new(
            McpErrorCode::Conflict,
            "asset already exists",
        ),
        _ => McpError::new(
            McpErrorCode::InternalError,
            "asset create failed",
        ),
    }
}

///
/// アセット操作境界で内部エラーの詳細を
/// 固定文言へ縮退する
///
/// # 引数
/// * `err` - サービス内部エラー
/// * `internal_message` - 内部失敗時の固定公開文言
///
/// # 戻り値
/// 内部失敗だけを固定文言へ置換する。
/// その他のエラーは維持する。
///
fn map_asset_boundary_error(
    err: McpError,
    internal_message: &str,
) -> McpError {
    if err.code() == McpErrorCode::InternalError {
        return McpError::new(McpErrorCode::InternalError, internal_message);
    }

    err
}

///
/// DBのアセット削除失敗をMCPエラーへ写像する
///
/// # 引数
/// * `err` - DB失敗
///
/// # 戻り値
/// 公開可能な固定MCPエラーへ写像した結果を返す。
///
fn map_delete_asset_db_error(err: Error) -> McpError {
    if err
        .downcast_ref::<crate::database::AssetLookupMismatch>()
        .is_some()
        || err
            .downcast_ref::<crate::database::AssetLookupMetadataMissing>()
            .is_some()
    {
        return McpError::new(McpErrorCode::NotFound, "asset not found");
    }
    if let Some(mapped) = map_shared_mutation_error(&err, "asset delete failed")
    {
        return mapped;
    }
    match err.downcast_ref::<DbError>() {
        Some(DbError::AssetNotFound | DbError::AssetDeleted) => {
            McpError::new(McpErrorCode::NotFound, "asset not found")
        }
        _ => McpError::new(
            McpErrorCode::InternalError,
            "asset delete failed",
        ),
    }
}

///
/// 共有認可/状態/明示整合性エラーを既存MCP境界へ変換する
///
/// # 引数
/// * `err` - DBエラー
/// * `internal` - 操作固有の内部失敗文言
///
/// # 概要
/// commit後失敗を先に扱い、認可と既存状態の拒否を分類する。
///
/// # 戻り値
/// 既知の境界エラー、またはNone。
///
fn map_shared_mutation_error(err: &Error, internal: &str) -> Option<McpError> {
    /*
     * 共有境界と操作固有条件を既存MCP形式へ写像する
     */
    if err
        .downcast_ref::<crate::database::MutationOutcome>()
        .is_some_and(|outcome| {
            outcome.committed && outcome.post_processing_failure
        })
    {
        return Some(McpError::new(McpErrorCode::InternalError, internal));
    }
    if err
        .downcast_ref::<crate::database::PathPrefixDenied>()
        .is_some()
    {
        return Some(McpError::new(
            McpErrorCode::Forbidden,
            "path prefix is not allowed",
        ));
    }
    if let Some(version) =
        err.downcast_ref::<crate::database::MutationVersionError>()
    {
        return Some(match version {
            crate::database::MutationVersionError::Revision => McpError::new(
                McpErrorCode::NotLatestRevision, "revision is not latest"),
            crate::database::MutationVersionError::Instance => McpError::new(
                McpErrorCode::InstanceIdNotMatch,
                "instance_id does not match latest content",
            ),
        });
    }
    match err.downcast_ref::<DbError>() {
        Some(DbError::PageDeleted) => Some(McpError::new(
            McpErrorCode::NotFound, "page not found")),
        Some(DbError::DraftPage) => Some(McpError::new(
            McpErrorCode::Conflict, "draft page is not supported")),
        Some(DbError::PageLocked) => Some(McpError::new(
            McpErrorCode::Conflict, "page is locked")),
        _ => None,
    }
}

///
/// DB の作成失敗を MCP エラーへ写像する
///
/// # 引数
/// * `err` - DB 失敗
///
/// # 戻り値
/// MCP エラーへ写像した結果を返す。
///
fn map_create_db_error(err: Error) -> McpError {
    if let Some(mapped) = map_shared_mutation_error(&err, "page create failed")
    {
        return mapped;
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::PageAlreadyExists)
        })
    {
        return McpError::new(
            McpErrorCode::Conflict,
            "page already exists",
        );
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::UserNotFound)
        })
    {
        return McpError::new(
            McpErrorCode::InternalError,
            "user resolution failed",
        );
    }

    McpError::new(
        McpErrorCode::InternalError,
        format!("page create failed: {}", err),
    )
}

///
/// DB の更新失敗を MCP エラーへ写像する
///
/// # 引数
/// * `err` - DB 失敗
///
/// # 戻り値
/// MCP エラーへ写像した結果を返す。
///
fn map_update_db_error(err: Error) -> McpError {
    if let Some(mapped) = map_shared_mutation_error(&err, "page update failed")
    {
        return mapped;
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::PageNotFound)
        })
    {
        return McpError::new(McpErrorCode::NotFound, "page not found");
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::AmendForbidden)
        })
    {
        return McpError::new(
            McpErrorCode::Conflict,
            "append amend is not allowed",
        );
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::UserNotFound)
        })
    {
        return McpError::new(
            McpErrorCode::InternalError,
            "user resolution failed",
        );
    }

    McpError::new(
        McpErrorCode::InternalError,
        format!("page update failed: {}", err),
    )
}

///
/// DB の rename 失敗を MCP エラーへ写像する
///
/// # 引数
/// * `err` - DB 失敗
///
/// # 戻り値
/// MCP エラーへ写像した結果を返す。
///
fn map_rename_db_error(err: Error) -> McpError {
    if let Some(mapped) = map_shared_mutation_error(&err, "page rename failed")
    {
        return mapped;
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::PageNotFound)
        })
    {
        return McpError::new(McpErrorCode::NotFound, "page not found");
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::PageAlreadyExists)
        })
    {
        return McpError::new(
            McpErrorCode::Conflict,
            "page already exists",
        );
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::PageLocked)
        })
    {
        return McpError::new(McpErrorCode::Conflict, "page is locked");
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(
                db_err,
                crate::database::DbError::InvalidMoveDestination
            )
        })
    {
        return McpError::new(
            McpErrorCode::InvalidInput,
            "invalid destination path",
        );
    }
    if err
        .downcast_ref::<crate::database::DbError>()
        .is_some_and(|db_err| {
            matches!(db_err, crate::database::DbError::RootPageProtected)
        })
    {
        return McpError::new(
            McpErrorCode::Forbidden,
            "operation is not allowed for root page",
        );
    }

    McpError::new(
        McpErrorCode::InternalError,
        format!("page rename failed: {}", err),
    )
}

///
/// DB の `append` 失敗を MCP エラーへ写像する
///
/// # 引数
/// * `err` - DB 失敗
///
/// # 戻り値
/// MCP エラーへ写像した結果を返す。
///
fn map_append_db_error(err: Error) -> McpError {
    if let Some(mapped) = map_shared_mutation_error(&err, "page append failed")
    {
        return mapped;
    }
    if err.downcast_ref::<DbError>().is_some_and(|db_err| {
        matches!(db_err, DbError::PageNotFound)
    }) {
        return McpError::new(McpErrorCode::NotFound, "page not found");
    }
    if err.downcast_ref::<DbError>().is_some_and(|db_err| {
        matches!(db_err, DbError::DraftPage)
    }) {
        return McpError::new(
            McpErrorCode::Conflict,
            "draft page is not supported",
        );
    }
    if err.downcast_ref::<DbError>().is_some_and(|db_err| {
        matches!(
            db_err,
            DbError::PageLocked
                | DbError::RevisionConflict
                | DbError::AmendForbidden
        )
    }) {
        return McpError::new(McpErrorCode::Conflict, "append conflict");
    }
    if err.downcast_ref::<DbError>().is_some_and(|db_err| {
        matches!(db_err, DbError::UserNotFound)
    }) {
        return McpError::new(
            McpErrorCode::InternalError,
            "user resolution failed",
        );
    }

    McpError::new(
        McpErrorCode::InternalError,
        format!("page append failed: {}", err),
    )
}

#[cfg(test)]
mod tests;
