/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! prompt 候補派生データ生成処理を提供するモジュール
//!

use crate::database::types::{
    PromptArgumentEntry,
    PromptCandidateEntry,
};
use crate::markdown_source::front_matter::{
    FrontMatterError,
    PromptArgumentFrontMatter,
    PromptPageFrontMatter,
    extract_prompt_page_front_matter,
};

///
/// prompt 引数情報から prompt 引数派生データを生成する
///
/// # 引数
/// * `argument` - prompt 引数情報
///
/// # 戻り値
/// 生成した prompt 引数派生データを返す。
///
fn build_prompt_argument_entry(
    argument: &PromptArgumentFrontMatter,
) -> PromptArgumentEntry {
    PromptArgumentEntry::new(
        argument.name().to_string(),
        argument.description().to_string(),
        argument.required(),
    )
}

///
/// prompt ページ情報から prompt 候補派生データを生成する
///
/// # 引数
/// * `prompt` - prompt ページ情報
///
/// # 戻り値
/// 生成した prompt 候補派生データを返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_prompt_candidate_entry(
    prompt: &PromptPageFrontMatter,
) -> PromptCandidateEntry {
    let arguments = prompt
        .arguments()
        .iter()
        .map(build_prompt_argument_entry)
        .collect();

    PromptCandidateEntry::new(
        prompt.name().to_string(),
        prompt.description().to_string(),
        prompt.system().map(str::to_string),
        arguments,
    )
}

///
/// Markdown ソースから prompt 候補派生データを生成する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// prompt ページでない場合は `Ok(None)` を返す。
///
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_prompt_candidate_entry_from_source(
    source: &str,
) -> Result<Option<PromptCandidateEntry>, FrontMatterError> {
    Ok(extract_prompt_page_front_matter(source)?
        .as_ref()
        .map(build_prompt_candidate_entry))
}

#[cfg(test)]
mod tests {
    use super::{
        build_prompt_candidate_entry,
        build_prompt_candidate_entry_from_source,
    };
    use crate::markdown_source::front_matter::{
        extract_prompt_page_front_matter,
    };

    ///
    /// prompt ページ情報の全フィールドを候補へ
    /// 射影できることを確認する
    ///
    /// 注記: 複合用途、複数行、引数順序、
    /// required三状態をまとめて検証する。
    ///
    #[test]
    fn build_prompt_candidate_entry_copies_prompt_attributes() {
        let source = concat!(
            "---\n",
            "wiki:\n",
            "  template:\n",
            "    name: Template\n",
            "mcp:\n",
            "  primitive: prompt\n",
            "  name: summarize\n",
            "  description: \"  説明  \"\n",
            "  system: |\n",
            "    first\n",
            "    second\n",
            "  arguments:\n",
            "    - name: first\n",
            "      description: 最初\n",
            "    - name: second\n",
            "      description: 次\n",
            "      required: false\n",
            "    - name: third\n",
            "      description: 最後\n",
            "      required: true\n",
            "---\n",
            "本文",
        );
        let prompt = extract_prompt_page_front_matter(source)
            .expect("extract failed")
            .expect("prompt page missing");

        let entry = build_prompt_candidate_entry(&prompt);
        let arguments = entry.arguments();

        assert_eq!(entry.name(), "summarize");
        assert_eq!(entry.description(), "  説明  ");
        assert_eq!(entry.system(), Some("first\nsecond\n"));
        assert_eq!(arguments[0].name(), "first");
        assert_eq!(arguments[0].description(), "最初");
        assert_eq!(arguments[0].required(), None);
        assert_eq!(arguments[1].name(), "second");
        assert_eq!(arguments[1].required(), Some(false));
        assert_eq!(arguments[2].name(), "third");
        assert_eq!(arguments[2].required(), Some(true));
    }

    ///
    /// arguments未指定のpromptを空引数候補へ
    /// 射影できることを確認する
    ///
    /// 注記: systemとargumentsを省略したソースから
    /// 候補を生成する。
    ///
    #[test]
    fn build_prompt_candidate_entry_from_source_keeps_optional_absence() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: prompt\n",
            "  name: prompt\n",
            "  description: desc\n",
            "---\n",
            "本文",
        );

        let entry = build_prompt_candidate_entry_from_source(source)
            .expect("build failed")
            .expect("prompt candidate missing");

        assert_eq!(entry.system(), None);
        assert!(entry.arguments().is_empty());
    }

    ///
    /// promptでないページから候補を生成しないことを
    /// 確認する
    ///
    /// 注記: front matterなし、通常、template、
    /// resourceを順に処理する。
    ///
    #[test]
    fn build_prompt_candidate_entry_from_source_returns_none_for_non_prompt() {
        let sources = [
            "# title\n本文",
            "---\ncustom_meta:\n  project: alpha\n---\n本文",
            "---\nwiki:\n  template:\n    name: Template\n---\n本文",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: resource\n",
                "  name: resource\n",
                "  description: desc\n",
                "---\n",
                "本文",
            ),
        ];

        for source in sources {
            let entry = build_prompt_candidate_entry_from_source(source)
                .expect("build failed");
            assert!(entry.is_none());
        }
    }

    ///
    /// 不正なprompt front matterのエラーを
    /// 伝播することを確認する
    ///
    /// 注記: 不正名、重複引数、未知プロパティを
    /// 順に処理する。
    ///
    #[test]
    fn build_prompt_candidate_entry_from_source_propagates_errors() {
        let sources = [
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: \"\"\n",
                "  description: desc\n",
                "---\n",
                "本文",
            ),
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: prompt\n",
                "  description: desc\n",
                "  arguments:\n",
                "    - name: target\n",
                "      description: first\n",
                "    - name: target\n",
                "      description: second\n",
                "---\n",
                "本文",
            ),
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: prompt\n",
                "  description: desc\n",
                "  unknown: value\n",
                "---\n",
                "本文",
            ),
        ];

        for source in sources {
            assert!(
                build_prompt_candidate_entry_from_source(source).is_err()
            );
        }
    }
}
