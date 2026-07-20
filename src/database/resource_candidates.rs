/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! resource 候補派生データ生成処理を提供するモジュール
//!

use anyhow::Result;

use crate::database::resource_uris::derive_resource_path_from_path;
use crate::database::types::ResourceCandidateEntry;
use crate::markdown_source::front_matter::{
    ResourcePageFrontMatter,
    extract_resource_page_front_matter,
};

///
/// resource ページ情報から resource 候補派生データを生成する
///
/// # 引数
/// * `resource_path` - 検証済み resource path
/// * `resource` - resource ページ情報
///
/// # 戻り値
/// 生成した resource 候補派生データを返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_resource_candidate_entry(
    resource_path: String,
    resource: &ResourcePageFrontMatter,
) -> ResourceCandidateEntry {
    ResourceCandidateEntry::new(
        resource_path,
        resource.name().to_string(),
        resource.description().to_string(),
        resource.mime_type().map(str::to_string),
    )
}

///
/// Markdown ソースから resource 候補派生データを生成する
///
/// # 引数
/// * `current_path` - 対象ページの current path
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// resource ページでない場合は `Ok(None)` を返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_resource_candidate_entry_from_source(
    current_path: &str,
    source: &str,
) -> Result<Option<ResourceCandidateEntry>> {
    let Some(resource) = extract_resource_page_front_matter(source)? else {
        return Ok(None);
    };
    let resource_path = match resource.resource_path() {
        Some(resource_path) => resource_path.to_string(),
        None => derive_resource_path_from_path(current_path)?,
    };

    Ok(Some(build_resource_candidate_entry(
        resource_path,
        &resource,
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        build_resource_candidate_entry,
        build_resource_candidate_entry_from_source,
    };
    use crate::markdown_source::front_matter::{
        extract_resource_page_front_matter,
    };

    ///
    /// resource ページ情報の全フィールドを候補へ
    /// 射影できることを確認する。
    ///
    #[test]
    fn build_resource_candidate_entry_copies_resource_attributes() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_path: /docs/spec\n",
            "  name: spec\n",
            "  description: resource description\n",
            "  mime_type: text/markdown\n",
            "---\n",
            "本文",
        );
        let resource = extract_resource_page_front_matter(source)
            .expect("extract failed")
            .expect("resource page missing");

        let entry =
            build_resource_candidate_entry("/docs/spec".to_string(), &resource);

        assert_eq!(entry.resource_path(), "/docs/spec");
        assert_eq!(entry.name(), "spec");
        assert_eq!(entry.description(), "resource description");
        assert_eq!(entry.mime_type(), Some("text/markdown"));
    }

    ///
    /// 明示 resource_path を候補へ反映できることを確認する。
    ///
    #[test]
    fn build_resource_candidate_entry_from_source_uses_explicit_resource_path() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_path: /docs/explicit\n",
            "  name: explicit\n",
            "  description: desc\n",
            "---\n",
            "本文は候補へ保存しない",
        );

        let entry = build_resource_candidate_entry_from_source(
            "/resources/path",
            source,
        )
        .expect("build failed")
        .expect("resource candidate missing");

        assert_eq!(entry.resource_path(), "/docs/explicit");
        assert_eq!(entry.name(), "explicit");
        assert_eq!(entry.description(), "desc");
        assert_eq!(entry.mime_type(), None);
    }

    ///
    /// resource_path 省略時に current path 由来 resource_path を
    /// 候補へ反映できることを確認する。
    ///
    #[test]
    fn build_resource_candidate_entry_from_source_uses_path_resource_path() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  name: path\n",
            "  description: desc\n",
            "  mime_type: application/json\n",
            "---\n",
            "本文",
        );

        let entry = build_resource_candidate_entry_from_source(
            "/resources/path-derived",
            source,
        )
        .expect("build failed")
        .expect("resource candidate missing");

        assert_eq!(entry.resource_path(), "/pages/resources/path-derived");
        assert_eq!(entry.mime_type(), Some("application/json"));
    }

    ///
    /// resource でないページから候補を生成しないことを確認する。
    ///
    #[test]
    fn build_resource_candidate_entry_from_source_returns_none_for_non_resource()
    {
        let sources = [
            "# title\n本文",
            "---\ncustom_meta:\n  project: alpha\n---\n本文",
            "---\nwiki:\n  template:\n    name: Template\n---\n本文",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: prompt\n",
                "  description: desc\n",
                "---\n",
                "本文",
            ),
        ];

        for source in sources {
            let entry = build_resource_candidate_entry_from_source(
                "/resources/non-resource",
                source,
            )
            .expect("build failed");
            assert!(entry.is_none());
        }
    }

    ///
    /// ルート path 由来 resource_path の不正をエラーとして
    /// 伝播することを確認する。
    ///
    #[test]
    fn build_resource_candidate_entry_from_source_propagates_path_errors() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  name: builtin\n",
            "  description: desc\n",
            "---\n",
            "本文",
        );

        let error = build_resource_candidate_entry_from_source(
            "/",
            source,
        )
        .expect_err("invalid path-derived resource_path must fail");

        assert!(
            error.to_string()
                .contains("mcp.resource_path must not end with /")
        );
    }

    ///
    /// 不正な resource front matter のエラーを
    /// 伝播することを確認する。
    ///
    #[test]
    fn build_resource_candidate_entry_from_source_propagates_front_matter_errors()
    {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_path: /docs/spec\n",
            "  name: spec\n",
            "  description: desc\n",
            "  mime_type: text /markdown\n",
            "---\n",
            "本文",
        );

        let error = build_resource_candidate_entry_from_source(
            "/resources/spec",
            source,
        )
        .expect_err("invalid resource front matter must fail");

        assert!(
            error
                .to_string()
                .contains("mcp.mime_type must not contain whitespace")
        );
    }
}
