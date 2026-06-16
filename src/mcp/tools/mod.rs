/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPで公開するツール定義の骨格をまとめるモジュール
//!

pub(crate) mod append_page;
pub(crate) mod create_page;
pub(crate) mod edit_page;
pub(crate) mod get_page;
pub(crate) mod get_page_section;
pub(crate) mod get_page_toc;
pub(crate) mod list_pages;
pub(crate) mod rename_page;
pub(crate) mod search_pages;
pub(crate) mod update_page;

use rmcp::schemars;
use serde::Deserialize;

use crate::fts::FtsSearchTarget;

///
/// 初期実装で扱うMCPツール名
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum McpToolName {
    /// ページ取得
    GetPage,

    /// 目次取得
    GetPageToc,

    /// ページ一覧取得
    ListPages,

    /// ページ検索
    SearchPages,

    /// ページ作成
    CreatePage,

    /// ページ更新
    UpdatePage,

    /// ページ編集
    EditPage,

    /// ページ追記
    AppendPage,

    /// ページリネーム
    RenamePage,

    /// セクション取得
    GetPageSection,
}

impl McpToolName {
    ///
    /// 外部公開用のツール名を返す
    ///
    /// # 戻り値
    /// MCPクライアントへ公開するツール名を返す。
    ///
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::GetPage => "get_page",
            Self::GetPageToc => "get_page_toc",
            Self::ListPages => "list_pages",
            Self::SearchPages => "search_pages",
            Self::CreatePage => "create_page",
            Self::UpdatePage => "update_page",
            Self::EditPage => "edit_page",
            Self::AppendPage => "append_page",
            Self::RenamePage => "rename_page",
            Self::GetPageSection => "get_page_section",
        }
    }

}

///
/// `get_page` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetPageToolArgs {
    /// 対象ページの絶対 path
    pub(crate) path: String,

    /// 対象 revision
    pub(crate) revision: Option<u64>,
}

///
/// `get_page_toc` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetPageTocToolArgs {
    /// 対象ページの絶対 path
    pub(crate) path: String,

    /// 対象 revision
    pub(crate) revision: Option<u64>,
}

///
/// `list_pages` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListPagesToolArgs {
    /// 一覧対象 prefix
    pub(crate) prefix: String,

    /// 最大取得件数
    pub(crate) limit: Option<usize>,

    /// 継続取得 cursor
    pub(crate) cursor: Option<String>,
}

///
/// `search_pages` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchPagesTargetArg {
    /// 見出し
    Headings,

    /// 本文
    Body,

    /// コードブロック
    Code,

    /// front matter
    FrontMatter,
}

impl SearchPagesTargetArg {
    ///
    /// FTS 検索対象へ変換する
    ///
    /// # 戻り値
    /// 対応する FTS 検索対象
    ///
    pub(crate) fn to_fts_search_target(&self) -> FtsSearchTarget {
        match self {
            Self::Headings => FtsSearchTarget::Headings,
            Self::Body => FtsSearchTarget::Body,
            Self::Code => FtsSearchTarget::Code,
            Self::FrontMatter => FtsSearchTarget::FrontMatter,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SearchPagesToolArgs {
    /// 全文検索式
    pub(crate) query: String,

    /// 検索対象
    pub(crate) target: Vec<SearchPagesTargetArg>,

    /// 検索対象 prefix
    pub(crate) prefix: Option<String>,

    /// 最大取得件数
    pub(crate) limit: Option<usize>,
}

///
/// `create_page` / `update_page` / `append_page` 用の共通引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct WritePageToolArgs {
    /// 対象ページの絶対 path
    pub(crate) path: String,

    /// 本文または追記内容
    pub(crate) content: String,
}

///
/// `rename_page` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RenamePageToolArgs {
    /// 移動元 path
    pub(crate) path: String,

    /// 移動先 path
    pub(crate) rename_to: String,
}

///
/// `edit_page` の section selector 指定方法
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EditPageSectionSelectorBy {
    /// section ID 指定
    Id,

    /// 見出し文字列指定
    Title,
}

///
/// `edit_page` の section selector object 形式
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct EditPageSectionSelectorObject {
    /// selector 方式
    pub(crate) by: EditPageSectionSelectorBy,

    /// selector 値
    pub(crate) value: String,
}

///
/// `edit_page` の section selector
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub(crate) enum EditPageSectionSelector {
    /// 見出し文字列そのものを指定する省略形
    Text(String),

    /// selector object 指定
    Structured(EditPageSectionSelectorObject),
}

///
/// `insert_section` の挿入位置
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EditPageInsertSectionPlacement {
    /// anchor の直前
    Before,

    /// anchor の直後
    After,
}

///
/// `replace_text` の一致対象
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EditPageReplaceTextOccurrence {
    /// 先頭一致のみ
    First,

    /// 全一致
    All,
}

///
/// `edit_page` の編集操作
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum EditPageToolOperation {
    /// セクション本文の置換
    ReplaceSection {
        /// 対象セクション
        section: EditPageSectionSelector,

        /// 置換後本文
        content: String,
    },

    /// セクション挿入
    InsertSection {
        /// 挿入位置基準セクション
        anchor: EditPageSectionSelector,

        /// 挿入位置
        placement: EditPageInsertSectionPlacement,

        /// 挿入する完全なセクション本文
        content: String,
    },

    /// セクション削除
    DeleteSection {
        /// 削除対象セクション
        section: EditPageSectionSelector,
    },

    /// テキスト置換
    ReplaceText {
        /// 置換前文字列
        old_text: String,

        /// 置換後文字列
        new_text: String,

        /// 複数一致時の対象指定
        occurrence: Option<EditPageReplaceTextOccurrence>,
    },
}

///
/// `edit_page` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct EditPageToolArgs {
    /// 対象ページの絶対 path
    pub(crate) path: String,

    /// 対象 revision
    pub(crate) revision: u64,

    /// ページ内容の一意性を表すインスタンスID
    pub(crate) instance_id: String,

    /// 単一の編集操作
    pub(crate) operation: EditPageToolOperation,
}

///
/// `get_page_section` の section 指定
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "by", content = "value", rename_all = "snake_case")]
pub(crate) enum GetPageSectionToolSelector {
    /// section ID 指定
    Id(String),

    /// 見出し文字列指定
    Title(String),
}

///
/// `get_page_section` 用の tool 引数
///
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetPageSectionToolArgs {
    /// 対象ページの絶対 path
    pub(crate) path: String,

    /// セクション指定
    pub(crate) section: GetPageSectionToolSelector,

    /// 対象 revision
    pub(crate) revision: Option<u64>,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::mcp::errors::{McpError, McpErrorCode};

    use super::{
        EditPageSectionSelector, EditPageSectionSelectorBy, EditPageToolArgs,
        EditPageToolOperation,
    };

    fn deserialize_edit_page_args(
        value: Value,
    ) -> Result<EditPageToolArgs, McpError> {
        serde_json::from_value(value).map_err(|error| {
            McpError::new(
                McpErrorCode::InvalidInput,
                format!("edit_page arguments are invalid: {error}"),
            )
        })
    }

    #[test]
    fn edit_page_operation_rejects_unknown_type_as_invalid_input() {
        let error = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_lines",
                "old_text": "before",
                "new_text": "after"
            }
        }))
        .expect_err("unknown operation type must fail");

        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert!(
            error.message().contains("unknown variant"),
            "unexpected error: {}",
            error.message(),
        );
    }

    #[test]
    fn edit_page_operation_rejects_missing_required_field_as_invalid_input() {
        let error = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_section",
                "section": "Overview"
            }
        }))
        .expect_err("missing required field must fail");

        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert!(
            error.message().contains("missing field"),
            "unexpected error: {}",
            error.message(),
        );
        assert!(
            error.message().contains("content"),
            "unexpected error: {}",
            error.message(),
        );
    }

    #[test]
    fn edit_page_operation_rejects_invalid_field_type_as_invalid_input() {
        let error = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_text",
                "old_text": "before",
                "new_text": "after",
                "occurrence": 1
            }
        }))
        .expect_err("invalid field type must fail");

        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert!(
            error.message().contains("invalid type"),
            "unexpected error: {}",
            error.message(),
        );
    }

    #[test]
    fn edit_page_selector_accepts_text_id_and_title_forms() {
        let text_args = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_section",
                "section": "Overview",
                "content": "updated"
            }
        }))
        .expect("text selector must deserialize");
        let by_id_args = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_section",
                "section": {
                    "by": "id",
                    "value": "s-001"
                },
                "content": "updated"
            }
        }))
        .expect("id selector must deserialize");
        let by_title_args = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_section",
                "section": {
                    "by": "title",
                    "value": "Overview"
                },
                "content": "updated"
            }
        }))
        .expect("title selector must deserialize");

        match text_args.operation {
            EditPageToolOperation::ReplaceSection { section, .. } => {
                match section {
                    EditPageSectionSelector::Text(value) => {
                        assert_eq!(value, "Overview");
                    }
                    EditPageSectionSelector::Structured(_) => {
                        panic!("unexpected structured selector")
                    }
                }
            }
            _ => panic!("unexpected operation"),
        }
        match by_id_args.operation {
            EditPageToolOperation::ReplaceSection { section, .. } => {
                match section {
                    EditPageSectionSelector::Structured(selector) => {
                        match selector.by {
                            EditPageSectionSelectorBy::Id => {}
                            EditPageSectionSelectorBy::Title => {
                                panic!("unexpected title selector")
                            }
                        }
                        assert_eq!(selector.value, "s-001");
                    }
                    EditPageSectionSelector::Text(_) => {
                        panic!("unexpected text selector")
                    }
                }
            }
            _ => panic!("unexpected operation"),
        }
        match by_title_args.operation {
            EditPageToolOperation::ReplaceSection { section, .. } => {
                match section {
                    EditPageSectionSelector::Structured(selector) => {
                        match selector.by {
                            EditPageSectionSelectorBy::Title => {}
                            EditPageSectionSelectorBy::Id => {
                                panic!("unexpected id selector")
                            }
                        }
                        assert_eq!(selector.value, "Overview");
                    }
                    EditPageSectionSelector::Text(_) => {
                        panic!("unexpected text selector")
                    }
                }
            }
            _ => panic!("unexpected operation"),
        }
    }

    #[test]
    fn edit_page_selector_rejects_invalid_selector_as_invalid_input() {
        let unknown_by = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_section",
                "section": {
                    "by": "slug",
                    "value": "overview"
                },
                "content": "updated"
            }
        }))
        .expect_err("unknown selector type must fail");
        assert_eq!(unknown_by.code(), McpErrorCode::InvalidInput);
        assert!(
            unknown_by
                .message()
                .contains("did not match any variant of untagged enum"),
            "unexpected error: {}",
            unknown_by.message(),
        );

        let missing_value = deserialize_edit_page_args(json!({
            "path": "/docs/page",
            "revision": 3,
            "instance_id": "instance-1",
            "operation": {
                "type": "replace_section",
                "section": {
                    "by": "title"
                },
                "content": "updated"
            }
        }))
        .expect_err("missing selector value must fail");
        assert_eq!(missing_value.code(), McpErrorCode::InvalidInput);
        assert!(
            missing_value
                .message()
                .contains("did not match any variant of untagged enum"),
            "unexpected error: {}",
            missing_value.message(),
        );
    }
}
