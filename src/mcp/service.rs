/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP pathベースサービス層の認可補助を定義するモジュール
//!

use std::cmp::Ordering;
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Error;
use chrono::{DateTime, Local};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use serde_json::{Map as JsonObject, Value as JsonValue};

use crate::auth::AuthContext;
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
    BearerScope,
    PageId,
    PageIndex,
    TokenId,
    UserAttribute,
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

/// path で禁止する文字
const FORBIDDEN_PATH_CHARS: &[char] = &['\\'];

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
/// MCPで扱う操作種別
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum McpOperation {
    /// ページ参照
    GetPage,

    /// 目次参照
    GetPageToc,

    /// 一覧取得
    ListPages,

    /// prompt一覧取得
    ListPrompts,

    /// resource一覧取得
    ListResources,

    /// resource取得
    ReadResource,

    /// prompt取得
    GetPrompt,

    /// 検索
    SearchPages,

    /// セクション参照
    GetPageSection,

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

    /// 将来拡張用の削除
    DeletePage,
}

///
/// prefix 要求の解決結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedPrefixRequest {
    /// クライアントが明示指定した要求 prefix
    requested_prefix: Option<String>,

    /// 結果に適用するフィルタ方式
    filter_mode: PrefixFilterMode,
}

///
/// prefix 指定時の結果フィルタ方式
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PrefixFilterMode {
    /// prefix 指定なし
    NoPrefix,

    /// 指定 prefix 配下に限定
    DescendantsOf(String),
}

///
/// current path から解決した現在ページ情報
///
#[derive(Clone, Debug)]
pub(crate) struct ResolvedPage {
    /// 正規化済み current path
    normalized_path: String,

    /// ページID
    page_id: PageId,

    /// ページインデックス
    page_index: PageIndex,

    /// 最新 revision
    latest_revision: Option<u64>,
}

///
/// `get_page` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageResult {
    /// current path
    path: String,

    /// 取得した revision
    revision: u64,

    /// 取得した instance_id
    instance_id: String,

    /// Markdown 本文全体
    content: String,
}

///
/// `get_page_toc` / `get_page_section` で使うセクション情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TocSection {
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

    /// セクション本文の文字数
    section_chars: usize,
}

///
/// `get_page_toc` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageTocResult {
    /// current path
    path: String,

    /// 取得した revision
    revision: u64,

    /// 取得した instance_id
    instance_id: String,

    /// 見出し一覧
    sections: Vec<TocSection>,
}

///
/// `list_pages` の一覧項目
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListPageItem {
    /// current path
    path: String,

    /// 最新 revision
    revision: u64,

    /// 最終更新日時
    updated_at: String,

    /// 最終更新ユーザ名
    updated_by: String,
}

///
/// `list_pages` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListPagesResult {
    /// 一覧項目
    items: Vec<ListPageItem>,

    /// 続き有無
    has_more: bool,

    /// 次回 cursor
    next_cursor: Option<String>,
}

///
/// `search_pages` の一覧項目
///
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchPageItem {
    /// current path
    path: String,

    /// 対応 revision
    revision: u64,

    /// 検索スコア
    score: f32,

    /// スニペット
    snippet: String,
}

///
/// `search_pages` の戻り値
///
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchPagesResult {
    /// 検索結果一覧
    items: Vec<SearchPageItem>,
}

///
/// `get_page_section` の selector
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SectionSelector {
    /// section ID で指定
    ById(String),

    /// 見出し文字列で指定
    ByTitle(String),
}

///
/// `insert_section` の挿入位置
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditPageInsertSectionPlacement {
    /// anchor の直前
    Before,

    /// anchor の直後
    After,
}

///
/// `replace_text` の一致対象
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditPageReplaceTextOccurrence {
    /// 先頭一致のみ
    First,

    /// 全一致
    All,
}

///
/// `edit_page` の service 層 operation
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EditPageOperation {
    /// セクション本文の置換
    ReplaceSection {
        /// 対象セクション
        section: SectionSelector,

        /// 置換後本文
        content: String,
    },

    /// セクション挿入
    InsertSection {
        /// 挿入位置基準セクション
        anchor: SectionSelector,

        /// 挿入位置
        placement: EditPageInsertSectionPlacement,

        /// 挿入する完全なセクション本文
        content: String,
    },

    /// セクション削除
    DeleteSection {
        /// 削除対象セクション
        section: SectionSelector,
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
/// `edit_page` の service 層入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EditPageRequest {
    /// 対象ページの絶対 path
    path: String,

    /// 対象 revision
    revision: u64,

    /// 内容整合性確認用 instance_id
    instance_id: String,

    /// 単一の編集操作
    operation: EditPageOperation,
}

///
/// `get_page_section` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageSectionResult {
    /// current path
    path: String,

    /// 取得した revision
    revision: u64,

    /// 解決後 section
    section: TocSection,

    /// section 本文
    content: String,
}

///
/// `create_page` / `update_page` / `rename_page` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WritePageResult {
    /// current path
    path: String,

    /// 更新後 revision
    revision: u64,

    /// 更新後 instance_id
    instance_id: String,

    /// 実行結果要約
    summary: String,
}

///
/// `edit_page` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EditPageResult {
    /// current path
    path: String,

    /// 更新後 revision
    revision: u64,

    /// 更新後 instance_id
    instance_id: String,

    /// 実行結果要約
    summary: String,
}

///
/// `append_page` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppendServiceResult {
    /// current path
    path: String,

    /// 更新後 revision
    revision: u64,

    /// 更新後 instance_id
    instance_id: String,

    /// 実行結果要約
    summary: String,

    /// amend 相当保存有無
    amended: bool,
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

impl McpOperation {
    ///
    /// 操作種別に対応する required scope を返す
    ///
    /// # 戻り値
    /// 操作種別に固定された required scope を返す。
    ///
    pub(crate) fn required_scope(self) -> BearerScope {
        match self {
            Self::GetPage
            | Self::GetPageToc
            | Self::ListPages
            | Self::ListPrompts
            | Self::ListResources
            | Self::ReadResource
            | Self::GetPrompt
            | Self::SearchPages
            | Self::GetPageSection => BearerScope::Read,
            Self::CreatePage => BearerScope::Create,
            Self::UpdatePage | Self::EditPage | Self::RenamePage => {
                BearerScope::Update
            }
            Self::AppendPage => BearerScope::Append,
            Self::DeletePage => BearerScope::Delete,
        }
    }

    ///
    /// write 系操作かどうかを返す
    ///
    /// # 戻り値
    /// write 系操作なら `true` を返す。
    ///
    pub(crate) fn is_write(self) -> bool {
        !matches!(
            self,
            Self::GetPage
                | Self::GetPageToc
                | Self::ListPages
                | Self::ListPrompts
                | Self::ListResources
                | Self::ReadResource
                | Self::GetPrompt
                | Self::SearchPages
                | Self::GetPageSection
        )
    }
}

impl ResolvedPrefixRequest {
    ///
    /// prefix 要求の解決結果を生成する
    ///
    /// # 引数
    /// * `requested_prefix` - 明示指定された prefix
    /// * `filter_mode` - 結果フィルタ方式
    ///
    /// # 戻り値
    /// 解決済み prefix 要求を返す。
    ///
    pub(crate) fn new(
        requested_prefix: Option<String>,
        filter_mode: PrefixFilterMode,
    ) -> Self {
        Self {
            requested_prefix,
            filter_mode,
        }
    }

    ///
    /// 要求 prefix を返す
    ///
    /// # 戻り値
    /// 明示指定された prefix がある場合はそれを返す。
    ///
    pub(crate) fn requested_prefix(&self) -> Option<&str> {
        self.requested_prefix.as_deref()
    }

    ///
    /// 結果フィルタ方式を返す
    ///
    /// # 戻り値
    /// 結果フィルタ方式を返す。
    ///
    pub(crate) fn filter_mode(&self) -> &PrefixFilterMode {
        &self.filter_mode
    }
}

impl ResolvedPage {
    ///
    /// 解決済み現在ページ情報を生成する
    ///
    /// # 引数
    /// * `normalized_path` - 正規化済み current path
    /// * `page_id` - ページID
    /// * `page_index` - ページインデックス
    /// * `latest_revision` - 最新 revision
    ///
    /// # 戻り値
    /// 解決済み現在ページ情報を返す。
    ///
    pub(crate) fn new(
        normalized_path: String,
        page_id: PageId,
        page_index: PageIndex,
        latest_revision: Option<u64>,
    ) -> Self {
        Self {
            normalized_path,
            page_id,
            page_index,
            latest_revision,
        }
    }

    ///
    /// 正規化済み current path を返す
    ///
    /// # 戻り値
    /// 正規化済み current path を返す。
    ///
    pub(crate) fn normalized_path(&self) -> &str {
        &self.normalized_path
    }

    ///
    /// ページIDを返す
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// ページインデックスを返す
    ///
    /// # 戻り値
    /// ページインデックスを返す。
    ///
    pub(crate) fn page_index(&self) -> PageIndex {
        self.page_index.clone()
    }

    ///
    /// 最新 revision を返す
    ///
    /// # 戻り値
    /// draft 以外では最新 revision を返す。
    ///
    pub(crate) fn latest_revision(&self) -> Option<u64> {
        self.latest_revision
    }
}

impl GetPageResult {
    ///
    /// `get_page` 結果を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 取得 revision
    /// * `instance_id` - 取得した instance_id
    /// * `content` - Markdown 本文
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        instance_id: String,
        content: String,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            content,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// revision を返す
    ///
    /// # 戻り値
    /// 取得 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// instance_id を返す
    ///
    /// # 戻り値
    /// 取得した instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    ///
    /// Markdown 本文を返す
    ///
    /// # 戻り値
    /// Markdown 本文全体を返す。
    ///
    pub(crate) fn content(&self) -> &str {
        &self.content
    }
}

impl TocSection {
    ///
    /// TOC 用 section 情報を生成する
    ///
    /// # 引数
    /// * `id` - section ID
    /// * `title` - 見出し文字列
    /// * `level` - 見出しレベル
    /// * `ordinal` - 文書順番号
    /// * `parent_id` - 親 section ID
    /// * `section_chars` - セクション本文文字数
    ///
    /// # 戻り値
    /// 生成した section 情報を返す。
    ///
    fn new(
        id: String,
        title: String,
        level: u32,
        ordinal: u32,
        parent_id: Option<String>,
        section_chars: usize,
    ) -> Self {
        Self {
            id,
            title,
            level,
            ordinal,
            parent_id,
            section_chars,
        }
    }

    ///
    /// section ID を返す
    ///
    /// # 戻り値
    /// section ID を返す。
    ///
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    ///
    /// 見出し文字列を返す
    ///
    /// # 戻り値
    /// 見出し文字列を返す。
    ///
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    ///
    /// 見出しレベルを返す
    ///
    /// # 戻り値
    /// 見出しレベルを返す。
    ///
    pub(crate) fn level(&self) -> u32 {
        self.level
    }

    ///
    /// 文書順番号を返す
    ///
    /// # 戻り値
    /// 文書順番号を返す。
    ///
    pub(crate) fn ordinal(&self) -> u32 {
        self.ordinal
    }

    ///
    /// 親 section ID を返す
    ///
    /// # 戻り値
    /// 親 section ID がある場合はそれを返す。
    ///
    pub(crate) fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }

    ///
    /// セクション本文文字数を返す
    ///
    /// # 戻り値
    /// セクション本文文字数を返す。
    ///
    pub(crate) fn section_chars(&self) -> usize {
        self.section_chars
    }
}

impl GetPageTocResult {
    ///
    /// `get_page_toc` 結果を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 取得 revision
    /// * `instance_id` - 取得した instance_id
    /// * `sections` - 見出し一覧
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        instance_id: String,
        sections: Vec<TocSection>,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            sections,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// revision を返す
    ///
    /// # 戻り値
    /// 取得 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// instance_id を返す
    ///
    /// # 戻り値
    /// 取得した instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    ///
    /// section 一覧を返す
    ///
    /// # 戻り値
    /// section 一覧を返す。
    ///
    pub(crate) fn sections(&self) -> &[TocSection] {
        &self.sections
    }
}

impl ListPageItem {
    ///
    /// `list_pages` 一覧項目を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 最新 revision
    /// * `updated_at` - 最終更新日時
    /// * `updated_by` - 最終更新ユーザ名
    ///
    /// # 戻り値
    /// 生成した一覧項目を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        updated_at: String,
        updated_by: String,
    ) -> Self {
        Self {
            path,
            revision,
            updated_at,
            updated_by,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 最新 revision を返す
    ///
    /// # 戻り値
    /// 最新 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 最終更新日時を返す
    ///
    /// # 戻り値
    /// 最終更新日時を返す。
    ///
    pub(crate) fn updated_at(&self) -> &str {
        &self.updated_at
    }

    ///
    /// 最終更新ユーザ名を返す
    ///
    /// # 戻り値
    /// 最終更新ユーザ名を返す。
    ///
    pub(crate) fn updated_by(&self) -> &str {
        &self.updated_by
    }
}

impl ListPagesResult {
    ///
    /// `list_pages` 結果を生成する
    ///
    /// # 引数
    /// * `items` - 一覧項目
    /// * `has_more` - 続き有無
    /// * `next_cursor` - 次回 cursor
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        items: Vec<ListPageItem>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            items,
            has_more,
            next_cursor,
        }
    }

    ///
    /// 一覧項目を返す
    ///
    /// # 戻り値
    /// 一覧項目を返す。
    ///
    pub(crate) fn items(&self) -> &[ListPageItem] {
        &self.items
    }

    ///
    /// 続き有無を返す
    ///
    /// # 戻り値
    /// 続きがある場合は `true` を返す。
    ///
    pub(crate) fn has_more(&self) -> bool {
        self.has_more
    }

    ///
    /// 次回 cursor を返す
    ///
    /// # 戻り値
    /// 次回 cursor がある場合はそれを返す。
    ///
    pub(crate) fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

impl SearchPageItem {
    ///
    /// `search_pages` 一覧項目を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 対応 revision
    /// * `score` - 検索スコア
    /// * `snippet` - スニペット
    ///
    /// # 戻り値
    /// 生成した一覧項目を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        score: f32,
        snippet: String,
    ) -> Self {
        Self {
            path,
            revision,
            score,
            snippet,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 対応 revision を返す
    ///
    /// # 戻り値
    /// 対応 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 検索スコアを返す
    ///
    /// # 戻り値
    /// 検索スコアを返す。
    ///
    pub(crate) fn score(&self) -> f32 {
        self.score
    }

    ///
    /// スニペットを返す
    ///
    /// # 戻り値
    /// スニペットを返す。
    ///
    pub(crate) fn snippet(&self) -> &str {
        &self.snippet
    }
}

impl SearchPagesResult {
    ///
    /// `search_pages` 結果を生成する
    ///
    /// # 引数
    /// * `items` - 検索結果一覧
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(items: Vec<SearchPageItem>) -> Self {
        Self { items }
    }

    ///
    /// 検索結果一覧を返す
    ///
    /// # 戻り値
    /// 検索結果一覧を返す。
    ///
    pub(crate) fn items(&self) -> &[SearchPageItem] {
        &self.items
    }
}

impl EditPageRequest {
    ///
    /// `edit_page` service 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `revision` - 対象 revision
    /// * `instance_id` - 内容整合性確認用 instance_id
    /// * `operation` - 単一の編集操作
    ///
    /// # 戻り値
    /// 生成した service 入力を返す。
    ///
    pub(crate) fn new(
        path: String,
        revision: u64,
        instance_id: String,
        operation: EditPageOperation,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            operation,
        }
    }

    ///
    /// 対象ページ path を返す
    ///
    /// # 戻り値
    /// 対象ページの絶対 path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 対象 revision を返す
    ///
    /// # 戻り値
    /// 対象 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 内容整合性確認用 instance_id を返す
    ///
    /// # 戻り値
    /// 入力 instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    ///
    /// 編集操作を返す
    ///
    /// # 戻り値
    /// 単一の編集操作を返す。
    ///
    pub(crate) fn operation(&self) -> &EditPageOperation {
        &self.operation
    }
}

impl GetPageSectionResult {
    ///
    /// `get_page_section` 結果を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 取得 revision
    /// * `section` - 解決後 section
    /// * `content` - section 本文
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        section: TocSection,
        content: String,
    ) -> Self {
        Self {
            path,
            revision,
            section,
            content,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 取得 revision を返す
    ///
    /// # 戻り値
    /// 取得 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 解決後 section を返す
    ///
    /// # 戻り値
    /// 解決後 section を返す。
    ///
    pub(crate) fn section(&self) -> &TocSection {
        &self.section
    }

    ///
    /// section 本文を返す
    ///
    /// # 戻り値
    /// section 本文を返す。
    ///
    pub(crate) fn content(&self) -> &str {
        &self.content
    }
}

impl WritePageResult {
    ///
    /// 更新系結果を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 更新後 revision
    /// * `instance_id` - 更新後 instance_id
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        instance_id: String,
        summary: String,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            summary,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 更新後 revision を返す
    ///
    /// # 戻り値
    /// 更新後 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 更新後 instance_id を返す
    ///
    /// # 戻り値
    /// 更新後 instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    ///
    /// 実行結果要約を返す
    ///
    /// # 戻り値
    /// 実行結果要約を返す。
    ///
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
}

impl EditPageResult {
    ///
    /// `edit_page` 結果を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 更新後 revision
    /// * `instance_id` - 更新後 instance_id
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        instance_id: String,
        summary: String,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            summary,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 更新後 revision を返す
    ///
    /// # 戻り値
    /// 更新後 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 更新後 instance_id を返す
    ///
    /// # 戻り値
    /// 更新後 instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    ///
    /// 実行結果要約を返す
    ///
    /// # 戻り値
    /// 実行結果要約を返す。
    ///
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
}

impl AppendServiceResult {
    ///
    /// `append_page` 結果を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 更新後 revision
    /// * `instance_id` - 更新後 instance_id
    /// * `summary` - 実行結果要約
    /// * `amended` - amend 相当保存有無
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        instance_id: String,
        summary: String,
        amended: bool,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            summary,
            amended,
        }
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 更新後 revision を返す
    ///
    /// # 戻り値
    /// 更新後 revision を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 更新後 instance_id を返す
    ///
    /// # 戻り値
    /// 更新後 instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    ///
    /// 実行結果要約を返す
    ///
    /// # 戻り値
    /// 実行結果要約を返す。
    ///
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }

    ///
    /// amend 相当保存有無を返す
    ///
    /// # 戻り値
    /// amend 相当保存時は `true` を返す。
    ///
    pub(crate) fn amended(&self) -> bool {
        self.amended
    }
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
        /*
         * 既存 path 検証規則の適用
         */
        validate_page_path_for_mcp(raw_path).map_err(|message| {
            McpError::new(McpErrorCode::InvalidInput, message)
        })?;

        /*
         * MCP 固有の正規化と追加検証
         */
        if has_dot_path_segment(raw_path) {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "path must not contain '.' or '..' segments",
            ));
        }

        Ok(normalize_absolute_path(raw_path))
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

        if operation.is_write()
            && auth.user_attributes().contains(UserAttribute::ReadOnly)
        {
            return Err(McpError::forbidden_read_only());
        }

        if auth.scopes().allows(required) {
            return Ok(());
        }

        Err(McpError::new(
            McpErrorCode::Forbidden,
            format!("required scope denied: {}", required.as_str()),
        ))
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
        let current_path = match page_index.current_path() {
            Some(current_path) => current_path,
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
        if current_path != normalized_path {
            return Err(McpError::new(
                McpErrorCode::InternalError,
                format!(
                    "current path mismatch: {} != {}",
                    current_path,
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
    /// `get_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象 path
    /// * `revision` - 取得 revision
    ///
    /// # 戻り値
    /// ページ本文取得結果を返す。
    ///
    pub(crate) fn get_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        revision: Option<u64>,
    ) -> Result<GetPageResult, McpError> {
        /*
         * path 認可とページ解決
         */
        let normalized_path =
            self.ensure_authorized_path(auth, McpOperation::GetPage, raw_path)?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * revision と source の解決
         */
        let (revision, instance_id, source) =
            self.resolve_revision_source(db, &resolved, revision)?;

        Ok(GetPageResult::new(
            resolved.normalized_path().to_string(),
            revision,
            instance_id,
            source,
        ))
    }

    ///
    /// `get_page_toc` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象 path
    /// * `revision` - 取得 revision
    ///
    /// # 戻り値
    /// TOC 取得結果を返す。
    ///
    pub(crate) fn get_page_toc(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        revision: Option<u64>,
    ) -> Result<GetPageTocResult, McpError> {
        /*
         * path 認可とページ解決
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::GetPageToc,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * revision と本文の解決
         */
        let (revision, instance_id, source) =
            self.resolve_revision_source(db, &resolved, revision)?;
        let parsed_sections = self.parse_markdown_toc_sections(&source)?;
        let sections = parsed_sections
            .into_iter()
            .map(|section| {
                let content = self.extract_section_content(&source, &section);
                TocSection::new(
                    section.id,
                    section.title,
                    section.level,
                    section.ordinal,
                    section.parent_id,
                    content.chars().count(),
                )
            })
            .collect();

        Ok(GetPageTocResult::new(
            resolved.normalized_path().to_string(),
            revision,
            instance_id,
            sections,
        ))
    }

    ///
    /// `list_pages` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_prefix` - 一覧対象 prefix
    /// * `limit` - 取得件数上限
    /// * `cursor` - 次ページ cursor
    ///
    /// # 戻り値
    /// 一覧取得結果を返す。
    ///
    pub(crate) fn list_pages(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_prefix: &str,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<ListPagesResult, McpError> {
        /*
         * 入力検証
         */
        self.ensure_operation_scope(auth, McpOperation::ListPages)?;
        let resolved_prefix =
            self.resolve_list_prefix_request(auth, Some(raw_prefix))?;
        let limit = self.resolve_limit(limit, DEFAULT_LIST_LIMIT)?;
        let normalized_prefix =
            resolved_prefix.requested_prefix().unwrap_or("/");
        let normalized_cursor = match cursor {
            Some(cursor) => {
                let cursor = self.validate_and_normalize_path(cursor)?;
                if !path_matches_prefix(&cursor, normalized_prefix) {
                    return Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        "cursor must be under requested prefix",
                    ));
                }
                Some(cursor)
            }
            None => None,
        };

        /*
         * 一覧取得と後段フィルタ
         */
        let mut entries = db
            .list_page_entries_by_prefix(normalized_prefix, false)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("list pages failed: {}", err),
                )
            })?;
        entries.sort_by_key(|entry| entry.path());

        let filtered_items = entries
            .into_iter()
            .filter(|entry| self.entry_visible_for_list(auth, entry))
            .filter(|entry| match normalized_cursor.as_deref() {
                Some(cursor) => entry.path().as_str() > cursor,
                None => true,
            })
            .collect::<Vec<_>>();

        let has_more = filtered_items.len() > limit;
        let selected = filtered_items
            .into_iter()
            .take(limit)
            .map(|entry| {
                ListPageItem::new(
                    entry.path(),
                    entry.latest_revision(),
                    format_mcp_timestamp(entry.timestamp()),
                    entry.user_name(),
                )
            })
            .collect::<Vec<_>>();
        let next_cursor =
            has_more.then(|| selected.last().map(|item| item.path.clone()))
            .flatten();

        Ok(ListPagesResult::new(selected, has_more, next_cursor))
    }

    ///
    /// `prompts/list`を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// prompt一覧取得結果を返す。
    ///
    pub(crate) fn list_prompts(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        cursor: Option<&str>,
    ) -> Result<ListPromptsServiceResult, McpError> {
        /*
         * read scopeとcursorを検証する
         */
        self.ensure_operation_scope(auth, McpOperation::ListPrompts)?;
        if let Some(cursor) = cursor {
            validate_prompt_name(cursor).map_err(|_| {
                McpError::new(
                    McpErrorCode::InvalidInput,
                    "cursor is invalid",
                )
            })?;
        }

        /*
         * 公開可能な候補を取得する
         */
        let mut entries = db.list_prompt_candidates().map_err(|_| {
            McpError::new(
                McpErrorCode::InternalError,
                "internal error",
            )
        })?;
        entries.sort_by(|left, right| left.name().cmp(right.name()));

        /*
         * cursor境界と既定件数を適用する
         */
        let filtered = entries
            .into_iter()
            .filter(|entry| match cursor {
                Some(cursor) => entry.name() > cursor,
                None => true,
            })
            .take(DEFAULT_PROMPT_LIST_LIMIT + 1)
            .collect::<Vec<_>>();
        let has_more = filtered.len() > DEFAULT_PROMPT_LIST_LIMIT;
        let selected = filtered
            .into_iter()
            .take(DEFAULT_PROMPT_LIST_LIMIT)
            .collect::<Vec<_>>();
        let next_cursor = if has_more {
            selected.last().map(|entry| entry.name().to_string())
        } else {
            None
        };

        /*
         * database候補をMCP内部モデルへ変換する
         */
        let items = selected
            .into_iter()
            .map(|entry| {
                let arguments = entry
                    .arguments()
                    .iter()
                    .map(|argument| {
                        PromptListArgument::new(
                            argument.name().to_string(),
                            argument.description().to_string(),
                            argument.required(),
                        )
                    })
                    .collect();
                PromptListItem::new(
                    entry.name().to_string(),
                    entry.description().to_string(),
                    arguments,
                )
            })
            .collect();

        Ok(ListPromptsServiceResult::new(items, next_cursor))
    }

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

    ///
    /// `prompts/get`を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// prompt取得結果を返す。
    ///
    pub(crate) fn get_prompt(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        name: &str,
        arguments: Option<&JsonObject<String, JsonValue>>,
    ) -> Result<GetPromptServiceResult, McpError> {
        /*
         * read scopeとprompt名を検証する
         */
        self.ensure_operation_scope(auth, McpOperation::GetPrompt)?;
        validate_prompt_name(name).map_err(|_| {
            McpError::new(
                McpErrorCode::NotFound,
                "prompt not found",
            )
        })?;

        /*
         * 名前索引から最新ページソースを解決する
         */
        let entry = db
            .get_prompt_source_by_name(name)
            .map_err(|_| prompt_internal_error())?
            .ok_or_else(prompt_not_found)?;
        let extracted = extract_front_matter(entry.source())
            .map_err(|_| prompt_internal_error())?
            .ok_or_else(prompt_internal_error)?;
        let front_matter = parse_front_matter(
            extracted.front_matter(),
        )
        .map_err(|_| prompt_internal_error())?;
        let prompt = front_matter
            .prompt_page()
            .ok_or_else(prompt_internal_error)?;
        if prompt.name() != name {
            return Err(prompt_internal_error());
        }

        /*
         * 引数を検証・展開して単一message本文を生成する
         */
        let values = validate_prompt_arguments(&prompt, arguments)?;
        let body = expand_prompt_text(extracted.body(), &values)?;
        let message = match prompt.system() {
            Some(system) => {
                let system = expand_prompt_text(system, &values)?;
                format!("{}\n\n{}", system, body)
            }
            None => body,
        };

        Ok(GetPromptServiceResult::new(
            prompt.description().to_string(),
            message,
            entry.revision(),
        ))
    }

    ///
    /// `search_pages` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `fts_config` - FTS 設定
    /// * `query` - 検索式
    /// * `raw_prefix` - 検索 prefix
    /// * `limit` - 取得件数上限
    ///
    /// # 戻り値
    /// 検索結果を返す。
    ///
    pub(crate) fn search_pages(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        fts_config: &FtsIndexConfig,
        query: &str,
        targets: &[FtsSearchTarget],
        raw_prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<SearchPagesResult, McpError> {
        /*
         * 入力検証
         */
        self.ensure_operation_scope(auth, McpOperation::SearchPages)?;
        let query = query.trim();
        if query.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "query must not be empty",
            ));
        }
        if targets.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "target must not be empty",
            ));
        }
        let resolved_prefix =
            self.resolve_search_prefix_request(auth, raw_prefix)?;
        let limit = self.resolve_limit(limit, DEFAULT_SEARCH_LIMIT)?;

        /*
         * FTS の実行とスコアマージ
         */
        let mut merged = HashMap::new();
        for target in targets {
            let results = fts::search_index(
                fts_config,
                *target,
                query,
                false,
                false,
            )
            .map_err(map_search_error)?;
            self.merge_search_results(&mut merged, results);
        }

        /*
         * current path 解決と後段フィルタ
         */
        let page_ids = merged
            .values()
            .map(|result| result.page_id())
            .collect::<Vec<_>>();
        let current_paths = db.get_current_page_paths_by_ids(&page_ids).map_err(
            |err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("current path resolution failed: {}", err),
                )
            },
        )?;
        let mut items = Vec::new();
        for result in merged.into_values() {
            let Some(current_path) = current_paths.get(&result.page_id()) else {
                continue;
            };
            if current_path.deleted() || current_path.draft() {
                continue;
            }
            if !self.is_path_prefix_allowed(auth, current_path.current_path()) {
                continue;
            }
            if !self.matches_requested_prefix(
                &resolved_prefix,
                current_path.current_path(),
            ) {
                continue;
            }

            items.push(SearchPageItem::new(
                current_path.current_path().to_string(),
                result.revision(),
                result.score(),
                result.snippet(),
            ));
        }

        items.sort_by(|lhs, rhs| {
            rhs.score
                .partial_cmp(&lhs.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| lhs.path.cmp(&rhs.path))
        });
        items.truncate(limit);

        Ok(SearchPagesResult::new(items))
    }

    ///
    /// `get_page_section` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象 path
    /// * `section_selector` - section selector
    /// * `revision` - 取得 revision
    ///
    /// # 戻り値
    /// section 取得結果を返す。
    ///
    pub(crate) fn get_page_section(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        section_selector: SectionSelector,
        revision: Option<u64>,
    ) -> Result<GetPageSectionResult, McpError> {
        /*
         * path 認可とページ解決
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::GetPageSection,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * revision と本文の解決
         */
        let (revision, _, source) =
            self.resolve_revision_source(db, &resolved, revision)?;
        let parsed_sections = self.parse_markdown_toc_sections(&source)?;
        let target = self.resolve_section_selector(
            &parsed_sections,
            section_selector,
        )?;
        let content = self.extract_section_content(&source, &target);
        let section = TocSection::new(
            target.id,
            target.title,
            target.level,
            target.ordinal,
            target.parent_id,
            content.chars().count(),
        );

        Ok(GetPageSectionResult::new(
            resolved.normalized_path().to_string(),
            revision,
            section,
            content,
        ))
    }

    ///
    /// `create_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 作成先 path
    /// * `content` - 初期 Markdown 本文
    ///
    /// # 戻り値
    /// ページ作成結果を返す。
    ///
    pub(crate) fn create_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        content: &str,
    ) -> Result<WritePageResult, McpError> {
        /*
         * 入力と認可を検証する
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::CreatePage,
            raw_path,
        )?;
        if normalized_path == "/" {
            return Err(McpError::new(
                McpErrorCode::Forbidden,
                "operation is not allowed for root page",
            ));
        }

        /*
         * 作成処理を実行する
         */
        db.create_page(
            &normalized_path,
            auth.user().user_id(),
            content.to_string(),
        )
        .map_err(map_create_db_error)?;

        let page_id = db
            .get_page_id_by_path(&normalized_path)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("page id lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    "created page id not found",
                )
            })?;
        let instance_id =
            self.lookup_saved_instance_id(db, &page_id, 1, "created")?;

        Ok(WritePageResult::new(
            normalized_path,
            1,
            instance_id,
            "page created".to_string(),
        ))
    }

    ///
    /// `update_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 更新対象 path
    /// * `content` - 更新後 Markdown 本文
    ///
    /// # 戻り値
    /// ページ更新結果を返す。
    ///
    pub(crate) fn update_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        content: &str,
    ) -> Result<WritePageResult, McpError> {
        /*
         * path 認可とページ解決を行う
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::UpdatePage,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * ロック状態を確認してから保存する
         */
        let (revision, instance_id) = self.save_updated_page(
            db,
            &resolved,
            auth.user().user_id(),
            content.to_string(),
        )?;

        Ok(WritePageResult::new(
            resolved.normalized_path().to_string(),
            revision,
            instance_id,
            "page updated".to_string(),
        ))
    }

    ///
    /// `edit_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `request` - `edit_page` service 入力
    ///
    /// # 戻り値
    /// ページ編集結果を返す。
    ///
    pub(crate) fn edit_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        request: &EditPageRequest,
    ) -> Result<EditPageResult, McpError> {
        /*
         * path 認可とページ解決を先に行い、
         * 後続タスクで整合確認と operation 適用を接続する。
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::EditPage,
            request.path(),
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;
        let (latest_revision, _, latest_source) =
            self.resolve_revision_source(db, &resolved, None)?;
        if request.revision() != latest_revision {
            return Err(McpError::new(
                McpErrorCode::NotLatestRevision,
                "revision is not latest",
            ));
        }
        let latest_page_source = db
            .get_page_source(&resolved.page_id(), latest_revision)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("latest page source lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    "latest page source not found",
                )
            })?;
        let latest_instance_id = latest_page_source.instance_id().ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "latest page source instance_id is missing",
            )
        })?;
        if request.instance_id() != latest_instance_id.to_string() {
            return Err(McpError::new(
                McpErrorCode::InstanceIdNotMatch,
                "instance_id does not match latest content",
            ));
        }

        /*
         * 整合確認を通過した後にだけ本文変換と update 系保存へ進める。
         */
        let updated_source =
            self.apply_edit_page_operation(&latest_source, request.operation())?;
        let (revision, instance_id) = self.save_updated_page(
            db,
            &resolved,
            auth.user().user_id(),
            updated_source,
        )?;

        Ok(EditPageResult::new(
            resolved.normalized_path().to_string(),
            revision,
            instance_id,
            "page edited".to_string(),
        ))
    }

    ///
    /// `rename_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - リネーム元 path
    /// * `raw_rename_to` - リネーム先 path
    ///
    /// # 戻り値
    /// ページリネーム結果を返す。
    ///
    pub(crate) fn rename_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        raw_rename_to: &str,
    ) -> Result<WritePageResult, McpError> {
        /*
         * 入力 path を正規化し、移動元ページを解決する
         */
        let normalized_path = self.validate_and_normalize_path(raw_path)?;
        let normalized_rename_to =
            self.validate_and_normalize_path(raw_rename_to)?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * 認可と MCP 固有制約を検証する
         */
        self.ensure_rename_authorized(
            auth,
            resolved.normalized_path(),
            &normalized_rename_to,
        )?;
        if resolved.normalized_path() == "/" {
            return Err(McpError::new(
                McpErrorCode::Forbidden,
                "operation is not allowed for root page",
            ));
        }
        if resolved.normalized_path() == normalized_rename_to {
            let instance_id = self.lookup_saved_instance_id(
                db,
                &resolved.page_id(),
                resolved.latest_revision().unwrap_or(0),
                "renamed",
            )?;
            return Ok(WritePageResult::new(
                normalized_rename_to,
                resolved.latest_revision().unwrap_or(0),
                instance_id,
                "page rename skipped".to_string(),
            ));
        }
        if path_matches_prefix(
            &normalized_rename_to,
            resolved.normalized_path(),
        ) {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "invalid destination path",
            ));
        }

        /*
         * 再帰 rename を実行する
         */
        let renamed_revision = resolved.latest_revision().unwrap_or(0) + 1;
        db.rename_pages_recursive_by_id(
            &resolved.page_id(),
            &normalized_rename_to,
        )
        .map_err(map_rename_db_error)?;
        let instance_id = self.lookup_saved_instance_id(
            db,
            &resolved.page_id(),
            renamed_revision,
            "renamed",
        )?;

        Ok(WritePageResult::new(
            normalized_rename_to,
            renamed_revision,
            instance_id,
            format!("page renamed from {}", resolved.normalized_path()),
        ))
    }

    ///
    /// `append_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 追記対象 path
    /// * `content` - 追記文字列
    ///
    /// # 戻り値
    /// ページ追記結果を返す。
    ///
    pub(crate) fn append_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        content: &str,
    ) -> Result<AppendServiceResult, McpError> {
        /*
         * path と入力内容を検証する
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::AppendPage,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;
        if content.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "content must not be empty",
            ));
        }
        let user_id = self.resolve_auth_user_id(db, auth.user_id())?;

        /*
         * 競合が解消するまで短時間だけ再試行する
         */
        let started_at = Instant::now();
        loop {
            let state = self
                .get_append_conflict_state(db, &resolved.page_id())?;
            if state.draft() {
                return Err(McpError::new(
                    McpErrorCode::Conflict,
                    "draft page is not supported",
                ));
            }

            if state.locked() {
                if self.append_wait_timed_out(started_at) {
                    return Err(McpError::new(
                        McpErrorCode::Conflict,
                        "page is locked",
                    ));
                }

                thread::sleep(Duration::from_millis(
                    APPEND_WAIT_INTERVAL_MS,
                ));
                continue;
            }

            let latest_revision = state.latest_revision().ok_or_else(|| {
                McpError::new(
                    McpErrorCode::Conflict,
                    "draft page does not have latest revision",
                )
            })?;
            let source = self.resolve_append_base_source(
                db,
                &resolved.page_id(),
                latest_revision,
            )?;
            let appended_source = format!("{}{}", source, content);
            let allow_amend = state.latest_user_id() == Some(user_id.clone());
            let request = AppendPageRequest::new(
                resolved.page_id(),
                auth.user_id().to_string(),
                appended_source,
                latest_revision,
                allow_amend,
            );

            match db.append_page_by_id(&request) {
                Ok(result) => {
                    return Ok(self.build_append_result(
                        db,
                        &resolved.page_id(),
                        resolved.normalized_path(),
                        result,
                    )?);
                }
                Err(err) => {
                    if self.is_retryable_append_conflict(&err) {
                        if self.append_wait_timed_out(started_at) {
                            return Err(McpError::new(
                                McpErrorCode::Conflict,
                                "append conflict",
                            ));
                        }

                        thread::sleep(Duration::from_millis(
                            APPEND_WAIT_INTERVAL_MS,
                        ));
                        continue;
                    }

                    return Err(map_append_db_error(err));
                }
            }
        }
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
        if auth.path_prefixes().allows_all() {
            return true;
        }

        auth.path_prefixes()
            .iter()
            .any(|prefix| path_matches_prefix(target_path, prefix))
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
        &self,
        db: &DatabaseManager,
        resolved: &ResolvedPage,
        user_name: &str,
        content: String,
    ) -> Result<(u64, String), McpError> {
        self.ensure_page_not_locked(db, &resolved.page_id())?;
        db.put_page(&resolved.page_id(), user_name, content, false)
            .map_err(map_update_db_error)?;

        let revision = resolved.latest_revision().unwrap_or(0) + 1;
        let page_source = db
            .get_page_source(&resolved.page_id(), revision)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("saved page source lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    "saved page source not found",
                )
            })?;
        let instance_id = page_source.instance_id().ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "saved page source instance_id is missing",
            )
        })?;

        Ok((revision, instance_id.to_string()))
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
        db: &DatabaseManager,
        page_id: &PageId,
    ) -> Result<crate::database::AppendConflictState, McpError> {
        db.get_append_conflict_state_by_id(page_id)
            .map_err(|err| {
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

    ///
    /// Markdown 本文から TOC / section編集用 section 一覧を構築する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    ///
    /// # 戻り値
    /// 解析した section 一覧を返す。
    ///
    fn parse_markdown_toc_sections(
        &self,
        source: &str,
    ) -> Result<Vec<ParsedHeading>, McpError> {
        /*
         * 見出しイベントの収集
         */
        let mut raw_headings = Vec::new();
        let mut current_heading: Option<(u32, usize, String)> = None;
        let parser = Parser::new(source).into_offset_iter();

        for (event, range) in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    current_heading = Some((
                        heading_level_to_u32(level),
                        range.start,
                        String::new(),
                    ));
                }
                Event::End(TagEnd::Heading(_)) => {
                    let Some((level, start, title)) =
                        current_heading.take()
                    else {
                        return Err(McpError::new(
                            McpErrorCode::InternalError,
                            "heading parser state is inconsistent",
                        ));
                    };
                    raw_headings.push((level, start, range.end, title));
                }
                Event::Text(text) | Event::Code(text) => {
                    if let Some((_, _, title)) = current_heading.as_mut() {
                        title.push_str(&text);
                    }
                }
                _ => {}
            }
        }

        /*
         * section メタ情報の構築
         */
        let mut parsed = Vec::new();
        let mut stack: Vec<(u32, String)> = Vec::new();
        for (index, (level, _, heading_end, title)) in
            raw_headings.iter().enumerate()
        {
            while stack
                .last()
                .is_some_and(|(parent_level, _)| *parent_level >= *level)
            {
                stack.pop();
            }

            let id = format!("s-{:03}", index + 1);
            let parent_id = stack.last().map(|(_, id)| id.clone());
            let content_end = raw_headings
                .iter()
                .skip(index + 1)
                .find(|(next_level, _, _, _)| *next_level <= *level)
                .map(|(_, start, _, _)| *start)
                .unwrap_or(source.len());

            parsed.push(ParsedHeading {
                id: id.clone(),
                title: title.trim().to_string(),
                level: *level,
                ordinal: (index + 1) as u32,
                parent_id,
                heading_start: raw_headings[index].1,
                content_start: *heading_end,
                content_end,
            });
            stack.push((*level, id));
        }

        Ok(parsed)
    }

    ///
    /// section 本文を抽出する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `section` - 対象 section
    ///
    /// # 戻り値
    /// section 本文を返す。
    ///
    fn extract_section_content(
        &self,
        source: &str,
        section: &ParsedHeading,
    ) -> String {
        source[section.content_start..section.content_end]
            .trim_start_matches(['\r', '\n'])
            .to_string()
    }

    ///
    /// section 本文を置き換える
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `section` - 置き換え対象 section
    /// * `new_content` - 置き換え後本文
    ///
    /// # 戻り値
    /// 対象見出し行を保持し、本文部分だけを差し替えた Markdown 本文を返す。
    ///
    /// # 注記
    /// 対象 section の本文範囲は `content_start..content_end` で扱うため、
    /// 子見出し配下も含めて置き換える。
    ///
    fn replace_section_content(
        &self,
        source: &str,
        section: &ParsedHeading,
        new_content: &str,
    ) -> String {
        let before = &source[..section.content_start];
        let target_body = &source[section.content_start..section.content_end];
        let after = &source[section.content_end..];

        let leading_end = target_body
            .char_indices()
            .find(|(_, ch)| !matches!(*ch, '\r' | '\n'))
            .map(|(index, _)| index)
            .unwrap_or(target_body.len());
        let trailing_start = target_body
            .char_indices()
            .rev()
            .find(|(_, ch)| !matches!(*ch, '\r' | '\n'))
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);
        let leading_breaks = &target_body[..leading_end];
        let trailing_breaks = if trailing_start < leading_end {
            ""
        } else {
            &target_body[trailing_start..]
        };

        let mut replaced = String::with_capacity(
            before.len()
                + leading_breaks.len()
                + new_content.len()
                + trailing_breaks.len()
                + after.len(),
        );
        replaced.push_str(before);
        replaced.push_str(leading_breaks);
        replaced.push_str(new_content);
        replaced.push_str(trailing_breaks);
        replaced.push_str(after);
        replaced
    }

    ///
    /// section を挿入する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `anchor` - 挿入位置の基準 section
    /// * `placement` - anchor に対する前後指定
    /// * `new_section` - 挿入する完全なセクション本文
    ///
    /// # 戻り値
    /// 指定位置へ section を挿入した Markdown 本文を返す。
    ///
    fn insert_section_content(
        &self,
        source: &str,
        anchor: &ParsedHeading,
        placement: EditPageInsertSectionPlacement,
        new_section: &str,
    ) -> String {
        let insert_at = match placement {
            EditPageInsertSectionPlacement::Before => anchor.heading_start,
            EditPageInsertSectionPlacement::After => anchor.content_end,
        };
        let before = &source[..insert_at];
        let after = &source[insert_at..];

        let mut inserted = String::with_capacity(
            before.len() + new_section.len() + after.len() + 2,
        );
        inserted.push_str(before);
        inserted.push_str(new_section);
        if !new_section.ends_with('\n') && !after.starts_with('\n') {
            inserted.push('\n');
        }
        inserted.push_str(after);
        inserted
    }

    ///
    /// section を削除する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `section` - 削除対象 section
    ///
    /// # 戻り値
    /// 対象 section を削除した Markdown 本文を返す。
    ///
    /// # 注記
    /// 削除範囲は見出し行を含む section 全体とし、子見出し配下もまとめて削除する。
    ///
    fn delete_section_content(
        &self,
        source: &str,
        section: &ParsedHeading,
    ) -> String {
        let before = &source[..section.heading_start];
        let after = &source[section.content_end..];
        let mut deleted = String::with_capacity(before.len() + after.len());
        deleted.push_str(before);
        deleted.push_str(after);
        deleted
    }

    ///
    /// テキスト置換を適用する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `old_text` - 置換対象文字列
    /// * `new_text` - 置換後文字列
    /// * `occurrence` - 複数一致時の対象指定
    ///
    /// # 戻り値
    /// 置換後の Markdown 本文を返す。
    ///
    /// # 注記
    /// `occurrence = None` は `First` と同じ意味で扱う。
    ///
    fn replace_text_content(
        &self,
        source: &str,
        old_text: &str,
        new_text: &str,
        occurrence: Option<EditPageReplaceTextOccurrence>,
    ) -> String {
        match occurrence.unwrap_or(EditPageReplaceTextOccurrence::First) {
            EditPageReplaceTextOccurrence::First => {
                source.replacen(old_text, new_text, 1)
            }
            EditPageReplaceTextOccurrence::All => {
                source.replace(old_text, new_text)
            }
        }
    }

    ///
    /// `edit_page` の単一 operation を本文へ適用する
    ///
    /// # 引数
    /// * `source` - 変換対象の Markdown 本文
    /// * `operation` - 適用する編集操作
    ///
    /// # 戻り値
    /// 変換後の Markdown 本文を返す。
    ///
    /// # 注記
    /// operation 共通の失敗分類は本 helper で固定する。
    /// - selector 解決失敗は `resolve_section_selector()` の分類をそのまま使う
    /// - `replace_text.old_text` が空文字なら `invalid_input`
    /// - `replace_text` で一致箇所が 0 件なら `not_found`
    ///
    fn apply_edit_page_operation(
        &self,
        source: &str,
        operation: &EditPageOperation,
    ) -> Result<String, McpError> {
        match operation {
            EditPageOperation::ReplaceSection { section, content } => {
                let sections = self.parse_markdown_toc_sections(source)?;
                let target =
                    self.resolve_section_selector(&sections, section.clone().into())?;
                Ok(self.replace_section_content(source, &target, content))
            }
            EditPageOperation::InsertSection {
                anchor,
                placement,
                content,
            } => {
                let sections = self.parse_markdown_toc_sections(source)?;
                let target =
                    self.resolve_section_selector(&sections, anchor.clone().into())?;
                Ok(self.insert_section_content(
                    source,
                    &target,
                    *placement,
                    content,
                ))
            }
            EditPageOperation::DeleteSection { section } => {
                let sections = self.parse_markdown_toc_sections(source)?;
                let target =
                    self.resolve_section_selector(&sections, section.clone().into())?;
                Ok(self.delete_section_content(source, &target))
            }
            EditPageOperation::ReplaceText {
                old_text,
                new_text,
                occurrence,
            } => {
                if old_text.is_empty() {
                    return Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        "old_text must not be empty",
                    ));
                }

                if !source.contains(old_text) {
                    return Err(McpError::new(
                        McpErrorCode::NotFound,
                        format!("text not found: {}", old_text),
                    ));
                }

                Ok(self.replace_text_content(
                    source,
                    old_text,
                    new_text,
                    *occurrence,
                ))
            }
        }
    }

    ///
    /// section selector を解決する
    ///
    /// # 引数
    /// * `sections` - 解析済み section 一覧
    /// * `selector` - section selector
    ///
    /// # 戻り値
    /// 解決した section を返す。
    ///
    /// # 注記
    /// 本 helper は `get_page_section` だけでなく、`edit_page` における
    /// `replace_section` / `insert_section` / `delete_section` の selector 解決でも
    /// 共通利用する前提とする。
    /// `ById` と `ByTitle` の両方をここで扱い、`ByTitle` の空文字拒否、
    /// 見出し未存在時の `not_found`、同名複数時の `invalid_input` も
    /// `edit_page` 側で再利用する。
    /// したがって selector 解決失敗の分類は以下で固定する。
    /// - `ById` 未存在: `not_found`
    /// - `ByTitle` 未存在: `not_found`
    /// - `ByTitle` 空文字: `invalid_input`
    /// - `ByTitle` 複数一致: `invalid_input`
    ///
    fn resolve_section_selector(
        &self,
        sections: &[ParsedHeading],
        selector: SectionSelector,
    ) -> Result<ParsedHeading, McpError> {
        match selector {
            SectionSelector::ById(id) => sections
                .iter()
                .find(|section| section.id == id)
                .cloned()
                .ok_or_else(|| {
                    McpError::new(
                        McpErrorCode::NotFound,
                        format!("section not found: {}", id),
                    )
                }),
            SectionSelector::ByTitle(title) => {
                let title = title.trim();
                if title.is_empty() {
                    return Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        "section title must not be empty",
                    ));
                }

                let matches = sections
                    .iter()
                    .filter(|section| section.title == title)
                    .cloned()
                    .collect::<Vec<_>>();
                match matches.len() {
                    0 => Err(McpError::new(
                        McpErrorCode::NotFound,
                        format!("section not found: {}", title),
                    )),
                    1 => Ok(matches[0].clone()),
                    /*
                     * 同名見出しが複数ある場合は selector として一意に解決できないため、
                     * `get_page_section` / `edit_page` の双方で `invalid_input` とする。
                     */
                    _ => Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        format!("section title is ambiguous: {}", title),
                    )),
                }
            }
        }
    }
}

///
/// path が prefix 境界一致で許可されるかを返す
///
/// # 引数
/// * `target_path` - 判定対象 path
/// * `prefix` - 判定に用いる prefix
///
/// # 戻り値
/// `target_path == prefix` または `target_path` が `prefix/` 配下なら
/// `true` を返す。
///
fn path_matches_prefix(target_path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return true;
    }

    target_path == prefix
        || target_path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
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
/// MCP 用のページ path 妥当性検証を行う
///
/// # 引数
/// * `path` - 検証対象 path
///
/// # 戻り値
/// 検証に成功した場合は `Ok(())` を返す。
///
fn validate_page_path_for_mcp(path: &str) -> Result<(), &'static str> {
    if !path.starts_with('/') {
        return Err("path must be absolute");
    }

    if path.chars().any(|ch| FORBIDDEN_PATH_CHARS.contains(&ch)) {
        return Err("path contains invalid character");
    }

    Ok(())
}

///
/// 絶対 path を MCP 内部表現へ正規化する
///
/// # 引数
/// * `raw_path` - 正規化前の path
///
/// # 戻り値
/// root 以外の末尾 `/` を除去した path を返す。
///
fn normalize_absolute_path(raw_path: &str) -> String {
    if raw_path == "/" {
        return "/".to_string();
    }

    raw_path.trim_end_matches('/').to_string()
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
/// `.` / `..` セグメントを含むかを返す
///
/// # 引数
/// * `path` - 判定対象 path
///
/// # 戻り値
/// `.` または `..` を含む場合は `true` を返す。
///
fn has_dot_path_segment(path: &str) -> bool {
    path.split('/').any(|segment| segment == "." || segment == "..")
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

///
/// prompt取得の不存在エラーを生成する
///
/// # 戻り値
/// 情報を秘匿したprompt不存在エラーを返す。
///
fn prompt_not_found() -> McpError {
    McpError::new(McpErrorCode::NotFound, "prompt not found")
}

///
/// prompt取得の内部エラーを生成する
///
/// # 戻り値
/// 内部情報を秘匿したエラーを返す。
///
fn prompt_internal_error() -> McpError {
    McpError::new(McpErrorCode::InternalError, "internal error")
}

///
/// prompt要求引数を検証して展開値を構築する
///
/// # 引数
/// * `prompt` - prompt定義
/// * `arguments` - 要求引数
///
/// # 戻り値
/// 宣言済み引数ごとの展開値を返す。
///
fn validate_prompt_arguments(
    prompt: &PromptPageFrontMatter,
    arguments: Option<&JsonObject<String, JsonValue>>,
) -> Result<HashMap<String, String>, McpError> {
    let definitions = prompt
        .arguments()
        .iter()
        .map(|argument| (argument.name(), argument))
        .collect::<HashMap<_, _>>();

    /*
     * 未知引数と値型を検証する
     */
    if let Some(arguments) = arguments {
        for (name, value) in arguments {
            if !definitions.contains_key(name.as_str()) {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    format!("unknown prompt argument: {}", name),
                ));
            }
            if !value.is_string() {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    format!(
                        "prompt argument must be a string: {}",
                        name,
                    ),
                ));
            }
        }
    }

    /*
     * 必須判定とoptional既定値を適用する
     */
    let mut values = HashMap::new();
    for definition in prompt.arguments() {
        let value = arguments
            .and_then(|arguments| arguments.get(definition.name()))
            .and_then(JsonValue::as_str);
        if definition.required() == Some(true) && value.is_none() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                format!(
                    "required prompt argument is missing: {}",
                    definition.name(),
                ),
            ));
        }
        values.insert(
            definition.name().to_string(),
            value.unwrap_or_default().to_string(),
        );
    }

    Ok(values)
}

///
/// prompt専用placeholderを一回だけ展開する
///
/// # 引数
/// * `text` - 展開対象文字列
/// * `values` - 宣言済み引数ごとの展開値
///
/// # 戻り値
/// 展開済み文字列を返す。
///
fn expand_prompt_text(
    text: &str,
    values: &HashMap<String, String>,
) -> Result<String, McpError> {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    /*
     * placeholder候補を左から一回だけ走査する
     */
    while let Some(start) = rest.find("{{@") {
        result.push_str(&rest[..start]);
        let placeholder = &rest[start..];
        let Some(end) = placeholder.find("}}") else {
            result.push_str(placeholder);
            return Ok(result);
        };
        let token = &placeholder[..end + 2];
        let inner = &placeholder[3..end];

        if let Some(escaped_name) = inner.strip_prefix('@') {
            if is_valid_prompt_argument_name(escaped_name) {
                result.push_str("{{@");
                result.push_str(escaped_name);
                result.push_str("}}");
            } else {
                result.push_str(token);
            }
        } else if is_valid_prompt_argument_name(inner) {
            let value = values
                .get(inner)
                .ok_or_else(prompt_internal_error)?;
            result.push_str(value);
        } else {
            result.push_str(token);
        }
        rest = &placeholder[end + 2..];
    }
    result.push_str(rest);

    Ok(result)
}

///
/// 見出しレベルを数値へ変換する
///
/// # 引数
/// * `level` - pulldown-cmark の見出しレベル
///
/// # 戻り値
/// 数値化した見出しレベルを返す。
///
fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
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
/// DB の作成失敗を MCP エラーへ写像する
///
/// # 引数
/// * `err` - DB 失敗
///
/// # 戻り値
/// MCP エラーへ写像した結果を返す。
///
fn map_create_db_error(err: Error) -> McpError {
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
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::auth::{AuthContext, AuthUser};
    use crate::database::DatabaseManager;
    use crate::database::types::{
        BearerScopeSet,
        PathPrefixSet,
        UserAttribute,
        UserAttributeSet,
    };
    use crate::fts::{FtsDocument, FtsIndexConfig, extract_markdown_sections};

    use super::*;

    ///
    /// MCP サービス層テスト用の DB を生成する
    ///
    /// # 戻り値
    /// `(一時ディレクトリ, DatabaseManager)` を返す。
    ///
    fn open_test_manager() -> (PathBuf, DatabaseManager) {
        let base_dir = std::env::temp_dir()
            .join(format!("luwiki-mcp-service-{}", ulid::Ulid::new()));
        let asset_dir = base_dir.join("assets");
        let db_path = base_dir.join("test.redb");

        fs::create_dir_all(&asset_dir).expect("create asset dir failed");

        let manager = DatabaseManager::open(&db_path, &asset_dir)
            .expect("open manager failed");

        (base_dir, manager)
    }

    ///
    /// MCP サービス層テスト用の FTS 設定を生成する
    ///
    /// # 引数
    /// * `base_dir` - テスト基底ディレクトリ
    ///
    /// # 戻り値
    /// FTS 設定を返す。
    ///
    fn open_test_fts_config(base_dir: &PathBuf) -> FtsIndexConfig {
        FtsIndexConfig::new(base_dir.join("fts"))
    }

    ///
    /// 指定ページの FTS 文書を再構築する
    ///
    /// # 引数
    /// * `manager` - データベースマネージャ
    /// * `fts_config` - FTS 設定
    /// * `path` - 対象 current path
    ///
    /// # 戻り値
    /// なし
    ///
    fn rebuild_test_fts_page(
        manager: &DatabaseManager,
        fts_config: &FtsIndexConfig,
        path: &str,
    ) {
        let page_id = manager
            .get_page_id_by_path(path)
            .expect("resolve page id failed")
            .expect("page id missing");
        fts::update_pages_index(fts_config, manager, &[page_id], false)
            .expect("update index failed");
    }

    #[test]
    fn operation_scope_follows_required_scope_table() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::new(),
            None,
        );

        assert!(service
            .ensure_operation_scope(&auth, McpOperation::GetPage)
            .is_ok());
        assert_eq!(
            McpOperation::ListPrompts.required_scope(),
            BearerScope::Read,
        );
        assert!(!McpOperation::ListPrompts.is_write());
        assert_eq!(
            McpOperation::ListResources.required_scope(),
            BearerScope::Read,
        );
        assert!(!McpOperation::ListResources.is_write());
        assert_eq!(
            McpOperation::GetPrompt.required_scope(),
            BearerScope::Read,
        );
        assert!(!McpOperation::GetPrompt.is_write());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::CreatePage)
            .is_ok());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::UpdatePage)
            .is_ok());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::AppendPage)
            .is_ok());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::DeletePage)
            .is_ok());
    }

    ///
    /// prompts/getが要求引数の必須性、名前、型を
    /// 決定済み規則で検証することを確認する。
    ///
    /// # 注記
    /// required不足、未知引数、各非string型を拒否し、
    /// optional未指定と未使用引数を許可する。
    ///
    #[test]
    fn get_prompt_validates_argument_inputs() {
        /*
         * 引数定義を持つpromptとread認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/validation",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: validate-arguments\n",
                    "  description: validate arguments\n",
                    "  arguments:\n",
                    "    - name: required\n",
                    "      description: required value\n",
                    "      required: true\n",
                    "    - name: optional\n",
                    "      description: optional value\n",
                    "    - name: unused\n",
                    "      description: unused value\n",
                    "---\n",
                    "{{@required}}/{{@optional}}",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let service = McpService::new();

        /*
         * rmcp引数mapのキー一意性を確認する
         */
        let mut unique_arguments = JsonObject::new();
        unique_arguments.insert(
            "required".to_string(),
            JsonValue::String("first".to_string()),
        );
        unique_arguments.insert(
            "required".to_string(),
            JsonValue::String("second".to_string()),
        );
        assert_eq!(unique_arguments.len(), 1);
        assert_eq!(
            unique_arguments
                .get("required")
                .and_then(JsonValue::as_str),
            Some("second"),
        );

        /*
         * required不足と未知引数を拒否する
         */
        let missing = service
            .get_prompt(
                &auth,
                &manager,
                "validate-arguments",
                None,
            )
            .expect_err("required argument must be rejected");
        assert_eq!(missing.code(), McpErrorCode::InvalidInput);
        assert_eq!(
            missing.message(),
            "required prompt argument is missing: required",
        );
        let unknown = JsonObject::from_iter([
            (
                "required".to_string(),
                JsonValue::String("value".to_string()),
            ),
            (
                "unknown".to_string(),
                JsonValue::String("secret-value".to_string()),
            ),
        ]);
        let unknown_error = service
            .get_prompt(
                &auth,
                &manager,
                "validate-arguments",
                Some(&unknown),
            )
            .expect_err("unknown argument must be rejected");
        assert_eq!(
            unknown_error.message(),
            "unknown prompt argument: unknown",
        );
        assert!(!unknown_error.message().contains("secret-value"));

        /*
         * string以外の全JSON型を拒否する
         */
        for value in [
            JsonValue::Null,
            JsonValue::Bool(true),
            JsonValue::Number(1.into()),
            JsonValue::Array(Vec::new()),
            JsonValue::Object(JsonObject::new()),
        ] {
            let arguments = JsonObject::from_iter([
                (
                    "required".to_string(),
                    JsonValue::String("value".to_string()),
                ),
                ("optional".to_string(), value),
            ]);
            let error = service
                .get_prompt(
                    &auth,
                    &manager,
                    "validate-arguments",
                    Some(&arguments),
                )
                .expect_err("non-string argument must be rejected");
            assert_eq!(error.code(), McpErrorCode::InvalidInput);
            assert_eq!(
                error.message(),
                "prompt argument must be a string: optional",
            );
        }

        /*
         * optional未指定と未使用引数を許可する
         */
        let arguments = JsonObject::from_iter([(
            "required".to_string(),
            JsonValue::String("value".to_string()),
        )]);
        let result = service
            .get_prompt(
                &auth,
                &manager,
                "validate-arguments",
                Some(&arguments),
            )
            .expect("valid arguments must succeed");
        assert_eq!(result.message(), "value/");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// prompts/getがplaceholderを一回だけ展開し、
    /// 専用エスケープ規則を適用することを確認する。
    ///
    /// # 注記
    /// system、Markdownコード、複数出現、不正形式、
    /// 挿入値内placeholderを同時に検証する。
    ///
    #[test]
    fn get_prompt_expands_placeholders_once() {
        /*
         * 展開規則を判別できるpromptを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/expansion",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: expand-arguments\n",
                    "  description: expand arguments\n",
                    "  system: \"System {{@value}}\"\n",
                    "  arguments:\n",
                    "    - name: value\n",
                    "      description: required value\n",
                    "      required: true\n",
                    "    - name: optional\n",
                    "      description: optional value\n",
                    "---\n",
                    "{{@value}}|{{@value}}|{{@optional}}\n",
                    "{{@@literal}}|\\{{@value}}\n",
                    "{{@ value }}|{{@value }}\n",
                    "`{{@value}}`\n",
                    "```\n{{@value}}\n```\n",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let inserted = "{{@optional}}/{{macro}}/{{!macro}}";
        let arguments = JsonObject::from_iter([(
            "value".to_string(),
            JsonValue::String(inserted.to_string()),
        )]);

        /*
         * systemと本文の一回展開結果を確認する
         */
        let result = McpService::new()
            .get_prompt(
                &auth,
                &manager,
                "expand-arguments",
                Some(&arguments),
            )
            .expect("expand prompt failed");
        assert_eq!(
            result.message(),
            concat!(
                "System {{@optional}}/{{macro}}/{{!macro}}\n\n",
                "{{@optional}}/{{macro}}/{{!macro}}|",
                "{{@optional}}/{{macro}}/{{!macro}}|\n",
                "{{@literal}}|\\{{@optional}}/{{macro}}/{{!macro}}\n",
                "{{@ value }}|{{@value }}\n",
                "`{{@optional}}/{{macro}}/{{!macro}}`\n",
                "```\n",
                "{{@optional}}/{{macro}}/{{!macro}}\n",
                "```\n",
            ),
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 分解済みスコープでは他操作を暗黙包含しないことを確認する。
    ///
    /// # 注記
    /// `append` と `update` を例に、スコープ不足が forbidden で
    /// 返ることを検証する。
    ///
    #[test]
    fn operation_scope_rejects_insufficient_decomposed_scope() {
        let service = McpService::new();
        let append_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let update_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 分解済みスコープが別 write 系操作を包含しないことを検証する
         */
        let update_err = service
            .ensure_operation_scope(&append_only, McpOperation::UpdatePage)
            .expect_err("append only must not allow update");
        let append_err = service
            .ensure_operation_scope(&update_only, McpOperation::AppendPage)
            .expect_err("update only must not allow append");

        assert_eq!(update_err.code(), McpErrorCode::Forbidden);
        assert_eq!(append_err.code(), McpErrorCode::Forbidden);
    }

    ///
    /// ReadOnly 属性を持つユーザでは write 系操作だけが拒否されることを確認する。
    ///
    /// # 注記
    /// read 系操作は従来どおり成功し、write 系操作だけが
    /// `forbidden` になることを検証する。
    ///
    #[test]
    fn operation_scope_rejects_read_only_user_only_for_write_operations() {
        let service = McpService::new();
        let auth = AuthContext::new_with_attributes(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::new(),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );

        assert!(service
            .ensure_operation_scope(&auth, McpOperation::GetPage)
            .is_ok());

        let err = service
            .ensure_operation_scope(&auth, McpOperation::UpdatePage)
            .expect_err("read only user must not allow update");

        assert_eq!(err.code(), McpErrorCode::Forbidden);
        assert_eq!(
            err.message(),
            "read only denied: write operation is not allowed"
        );
    }

    ///
    /// ReadOnly 属性付きユーザで read 系が成功し、write 系が forbidden
    /// になることを確認する。
    ///
    /// # 注記
    /// `get_page` / `get_page_toc` / `list_pages` / `search_pages` /
    /// `get_page_section` は成功し、`create_page` / `update_page` /
    /// `rename_page` / `append_page` は `forbidden` になることを検証する。
    ///
    #[test]
    fn read_only_user_allows_mcp_read_operations_and_rejects_write_operations() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/readonly",
                "user",
                "# Title\n\nintro\n\n## Child\n\nchild text\n".to_string(),
            )
            .expect("create page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/readonly");

        let service = McpService::new();
        let auth = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/mcp"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );

        /*
         * read 系操作が成功することを検証する
         */
        let page = service
            .get_page(&auth, &manager, "/mcp/readonly", None)
            .expect("get page failed");
        assert_eq!(page.path(), "/mcp/readonly");

        let toc = service
            .get_page_toc(&auth, &manager, "/mcp/readonly", None)
            .expect("get page toc failed");
        assert_eq!(toc.sections().len(), 2);

        let list = service
            .list_pages(&auth, &manager, "/mcp", Some(10), None)
            .expect("list pages failed");
        assert_eq!(list.items().len(), 1);
        assert_eq!(list.items()[0].path(), "/mcp/readonly");

        let search = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "child",
                &[FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("search pages failed");
        assert_eq!(search.items().len(), 1);
        assert_eq!(search.items()[0].path(), "/mcp/readonly");

        let section = service
            .get_page_section(
                &auth,
                &manager,
                "/mcp/readonly",
                SectionSelector::ByTitle("Child".to_string()),
                None,
            )
            .expect("get page section failed");
        assert_eq!(section.section().title(), "Child");

        /*
         * write 系操作が forbidden になることを検証する
         */
        let create_err = service
            .create_page(&auth, &manager, "/mcp/readonly-created", "# created")
            .expect_err("read only user must not create page");
        assert_eq!(create_err.code(), McpErrorCode::Forbidden);

        let update_err = service
            .update_page(&auth, &manager, "/mcp/readonly", "# updated")
            .expect_err("read only user must not update page");
        assert_eq!(update_err.code(), McpErrorCode::Forbidden);

        let rename_err = service
            .rename_page(
                &auth,
                &manager,
                "/mcp/readonly",
                "/mcp/readonly-renamed",
            )
            .expect_err("read only user must not rename page");
        assert_eq!(rename_err.code(), McpErrorCode::Forbidden);

        let append_err = service
            .append_page(&auth, &manager, "/mcp/readonly", "\nnext")
            .expect_err("read only user must not append page");
        assert_eq!(append_err.code(), McpErrorCode::Forbidden);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn read_only_user_rejects_edit_page_as_forbidden() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/edit-readonly",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");

        let latest_source = manager
            .get_page_source(
                &manager
                    .get_page_id_by_path("/mcp/edit-readonly")
                    .expect("resolve page id failed")
                    .expect("page id not found"),
                1,
            )
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-readonly".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("read only user must not edit page");

        assert_eq!(error.code(), McpErrorCode::Forbidden);
        assert_eq!(
            error.message(),
            "read only denied: write operation is not allowed"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_succeeds_when_revision_and_instance_id_match_latest() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-success",
                "user",
                "# Title\n\nbefore body\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-success".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "before".to_string(),
                new_text: "after".to_string(),
                occurrence: None,
            },
        );

        let result = service
            .edit_page(&auth, &manager, &request)
            .expect("edit page should succeed");
        let saved_source = manager
            .get_page_source(&page_id, result.revision())
            .expect("lookup saved source failed")
            .expect("saved source missing");

        assert_eq!(result.path(), "/mcp/edit-success");
        assert_eq!(result.revision(), 2);
        assert_eq!(result.summary(), "page edited");
        assert_eq!(
            result.instance_id(),
            saved_source
                .instance_id()
                .expect("saved instance_id missing")
                .to_string()
        );
        assert!(saved_source.source().contains("after body"));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_rejects_non_latest_revision() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-revision",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");
        manager
            .put_page(&page_id, "user", "# Title\n\nnew body\n".to_string(), false)
            .expect("put page failed");
        let stale_source = manager
            .get_page_source(&page_id, 1)
            .expect("get stale source failed")
            .expect("stale source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-revision".to_string(),
            1,
            stale_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("stale revision must fail");

        assert_eq!(error.code(), McpErrorCode::NotLatestRevision);
        assert_eq!(error.message(), "revision is not latest");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_rejects_mismatched_instance_id() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/edit-instance",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-instance".to_string(),
            1,
            "instance-mismatch".to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("mismatched instance_id must fail");

        assert_eq!(error.code(), McpErrorCode::InstanceIdNotMatch);
        assert_eq!(
            error.message(),
            "instance_id does not match latest content"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_resolves_current_path_and_saves_updated_page() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-flow",
                "user",
                "# Title\n\nbefore body\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-flow/".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "before".to_string(),
                new_text: "after".to_string(),
                occurrence: None,
            },
        );

        let result = service
            .edit_page(&auth, &manager, &request)
            .expect("edit page failed");
        let saved_source = manager
            .get_page_source(&page_id, result.revision())
            .expect("lookup saved source failed")
            .expect("saved source missing");

        assert_eq!(result.path(), "/mcp/edit-flow");
        assert_eq!(result.revision(), 2);
        assert_eq!(result.summary(), "page edited");
        assert_eq!(saved_source.source(), "# Title\n\nafter body\n");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_returns_conflict_when_page_is_locked() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-locked",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let _lock_info = manager
            .acquire_page_lock(&page_id, "user")
            .expect("acquire lock failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-locked".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("locked page must fail");

        assert_eq!(error.code(), McpErrorCode::Conflict);
        assert_eq!(error.message(), "page is locked");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_classifies_selector_and_operation_failures() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-failure",
                "user",
                "# Root\n\nintro\n\n## Child\n\nfirst\n\n## Child\n\nsecond\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let instance_id = latest_source
            .instance_id()
            .expect("instance_id missing")
            .to_string();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        let missing_section = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::DeleteSection {
                        section: SectionSelector::ById("s-999".to_string()),
                    },
                ),
            )
            .expect_err("missing section must fail");
        assert_eq!(missing_section.code(), McpErrorCode::NotFound);

        let ambiguous_title = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ByTitle("Child".to_string()),
                        content: "updated".to_string(),
                    },
                ),
            )
            .expect_err("ambiguous title must fail");
        assert_eq!(ambiguous_title.code(), McpErrorCode::InvalidInput);

        let missing_text = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::ReplaceText {
                        old_text: "missing".to_string(),
                        new_text: "updated".to_string(),
                        occurrence: None,
                    },
                ),
            )
            .expect_err("missing text must fail");
        assert_eq!(missing_text.code(), McpErrorCode::NotFound);

        let invalid_text = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id,
                    EditPageOperation::ReplaceText {
                        old_text: "".to_string(),
                        new_text: "updated".to_string(),
                        occurrence: None,
                    },
                ),
            )
            .expect_err("empty old_text must fail");
        assert_eq!(invalid_text.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_preserves_saved_content_latest_revision_and_instance_id_consistency() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-consistency",
                "user",
                "# Title\n\nalpha beta alpha\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-consistency".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "alpha".to_string(),
                new_text: "gamma".to_string(),
                occurrence: Some(EditPageReplaceTextOccurrence::All),
            },
        );

        let result = service
            .edit_page(&auth, &manager, &request)
            .expect("edit page failed");
        let resolved = service
            .resolve_page_by_path(&manager, "/mcp/edit-consistency")
            .expect("resolve updated page failed");
        let latest_source = manager
            .get_page_source(&page_id, result.revision())
            .expect("get latest source failed")
            .expect("latest source missing");

        assert_eq!(resolved.latest_revision(), Some(result.revision()));
        assert_eq!(result.revision(), 2);
        assert_eq!(latest_source.source(), "# Title\n\ngamma beta gamma\n");
        assert_eq!(
            latest_source
                .instance_id()
                .expect("latest instance_id missing")
                .to_string(),
            result.instance_id()
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn path_prefix_matches_boundary_only() {
        assert!(path_matches_prefix("/docs", "/docs"));
        assert!(path_matches_prefix("/docs/a", "/docs"));
        assert!(!path_matches_prefix("/docs2", "/docs"));
    }

    #[test]
    fn validate_and_normalize_path_trims_non_root_trailing_slash() {
        let service = McpService::new();

        assert_eq!(
            service
                .validate_and_normalize_path("/docs/topic/")
                .expect("normalize failed"),
            "/docs/topic",
        );
        assert_eq!(
            service
                .validate_and_normalize_path("/")
                .expect("root normalize failed"),
            "/",
        );
    }

    #[test]
    fn validate_and_normalize_path_rejects_dot_segments() {
        let service = McpService::new();

        let err = service
            .validate_and_normalize_path("/docs/../private")
            .expect_err("dot segment must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
    }

    #[test]
    fn resolve_prefix_request_denies_outside_prefix() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let err = service
            .resolve_list_prefix_request(&auth, Some("/private"))
            .expect_err("outside prefix must fail");

        assert_eq!(err.code(), McpErrorCode::Forbidden);
    }

    #[test]
    fn filter_authorized_paths_keeps_only_allowed_paths() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let filtered = service.filter_authorized_paths(
            &auth,
            vec!["/docs", "/docs/a", "/private", "/docs2"],
        );

        assert_eq!(filtered, vec!["/docs", "/docs/a"]);
    }

    #[test]
    fn rename_requires_both_source_and_destination_in_prefix() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        assert!(service
            .ensure_rename_authorized(&auth, "/docs/a", "/docs/b")
            .is_ok());

        let err = service
            .ensure_rename_authorized(&auth, "/docs/a", "/private/b")
            .expect_err("outside destination must fail");
        assert_eq!(err.code(), McpErrorCode::Forbidden);
    }

    #[test]
    fn search_prefix_request_uses_search_scope_and_prefix_filter() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let resolved = service
            .resolve_search_prefix_request(&auth, Some("/docs"))
            .expect("search prefix resolve failed");
        let filtered = service.filter_paths_for_prefix_request(
            &auth,
            &resolved,
            vec!["/docs", "/docs/a", "/private", "/docs2"],
        );

        assert_eq!(filtered, vec!["/docs", "/docs/a"]);
    }

    #[test]
    fn resolve_prefix_request_normalizes_trailing_slash() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let resolved = service
            .resolve_list_prefix_request(&auth, Some("/docs/"))
            .expect("prefix normalize failed");

        assert_eq!(resolved.requested_prefix(), Some("/docs"));
        assert_eq!(
            resolved.filter_mode(),
            &PrefixFilterMode::DescendantsOf("/docs".to_string()),
        );
    }

    #[test]
    fn resolve_page_by_path_returns_resolved_page() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/page", "user", "# page".to_string())
            .expect("create page failed");
        let service = McpService::new();

        /*
         * current path 解決結果を検証する
         */
        let resolved = service
            .resolve_page_by_path(&manager, "/mcp/page/")
            .expect("resolve page failed");

        assert_eq!(resolved.normalized_path(), "/mcp/page");
        assert_eq!(resolved.page_id(), page_id);
        assert_eq!(resolved.latest_revision(), Some(1));
        assert!(!resolved.page_index().is_draft());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_page_by_path_rejects_draft_page() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_draft_page("/mcp/draft", "user")
            .expect("create draft failed");
        let service = McpService::new();

        /*
         * draft は conflict 扱いで拒否されることを検証する
         */
        let err = service
            .resolve_page_by_path(&manager, "/mcp/draft")
            .expect_err("draft must fail");

        assert_eq!(err.code(), McpErrorCode::Conflict);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_page_by_path_returns_not_found_for_missing_page() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();

        /*
         * 未存在 path は not_found になることを検証する
         */
        let err = service
            .resolve_page_by_path(&manager, "/mcp/missing")
            .expect_err("missing page must fail");

        assert_eq!(err.code(), McpErrorCode::NotFound);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn get_page_and_get_page_toc_follow_read_contract() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/read",
                "user",
                "# Title\n\nintro\n\n## Child\n\nchild body\n".to_string(),
            )
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * ページ全体取得を検証する
         */
        let page = service
            .get_page(&auth, &manager, "/mcp/read", None)
            .expect("get page failed");
        assert_eq!(page.path(), "/mcp/read");
        assert_eq!(page.revision(), 1);
        assert!(page.content().contains("## Child"));

        /*
         * TOC 取得を検証する
         */
        let toc = service
            .get_page_toc(&auth, &manager, "/mcp/read", None)
            .expect("get toc failed");
        assert_eq!(toc.path(), "/mcp/read");
        assert_eq!(toc.revision(), 1);
        assert_eq!(toc.sections().len(), 2);
        assert_eq!(toc.sections()[0].id(), "s-001");
        assert_eq!(toc.sections()[0].title(), "Title");
        assert_eq!(toc.sections()[1].parent_id(), Some("s-001"));
        assert!(toc.sections()[0].section_chars() > 0);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 単一 path 操作で path prefix 制約違反を拒否することを確認する。
    ///
    /// # 注記
    /// `get_page` 実行時に許可外 path を指定し、認可失敗になることを
    /// 検証する。
    ///
    #[test]
    fn get_page_rejects_path_outside_prefix_constraint() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/private/read", "user", "# secret".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * path prefix 制約違反が forbidden になることを検証する
         */
        let err = service
            .get_page(&auth, &manager, "/private/read", None)
            .expect_err("outside path must fail");

        assert_eq!(err.code(), McpErrorCode::Forbidden);
        assert_eq!(err.message(), "path prefix denied: /private/read");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn list_pages_applies_prefix_cursor_and_limit() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/a", "user", "a".to_string())
            .expect("create page a failed");
        manager
            .create_page("/mcp/b", "user", "b".to_string())
            .expect("create page b failed");
        manager
            .create_page("/mcp/c", "user", "c".to_string())
            .expect("create page c failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * cursor と limit によるページングを検証する
         */
        let first = service
            .list_pages(&auth, &manager, "/mcp", Some(2), None)
            .expect("first list failed");
        assert_eq!(first.items().len(), 2);
        assert_eq!(first.items()[0].path(), "/mcp/a");
        assert_eq!(first.items()[1].path(), "/mcp/b");
        assert!(first.has_more());
        assert_eq!(first.next_cursor(), Some("/mcp/b"));

        let second = service
            .list_pages(&auth, &manager, "/mcp", Some(2), first.next_cursor())
            .expect("second list failed");
        assert_eq!(second.items().len(), 1);
        assert_eq!(second.items()[0].path(), "/mcp/c");
        assert!(!second.has_more());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `list_pages` が要求 prefix 外の cursor を拒否することを確認する。
    ///
    /// # 注記
    /// `cursor` に prefix 外 path を与え、`InvalidInput` を期待する。
    ///
    #[test]
    fn list_pages_rejects_cursor_outside_requested_prefix() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/a", "user", "a".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 要求 prefix 外 cursor が入力不正になることを検証する
         */
        let err = service
            .list_pages(&auth, &manager, "/mcp", Some(10), Some("/other/a"))
            .expect_err("outside cursor must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "cursor must be under requested prefix");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_merges_targets_and_filters_by_prefix() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/search",
                "user",
                "# SearchTitle\n\nkeyword in body\n".to_string(),
            )
            .expect("create search page failed");
        manager
            .create_page(
                "/other/search",
                "user",
                "# Other\n\nkeyword outside jail\n".to_string(),
            )
            .expect("create other page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/search");
        rebuild_test_fts_page(&manager, &fts_config, "/other/search");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * path jail と prefix の後段フィルタを検証する
         */
        let result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "keyword",
                &[FtsSearchTarget::Headings, FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("search pages failed");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].path(), "/mcp/search");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `search_pages` が上位 N 件へ切り詰めることを確認する。
    ///
    /// # 注記
    /// スコア差のある 2 件を作成し、`limit=1` で上位 1 件のみ返ることを
    /// 確認する。
    ///
    #[test]
    fn search_pages_applies_top_n_limit() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/high",
                "user",
                "# keyword keyword\n\nkeyword in body\nkeyword again\n"
                    .to_string(),
            )
            .expect("create high score page failed");
        manager
            .create_page(
                "/mcp/low",
                "user",
                "# keyword\n\nsingle hit\n".to_string(),
            )
            .expect("create low score page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/high");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/low");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * top-N 制約で上位 1 件だけ返ることを検証する
         */
        let result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "keyword",
                &[FtsSearchTarget::Headings, FtsSearchTarget::Body],
                Some("/mcp"),
                Some(1),
            )
            .expect("search pages failed");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].path(), "/mcp/high");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_rejects_empty_targets() {
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        let err = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "keyword",
                &[],
                Some("/mcp"),
                Some(10),
            )
            .expect_err("empty targets must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "target must not be empty");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_supports_front_matter_target() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/front-matter",
                "user",
                "---\ncustom_meta:\n  search_note: mcpfrontmattertoken\n---\nbody only\n"
                    .to_string(),
            )
            .expect("create front matter page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/front-matter");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * front_matter 指定でのみヒットすることを検証する
         */
        let front_matter_result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "mcpfrontmattertoken",
                &[FtsSearchTarget::FrontMatter],
                Some("/mcp"),
                Some(10),
            )
            .expect("front matter search pages failed");
        let body_result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "mcpfrontmattertoken",
                &[FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("body search pages failed");

        assert_eq!(front_matter_result.items().len(), 1);
        assert_eq!(front_matter_result.items()[0].path(), "/mcp/front-matter");
        assert!(body_result.items().is_empty());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_merges_front_matter_and_body_targets() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/merged-front-matter",
                "user",
                "---\ncustom_meta:\n  search_note: sharedmcpkeyword\n---\nbody only\n"
                    .to_string(),
            )
            .expect("create merged front matter page failed");
        manager
            .create_page(
                "/mcp/merged-body",
                "user",
                "body sharedmcpkeyword\n".to_string(),
            )
            .expect("create merged body page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/merged-front-matter");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/merged-body");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * front_matter と body の複合指定で双方のヒットを返すことを検証する
         */
        let result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "sharedmcpkeyword",
                &[FtsSearchTarget::FrontMatter, FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("merged search pages failed");

        assert_eq!(result.items().len(), 2);
        assert!(
            result
                .items()
                .iter()
                .any(|item| item.path() == "/mcp/merged-front-matter")
        );
        assert!(
            result
                .items()
                .iter()
                .any(|item| item.path() == "/mcp/merged-body")
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn get_page_section_supports_id_and_title_selector() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/section",
                "user",
                "# Root\n\nintro\n\n## Child\n\nchild text\n".to_string(),
            )
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * ID 指定と title 指定の両方を検証する
         */
        let by_id = service
            .get_page_section(
                &auth,
                &manager,
                "/mcp/section",
                SectionSelector::ById("s-001".to_string()),
                None,
            )
            .expect("get section by id failed");
        assert_eq!(by_id.section().id(), "s-001");
        assert!(by_id.content().contains("## Child"));

        let by_title = service
            .get_page_section(
                &auth,
                &manager,
                "/mcp/section",
                SectionSelector::ByTitle("Child".to_string()),
                None,
            )
            .expect("get section by title failed");
        assert_eq!(by_title.section().title(), "Child");
        assert_eq!(by_title.content().trim(), "child text");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_section_selector_rejects_ambiguous_title_as_invalid_input() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n\n## Child\n\nfirst\n\n## Child\n\nsecond\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");

        let error = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ByTitle("Child".to_string()),
            )
            .expect_err("ambiguous title must fail");

        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert!(
            error.message().contains("section title is ambiguous"),
            "unexpected error: {}",
            error.message(),
        );
    }

    #[test]
    fn replace_section_content_keeps_heading_and_replaces_body_range() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let target = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve section by id failed");

        let replaced = service.replace_section_content(
            source,
            &target,
            "replaced root body",
        );

        assert!(replaced.starts_with("# Root\n"));
        assert!(replaced.contains("replaced root body"));
        assert!(!replaced.contains("intro"));
        assert!(!replaced.contains("## Child"));
        assert!(replaced.contains("# Next"));
    }

    #[test]
    fn insert_section_content_supports_before_and_after_anchor() {
        let service = McpService::new();
        let source = "# Root\n\nroot body\n\n# Next\n\nnext body\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let root = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve root failed");

        let inserted_before = service.insert_section_content(
            source,
            &root,
            EditPageInsertSectionPlacement::Before,
            "# Inserted Before\n\nbefore body\n\n",
        );
        assert!(inserted_before.starts_with("# Inserted Before\n\nbefore body\n\n# Root"));

        let inserted_after = service.insert_section_content(
            source,
            &root,
            EditPageInsertSectionPlacement::After,
            "# Inserted After\n\nafter body\n\n",
        );
        assert!(inserted_after.contains(
            "# Root\n\nroot body\n\n# Inserted After\n\nafter body\n\n# Next"
        ));
    }

    #[test]
    fn delete_section_content_removes_only_target_section_range() {
        let service = McpService::new();
        let source =
            "# Root\n\nroot body\n\n## Child\n\nchild body\n\n# Next\n\nnext body\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let target = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve section failed");

        let deleted = service.delete_section_content(source, &target);

        assert!(!deleted.contains("# Root"));
        assert!(!deleted.contains("root body"));
        assert!(!deleted.contains("## Child"));
        assert!(!deleted.contains("child body"));
        assert!(deleted.contains("# Next"));
        assert!(deleted.contains("next body"));
    }

    #[test]
    fn replace_text_content_supports_default_first_and_all() {
        let service = McpService::new();
        let source = "alpha beta alpha beta";

        let replaced_default = service.replace_text_content(
            source,
            "alpha",
            "gamma",
            None,
        );
        assert_eq!(replaced_default, "gamma beta alpha beta");

        let replaced_first = service.replace_text_content(
            source,
            "alpha",
            "gamma",
            Some(EditPageReplaceTextOccurrence::First),
        );
        assert_eq!(replaced_first, "gamma beta alpha beta");

        let replaced_all = service.replace_text_content(
            source,
            "alpha",
            "gamma",
            Some(EditPageReplaceTextOccurrence::All),
        );
        assert_eq!(replaced_all, "gamma beta gamma beta");
    }

    #[test]
    fn insert_section_content_keeps_inserted_section_heading() {
        let service = McpService::new();
        let source = "# Root\n\nroot body\n\n# Next\n\nnext body\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let anchor = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve anchor failed");

        let inserted = service.insert_section_content(
            source,
            &anchor,
            EditPageInsertSectionPlacement::After,
            "## Inserted\n\ninserted body\n",
        );

        assert!(inserted.contains("## Inserted\n\ninserted body\n"));
        assert!(inserted.contains("# Root\n\nroot body\n\n## Inserted"));
        assert!(inserted.contains("inserted body\n# Next"));
    }

    #[test]
    fn apply_edit_page_operation_classifies_section_operation_failures() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n";

        let missing_replace_section = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-999".to_string()),
                    content: "updated".to_string(),
                },
            )
            .expect_err("missing replace target should fail");
        assert_eq!(missing_replace_section.code(), McpErrorCode::NotFound);

        let missing_insert_anchor = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::InsertSection {
                    anchor: SectionSelector::ById("s-999".to_string()),
                    placement: EditPageInsertSectionPlacement::After,
                    content: "# Inserted\n\nbody\n".to_string(),
                },
            )
            .expect_err("missing insert anchor should fail");
        assert_eq!(missing_insert_anchor.code(), McpErrorCode::NotFound);

        let missing_delete_section = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ById("s-999".to_string()),
                },
            )
            .expect_err("missing delete target should fail");
        assert_eq!(missing_delete_section.code(), McpErrorCode::NotFound);
    }

    #[test]
    fn apply_edit_page_operation_classifies_replace_text_failures() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n";

        let empty_old_text = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceText {
                    old_text: "".to_string(),
                    new_text: "x".to_string(),
                    occurrence: None,
                },
            )
            .expect_err("empty old_text should fail");
        assert_eq!(empty_old_text.code(), McpErrorCode::InvalidInput);

        let missing_text = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceText {
                    old_text: "missing".to_string(),
                    new_text: "x".to_string(),
                    occurrence: None,
                },
            )
            .expect_err("missing text should fail");
        assert_eq!(missing_text.code(), McpErrorCode::NotFound);
    }

    #[test]
    fn create_page_creates_page_with_create_scope() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Create]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * create の保存結果を検証する
         */
        let result = service
            .create_page(&auth, &manager, "/mcp/create", "# created")
            .expect("create page failed");
        let page_id = manager
            .get_page_id_by_path("/mcp/create")
            .expect("lookup page id failed")
            .expect("page id missing");
        let source = manager
            .get_page_source(&page_id, 1)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/create");
        assert_eq!(result.revision(), 1);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("created instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page created");
        assert_eq!(source.source(), "# created");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn create_page_rejects_root_path() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Create]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );

        /*
         * root path 作成禁止を検証する
         */
        let err = service
            .create_page(&auth, &manager, "/", "# root")
            .expect_err("root create must fail");

        assert_eq!(err.code(), McpErrorCode::Forbidden);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn update_page_overwrites_content_and_advances_revision() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/update", "user", "# before".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * update の保存結果を検証する
         */
        let result = service
            .update_page(&auth, &manager, "/mcp/update", "# after")
            .expect("update page failed");
        let source = manager
            .get_page_source(&page_id, 2)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/update");
        assert_eq!(result.revision(), 2);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("updated instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page updated");
        assert_eq!(source.source(), "# after");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn rename_page_moves_page_and_returns_new_path() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/rename", "user", "# page".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * rename の保存結果を検証する
         */
        let result = service
            .rename_page(&auth, &manager, "/mcp/rename", "/mcp/renamed")
            .expect("rename page failed");
        let new_page_id = manager
            .get_page_id_by_path("/mcp/renamed")
            .expect("lookup renamed page id failed")
            .expect("renamed page id missing");

        assert_eq!(result.path(), "/mcp/renamed");
        assert_eq!(result.revision(), 2);
        assert_eq!(
            result.instance_id(),
            &manager
                .get_page_source(&new_page_id, 2)
                .expect("lookup renamed source failed")
                .expect("renamed source missing")
                .instance_id()
                .expect("renamed instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page renamed from /mcp/rename");
        assert_eq!(new_page_id, page_id);
        assert!(manager
            .get_page_id_by_path("/mcp/rename")
            .expect("lookup old path failed")
            .is_none());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn rename_page_rejects_descendant_destination() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/tree", "user", "# page".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 子孫 path への移動禁止を検証する
         */
        let err = service
            .rename_page(&auth, &manager, "/mcp/tree", "/mcp/tree/child")
            .expect_err("descendant rename must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn append_page_adds_new_revision_for_different_latest_user() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .add_user("bob", "pass", None)
            .expect("add bob failed");
        let page_id = manager
            .create_page("/mcp/append", "alice", "# base".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("bob".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 別ユーザ追記時は新規 revision 追加になることを検証する
         */
        let result = service
            .append_page(&auth, &manager, "/mcp/append", "\nnext")
            .expect("append page failed");
        let source = manager
            .get_page_source(&page_id, 2)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/append");
        assert_eq!(result.revision(), 2);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("appended instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page appended");
        assert!(!result.amended());
        assert_eq!(source.source(), "# base\nnext");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn append_page_amends_when_latest_user_matches() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        let page_id = manager
            .create_page("/mcp/amend", "alice", "# base".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 同一ユーザ追記時は amend 相当になることを検証する
         */
        let result = service
            .append_page(&auth, &manager, "/mcp/amend", "\namended")
            .expect("append page failed");
        let source = manager
            .get_page_source(&page_id, 1)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/amend");
        assert_eq!(result.revision(), 1);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("amended instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page appended (amended)");
        assert!(result.amended());
        assert_eq!(source.source(), "# base\namended");
        assert!(
            !manager
                .has_page_source_for_test(&page_id, 2)
                .expect("revision 2 lookup failed")
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `append_page` がロック解放を待機してから成功できることを確認する。
    ///
    /// # 注記
    /// 先にページロックを取得し、別スレッドで短時間後に解除して
    /// 待機後成功することを検証する。
    ///
    #[test]
    fn append_page_waits_for_lock_release_then_succeeds() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .add_user("bob", "pass", None)
            .expect("add bob failed");
        let page_id = manager
            .create_page("/mcp/wait", "alice", "# base".to_string())
            .expect("create page failed");
        let lock_info = manager
            .acquire_page_lock(&page_id, "alice")
            .expect("acquire lock failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("bob".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * ロック解放待ち後に追記成功することを検証する
         */
        let started_at = Instant::now();
        std::thread::scope(|scope| {
            let release_page_id = page_id.clone();
            let release_token = lock_info.token();
            let manager_ref = &manager;
            scope.spawn(move || {
                thread::sleep(Duration::from_millis(200));
                manager_ref
                    .release_page_lock(
                        &release_page_id,
                        "alice",
                        &release_token,
                    )
                    .expect("release lock failed");
            });

            let result = service
                .append_page(&auth, &manager, "/mcp/wait", "\nnext")
                .expect("append page failed");
            let source = manager
                .get_page_source(&page_id, 2)
                .expect("lookup source failed")
                .expect("source missing");

            assert_eq!(result.path(), "/mcp/wait");
            assert_eq!(result.revision(), 2);
            assert_eq!(result.summary(), "page appended");
            assert!(!result.amended());
            assert_eq!(source.source(), "# base\nnext");
        });
        assert!(
            started_at.elapsed()
                >= Duration::from_millis(APPEND_WAIT_INTERVAL_MS)
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `append_page` がロック待機 timeout 時に conflict を返すことを
    /// 確認する。
    ///
    /// # 注記
    /// ロックを保持したまま待機上限を超過させ、部分保存が無いことも
    /// 併せて確認する。
    ///
    #[test]
    fn append_page_returns_conflict_after_lock_wait_timeout() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .add_user("bob", "pass", None)
            .expect("add bob failed");
        let page_id = manager
            .create_page("/mcp/timeout", "alice", "# base".to_string())
            .expect("create page failed");
        let _lock_info = manager
            .acquire_page_lock(&page_id, "alice")
            .expect("acquire lock failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("bob".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * timeout 時に conflict になり保存されないことを検証する
         */
        let started_at = Instant::now();
        let err = service
            .append_page(&auth, &manager, "/mcp/timeout", "\nnext")
            .expect_err("append must timeout");

        assert_eq!(err.code(), McpErrorCode::Conflict);
        assert_eq!(err.message(), "page is locked");
        assert!(
            started_at.elapsed()
                >= Duration::from_millis(APPEND_WAIT_TIMEOUT_MS)
        );
        assert!(
            !manager
                .has_page_source_for_test(&page_id, 2)
                .expect("revision 2 lookup failed")
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn append_page_rejects_empty_content() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .create_page("/mcp/empty", "alice", "# base".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 空文字追記は禁止されることを検証する
         */
        let err = service
            .append_page(&auth, &manager, "/mcp/empty", "")
            .expect_err("empty append must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn unsupported_operations_return_unsupported_code() {
        let service = McpService::new();

        let deleted = service
            .get_deleted_page("/mcp/deleted")
            .expect_err("deleted page request must fail");
        let restore = service
            .restore_page("/mcp/deleted", "/mcp/restored")
            .expect_err("restore request must fail");
        let asset = service
            .asset_operation("/mcp/page")
            .expect_err("asset operation must fail");
        let lock = service
            .lock_operation("/mcp/page")
            .expect_err("lock operation must fail");

        assert_eq!(deleted.code(), McpErrorCode::Unsupported);
        assert_eq!(restore.code(), McpErrorCode::Unsupported);
        assert_eq!(asset.code(), McpErrorCode::Unsupported);
        assert_eq!(lock.code(), McpErrorCode::Unsupported);
    }
}
