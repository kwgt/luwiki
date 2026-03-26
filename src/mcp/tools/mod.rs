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
pub(crate) mod get_page;
pub(crate) mod get_page_section;
pub(crate) mod get_page_toc;
pub(crate) mod list_pages;
pub(crate) mod rename_page;
pub(crate) mod search_pages;
pub(crate) mod update_page;

use rmcp::schemars;
use serde::Deserialize;

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
pub(crate) struct SearchPagesToolArgs {
    /// 全文検索式
    pub(crate) query: String,

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
