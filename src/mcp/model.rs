/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP公開面の要求・応答モデルを定義するモジュール
//!

use std::fmt;

use serde::Serialize;

use crate::fts::FtsSearchTarget;

use super::service::{
    AppendServiceResult,
    DeletePageAssetResult,
    EditPageRequest as ServiceEditPageRequest,
    EditPageResult,
    EditPageInsertSectionPlacement as ServiceEditPageInsertSectionPlacement,
    EditPageOperation as ServiceEditPageOperation,
    EditPageReplaceTextOccurrence as ServiceEditPageReplaceTextOccurrence,
    GetPageResult,
    GetPageAssetDataResult,
    GetPageAssetMetadataResult,
    GetPageSectionResult,
    GetPageTocResult,
    ListPageAssetsResult,
    ListPagesResult,
    PageAssetMetadata as ServicePageAssetMetadata,
    ResolvePageAssetResult,
    SearchPagesResult,
    SectionSelector,
    TocSection,
    UploadPageAssetResult,
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

    /// `list_page_assets` 入力
    ListPageAssets(ListPageAssetsRequest),

    /// `resolve_page_asset` 入力
    ResolvePageAsset(PageAssetByFileRequest),

    /// `get_page_asset_metadata` 入力
    GetPageAssetMetadata(PageAssetByFileRequest),

    /// `get_page_asset_data` 入力
    GetPageAssetData(PageAssetByFileRequest),

    /// `upload_page_asset` 入力
    UploadPageAsset(UploadPageAssetRequest),

    /// `delete_page_asset` 入力
    DeletePageAsset(PageAssetByFileRequest),
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

    /// `list_page_assets` 出力
    ListPageAssets(ListPageAssetsResponse),

    /// `resolve_page_asset` 出力
    ResolvePageAsset(ResolvePageAssetResponse),

    /// `get_page_asset_metadata` 出力
    GetPageAssetMetadata(GetPageAssetMetadataResponse),

    /// `get_page_asset_data` 出力
    GetPageAssetData(GetPageAssetDataResponse),

    /// `upload_page_asset` 出力
    UploadPageAsset(UploadPageAssetResponse),

    /// `delete_page_asset` 出力
    DeletePageAsset(DeletePageAssetResponse),
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
/// `list_page_assets` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListPageAssetsRequest {
    /// 対象ページの絶対 path
    path: String,
}

///
/// `resolve_page_asset` / `get_page_asset_metadata` /
/// `get_page_asset_data` 入力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageAssetByFileRequest {
    /// 対象ページの絶対 path
    path: String,

    /// 対象アセットファイル名
    file_name: String,
}

///
/// `upload_page_asset` 入力
///
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct UploadPageAssetRequest {
    /// 対象ページの絶対 path
    path: String,

    /// アップロードするアセットファイル名
    file_name: String,

    /// アップロードするアセットの MIME 種別
    mime_type: String,

    /// base64 エンコード済みアセット本体
    data_base64: String,
}

impl UploadPageAssetRequest {
    ///
    /// `upload_page_asset` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `file_name` - アップロードするアセットファイル名
    /// * `mime_type` - アップロードするアセットの MIME 種別
    /// * `data_base64` - base64 エンコード済みアセット本体
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(
        path: String,
        file_name: String,
        mime_type: String,
        data_base64: String,
    ) -> Self {
        Self {
            path,
            file_name,
            mime_type,
            data_base64,
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
    /// アセットファイル名を返す
    ///
    /// # 戻り値
    /// アップロードするアセットファイル名を返す。
    ///
    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
    }

    ///
    /// MIME 種別を返す
    ///
    /// # 戻り値
    /// アップロードするアセットの MIME 種別を返す。
    ///
    pub(crate) fn mime_type(&self) -> &str {
        &self.mime_type
    }

    ///
    /// base64 エンコード済みアセット本体を返す
    ///
    /// # 戻り値
    /// base64 エンコード済みアセット本体を返す。
    ///
    pub(crate) fn data_base64(&self) -> &str {
        &self.data_base64
    }
}

impl fmt::Debug for UploadPageAssetRequest {
    ///
    /// アセット本体を秘匿したデバッグ表現を生成する
    ///
    /// # 引数
    /// * `formatter` - デバッグ出力先
    ///
    /// # 戻り値
    /// デバッグ表現の生成結果を返す。
    ///
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadPageAssetRequest")
            .field("path", &self.path)
            .field("file_name", &self.file_name)
            .field("mime_type", &self.mime_type)
            .field("data_base64", &"[REDACTED]")
            .finish()
    }
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

        /// 変更後の見出し文字列
        title: Option<String>,

        /// 置換後本文
        content: String,

        /// サブセクションを含めて置換するかどうか
        include_subsection: Option<bool>,
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
/// アセットメタ情報の公開モデル
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct McpPageAssetMetadata {
    /// アセットID
    asset_id: String,

    /// アセットが紐付くページの current path
    path: String,

    /// アセットファイル名
    file_name: String,

    /// MIME 種別
    mime_type: String,

    /// raw binary size
    size: u64,

    /// 登録または更新日時
    timestamp: String,

    /// 登録または更新ユーザ名
    username: String,

    /// アセット内容の任意 instance_id
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
}

///
/// `list_page_assets` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ListPageAssetsResponse {
    /// 対象ページの current path
    path: String,

    /// アセットメタ情報一覧
    items: Vec<McpPageAssetMetadata>,
}

///
/// `resolve_page_asset` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ResolvePageAssetResponse {
    /// 解決されたアセットID
    asset_id: String,

    /// 対象ページの current path
    path: String,

    /// 解決されたアセットファイル名
    file_name: String,
}

///
/// `get_page_asset_metadata` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GetPageAssetMetadataResponse {
    /// アセットメタ情報
    asset: McpPageAssetMetadata,
}

///
/// `get_page_asset_data` 出力
///
#[derive(Clone, Eq, PartialEq, Serialize)]
pub(crate) struct GetPageAssetDataResponse {
    /// アセットメタ情報
    asset: McpPageAssetMetadata,

    /// base64 エンコード済みアセット本体
    data_base64: String,
}

impl fmt::Debug for GetPageAssetDataResponse {
    ///
    /// アセット本体を秘匿したデバッグ表現を生成する
    ///
    /// # 引数
    /// * `formatter` - デバッグ出力先
    ///
    /// # 戻り値
    /// デバッグ表現の生成結果を返す。
    ///
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GetPageAssetDataResponse")
            .field("asset", &self.asset)
            .field("data_base64", &"[REDACTED]")
            .finish()
    }
}

///
/// `upload_page_asset` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct UploadPageAssetResponse {
    /// アセットメタ情報
    asset: McpPageAssetMetadata,

    /// 実行結果要約
    summary: String,
}

///
/// `delete_page_asset` 出力
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DeletePageAssetResponse {
    /// 削除したアセットID
    asset_id: String,

    /// 対象ページの正規化済み path
    path: String,

    /// 削除したアセットファイル名
    file_name: String,

    /// 実行結果要約
    summary: String,
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

impl ListPageAssetsRequest {
    ///
    /// `list_page_assets` 入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(path: String) -> Self {
        Self { path }
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
}

impl PageAssetByFileRequest {
    ///
    /// path と file_name によるアセット入力を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページ path
    /// * `file_name` - 対象アセットファイル名
    ///
    /// # 戻り値
    /// 生成した入力モデルを返す。
    ///
    pub(crate) fn new(path: String, file_name: String) -> Self {
        Self { path, file_name }
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
    /// 対象アセットファイル名を返す
    ///
    /// # 戻り値
    /// 対象アセットファイル名を返す。
    ///
    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
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

impl McpPageAssetMetadata {
    ///
    /// アセットメタ情報の公開モデルを生成する
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    /// * `path` - current path
    /// * `file_name` - アセットファイル名
    /// * `mime_type` - MIME 種別
    /// * `size` - raw binary size
    /// * `timestamp` - 登録または更新日時
    /// * `username` - 登録または更新ユーザ名
    /// * `instance_id` - アセット内容の任意 instance_id
    ///
    /// # 戻り値
    /// 生成した公開モデルを返す。
    ///
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        asset_id: String,
        path: String,
        file_name: String,
        mime_type: String,
        size: u64,
        timestamp: String,
        username: String,
        instance_id: Option<String>,
    ) -> Self {
        Self {
            asset_id,
            path,
            file_name,
            mime_type,
            size,
            timestamp,
            username,
            instance_id,
        }
    }
}

impl ListPageAssetsResponse {
    ///
    /// `list_page_assets` 出力を生成する
    ///
    /// # 引数
    /// * `path` - current path
    /// * `items` - アセットメタ情報一覧
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
        path: String,
        items: Vec<McpPageAssetMetadata>,
    ) -> Self {
        Self { path, items }
    }
}

impl ResolvePageAssetResponse {
    ///
    /// `resolve_page_asset` 出力を生成する
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    /// * `path` - current path
    /// * `file_name` - アセットファイル名
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
        asset_id: String,
        path: String,
        file_name: String,
    ) -> Self {
        Self {
            asset_id,
            path,
            file_name,
        }
    }
}

impl GetPageAssetMetadataResponse {
    ///
    /// `get_page_asset_metadata` 出力を生成する
    ///
    /// # 引数
    /// * `asset` - アセットメタ情報
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(asset: McpPageAssetMetadata) -> Self {
        Self { asset }
    }
}

impl GetPageAssetDataResponse {
    ///
    /// `get_page_asset_data` 出力を生成する
    ///
    /// # 引数
    /// * `asset` - アセットメタ情報
    /// * `data_base64` - base64 エンコード済みアセット本体
    ///
    /// # 戻り値
    /// 生成した出力モデルを返す。
    ///
    pub(crate) fn new(
        asset: McpPageAssetMetadata,
        data_base64: String,
    ) -> Self {
        Self { asset, data_base64 }
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
            EditPageToolOperation::ReplaceSection {
                section,
                title,
                content,
                include_subsection,
            } => Self::ReplaceSection {
                section: section.into(),
                title,
                content,
                include_subsection,
            },
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
            McpEditPageOperation::ReplaceSection {
                section,
                title,
                content,
                include_subsection,
            } => Self::ReplaceSection {
                section: section.into(),
                title,
                content,
                include_subsection,
            },
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

impl From<ListPageAssetsResult> for ListPageAssetsResponse {
    fn from(result: ListPageAssetsResult) -> Self {
        let items = result
            .assets()
            .iter()
            .map(McpPageAssetMetadata::from)
            .collect();
        Self {
            path: result.path().to_string(),
            items,
        }
    }
}

impl From<ResolvePageAssetResult> for ResolvePageAssetResponse {
    fn from(result: ResolvePageAssetResult) -> Self {
        Self {
            asset_id: result.asset().asset_id().to_string(),
            path: result.path().to_string(),
            file_name: result.asset().file_name().to_string(),
        }
    }
}

impl From<GetPageAssetMetadataResult> for GetPageAssetMetadataResponse {
    fn from(result: GetPageAssetMetadataResult) -> Self {
        Self {
            asset: McpPageAssetMetadata::from(result.asset()),
        }
    }
}

impl From<GetPageAssetDataResult> for GetPageAssetDataResponse {
    fn from(result: GetPageAssetDataResult) -> Self {
        Self {
            asset: McpPageAssetMetadata::from(result.asset()),
            data_base64: result.data_base64().to_string(),
        }
    }
}

impl From<UploadPageAssetResult> for UploadPageAssetResponse {
    fn from(result: UploadPageAssetResult) -> Self {
        Self {
            asset: McpPageAssetMetadata::from(result.asset()),
            summary: result.summary().to_string(),
        }
    }
}

impl From<DeletePageAssetResult> for DeletePageAssetResponse {
    fn from(result: DeletePageAssetResult) -> Self {
        Self {
            asset_id: result.asset_id().to_string(),
            path: result.path().to_string(),
            file_name: result.file_name().to_string(),
            summary: result.summary().to_string(),
        }
    }
}

impl From<&ServicePageAssetMetadata> for McpPageAssetMetadata {
    fn from(asset: &ServicePageAssetMetadata) -> Self {
        Self {
            asset_id: asset.asset_id().to_string(),
            path: asset.path().to_string(),
            file_name: asset.file_name().to_string(),
            mime_type: asset.mime_type().to_string(),
            size: asset.size(),
            timestamp: asset.timestamp().to_string(),
            username: asset.username().to_string(),
            instance_id: asset.instance_id().map(str::to_string),
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

#[cfg(test)]
mod tests {
    use super::{
        DeletePageAssetResponse,
        EditPageSectionSelector,
        EditPageToolOperation,
        GetPageAssetDataResponse,
        GetPageAssetMetadataResponse,
        ListPageAssetsResponse,
        McpEditPageOperation,
        McpEditPageSectionSelector,
        McpPageAssetMetadata,
        McpRequestEnvelope,
        McpResponseEnvelope,
        McpToolName,
        McpToolRequest,
        McpToolResponse,
        ResolvePageAssetResponse,
        SectionSelector,
        ServiceEditPageOperation,
        UploadPageAssetRequest,
        UploadPageAssetResponse,
    };
    use crate::mcp::service::{
        GetPageAssetDataResult,
        PageAssetMetadata,
    };

    #[test]
    fn edit_page_replace_section_conversion_preserves_extension_fields() {
        let operation = EditPageToolOperation::ReplaceSection {
            section: EditPageSectionSelector::Text("Overview".to_string()),
            title: Some("Updated overview".to_string()),
            content: "updated".to_string(),
            include_subsection: Some(true),
        };

        let mcp_operation = McpEditPageOperation::from(operation);
        match &mcp_operation {
            McpEditPageOperation::ReplaceSection {
                section,
                title,
                content,
                include_subsection,
            } => {
                assert_eq!(
                    *section,
                    McpEditPageSectionSelector::Text("Overview".to_string()),
                );
                assert_eq!(title.as_deref(), Some("Updated overview"));
                assert_eq!(content, "updated");
                assert_eq!(*include_subsection, Some(true));
            }
            _ => panic!("unexpected operation"),
        }

        let service_operation = ServiceEditPageOperation::from(mcp_operation);
        match service_operation {
            ServiceEditPageOperation::ReplaceSection {
                section,
                title,
                content,
                include_subsection,
            } => {
                assert_eq!(section, SectionSelector::ByTitle("Overview".to_string()));
                assert_eq!(title.as_deref(), Some("Updated overview"));
                assert_eq!(content, "updated");
                assert_eq!(include_subsection, Some(true));
            }
            _ => panic!("unexpected operation"),
        }
    }

    ///
    /// upload requestのDebug表現がアセット本体を秘匿することを確認する。
    ///
    /// # 注記
    /// secret sentinelをrequest、tool request、envelopeの各境界で確認する。
    ///
    #[test]
    fn upload_page_asset_request_debug_redacts_data_base64() {
        const SECRET_SENTINEL: &str = "secret-upload-data-base64";

        let request = UploadPageAssetRequest::new(
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            SECRET_SENTINEL.to_string(),
        );
        let request_debug = format!("{request:?}");
        assert!(!request_debug.contains(SECRET_SENTINEL));
        assert!(request_debug.contains("[REDACTED]"));

        let tool_request = McpToolRequest::UploadPageAsset(request);
        let tool_request_debug = format!("{tool_request:?}");
        assert!(!tool_request_debug.contains(SECRET_SENTINEL));
        assert!(tool_request_debug.contains("[REDACTED]"));

        let envelope = McpRequestEnvelope::new(
            McpToolName::UploadPageAsset,
            tool_request,
        );
        let envelope_debug = format!("{envelope:?}");
        assert!(!envelope_debug.contains(SECRET_SENTINEL));
        assert!(envelope_debug.contains("[REDACTED]"));
    }

    ///
    /// get-data responseのDebug表現が
    /// アセット本体を秘匿することを確認する。
    ///
    /// # 注記
    /// secret sentinelをservice result、公開response、tool response、
    /// response envelopeの各境界で確認する。
    ///
    #[test]
    fn get_page_asset_data_response_debug_redacts_data_base64() {
        const SECRET_SENTINEL: &str = "secret-response-data-base64";

        let asset = PageAssetMetadata::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            123,
            "2026-09-05T00:00:00+09:00".to_string(),
            "alice".to_string(),
            None,
        );
        let result = GetPageAssetDataResult::new(
            "/docs/page".to_string(),
            asset,
            SECRET_SENTINEL.to_string(),
        );
        let result_debug = format!("{result:?}");
        assert!(!result_debug.contains(SECRET_SENTINEL));
        assert!(result_debug.contains("[REDACTED]"));

        let response = GetPageAssetDataResponse::from(result);
        let response_debug = format!("{response:?}");
        assert!(!response_debug.contains(SECRET_SENTINEL));
        assert!(response_debug.contains("[REDACTED]"));

        let tool_response = McpToolResponse::GetPageAssetData(response);
        let tool_response_debug = format!("{tool_response:?}");
        assert!(!tool_response_debug.contains(SECRET_SENTINEL));
        assert!(tool_response_debug.contains("[REDACTED]"));

        let envelope = McpResponseEnvelope::new(
            McpToolName::GetPageAssetData,
            tool_response,
        );
        let envelope_debug = format!("{envelope:?}");
        assert!(!envelope_debug.contains(SECRET_SENTINEL));
        assert!(envelope_debug.contains("[REDACTED]"));
    }

    ///
    /// upload responseがinstance_idの
    /// 既存省略規則を維持することを確認する。
    ///
    /// # 注記
    /// Noneの場合の省略とSomeの場合の出力を
    /// 同じ応答型で確認する。
    ///
    #[test]
    fn upload_page_asset_response_preserves_instance_id_rule() {
        let asset_without_instance = McpPageAssetMetadata::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            123,
            "2026-09-05T00:00:00+09:00".to_string(),
            "alice".to_string(),
            None,
        );
        let response_without_instance = UploadPageAssetResponse {
            asset: asset_without_instance,
            summary: "asset uploaded".to_string(),
        };
        let json_without_instance =
            serde_json::to_value(response_without_instance)
                .expect("serialize upload response without instance failed");
        let fields_without_instance = json_without_instance
            .as_object()
            .expect("upload response must be object");

        assert_eq!(fields_without_instance.len(), 2);
        assert!(fields_without_instance.contains_key("asset"));
        assert!(fields_without_instance.contains_key("summary"));
        assert!(json_without_instance["asset"]
            .get("instance_id")
            .is_none());
        assert!(json_without_instance.get("data_base64").is_none());

        let asset_with_instance = McpPageAssetMetadata::new(
            "asset-2".to_string(),
            "/docs/page".to_string(),
            "document.pdf".to_string(),
            "application/pdf".to_string(),
            456,
            "2026-09-05T00:00:00+09:00".to_string(),
            "alice".to_string(),
            Some("asset-instance".to_string()),
        );
        let response_with_instance = UploadPageAssetResponse {
            asset: asset_with_instance,
            summary: "asset uploaded".to_string(),
        };
        let json_with_instance = serde_json::to_value(response_with_instance)
            .expect("serialize upload response with instance failed");

        assert_eq!(
            json_with_instance["asset"]["instance_id"],
            "asset-instance",
        );
        assert_eq!(
            json_with_instance
                .as_object()
                .expect("upload response must be object")
                .len(),
            2,
        );
        assert!(json_with_instance.get("data_base64").is_none());
    }

    ///
    /// delete responseが最小識別情報とsummaryだけを含むことを確認する。
    ///
    /// # 注記
    /// serialize後のトップレベルfield数と各field名を確認する。
    ///
    #[test]
    fn delete_page_asset_response_contains_only_minimum_fields() {
        let response = DeletePageAssetResponse {
            asset_id: "asset-1".to_string(),
            path: "/docs/page".to_string(),
            file_name: "image.png".to_string(),
            summary: "asset deleted".to_string(),
        };
        let json = serde_json::to_value(response)
            .expect("serialize delete response failed");
        let fields = json
            .as_object()
            .expect("delete response must be object");

        assert_eq!(fields.len(), 4);
        assert_eq!(json["asset_id"], "asset-1");
        assert_eq!(json["path"], "/docs/page");
        assert_eq!(json["file_name"], "image.png");
        assert_eq!(json["summary"], "asset deleted");
        for excluded in [
            "asset",
            "mime_type",
            "size",
            "timestamp",
            "username",
            "instance_id",
            "data_base64",
        ] {
            assert!(fields.get(excluded).is_none());
        }
    }

    #[test]
    fn asset_metadata_response_omits_instance_id_and_data_base64() {
        let asset = McpPageAssetMetadata::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            123,
            "2026-08-29T00:00:00+09:00".to_string(),
            "alice".to_string(),
            None,
        );
        let response = GetPageAssetMetadataResponse::new(asset);
        let json = serde_json::to_value(response)
            .expect("serialize asset metadata response failed");

        assert!(json["asset"].get("instance_id").is_none());
        assert!(json.get("data_base64").is_none());
    }

    #[test]
    fn list_page_assets_response_omits_data_base64() {
        let asset = McpPageAssetMetadata::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            123,
            "2026-08-29T00:00:00+09:00".to_string(),
            "alice".to_string(),
            Some("asset-instance".to_string()),
        );
        let response =
            ListPageAssetsResponse::new("/docs/page".to_string(), vec![asset]);
        let json = serde_json::to_value(response)
            .expect("serialize list page assets response failed");

        assert_eq!(json["path"], "/docs/page");
        assert_eq!(json["items"][0]["instance_id"], "asset-instance");
        assert!(json.get("data_base64").is_none());
        assert!(json["items"][0].get("data_base64").is_none());
    }

    #[test]
    fn resolve_page_asset_response_contains_only_minimum_identifier_fields() {
        let response = ResolvePageAssetResponse::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
        );
        let json = serde_json::to_value(response)
            .expect("serialize resolve page asset response failed");
        let fields = json
            .as_object()
            .expect("resolve page asset response must be object");

        assert_eq!(json["asset_id"], "asset-1");
        assert_eq!(json["path"], "/docs/page");
        assert_eq!(json["file_name"], "image.png");
        assert_eq!(fields.len(), 3);
        assert!(json.get("asset").is_none());
        assert!(json.get("data_base64").is_none());
        assert!(json.get("instance_id").is_none());
    }

    #[test]
    fn asset_metadata_response_includes_instance_id_when_present() {
        let asset = McpPageAssetMetadata::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            123,
            "2026-08-29T00:00:00+09:00".to_string(),
            "alice".to_string(),
            Some("asset-instance".to_string()),
        );
        let response = GetPageAssetMetadataResponse::new(asset);
        let json = serde_json::to_value(response)
            .expect("serialize asset metadata response failed");

        assert_eq!(json["asset"]["instance_id"], "asset-instance");
        assert!(json.get("data_base64").is_none());
    }

    #[test]
    fn asset_data_response_contains_data_base64() {
        let asset = McpPageAssetMetadata::new(
            "asset-1".to_string(),
            "/docs/page".to_string(),
            "image.png".to_string(),
            "image/png".to_string(),
            123,
            "2026-08-29T00:00:00+09:00".to_string(),
            "alice".to_string(),
            Some("asset-instance".to_string()),
        );
        let response =
            GetPageAssetDataResponse::new(asset, "YmluYXJ5".to_string());
        let json = serde_json::to_value(response)
            .expect("serialize asset data response failed");

        assert_eq!(json["data_base64"], "YmluYXJ5");
        assert_eq!(json["asset"]["instance_id"], "asset-instance");
    }
}
