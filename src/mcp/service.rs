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

use crate::auth::AuthContext;
use crate::database::{
    AppendPageRequest,
    AppendPageResult,
    DatabaseManager,
    DbError,
    PageListEntry,
};
use crate::database::types::{
    BearerScope,
    PageId,
    PageIndex,
    UserAttribute,
    UserId,
};
use crate::fts::{self, FtsIndexConfig, FtsSearchTarget};

use super::errors::{McpError, McpErrorCode};

/// path で禁止する文字
const FORBIDDEN_PATH_CHARS: &[char] = &['\\'];

/// `list_pages` の既定件数
const DEFAULT_LIST_LIMIT: usize = 50;

/// `search_pages` の既定件数
const DEFAULT_SEARCH_LIMIT: usize = 20;

/// `list_pages` / `search_pages` の上限件数
const MAX_PAGE_RESULT_LIMIT: usize = 100;

/// `append` 競合待機の上限時間 (ミリ秒)
const APPEND_WAIT_TIMEOUT_MS: u64 = 1_000;

/// `append` 競合待機のポーリング間隔 (ミリ秒)
const APPEND_WAIT_INTERVAL_MS: u64 = 50;

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

    /// 検索
    SearchPages,

    /// セクション参照
    GetPageSection,

    /// ページ作成
    CreatePage,

    /// ページ更新
    UpdatePage,

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

    /// セクション本文開始位置
    content_start: usize,

    /// セクション本文終了位置
    content_end: usize,
}

///
/// MCPサービス層
///
#[derive(Clone, Debug, Default)]
pub(crate) struct McpService;

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
            | Self::SearchPages
            | Self::GetPageSection => BearerScope::Read,
            Self::CreatePage => BearerScope::Create,
            Self::UpdatePage | Self::RenamePage => BearerScope::Update,
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
    /// * `content` - Markdown 本文
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(path: String, revision: u64, content: String) -> Self {
        Self {
            path,
            revision,
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
    /// * `sections` - 見出し一覧
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(path: String, revision: u64, sections: Vec<TocSection>) -> Self {
        Self {
            path,
            revision,
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
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(path: String, revision: u64, summary: String) -> Self {
        Self {
            path,
            revision,
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
    /// * `summary` - 実行結果要約
    /// * `amended` - amend 相当保存有無
    ///
    /// # 戻り値
    /// 生成した結果を返す。
    ///
    fn new(
        path: String,
        revision: u64,
        summary: String,
        amended: bool,
    ) -> Self {
        Self {
            path,
            revision,
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
        Self
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
        let (revision, source) =
            self.resolve_revision_source(db, &resolved, revision)?;

        Ok(GetPageResult::new(
            resolved.normalized_path().to_string(),
            revision,
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
        let (revision, source) =
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
        let resolved_prefix =
            self.resolve_search_prefix_request(auth, raw_prefix)?;
        let limit = self.resolve_limit(limit, DEFAULT_SEARCH_LIMIT)?;

        /*
         * FTS の実行とスコアマージ
         */
        let mut merged = HashMap::new();
        for target in [
            FtsSearchTarget::Headings,
            FtsSearchTarget::Body,
            FtsSearchTarget::Code,
        ] {
            let results = fts::search_index(
                fts_config,
                target,
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
        let (revision, source) =
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

        Ok(WritePageResult::new(
            normalized_path,
            1,
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
        self.ensure_page_not_locked(db, &resolved.page_id())?;
        db.put_page(
            &resolved.page_id(),
            auth.user().user_id(),
            content.to_string(),
            false,
        )
        .map_err(map_update_db_error)?;

        Ok(WritePageResult::new(
            resolved.normalized_path().to_string(),
            resolved.latest_revision().unwrap_or(0) + 1,
            "page updated".to_string(),
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
            return Ok(WritePageResult::new(
                normalized_rename_to,
                resolved.latest_revision().unwrap_or(0),
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
        db.rename_pages_recursive_by_id(
            &resolved.page_id(),
            &normalized_rename_to,
        )
        .map_err(map_rename_db_error)?;

        Ok(WritePageResult::new(
            normalized_rename_to,
            resolved.latest_revision().unwrap_or(0) + 1,
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
                        resolved.normalized_path(),
                        result,
                    ));
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
    /// `(revision, source)` を返す。
    ///
    fn resolve_revision_source(
        &self,
        db: &DatabaseManager,
        resolved: &ResolvedPage,
        revision: Option<u64>,
    ) -> Result<(u64, String), McpError> {
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

        Ok((revision, source.source()))
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
        path: &str,
        result: AppendPageResult,
    ) -> AppendServiceResult {
        let summary = if result.amended() {
            "page appended (amended)"
        } else {
            "page appended"
        };

        AppendServiceResult::new(
            path.to_string(),
            result.revision(),
            summary.to_string(),
            result.amended(),
        )
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
    /// Markdown 本文から TOC 用 section 一覧を構築する
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
    /// section selector を解決する
    ///
    /// # 引数
    /// * `sections` - 解析済み section 一覧
    /// * `selector` - section selector
    ///
    /// # 戻り値
    /// 解決した section を返す。
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
                Some("/mcp"),
                Some(1),
            )
            .expect("search pages failed");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].path(), "/mcp/high");

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
