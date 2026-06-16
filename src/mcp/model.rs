/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP公開面の要求・応答モデルを定義するモジュール
//!

use serde::Serialize;

use crate::fts::FtsSearchTarget;

use super::service::{
    AppendServiceResult,
    EditPageRequest as ServiceEditPageRequest,
    EditPageResult,
    EditPageInsertSectionPlacement as ServiceEditPageInsertSectionPlacement,
    EditPageOperation as ServiceEditPageOperation,
    EditPageReplaceTextOccurrence as ServiceEditPageReplaceTextOccurrence,
    GetPageResult,
    GetPageSectionResult,
    GetPageTocResult,
    ListPagesResult,
    SearchPagesResult,
    SectionSelector,
    TocSection,
    WritePageResult,
};
use super::tools::{
    EditPageInsertSectionPlacement,
    EditPageReplaceTextOccurrence,
    EditPageSectionSelector,
    EditPageSectionSelectorBy,
    EditPageSectionSelectorObject,
    EditPageToolOperation,
    McpToolName,
};

///
/// MCP要求の共通エンベロープ
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct McpRequestEnvelope {
    /// 呼び出し対象ツール名
    tool_name: McpToolName,

    /// ツール別入力
    request: McpToolRequest,
}

///
/// MCP応答の共通エンベロープ
///
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct McpResponseEnvelope {
    /// 呼び出し対象ツール名
    tool_name: McpToolName,

    /// ツール別出力
    response: McpToolResponse,
}

///
/// MCPのツール別入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum McpToolRequest {
    /// `get_page` 入力
    GetPage(GetPageRequest),

    /// `get_page_toc` 入力
    GetPageToc(GetPageTocRequest),

    /// `list_pages` 入力
    ListPages(ListPagesRequest),

    /// `search_pages` 入力
    SearchPages(SearchPagesRequest),

    /// `create_page` 入力
    CreatePage(WritePageRequest),

    /// `update_page` 入力
    UpdatePage(WritePageRequest),

    /// `edit_page` 入力
    EditPage(EditPageRequest),

    /// `append_page` 入力
    AppendPage(WritePageRequest),

    /// `rename_page` 入力
    RenamePage(RenamePageRequest),

    /// `get_page_section` 入力
    GetPageSection(GetPageSectionRequest),
}

///
/// MCPのツール別出力
///
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) enum McpToolResponse {
    /// `get_page` 出力
    GetPage(GetPageResponse),

    /// `get_page_toc` 出力
    GetPageToc(GetPageTocResponse),

    /// `list_pages` 出力
    ListPages(ListPagesResponse),

    /// `search_pages` 出力
    SearchPages(SearchPagesResponse),

    /// `create_page` 出力
    CreatePage(WritePageResponse),

    /// `update_page` 出力
    UpdatePage(WritePageResponse),

    /// `edit_page` 出力
    EditPage(EditPageResponse),

    /// `append_page` 出力
    AppendPage(AppendPageResponse),

    /// `rename_page` 出力
    RenamePage(WritePageResponse),

    /// `get_page_section` 出力
    GetPageSection(GetPageSectionResponse),
}

///
/// `get_page` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageRequest {
    /// 対象ページの絶対 path
    path: String,

    /// 対象 revision
    revision: Option<u64>,
}

///
/// `get_page_toc` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageTocRequest {
    /// 対象ページの絶対 path
    path: String,

    /// 対象 revision
    revision: Option<u64>,
}

///
/// `list_pages` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListPagesRequest {
    /// 一覧対象 prefix
    prefix: String,

    /// 最大取得件数
    limit: Option<usize>,

    /// 継続取得 cursor
    cursor: Option<String>,
}

///
/// MCP prompt一覧用の引数情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PromptListArgument {
    /// 引数名
    name: String,

    /// 引数説明
    description: String,

    /// 必須可否
    required: Option<bool>,
}

impl PromptListArgument {
    ///
    /// prompt一覧用引数情報を生成する
    ///
    /// # 引数
    /// * `name` - 引数名
    /// * `description` - 引数説明
    /// * `required` - 必須可否
    ///
    /// # 戻り値
    /// prompt一覧用引数情報を返す。
    ///
    pub(crate) fn new(
        name: String,
        description: String,
        required: Option<bool>,
    ) -> Self {
        Self {
            name,
            description,
            required,
        }
    }

    ///
    /// 引数名を返す
    ///
    /// # 戻り値
    /// 引数名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// 引数説明を返す
    ///
    /// # 戻り値
    /// 引数説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// 必須可否を返す
    ///
    /// # 戻り値
    /// 必須可否を返す。
    ///
    pub(crate) fn required(&self) -> Option<bool> {
        self.required
    }
}

///
/// MCP prompt一覧項目
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PromptListItem {
    /// prompt名
    name: String,

    /// prompt説明
    description: String,

    /// prompt引数
    arguments: Vec<PromptListArgument>,
}

impl PromptListItem {
    ///
    /// prompt一覧項目を生成する
    ///
    /// # 引数
    /// * `name` - prompt名
    /// * `description` - prompt説明
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// prompt一覧項目を返す。
    ///
    pub(crate) fn new(
        name: String,
        description: String,
        arguments: Vec<PromptListArgument>,
    ) -> Self {
        Self {
            name,
            description,
            arguments,
        }
    }

    ///
    /// prompt名を返す
    ///
    /// # 戻り値
    /// prompt名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// prompt説明を返す
    ///
    /// # 戻り値
    /// prompt説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// prompt引数を返す
    ///
    /// # 戻り値
    /// 定義順のprompt引数を返す。
    ///
    pub(crate) fn arguments(&self) -> &[PromptListArgument] {
        &self.arguments
    }
}

///
/// MCP prompt一覧サービス結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListPromptsServiceResult {
    /// prompt一覧
    items: Vec<PromptListItem>,

    /// 次回cursor
    next_cursor: Option<String>,
}

impl ListPromptsServiceResult {
    ///
    /// prompt一覧サービス結果を生成する
    ///
    /// # 引数
    /// * `items` - prompt一覧
    /// * `next_cursor` - 次回cursor
    ///
    /// # 戻り値
    /// prompt一覧サービス結果を返す。
    ///
    pub(crate) fn new(
        items: Vec<PromptListItem>,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            items,
            next_cursor,
        }
    }

    ///
    /// prompt一覧を返す
    ///
    /// # 戻り値
    /// prompt一覧を返す。
    ///
    pub(crate) fn items(&self) -> &[PromptListItem] {
        &self.items
    }

    ///
    /// 次回cursorを返す
    ///
    /// # 戻り値
    /// 次回cursorが存在する場合はその値を返す。
    ///
    pub(crate) fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

///
/// MCP resource一覧項目
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceListItem {
    /// resource URI
    uri: String,

    /// resource 名
    name: String,

    /// resource 説明
    description: String,

    /// MIME type
    mime_type: String,
}

impl ResourceListItem {
    ///
    /// resource一覧項目を生成する
    ///
    /// # 引数
    /// * `uri` - resource URI
    /// * `name` - resource名
    /// * `description` - resource説明
    /// * `mime_type` - MIME type
    ///
    /// # 戻り値
    /// resource一覧項目を返す。
    ///
    pub(crate) fn new(
        uri: String,
        name: String,
        description: String,
        mime_type: String,
    ) -> Self {
        Self {
            uri,
            name,
            description,
            mime_type,
        }
    }

    ///
    /// resource URIを返す
    ///
    /// # 戻り値
    /// resource URIを返す。
    ///
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    ///
    /// resource名を返す
    ///
    /// # 戻り値
    /// resource名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// resource説明を返す
    ///
    /// # 戻り値
    /// resource説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
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
}

///
/// MCP resource一覧サービス結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListResourcesServiceResult {
    /// resource一覧
    items: Vec<ResourceListItem>,

    /// 次回cursor
    next_cursor: Option<String>,
}

impl ListResourcesServiceResult {
    ///
    /// resource一覧サービス結果を生成する
    ///
    /// # 引数
    /// * `items` - resource一覧
    /// * `next_cursor` - 次回cursor
    ///
    /// # 戻り値
    /// resource一覧サービス結果を返す。
    ///
    pub(crate) fn new(
        items: Vec<ResourceListItem>,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            items,
            next_cursor,
        }
    }

    ///
    /// resource一覧を返す
    ///
    /// # 戻り値
    /// resource一覧を返す。
    ///
    pub(crate) fn items(&self) -> &[ResourceListItem] {
        &self.items
    }

    ///
    /// 次回cursorを返す
    ///
    /// # 戻り値
    /// 次回cursorが存在する場合はその値を返す。
    ///
    pub(crate) fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

///
/// MCP resource取得サービス結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReadResourceServiceResult {
    /// resource URI
    uri: String,

    /// MIME type
    mime_type: String,

    /// resource本文
    text: String,

    /// 取得した最新revision
    revision: Option<u64>,
}

impl ReadResourceServiceResult {
    ///
    /// resource取得サービス結果を生成する
    ///
    /// # 引数
    /// * `uri` - resource URI
    /// * `mime_type` - MIME type
    /// * `text` - resource本文
    /// * `revision` - 取得した最新revision
    ///
    /// # 戻り値
    /// resource取得サービス結果を返す。
    ///
    pub(crate) fn new(
        uri: String,
        mime_type: String,
        text: String,
        revision: Option<u64>,
    ) -> Self {
        Self {
            uri,
            mime_type,
            text,
            revision,
        }
    }

    ///
    /// resource URIを返す
    ///
    /// # 戻り値
    /// resource URIを返す。
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
    /// resource本文を返す
    ///
    /// # 戻り値
    /// resource本文を返す。
    ///
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    ///
    /// 取得した最新revisionを返す
    ///
    /// # 戻り値
    /// ページ由来resourceの場合は最新revisionを返す。
    ///
    pub(crate) fn revision(&self) -> Option<u64> {
        self.revision
    }
}

///
/// MCP prompt取得サービス結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPromptServiceResult {
    /// prompt説明
    description: String,

    /// 展開済みmessage本文
    message: String,

    /// 取得した最新revision
    revision: u64,
}

impl GetPromptServiceResult {
    ///
    /// prompt取得サービス結果を生成する
    ///
    /// # 引数
    /// * `description` - prompt説明
    /// * `message` - 展開済みmessage本文
    /// * `revision` - 取得した最新revision
    ///
    /// # 戻り値
    /// prompt取得サービス結果を返す。
    ///
    pub(crate) fn new(
        description: String,
        message: String,
        revision: u64,
    ) -> Self {
        Self {
            description,
            message,
            revision,
        }
    }

    ///
    /// prompt説明を返す
    ///
    /// # 戻り値
    /// prompt説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// 展開済みmessage本文を返す
    ///
    /// # 戻り値
    /// 展開済みmessage本文を返す。
    ///
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    ///
    /// 取得した最新revisionを返す
    ///
    /// # 戻り値
    /// 取得した最新revisionを返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}

///
/// `search_pages` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SearchPagesRequest {
    /// 全文検索式
    query: String,

    /// 検索対象
    targets: Vec<FtsSearchTarget>,

    /// 検索対象 prefix
    prefix: Option<String>,

    /// 最大取得件数
    limit: Option<usize>,
}

///
/// `create_page` / `update_page` / `append_page` の共通入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WritePageRequest {
    /// 対象ページの絶対 path
    path: String,

    /// 本文または追記内容
    content: String,
}

///
/// `rename_page` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenamePageRequest {
    /// 移動元 path
    path: String,

    /// 移動先 path
    rename_to: String,
}

///
/// `edit_page` の公開 selector
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum McpEditPageSectionSelector {
    /// 見出し文字列そのものを指定する省略形
    Text(String),

    /// section ID 指定
    ById(String),

    /// 見出し文字列指定
    ByTitle(String),
}

///
/// `insert_section` の挿入位置
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum McpEditPageInsertSectionPlacement {
    /// anchor の直前
    Before,

    /// anchor の直後
    After,
}

///
/// `replace_text` の一致対象
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum McpEditPageReplaceTextOccurrence {
    /// 先頭一致のみ
    First,

    /// 全一致
    All,
}

///
/// `edit_page` の公開 operation
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum McpEditPageOperation {
    /// セクション本文の置換
    ReplaceSection {
        /// 対象セクション
        section: McpEditPageSectionSelector,

        /// 置換後本文
        content: String,
    },

    /// セクション挿入
    InsertSection {
        /// 挿入位置基準セクション
        anchor: McpEditPageSectionSelector,

        /// 挿入位置
        placement: McpEditPageInsertSectionPlacement,

        /// 挿入する完全なセクション本文
        content: String,
    },

    /// セクション削除
    DeleteSection {
        /// 削除対象セクション
        section: McpEditPageSectionSelector,
    },

    /// テキスト置換
    ReplaceText {
        /// 置換前文字列
        old_text: String,

        /// 置換後文字列
        new_text: String,

        /// 複数一致時の対象指定
        occurrence: Option<McpEditPageReplaceTextOccurrence>,
    },
}

///
/// `edit_page` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EditPageRequest {
    /// 対象ページの絶対 path
    path: String,

    /// 対象 revision
    revision: u64,

    /// ページ内容の一意性を表すインスタンスID
    instance_id: String,

    /// 単一の編集操作
    operation: McpEditPageOperation,
}

///
/// `get_page_section` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageSectionRequest {
    /// 対象ページの絶対 path
    path: String,

    /// セクション指定
    section: McpSectionSelector,

    /// 対象 revision
    revision: Option<u64>,
}

///
/// `get_page_section` の公開 selector
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum McpSectionSelector {
    /// section ID 指定
    ById(String),

    /// 見出し文字列指定
    ByTitle(String),
}

///
/// `get_page` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GetPageResponse {
    /// current path
    path: String,

    /// 対応 revision
    revision: u64,

    /// 対応 instance_id
    instance_id: String,

    /// Markdown 本文全体
    content: String,
}

///
/// `get_page_toc` / `get_page_section` の公開 section モデル
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct McpSectionInfo {
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

    /// セクション本文文字数
    section_chars: usize,
}

///
/// `get_page_toc` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GetPageTocResponse {
    /// current path
    path: String,

    /// 対応 revision
    revision: u64,

    /// 対応 instance_id
    instance_id: String,

    /// 見出し一覧
    sections: Vec<McpSectionInfo>,
}

///
/// `list_pages` 一覧項目
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct McpPageListItem {
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
/// `list_pages` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ListPagesResponse {
    /// 一覧項目
    items: Vec<McpPageListItem>,

    /// 続き有無
    has_more: bool,

    /// 次回 cursor
    next_cursor: Option<String>,
}

///
/// `search_pages` 一覧項目
///
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct McpSearchPageItem {
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
/// `search_pages` 出力
///
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SearchPagesResponse {
    /// 検索結果一覧
    items: Vec<McpSearchPageItem>,
}

///
/// `create_page` / `update_page` / `rename_page` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct WritePageResponse {
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
/// `edit_page` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct EditPageResponse {
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
/// `append_page` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppendPageResponse {
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
/// `get_page_section` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GetPageSectionResponse {
    /// current path
    path: String,

    /// 対応 revision
    revision: u64,

    /// 解決後 section
    section: McpSectionInfo,

    /// セクション本文
    content: String,
}

impl McpRequestEnvelope {
    ///
    /// MCP要求エンベロープを生成する
    ///
    /// # 引数
    /// * `tool_name` - 呼び出し対象ツール名
    /// * `request` - ツール別入力
    ///
    /// # 戻り値
    /// 生成した要求エンベロープを返す。
    ///
    pub(crate) fn new(tool_name: McpToolName, request: McpToolRequest) -> Self {
        Self { tool_name, request }
    }

    ///
    /// ツール名を返す
    ///
    /// # 戻り値
    /// 呼び出し対象ツール名を返す。
    ///
    pub(crate) fn tool_name(&self) -> McpToolName {
        self.tool_name
    }

    ///
    /// ツール別入力を返す
    ///
    /// # 戻り値
    /// ツール別入力を返す。
    ///
    pub(crate) fn request(&self) -> &McpToolRequest {
        &self.request
    }
}

impl McpResponseEnvelope {
    ///
    /// MCP応答エンベロープを生成する
    ///
    /// # 引数
    /// * `tool_name` - 呼び出し対象ツール名
    /// * `response` - ツール別出力
    ///
    /// # 戻り値
    /// 生成した応答エンベロープを返す。
    ///
    pub(crate) fn new(
        tool_name: McpToolName,
        response: McpToolResponse,
    ) -> Self {
        Self { tool_name, response }
    }

    ///
    /// ツール名を返す
    ///
    /// # 戻り値
    /// 呼び出し対象ツール名を返す。
    ///
    pub(crate) fn tool_name(&self) -> McpToolName {
        self.tool_name
    }

    ///
    /// ツール別出力を返す
    ///
    /// # 戻り値
    /// ツール別出力を返す。
    ///
    pub(crate) fn response(&self) -> &McpToolResponse {
        &self.response
    }
}

impl GetPageRequest {
    ///
    /// `get_page` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(path: String, revision: Option<u64>) -> Self {
        Self { path, revision }
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
    /// 指定されている場合は対象 revision を返す。
    ///
    pub(crate) fn revision(&self) -> Option<u64> {
        self.revision
    }
}

impl GetPageTocRequest {
    ///
    /// `get_page_toc` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(path: String, revision: Option<u64>) -> Self {
        Self { path, revision }
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
    /// 指定されている場合は対象 revision を返す。
    ///
    pub(crate) fn revision(&self) -> Option<u64> {
        self.revision
    }
}

impl ListPagesRequest {
    ///
    /// `list_pages` 入力を生成する
    ///
    /// # 引数
    /// * `prefix` - 一覧対象 prefix
    /// * `limit` - 最大取得件数
    /// * `cursor` - 継続取得 cursor
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(
        prefix: String,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Self {
        Self {
            prefix,
            limit,
            cursor,
        }
    }

    ///
    /// 一覧対象 prefix を返す
    ///
    /// # 戻り値
    /// 一覧対象の絶対 path prefix を返す。
    ///
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    ///
    /// 最大取得件数を返す
    ///
    /// # 戻り値
    /// 指定されている場合は最大取得件数を返す。
    ///
    pub(crate) fn limit(&self) -> Option<usize> {
        self.limit
    }

    ///
    /// 継続取得 cursor を返す
    ///
    /// # 戻り値
    /// 指定されている場合は継続取得 cursor を返す。
    ///
    pub(crate) fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }
}

impl SearchPagesRequest {
    ///
    /// `search_pages` 入力を生成する
    ///
    /// # 引数
    /// * `query` - 全文検索式
    /// * `prefix` - 検索対象 prefix
    /// * `limit` - 最大取得件数
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(
        query: String,
        targets: Vec<FtsSearchTarget>,
        prefix: Option<String>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            query,
            targets,
            prefix,
            limit,
        }
    }

    ///
    /// 全文検索式を返す
    ///
    /// # 戻り値
    /// 全文検索式を返す。
    ///
    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    ///
    /// 検索対象一覧を返す
    ///
    /// # 戻り値
    /// 検索対象一覧を返す。
    ///
    pub(crate) fn targets(&self) -> &[FtsSearchTarget] {
        &self.targets
    }

    ///
    /// 検索対象 prefix を返す
    ///
    /// # 戻り値
    /// 指定されている場合は検索対象 prefix を返す。
    ///
    pub(crate) fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    ///
    /// 最大取得件数を返す
    ///
    /// # 戻り値
    /// 指定されている場合は最大取得件数を返す。
    ///
    pub(crate) fn limit(&self) -> Option<usize> {
        self.limit
    }
}

impl WritePageRequest {
    ///
    /// 更新系入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `content` - 本文または追記内容
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(path: String, content: String) -> Self {
        Self { path, content }
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
    /// 本文または追記内容を返す
    ///
    /// # 戻り値
    /// 本文または追記内容を返す。
    ///
    pub(crate) fn content(&self) -> &str {
        &self.content
    }
}

impl RenamePageRequest {
    ///
    /// `rename_page` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 移動元 path
    /// * `rename_to` - 移動先 path
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(path: String, rename_to: String) -> Self {
        Self { path, rename_to }
    }

    ///
    /// 移動元 path を返す
    ///
    /// # 戻り値
    /// 移動元 path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 移動先 path を返す
    ///
    /// # 戻り値
    /// 移動先 path を返す。
    ///
    pub(crate) fn rename_to(&self) -> &str {
        &self.rename_to
    }
}

impl EditPageRequest {
    ///
    /// `edit_page` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `revision` - 対象 revision
    /// * `instance_id` - 内容整合性確認用 instance_id
    /// * `operation` - 単一の編集操作
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(
        path: String,
        revision: u64,
        instance_id: String,
        operation: McpEditPageOperation,
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
    /// 入力 instance_id を返す
    ///
    /// # 戻り値
    /// 内容整合性確認用 instance_id を返す。
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
    pub(crate) fn operation(&self) -> &McpEditPageOperation {
        &self.operation
    }
}

impl GetPageSectionRequest {
    ///
    /// `get_page_section` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `section` - セクション指定
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(
        path: String,
        section: McpSectionSelector,
        revision: Option<u64>,
    ) -> Self {
        Self {
            path,
            section,
            revision,
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
    /// セクション指定を返す
    ///
    /// # 戻り値
    /// セクション指定を返す。
    ///
    pub(crate) fn section(&self) -> &McpSectionSelector {
        &self.section
    }

    ///
    /// 対象 revision を返す
    ///
    /// # 戻り値
    /// 指定されている場合は対象 revision を返す。
    ///
    pub(crate) fn revision(&self) -> Option<u64> {
        self.revision
    }
}

impl GetPageResponse {
    ///
    /// `get_page` 出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 対応 revision
    /// * `instance_id` - 対応 instance_id
    /// * `content` - Markdown 本文
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
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
}

impl McpSectionInfo {
    ///
    /// 公開 section モデルを生成する
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
    /// 生成した section モデルを返す。
    ///
    pub(crate) fn new(
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
}

impl GetPageTocResponse {
    ///
    /// `get_page_toc` 出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 対応 revision
    /// * `instance_id` - 対応 instance_id
    /// * `sections` - 見出し一覧
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
        path: String,
        revision: u64,
        instance_id: String,
        sections: Vec<McpSectionInfo>,
    ) -> Self {
        Self {
            path,
            revision,
            instance_id,
            sections,
        }
    }
}

impl ListPagesResponse {
    ///
    /// `list_pages` 出力を生成する
    ///
    /// # 引数
    /// * `items` - 一覧項目
    /// * `has_more` - 続き有無
    /// * `next_cursor` - 次回 cursor
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
        items: Vec<McpPageListItem>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            items,
            has_more,
            next_cursor,
        }
    }
}

impl SearchPagesResponse {
    ///
    /// `search_pages` 出力を生成する
    ///
    /// # 引数
    /// * `items` - 検索結果一覧
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(items: Vec<McpSearchPageItem>) -> Self {
        Self { items }
    }
}

impl WritePageResponse {
    ///
    /// 更新系出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 更新後 revision
    /// * `instance_id` - 更新後 instance_id
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
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
}

impl EditPageResponse {
    ///
    /// `edit_page` 出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 更新後 revision
    /// * `instance_id` - 更新後 instance_id
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
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
}

impl AppendPageResponse {
    ///
    /// `append_page` 出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 更新後 revision
    /// * `instance_id` - 更新後 instance_id
    /// * `summary` - 実行結果要約
    /// * `amended` - amend 相当保存有無
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
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
}

impl GetPageSectionResponse {
    ///
    /// `get_page_section` 出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `revision` - 対応 revision
    /// * `section` - 解決後 section
    /// * `content` - セクション本文
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
        path: String,
        revision: u64,
        section: McpSectionInfo,
        content: String,
    ) -> Self {
        Self {
            path,
            revision,
            section,
            content,
        }
    }
}

impl From<SectionSelector> for McpSectionSelector {
    fn from(selector: SectionSelector) -> Self {
        match selector {
            SectionSelector::ById(id) => Self::ById(id),
            SectionSelector::ByTitle(title) => Self::ByTitle(title),
        }
    }
}

impl From<McpSectionSelector> for SectionSelector {
    fn from(selector: McpSectionSelector) -> Self {
        match selector {
            McpSectionSelector::ById(id) => Self::ById(id),
            McpSectionSelector::ByTitle(title) => Self::ByTitle(title),
        }
    }
}

impl From<EditPageSectionSelectorObject> for McpEditPageSectionSelector {
    fn from(selector: EditPageSectionSelectorObject) -> Self {
        match selector.by {
            EditPageSectionSelectorBy::Id => Self::ById(selector.value),
            EditPageSectionSelectorBy::Title => Self::ByTitle(selector.value),
        }
    }
}

impl From<EditPageSectionSelector> for McpEditPageSectionSelector {
    fn from(selector: EditPageSectionSelector) -> Self {
        match selector {
            EditPageSectionSelector::Text(value) => Self::Text(value),
            EditPageSectionSelector::Structured(value) => Self::from(value),
        }
    }
}

impl From<EditPageInsertSectionPlacement>
    for McpEditPageInsertSectionPlacement
{
    fn from(placement: EditPageInsertSectionPlacement) -> Self {
        match placement {
            EditPageInsertSectionPlacement::Before => Self::Before,
            EditPageInsertSectionPlacement::After => Self::After,
        }
    }
}

impl From<EditPageReplaceTextOccurrence> for McpEditPageReplaceTextOccurrence {
    fn from(occurrence: EditPageReplaceTextOccurrence) -> Self {
        match occurrence {
            EditPageReplaceTextOccurrence::First => Self::First,
            EditPageReplaceTextOccurrence::All => Self::All,
        }
    }
}

impl From<EditPageToolOperation> for McpEditPageOperation {
    fn from(operation: EditPageToolOperation) -> Self {
        match operation {
            EditPageToolOperation::ReplaceSection { section, content } => {
                Self::ReplaceSection {
                    section: section.into(),
                    content,
                }
            }
            EditPageToolOperation::InsertSection {
                anchor,
                placement,
                content,
            } => Self::InsertSection {
                anchor: anchor.into(),
                placement: placement.into(),
                content,
            },
            EditPageToolOperation::DeleteSection { section } => {
                Self::DeleteSection {
                    section: section.into(),
                }
            }
            EditPageToolOperation::ReplaceText {
                old_text,
                new_text,
                occurrence,
            } => Self::ReplaceText {
                old_text,
                new_text,
                occurrence: occurrence.map(Into::into),
            },
        }
    }
}

impl From<McpEditPageSectionSelector> for SectionSelector {
    fn from(selector: McpEditPageSectionSelector) -> Self {
        match selector {
            McpEditPageSectionSelector::Text(value) => Self::ByTitle(value),
            McpEditPageSectionSelector::ById(value) => Self::ById(value),
            McpEditPageSectionSelector::ByTitle(value) => {
                Self::ByTitle(value)
            }
        }
    }
}

impl From<McpEditPageInsertSectionPlacement>
    for ServiceEditPageInsertSectionPlacement
{
    fn from(placement: McpEditPageInsertSectionPlacement) -> Self {
        match placement {
            McpEditPageInsertSectionPlacement::Before => Self::Before,
            McpEditPageInsertSectionPlacement::After => Self::After,
        }
    }
}

impl From<McpEditPageReplaceTextOccurrence>
    for ServiceEditPageReplaceTextOccurrence
{
    fn from(occurrence: McpEditPageReplaceTextOccurrence) -> Self {
        match occurrence {
            McpEditPageReplaceTextOccurrence::First => Self::First,
            McpEditPageReplaceTextOccurrence::All => Self::All,
        }
    }
}

impl From<McpEditPageOperation> for ServiceEditPageOperation {
    fn from(operation: McpEditPageOperation) -> Self {
        match operation {
            McpEditPageOperation::ReplaceSection { section, content } => {
                Self::ReplaceSection {
                    section: section.into(),
                    content,
                }
            }
            McpEditPageOperation::InsertSection {
                anchor,
                placement,
                content,
            } => Self::InsertSection {
                anchor: anchor.into(),
                placement: placement.into(),
                content,
            },
            McpEditPageOperation::DeleteSection { section } => {
                Self::DeleteSection {
                    section: section.into(),
                }
            }
            McpEditPageOperation::ReplaceText {
                old_text,
                new_text,
                occurrence,
            } => Self::ReplaceText {
                old_text,
                new_text,
                occurrence: occurrence.map(Into::into),
            },
        }
    }
}

impl From<EditPageRequest> for ServiceEditPageRequest {
    fn from(request: EditPageRequest) -> Self {
        Self::new(
            request.path,
            request.revision,
            request.instance_id,
            request.operation.into(),
        )
    }
}

impl From<&TocSection> for McpSectionInfo {
    fn from(section: &TocSection) -> Self {
        Self {
            id: section.id().to_string(),
            title: section.title().to_string(),
            level: section.level(),
            ordinal: section.ordinal(),
            parent_id: section.parent_id().map(str::to_string),
            section_chars: section.section_chars(),
        }
    }
}

impl From<GetPageResult> for GetPageResponse {
    fn from(result: GetPageResult) -> Self {
        Self {
            path: result.path().to_string(),
            revision: result.revision(),
            instance_id: result.instance_id().to_string(),
            content: result.content().to_string(),
        }
    }
}

impl From<GetPageTocResult> for GetPageTocResponse {
    fn from(result: GetPageTocResult) -> Self {
        let sections = result.sections().iter().map(McpSectionInfo::from).collect();
        Self {
            path: result.path().to_string(),
            revision: result.revision(),
            instance_id: result.instance_id().to_string(),
            sections,
        }
    }
}

impl From<ListPagesResult> for ListPagesResponse {
    fn from(result: ListPagesResult) -> Self {
        let items = result
            .items()
            .iter()
            .map(McpPageListItem::from)
            .collect();
        Self {
            items,
            has_more: result.has_more(),
            next_cursor: result.next_cursor().map(str::to_string),
        }
    }
}

impl From<&super::service::ListPageItem> for McpPageListItem {
    fn from(item: &super::service::ListPageItem) -> Self {
        Self {
            path: item.path().to_string(),
            revision: item.revision(),
            updated_at: item.updated_at().to_string(),
            updated_by: item.updated_by().to_string(),
        }
    }
}

impl From<SearchPagesResult> for SearchPagesResponse {
    fn from(result: SearchPagesResult) -> Self {
        let items = result
            .items()
            .iter()
            .map(McpSearchPageItem::from)
            .collect();
        Self { items }
    }
}

impl From<&super::service::SearchPageItem> for McpSearchPageItem {
    fn from(item: &super::service::SearchPageItem) -> Self {
        Self {
            path: item.path().to_string(),
            revision: item.revision(),
            score: item.score(),
            snippet: item.snippet().to_string(),
        }
    }
}

impl From<WritePageResult> for WritePageResponse {
    fn from(result: WritePageResult) -> Self {
        Self {
            path: result.path().to_string(),
            revision: result.revision(),
            instance_id: result.instance_id().to_string(),
            summary: result.summary().to_string(),
        }
    }
}

impl From<EditPageResult> for EditPageResponse {
    fn from(result: EditPageResult) -> Self {
        Self {
            path: result.path().to_string(),
            revision: result.revision(),
            instance_id: result.instance_id().to_string(),
            summary: result.summary().to_string(),
        }
    }
}

impl From<AppendServiceResult> for AppendPageResponse {
    fn from(result: AppendServiceResult) -> Self {
        Self {
            path: result.path().to_string(),
            revision: result.revision(),
            instance_id: result.instance_id().to_string(),
            summary: result.summary().to_string(),
            amended: result.amended(),
        }
    }
}

impl From<GetPageSectionResult> for GetPageSectionResponse {
    fn from(result: GetPageSectionResult) -> Self {
        Self {
            path: result.path().to_string(),
            revision: result.revision(),
            section: McpSectionInfo::from(result.section()),
            content: result.content().to_string(),
        }
    }
}
