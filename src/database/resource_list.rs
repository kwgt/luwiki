/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! resource 一覧用の内部モデル変換処理を提供するモジュール
//!

use std::collections::HashSet;

use anyhow::{Result, anyhow};

use crate::database::entries::{
    ResourceCandidateListEntry,
    ResourceListEntry,
    ResourceListSource,
};

/// resource MIME type の既定値
pub(crate) const DEFAULT_RESOURCE_MIME_TYPE: &str = "text/markdown";

const BUILTIN_FRONT_MATTER_SPEC_ID: &str = "front-matter-spec";
const BUILTIN_MCP_PROMPT_SPEC_ID: &str = "mcp-prompt-spec";

///
/// 固定組み込みresource本文
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BuiltinResourceContents {
    /// MCP公開URI
    uri: String,

    /// MIME type
    mime_type: String,

    /// 本文
    text: String,
}

impl BuiltinResourceContents {
    ///
    /// 固定組み込みresource本文を生成する
    ///
    /// # 引数
    /// * `uri` - MCP公開URI
    /// * `mime_type` - MIME type
    /// * `text` - 本文
    ///
    /// # 戻り値
    /// 固定組み込みresource本文を返す。
    ///
    fn new(uri: String, mime_type: String, text: String) -> Self {
        Self {
            uri,
            mime_type,
            text,
        }
    }

    ///
    /// MCP公開URIを返す
    ///
    /// # 戻り値
    /// MCP公開URIを返す。
    ///
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    ///
    /// MIME typeを返す
    ///
    /// # 戻り値
    /// MIME typeを返す。
    ///
    pub(crate) fn mime_type(&self) -> &str {
        &self.mime_type
    }

    ///
    /// 本文を返す
    ///
    /// # 戻り値
    /// 本文を返す。
    ///
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

///
/// ページ由来resourceのMCP公開URIを生成する
///
/// # 引数
/// * `authority` - resource authority
/// * `resource_path` - resource path
///
/// # 戻り値
/// MCP公開URIを返す。
///
pub(crate) fn page_resource_uri(
    authority: &str,
    resource_path: &str,
) -> String {
    format!("luwiki://{}{}", authority, resource_path)
}

///
/// 固定組み込みresourceのMCP公開URIを生成する
///
/// # 引数
/// * `authority` - resource authority
/// * `builtin_id` - 固定組み込みresource識別子
///
/// # 戻り値
/// MCP公開URIを返す。
///
pub(crate) fn builtin_resource_uri(
    authority: &str,
    builtin_id: &str,
) -> String {
    format!("luwiki://{}/builtin/{}", authority, builtin_id)
}

///
/// 固定組み込みresource一覧を生成する
///
/// # 引数
/// * `authority` - resource authority
///
/// # 戻り値
/// 固定組み込みresourceの内部一覧エントリを返す。
///
pub(crate) fn builtin_resource_list_entries(
    authority: &str,
) -> Vec<ResourceListEntry> {
    vec![
        ResourceListEntry::new(
            builtin_resource_uri(authority, BUILTIN_FRONT_MATTER_SPEC_ID),
            "Front Matter Specification".to_string(),
            "LuWiki front matter specification".to_string(),
            DEFAULT_RESOURCE_MIME_TYPE.to_string(),
            ResourceListSource::Builtin,
            None,
            None,
        ),
        ResourceListEntry::new(
            builtin_resource_uri(authority, BUILTIN_MCP_PROMPT_SPEC_ID),
            "MCP Prompt Specification".to_string(),
            "LuWiki MCP prompt specification".to_string(),
            DEFAULT_RESOURCE_MIME_TYPE.to_string(),
            ResourceListSource::Builtin,
            None,
            None,
        ),
    ]
}

///
/// 固定組み込みresource本文を取得する
///
/// # 引数
/// * `authority` - resource authority
/// * `builtin_id` - 固定組み込みresource識別子
///
/// # 戻り値
/// 固定組み込みresourceが存在する場合は本文を返す。
///
pub(crate) fn builtin_resource_contents(
    authority: &str,
    builtin_id: &str,
) -> Option<BuiltinResourceContents> {
    match builtin_id {
        BUILTIN_FRONT_MATTER_SPEC_ID => Some(BuiltinResourceContents::new(
            builtin_resource_uri(authority, BUILTIN_FRONT_MATTER_SPEC_ID),
            DEFAULT_RESOURCE_MIME_TYPE.to_string(),
            include_str!("../../docs/FRONT_MATTER_SPECS.md").to_string(),
        )),
        BUILTIN_MCP_PROMPT_SPEC_ID => Some(BuiltinResourceContents::new(
            builtin_resource_uri(authority, BUILTIN_MCP_PROMPT_SPEC_ID),
            DEFAULT_RESOURCE_MIME_TYPE.to_string(),
            include_str!("../../docs/MCP_PROMPT_SPECS.md").to_string(),
        )),
        _ => None,
    }
}

///
/// ページ由来resource候補をresource一覧エントリへ変換する
///
/// # 引数
/// * `authority` - resource authority
/// * `candidate` - ページ由来resource候補
///
/// # 戻り値
/// resource一覧エントリを返す。
///
pub(crate) fn page_resource_list_entry(
    authority: &str,
    candidate: &ResourceCandidateListEntry,
) -> ResourceListEntry {
    ResourceListEntry::new(
        page_resource_uri(authority, candidate.resource_path()),
        candidate.name().to_string(),
        candidate.description().to_string(),
        candidate.mime_type().to_string(),
        ResourceListSource::Page,
        Some(candidate.page_id()),
        Some(candidate.current_path().to_string()),
    )
}

///
/// 固定組み込みresourceとページ由来resourceを合流する
///
/// # 引数
/// * `builtin_entries` - 固定組み込みresource一覧
/// * `page_entries` - ページ由来resource一覧
///
/// # 戻り値
/// URI昇順にソート済みのresource一覧を返す。
///
pub(crate) fn merge_resource_list_entries(
    builtin_entries: Vec<ResourceListEntry>,
    page_entries: Vec<ResourceListEntry>,
) -> Result<Vec<ResourceListEntry>> {
    let mut uris = HashSet::new();
    let mut entries = Vec::new();
    for entry in builtin_entries.into_iter().chain(page_entries) {
        if !uris.insert(entry.uri().to_string()) {
            return Err(anyhow!(
                "duplicate resource URI in list: uri={}",
                entry.uri(),
            ));
        }
        entries.push(entry);
    }

    entries.sort_by(|left, right| left.uri().cmp(right.uri()));

    Ok(entries)
}
