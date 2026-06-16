/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! テンプレート候補派生データ生成処理を提供するモジュール
//!

use crate::database::types::{
    TemplateCandidateEntry,
    TemplateCandidateSource,
};
use crate::markdown_source::front_matter::{
    FrontMatterError,
    TemplatePageFrontMatter,
    extract_template_page_front_matter,
};

///
/// テンプレートページ情報からテンプレート候補派生データを生成する
///
/// # 引数
/// * `template` - テンプレートページ情報
///
/// # 戻り値
/// 生成したテンプレート候補派生データを返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_template_candidate_entry(
    template: &TemplatePageFrontMatter,
) -> TemplateCandidateEntry {
    TemplateCandidateEntry::new(
        template.name().to_string(),
        template.description().map(str::to_string),
        template.macro_expand(),
        TemplateCandidateSource::FrontMatter,
    )
}

///
/// パスから legacy template_root 候補を生成する
///
/// # 引数
/// * `path` - 対象ページパス
///
/// # 戻り値
/// 生成した legacy 候補を返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_legacy_template_candidate_entry(
    path: &str,
) -> TemplateCandidateEntry {
    TemplateCandidateEntry::new(
        extract_template_name(path),
        None,
        None,
        TemplateCandidateSource::LegacyTemplateRoot,
    )
}

///
/// テンプレートルート直下候補かどうかを判定する
///
/// # 引数
/// * `root` - テンプレートルート
/// * `path` - 対象ページパス
///
/// # 戻り値
/// 候補の場合は `true` を返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_direct_child_template_path(root: &str, path: &str) -> bool {
    let normalized_root = if root.len() > 1 {
        root.trim_end_matches('/')
    } else {
        root
    };

    if normalized_root == "/" {
        if !path.starts_with('/') || path == "/" {
            return false;
        }
        let rest = &path[1..];
        return !rest.is_empty() && !rest.contains('/');
    }

    if !path.starts_with(normalized_root) {
        return false;
    }

    let rest = &path[normalized_root.len()..];
    if !rest.starts_with('/') {
        return false;
    }

    let child = &rest[1..];
    !child.is_empty() && !child.contains('/')
}

fn extract_template_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or_default().to_string()
}

///
/// Markdown ソースからテンプレート候補派生データを生成する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// `wiki.template` を持たない場合は `Ok(None)` を返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_template_candidate_entry_from_source(
    source: &str,
) -> Result<Option<TemplateCandidateEntry>, FrontMatterError> {
    Ok(extract_template_page_front_matter(source)?
        .as_ref()
        .map(build_template_candidate_entry))
}

#[cfg(test)]
mod tests {
    use super::{
        build_legacy_template_candidate_entry,
        build_template_candidate_entry,
        build_template_candidate_entry_from_source,
        is_direct_child_template_path,
    };
    use crate::markdown_source::front_matter::extract_template_page_front_matter;
    use crate::database::types::TemplateCandidateSource;

    #[test]
    fn build_template_candidate_entry_copies_template_attributes() {
        let template = extract_template_page_front_matter(
            "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n---\n# title\n本文",
        )
        .expect("extract failed")
        .expect("template page missing");

        let entry = build_template_candidate_entry(&template);

        assert_eq!(entry.name(), "議事録");
        assert_eq!(entry.description(), Some("定例会議"));
        assert_eq!(entry.macro_expand(), Some(true));
        assert_eq!(entry.source(), &TemplateCandidateSource::FrontMatter);
    }

    #[test]
    fn build_template_candidate_entry_from_source_returns_entry() {
        let entry = build_template_candidate_entry_from_source(
            "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n---\n# title\n本文",
        )
        .expect("build failed")
        .expect("entry missing");

        assert_eq!(entry.name(), "議事録");
        assert_eq!(entry.description(), Some("定例会議"));
        assert_eq!(entry.macro_expand(), Some(true));
        assert_eq!(entry.source(), &TemplateCandidateSource::FrontMatter);
    }

    #[test]
    fn build_template_candidate_entry_from_source_returns_none_for_normal_page() {
        let entry = build_template_candidate_entry_from_source(
            "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文",
        )
        .expect("build failed");

        assert!(entry.is_none());
    }

    #[test]
    fn build_legacy_template_candidate_entry_uses_path_suffix() {
        let entry = build_legacy_template_candidate_entry("/templates/minutes");

        assert_eq!(entry.name(), "minutes");
        assert_eq!(entry.description(), None);
        assert_eq!(entry.macro_expand(), None);
        assert_eq!(
            entry.source(),
            &TemplateCandidateSource::LegacyTemplateRoot,
        );
    }

    #[test]
    fn is_direct_child_template_path_matches_direct_children_only() {
        assert!(is_direct_child_template_path("/templates", "/templates/a"));
        assert!(!is_direct_child_template_path("/templates", "/templates/a/b"));
        assert!(!is_direct_child_template_path("/templates", "/other/a"));
    }
}
