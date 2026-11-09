/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPサービスのresources処理を定義するモジュール
//!

use super::{
    AuthContext,
    DEFAULT_RESOURCE_LIST_LIMIT,
    DEFAULT_RESOURCE_MIME_TYPE,
    DatabaseManager,
    ListResourcesServiceResult,
    McpError,
    McpErrorCode,
    McpOperation,
    McpService,
    ReadResourceServiceResult,
    ResourceAclDefaultAction,
    ResourceAclFrontMatter,
    ResourceAclOperation,
    ResourceListItem,
    ResourceSourceLookupResult,
    TokenId,
    builtin_resource_contents,
    builtin_resource_list_entries,
    extract_front_matter,
    merge_resource_list_entries,
    page_resource_list_entry,
    page_resource_uri,
    parse_front_matter,
    validate_resource_path,
    validate_resource_path_shape,
};

// 可読性確保のため、業務別のimplを子モジュールへ分離する。
impl McpService {
    ///
    /// `resources/list`を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// resource一覧取得結果を返す。
    ///
    pub(crate) fn list_resources(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        cursor: Option<&str>,
    ) -> Result<ListResourcesServiceResult, McpError> {
        /*
         * read scopeとcursorを検証する
         */
        self.ensure_operation_scope(auth, McpOperation::ListResources)?;
        if let Some(cursor) = cursor {
            validate_resource_list_cursor(
                cursor,
                self.resource_authority(),
            )?;
        }

        /*
         * 固定組み込みresourceとACL許可済みページ由来resourceを合流する
         */
        let mut page_entries = Vec::new();
        for entry in db.list_resource_candidates().map_err(|_| {
            McpError::new(McpErrorCode::InternalError, "internal error")
        })? {
            let lookup = db
                .get_resource_source_by_path(entry.resource_path())
                .map_err(|_| resource_internal_error())?;
            let source_entry = match lookup {
                ResourceSourceLookupResult::Found(source_entry) => source_entry,
                ResourceSourceLookupResult::NotFound
                | ResourceSourceLookupResult::Unavailable => continue,
                ResourceSourceLookupResult::Inconsistent => {
                    return Err(resource_internal_error());
                }
            };
            let extracted = extract_front_matter(source_entry.source())
                .map_err(|_| resource_internal_error())?
                .ok_or_else(resource_internal_error)?;
            let front_matter = parse_front_matter(extracted.front_matter())
                .map_err(|_| resource_internal_error())?;
            let resource = front_matter
                .resource_page()
                .ok_or_else(resource_internal_error)?;
            if !resource_acl_allows(
                auth,
                resource.resource_acl(),
                ResourceAclOperation::List,
            ) {
                continue;
            }

            page_entries.push(page_resource_list_entry(
                self.resource_authority(),
                &entry,
            ));
        }
        let entries = merge_resource_list_entries(
            builtin_resource_list_entries(self.resource_authority()),
            page_entries,
        )
        .map_err(|_| {
            McpError::new(
                McpErrorCode::InternalError,
                "internal error",
            )
        })?;

        /*
         * cursor境界と既定件数を適用する
         */
        let filtered = entries
            .into_iter()
            .filter(|entry| match cursor {
                Some(cursor) => entry.uri() > cursor,
                None => true,
            })
            .take(DEFAULT_RESOURCE_LIST_LIMIT + 1)
            .collect::<Vec<_>>();
        let has_more = filtered.len() > DEFAULT_RESOURCE_LIST_LIMIT;
        let selected = filtered
            .into_iter()
            .take(DEFAULT_RESOURCE_LIST_LIMIT)
            .collect::<Vec<_>>();
        let next_cursor = if has_more {
            selected.last().map(|entry| entry.uri().to_string())
        } else {
            None
        };

        /*
         * database一覧エントリをMCP内部モデルへ変換する
         */
        let items = selected
            .into_iter()
            .map(|entry| {
                ResourceListItem::new(
                    entry.uri().to_string(),
                    entry.name().to_string(),
                    entry.description().to_string(),
                    entry.mime_type().to_string(),
                )
            })
            .collect();

        Ok(ListResourcesServiceResult::new(items, next_cursor))
    }

    ///
    /// `resources/read`を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// resource取得結果を返す。
    ///
    pub(crate) fn read_resource(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        uri: &str,
    ) -> Result<ReadResourceServiceResult, McpError> {
        /*
         * read scopeとURIを検証する
         */
        self.ensure_operation_scope(auth, McpOperation::ReadResource)?;
        let target = parse_resource_read_uri(
            uri,
            self.resource_authority(),
        )?;

        match target {
            ResourceUriTarget::Builtin { builtin_id } => {
                let contents = builtin_resource_contents(
                    self.resource_authority(),
                    &builtin_id,
                )
                .ok_or_else(resource_not_found)?;

                Ok(ReadResourceServiceResult::new(
                    contents.uri().to_string(),
                    contents.mime_type().to_string(),
                    contents.text().to_string(),
                    None,
                ))
            }
            ResourceUriTarget::Page { resource_path } => {
                let lookup = db
                    .get_resource_source_by_path(&resource_path)
                    .map_err(|_| resource_internal_error())?;
                let entry = match lookup {
                    ResourceSourceLookupResult::Found(entry) => entry,
                    ResourceSourceLookupResult::NotFound
                    | ResourceSourceLookupResult::Unavailable => {
                        return Err(resource_not_found());
                    }
                    ResourceSourceLookupResult::Inconsistent => {
                        return Err(resource_internal_error());
                    }
                };
                let extracted = extract_front_matter(entry.source())
                    .map_err(|_| resource_internal_error())?
                    .ok_or_else(resource_internal_error)?;
                let front_matter = parse_front_matter(
                    extracted.front_matter(),
                )
                .map_err(|_| resource_internal_error())?;
                let resource = front_matter
                    .resource_page()
                    .ok_or_else(resource_internal_error)?;
                let actual_resource_path = match resource.resource_path() {
                    Some(resource_path) => resource_path.to_string(),
                    None => resource_path_from_current_path(
                        entry.current_path(),
                    )?,
                };
                if actual_resource_path != resource_path {
                    return Err(resource_internal_error());
                }
                if !resource_acl_allows(
                    auth,
                    resource.resource_acl(),
                    ResourceAclOperation::Read,
                ) {
                    return Err(resource_not_found());
                }

                let mime_type = resource
                    .mime_type()
                    .unwrap_or(DEFAULT_RESOURCE_MIME_TYPE)
                    .to_string();

                Ok(ReadResourceServiceResult::new(
                    page_resource_uri(
                        self.resource_authority(),
                        &resource_path,
                    ),
                    mime_type,
                    extracted.body().to_string(),
                    Some(entry.revision()),
                ))
            }
        }
    }

}

///
/// resources/list 用 cursor の妥当性検証を行う
///
/// # 引数
/// * `cursor` - 検証対象 cursor
///
/// # 戻り値
/// 検証に成功した場合は `Ok(())` を返す。
///
fn validate_resource_list_cursor(
    cursor: &str,
    expected_authority: &str,
) -> Result<(), McpError> {
    let invalid_cursor = || {
        McpError::new(
            McpErrorCode::InvalidInput,
            "cursor is invalid",
        )
    };

    /*
     * 空値、境界空白、制御文字、最大長を検証する
     */
    if cursor.trim().is_empty() || cursor.trim() != cursor {
        return Err(invalid_cursor());
    }
    if cursor.chars().any(char::is_control) {
        return Err(invalid_cursor());
    }
    let max_cursor_chars = format!("luwiki://{}", expected_authority)
        .chars()
        .count()
        + 512;
    if cursor.chars().count() > max_cursor_chars {
        return Err(invalid_cursor());
    }

    /*
     * LuWiki resource URI として扱える形かを検証する
     */
    let Some(rest) = cursor.strip_prefix("luwiki://") else {
        return Err(invalid_cursor());
    };
    let Some(path_start) = rest.find('/') else {
        return Err(invalid_cursor());
    };
    let authority = &rest[..path_start];
    if authority != expected_authority {
        return Err(invalid_cursor());
    }
    let path = &rest[path_start..];

    if let Some(builtin_id) = path.strip_prefix("/builtin/") {
        if builtin_id.is_empty()
            || builtin_id.contains('/')
            || builtin_id.trim() != builtin_id
            || builtin_id.chars().any(char::is_control)
        {
            return Err(invalid_cursor());
        }

        return Ok(());
    }

    validate_resource_path(path).map_err(|_| invalid_cursor())
}

///
/// resources/read 対象URI
///
#[derive(Clone, Debug, Eq, PartialEq)]
enum ResourceUriTarget {
    /// 固定組み込みresource
    Builtin {
        /// 固定組み込みresource識別子
        builtin_id: String,
    },

    /// ページ由来resource
    Page {
        /// resource path
        resource_path: String,
    },
}

///
/// resources/read 用URIを分解する
///
/// # 引数
/// * `uri` - resource URI
///
/// # 戻り値
/// 解決対象のresource種別と識別子を返す。
///
fn parse_resource_read_uri(
    uri: &str,
    expected_authority: &str,
) -> Result<ResourceUriTarget, McpError> {
    /*
     * 空値、境界空白、制御文字を検証する
     */
    if uri.trim().is_empty()
        || uri.trim() != uri
        || uri.chars().any(char::is_control)
    {
        return Err(resource_uri_invalid());
    }

    /*
     * LuWiki resource URIとして分解する
     */
    let Some(rest) = uri.strip_prefix("luwiki://") else {
        return Err(resource_uri_invalid());
    };
    let Some(path_start) = rest.find('/') else {
        return Err(resource_uri_invalid());
    };
    let authority = &rest[..path_start];
    if authority != expected_authority {
        return Err(resource_not_found());
    }
    let path = &rest[path_start..];

    if let Some(builtin_id) = path.strip_prefix("/builtin/") {
        if builtin_id.is_empty()
            || builtin_id.contains('/')
            || builtin_id.trim() != builtin_id
            || builtin_id.chars().any(char::is_control)
        {
            return Err(resource_not_found());
        }

        return Ok(ResourceUriTarget::Builtin {
            builtin_id: builtin_id.to_string(),
        });
    }

    validate_resource_path(path).map_err(|_| resource_uri_invalid())?;

    Ok(ResourceUriTarget::Page {
        resource_path: path.to_string(),
    })
}

///
/// current pathからresource_pathを導出する
///
/// # 引数
/// * `current_path` - 対象ページのcurrent path
///
/// # 戻り値
/// 導出したresource_pathを返す。
///
fn resource_path_from_current_path(
    current_path: &str,
) -> Result<String, McpError> {
    let path_without_root = current_path
        .strip_prefix('/')
        .unwrap_or(current_path);
    let resource_path = format!("/pages/{}", path_without_root);
    validate_resource_path_shape(&resource_path)
        .map_err(|_| resource_internal_error())?;

    Ok(resource_path)
}



///
/// resource ACL がoperationを許可するかを判定する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `acl` - resource ACL
/// * `operation` - 判定対象operation
///
/// # 戻り値
/// 許可される場合は `true` を返す。
///
fn resource_acl_allows(
    auth: &AuthContext,
    acl: Option<&ResourceAclFrontMatter>,
    operation: ResourceAclOperation,
) -> bool {
    let Some(acl) = acl else {
        return true;
    };
    let operation_acl = match operation {
        ResourceAclOperation::List => acl.list(),
        ResourceAclOperation::Read => acl.read(),
    };
    let default_action = match operation {
        ResourceAclOperation::List => acl.default_list(),
        ResourceAclOperation::Read => acl.default_read(),
    }
    .unwrap_or(ResourceAclDefaultAction::Allow);

    if let Some(operation_acl) = operation_acl {
        if operation_acl
            .deny()
            .iter()
            .any(|principal| resource_acl_principal_matches(auth, principal))
        {
            return false;
        }
        if operation_acl
            .allow()
            .iter()
            .any(|principal| resource_acl_principal_matches(auth, principal))
        {
            return true;
        }
    }

    matches!(default_action, ResourceAclDefaultAction::Allow)
}

///
/// resource ACL principal が認証tokenに一致するかを判定する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `principal` - token ID または token name
///
/// # 戻り値
/// 一致する場合は `true` を返す。
///
fn resource_acl_principal_matches(
    auth: &AuthContext,
    principal: &str,
) -> bool {
    if TokenId::from_string(principal).is_ok() {
        return auth
            .token_id()
            .is_some_and(|token_id| token_id.to_string() == principal);
    }

    auth.token_name()
        .is_some_and(|token_name| token_name == principal)
}

///
/// resource URI不正エラーを生成する
///
/// # 戻り値
/// resource URI不正エラーを返す。
///
fn resource_uri_invalid() -> McpError {
    McpError::new(
        McpErrorCode::InvalidInput,
        "resource uri is invalid",
    )
}

///
/// resource取得の不存在エラーを生成する
///
/// # 戻り値
/// 情報を秘匿したresource不存在エラーを返す。
///
fn resource_not_found() -> McpError {
    McpError::new(McpErrorCode::NotFound, "resource not found")
}

///
/// resource取得の内部エラーを生成する
///
/// # 戻り値
/// 内部情報を秘匿したエラーを返す。
///
fn resource_internal_error() -> McpError {
    McpError::new(McpErrorCode::InternalError, "internal error")
}

