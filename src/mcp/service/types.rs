/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPサービスの要求・結果・操作型と対応する実装を定義するモジュール
//!

use std::fmt;

use crate::database::types::{BearerScope, PageId, PageIndex};

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

    /// ページアセット一覧取得
    ListPageAssets,

    /// ページアセット解決
    ResolvePageAsset,

    /// ページアセットメタ情報取得
    GetPageAssetMetadata,

    /// ページアセット本体取得
    GetPageAssetData,

    /// ページアセットアップロード
    UploadPageAsset,

    /// ページアセット削除
    DeletePageAsset,

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
            | Self::GetPageSection
            | Self::ListPageAssets
            | Self::ResolvePageAsset
            | Self::GetPageAssetMetadata
            | Self::GetPageAssetData => BearerScope::Read,
            Self::CreatePage => BearerScope::Create,
            Self::UpdatePage
            | Self::EditPage
            | Self::RenamePage
            | Self::UploadPageAsset => {
                BearerScope::Update
            }
            Self::AppendPage => BearerScope::Append,
            Self::DeletePage | Self::DeletePageAsset => BearerScope::Delete,
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
                | Self::ListPageAssets
                | Self::ResolvePageAsset
                | Self::GetPageAssetMetadata
                | Self::GetPageAssetData
        )
    }
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
    pub(super) fn new(
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
    pub(super) fn new(
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
    pub(super) fn new(
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
    pub(super) fn new(
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
    pub(super) fn new(
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

///
/// MCP公開用のアセットメタ情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageAssetMetadata {
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
    instance_id: Option<String>,
}

impl PageAssetMetadata {
    ///
    /// MCP公開用のアセットメタ情報を生成する
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    /// * `path` - アセットが紐付くページの current path
    /// * `file_name` - アセットファイル名
    /// * `mime_type` - MIME 種別
    /// * `size` - raw binary size
    /// * `timestamp` - 登録または更新日時
    /// * `username` - 登録または更新ユーザ名
    /// * `instance_id` - アセット内容の任意 instance_id
    ///
    /// # 戻り値
    /// 生成したメタ情報を返す。
    ///
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

    ///
    /// アセットIDを返す
    ///
    /// # 戻り値
    /// アセットIDを返す。
    ///
    pub(crate) fn asset_id(&self) -> &str {
        &self.asset_id
    }

    ///
    /// current path を返す
    ///
    /// # 戻り値
    /// アセットが紐付くページの current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// アセットファイル名を返す
    ///
    /// # 戻り値
    /// アセットファイル名を返す。
    ///
    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
    }

    ///
    /// MIME 種別を返す
    ///
    /// # 戻り値
    /// MIME 種別を返す。
    ///
    pub(crate) fn mime_type(&self) -> &str {
        &self.mime_type
    }

    ///
    /// raw binary size を返す
    ///
    /// # 戻り値
    /// raw binary size を返す。
    ///
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    ///
    /// 登録または更新日時を返す
    ///
    /// # 戻り値
    /// 登録または更新日時を返す。
    ///
    pub(crate) fn timestamp(&self) -> &str {
        &self.timestamp
    }

    ///
    /// 登録または更新ユーザ名を返す
    ///
    /// # 戻り値
    /// 登録または更新ユーザ名を返す。
    ///
    pub(crate) fn username(&self) -> &str {
        &self.username
    }

    ///
    /// アセット内容の instance_id を返す
    ///
    /// # 戻り値
    /// 存在する場合は instance_id を返す。
    ///
    pub(crate) fn instance_id(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }
}

///
/// `list_page_assets` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListPageAssetsResult {
    /// 対象ページの current path
    path: String,

    /// アセットメタ情報一覧
    assets: Vec<PageAssetMetadata>,
}

impl ListPageAssetsResult {
    ///
    /// `list_page_assets` の戻り値を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページの current path
    /// * `assets` - アセットメタ情報一覧
    ///
    /// # 戻り値
    /// 生成した戻り値を返す。
    ///
    pub(crate) fn new(path: String, assets: Vec<PageAssetMetadata>) -> Self {
        Self { path, assets }
    }

    ///
    /// 対象ページの current path を返す
    ///
    /// # 戻り値
    /// 対象ページの current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// アセットメタ情報一覧を返す
    ///
    /// # 戻り値
    /// アセットメタ情報一覧を返す。
    ///
    pub(crate) fn assets(&self) -> &[PageAssetMetadata] {
        &self.assets
    }
}

///
/// `resolve_page_asset` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvePageAssetResult {
    /// 対象ページの current path
    path: String,

    /// 解決されたアセットメタ情報
    asset: PageAssetMetadata,
}

impl ResolvePageAssetResult {
    ///
    /// `resolve_page_asset` の戻り値を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページの current path
    /// * `asset` - 解決されたアセットメタ情報
    ///
    /// # 戻り値
    /// 生成した戻り値を返す。
    ///
    pub(crate) fn new(path: String, asset: PageAssetMetadata) -> Self {
        Self { path, asset }
    }

    ///
    /// 対象ページの current path を返す
    ///
    /// # 戻り値
    /// 対象ページの current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 解決されたアセットメタ情報を返す
    ///
    /// # 戻り値
    /// 解決されたアセットメタ情報を返す。
    ///
    pub(crate) fn asset(&self) -> &PageAssetMetadata {
        &self.asset
    }
}

///
/// `get_page_asset_metadata` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetPageAssetMetadataResult {
    /// 対象ページの current path
    path: String,

    /// アセットメタ情報
    asset: PageAssetMetadata,
}

impl GetPageAssetMetadataResult {
    ///
    /// `get_page_asset_metadata` の戻り値を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページの current path
    /// * `asset` - アセットメタ情報
    ///
    /// # 戻り値
    /// 生成した戻り値を返す。
    ///
    pub(crate) fn new(path: String, asset: PageAssetMetadata) -> Self {
        Self { path, asset }
    }

    ///
    /// 対象ページの current path を返す
    ///
    /// # 戻り値
    /// 対象ページの current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// アセットメタ情報を返す
    ///
    /// # 戻り値
    /// アセットメタ情報を返す。
    ///
    pub(crate) fn asset(&self) -> &PageAssetMetadata {
        &self.asset
    }
}

///
/// `get_page_asset_data` の戻り値
///
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct GetPageAssetDataResult {
    /// 対象ページの current path
    path: String,

    /// アセットメタ情報
    asset: PageAssetMetadata,

    /// base64 エンコード済みアセット本体
    data_base64: String,
}

impl GetPageAssetDataResult {
    ///
    /// `get_page_asset_data` の戻り値を生成する
    ///
    /// # 引数
    /// * `path` - 対象ページの current path
    /// * `asset` - アセットメタ情報
    /// * `data_base64` - base64 エンコード済みアセット本体
    ///
    /// # 戻り値
    /// 生成した戻り値を返す。
    ///
    pub(crate) fn new(
        path: String,
        asset: PageAssetMetadata,
        data_base64: String,
    ) -> Self {
        Self {
            path,
            asset,
            data_base64,
        }
    }

    ///
    /// 対象ページの current path を返す
    ///
    /// # 戻り値
    /// 対象ページの current path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// アセットメタ情報を返す
    ///
    /// # 戻り値
    /// アセットメタ情報を返す。
    ///
    pub(crate) fn asset(&self) -> &PageAssetMetadata {
        &self.asset
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

impl fmt::Debug for GetPageAssetDataResult {
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
            .debug_struct("GetPageAssetDataResult")
            .field("path", &self.path)
            .field("asset", &self.asset)
            .field("data_base64", &"[REDACTED]")
            .finish()
    }
}

///
/// `upload_page_asset` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UploadPageAssetResult {
    /// アップロードしたアセットメタ情報
    asset: PageAssetMetadata,

    /// 実行結果要約
    summary: String,
}

impl UploadPageAssetResult {
    ///
    /// `upload_page_asset` の戻り値を生成する
    ///
    /// # 引数
    /// * `asset` - アップロードしたアセットメタ情報
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した戻り値を返す。
    ///
    pub(crate) fn new(asset: PageAssetMetadata, summary: String) -> Self {
        Self { asset, summary }
    }

    ///
    /// アセットメタ情報を返す
    ///
    /// # 戻り値
    /// アップロードしたアセットメタ情報を返す。
    ///
    pub(crate) fn asset(&self) -> &PageAssetMetadata {
        &self.asset
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

///
/// `delete_page_asset` の戻り値
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeletePageAssetResult {
    /// 削除したアセットID
    asset_id: String,

    /// 対象ページの正規化済み path
    path: String,

    /// 削除したアセットファイル名
    file_name: String,

    /// 実行結果要約
    summary: String,
}

impl DeletePageAssetResult {
    ///
    /// `delete_page_asset` の戻り値を生成する
    ///
    /// # 引数
    /// * `asset_id` - 削除したアセットID
    /// * `path` - 対象ページの正規化済み path
    /// * `file_name` - 削除したアセットファイル名
    /// * `summary` - 実行結果要約
    ///
    /// # 戻り値
    /// 生成した戻り値を返す。
    ///
    pub(crate) fn new(
        asset_id: String,
        path: String,
        file_name: String,
        summary: String,
    ) -> Self {
        Self {
            asset_id,
            path,
            file_name,
            summary,
        }
    }

    ///
    /// 削除したアセットIDを返す
    ///
    /// # 戻り値
    /// 削除したアセットIDを返す。
    ///
    pub(crate) fn asset_id(&self) -> &str {
        &self.asset_id
    }

    ///
    /// 対象ページの正規化済み path を返す
    ///
    /// # 戻り値
    /// 対象ページの正規化済み path を返す。
    ///
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    ///
    /// 削除したアセットファイル名を返す
    ///
    /// # 戻り値
    /// 削除したアセットファイル名を返す。
    ///
    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
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
    pub(super) fn new(
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

///
/// `search_pages` の戻り値
///
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchPagesResult {
    /// 検索結果一覧
    items: Vec<SearchPageItem>,
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
    pub(super) fn new(items: Vec<SearchPageItem>) -> Self {
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
    pub(super) fn new(
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
    pub(super) fn new(
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
    pub(super) fn new(
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
    pub(super) fn new(
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

