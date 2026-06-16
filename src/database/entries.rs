/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース一覧出力用のエントリ型を定義するモジュール
//!

use chrono::{DateTime, Local};

use crate::database::types::{
    AssetId,
    LockToken,
    PageId,
    PageIndex,
    PageSource,
    PromptArgumentEntry,
};

///
/// page list 用のページ情報
///
pub(crate) struct PageListEntry {
    /// ページID
    id: PageId,

    /// ページパス
    path: String,

    /// 最新リビジョン番号
    latest_revision: u64,

    /// 作成日時
    timestamp: DateTime<Local>,

    /// 記述ユーザ名
    user_name: String,

    /// 削除済みフラグ
    deleted: bool,

    /// ドラフトフラグ
    draft: bool,

    /// ロックフラグ
    locked: bool,
}

///
/// template list 用のテンプレート候補情報
///
pub(crate) struct TemplateCandidateListEntry {
    /// ページID
    page_id: PageId,

    /// current path
    current_path: String,

    /// テンプレート表示名
    name: String,

    /// テンプレート説明
    description: Option<String>,

    /// マクロ即時展開可否
    macro_expand: Option<bool>,
}

///
/// prompt list用のprompt候補情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PromptCandidateListEntry {
    /// ページID
    page_id: PageId,

    /// current path
    current_path: String,

    /// prompt名
    name: String,

    /// prompt説明
    description: String,

    /// system情報
    system: Option<String>,

    /// prompt引数
    arguments: Vec<PromptArgumentEntry>,
}

///
/// prompt名から解決した最新ページソース
///
#[derive(Clone, Debug)]
pub(crate) struct PromptSourceEntry {
    /// 最新リビジョン番号
    revision: u64,

    /// 最新ページソース
    source: String,
}

///
/// resource 候補一覧用のページ由来resource情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceCandidateListEntry {
    /// ページID
    page_id: PageId,

    /// current path
    current_path: String,

    /// resource 識別子
    resource_id: String,

    /// resource 名
    name: String,

    /// resource 説明
    description: String,

    /// MIME type
    mime_type: String,
}

///
/// resource URIから解決した最新ページソース
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceSourceEntry {
    /// current path
    current_path: String,

    /// 最新リビジョン番号
    revision: u64,

    /// 最新ページソース
    source: String,
}

///
/// resource URIからの最新ページソース解決結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResourceSourceLookupResult {
    /// 最新ページソースを取得できた
    Found(ResourceSourceEntry),

    /// URI索引が存在しない
    NotFound,

    /// draft、soft delete等により公開不能
    Unavailable,

    /// URI索引とページ正本の内部不整合
    Inconsistent,
}

///
/// resource 一覧エントリの由来
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResourceListSource {
    /// 固定組み込みresource
    Builtin,

    /// ページ由来resource
    Page,
}

///
/// resource list用のresource情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceListEntry {
    /// MCP公開URI
    uri: String,

    /// resource 名
    name: String,

    /// resource 説明
    description: String,

    /// MIME type
    mime_type: String,

    /// resourceの由来
    source: ResourceListSource,

    /// ページID
    page_id: Option<PageId>,

    /// current path
    current_path: Option<String>,
}

///
/// FTS用のページインデックス情報
///
pub(crate) struct PageIndexEntry {
    /// ページID
    id: PageId,
    /// ページインデックス
    index: PageIndex,
}

///
/// FTS用のページソース情報
///
pub(crate) struct PageSourceEntry {
    /// ページID
    page_id: PageId,

    /// リビジョン番号
    revision: u64,

    /// ページソース
    source: PageSource,
}

///
/// lock list 用のロック情報
///
pub(crate) struct LockListEntry {
    /// ロック解除トークン
    token: LockToken,

    /// ページID
    #[allow(dead_code)]
    page_id: PageId,

    /// ページパス
    page_path: String,

    /// 有効期限
    expire: DateTime<Local>,

    /// ユーザ名
    user_name: String,
}

///
/// asset list 用のアセット情報
///
pub(crate) struct AssetListEntry {
    /// アセットID
    id: AssetId,

    /// ファイル名
    file_name: String,

    /// MIME種別
    mime: String,

    /// サイズ(バイト)
    size: u64,

    /// 登録日時
    timestamp: DateTime<Local>,

    /// 登録ユーザ名
    user_name: String,

    /// 所有ページパス
    page_path: Option<String>,

    /// 削除済みフラグ
    deleted: bool,
}

///
/// アセット移動結果
///
pub(crate) enum AssetMoveResult {
    /// 移動成功
    Moved,

    /// 移動先ページが存在しない
    PageNotFound,

    /// 移動先ページが削除済み
    PageDeleted,

    /// 移動先に同名アセットが存在する
    NameConflict,
}

impl AssetListEntry {
    ///
    /// アセット一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `id` - アセットID
    /// * `file_name` - ファイル名
    /// * `mime` - MIME種別
    /// * `size` - サイズ(バイト)
    /// * `timestamp` - 登録日時
    /// * `user_name` - 登録ユーザ名
    /// * `page_path` - 所有ページパス
    /// * `deleted` - 削除済みフラグ
    ///
    /// # 戻り値
    /// AssetListEntryを返す。
    ///
    pub(in crate::database) fn new(
        id: AssetId,
        file_name: String,
        mime: String,
        size: u64,
        timestamp: DateTime<Local>,
        user_name: String,
        page_path: Option<String>,
        deleted: bool,
    ) -> Self {
        Self {
            id,
            file_name,
            mime,
            size,
            timestamp,
            user_name,
            page_path,
            deleted,
        }
    }
    ///
    /// アセットIDへのアクセサ
    ///
    /// # 戻り値
    /// アセットIDを返す。
    ///
    pub(crate) fn id(&self) -> AssetId {
        self.id.clone()
    }

    ///
    /// ファイル名へのアクセサ
    ///
    /// # 戻り値
    /// ファイル名を返す。
    ///
    pub(crate) fn file_name(&self) -> String {
        self.file_name.clone()
    }

    ///
    /// MIME種別へのアクセサ
    ///
    /// # 戻り値
    /// MIME種別を返す。
    ///
    pub(crate) fn mime(&self) -> String {
        self.mime.clone()
    }

    ///
    /// サイズへのアクセサ
    ///
    /// # 戻り値
    /// サイズ(バイト)を返す。
    ///
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    ///
    /// 登録日時へのアクセサ
    ///
    /// # 戻り値
    /// 登録日時を返す。
    ///
    pub(crate) fn timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    ///
    /// 登録ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 登録ユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 所有ページパスへのアクセサ
    ///
    /// # 戻り値
    /// 所有ページのパスを返す。ゾンビの場合はNone。
    ///
    pub(crate) fn page_path(&self) -> Option<String> {
        self.page_path.clone()
    }

    ///
    /// 削除済みフラグへのアクセサ
    ///
    /// # 戻り値
    /// 削除済みの場合は`true`を返す。
    ///
    pub(crate) fn deleted(&self) -> bool {
        self.deleted
    }

    ///
    /// ゾンビ状態の判定
    ///
    /// # 戻り値
    /// ゾンビ状態の場合は`true`を返す。
    ///
    pub(crate) fn is_zombie(&self) -> bool {
        self.page_path.is_none()
    }

    #[cfg(test)]
    ///
    /// テスト用のアセット一覧情報を生成
    ///
    /// # 引数
    /// * `id` - アセットID
    /// * `file_name` - ファイル名
    /// * `mime` - MIME種別
    /// * `size` - サイズ(バイト)
    /// * `timestamp` - 登録日時
    /// * `user_name` - 登録ユーザ名
    /// * `page_path` - 所有ページパス
    /// * `deleted` - 削除済みフラグ
    ///
    /// # 戻り値
    /// テスト用のAssetListEntryを返す。
    ///
    pub(crate) fn new_for_test(
        id: AssetId,
        file_name: String,
        mime: String,
        size: u64,
        timestamp: DateTime<Local>,
        user_name: String,
        page_path: Option<String>,
        deleted: bool,
    ) -> Self {
        Self {
            id,
            file_name,
            mime,
            size,
            timestamp,
            user_name,
            page_path,
            deleted,
        }
    }
}

impl LockListEntry {
    ///
    /// ロック一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `token` - ロック解除トークン
    /// * `page_id` - ページID
    /// * `page_path` - ページパス
    /// * `expire` - 有効期限
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// LockListEntryを返す。
    ///
    pub(in crate::database) fn new(
        token: LockToken,
        page_id: PageId,
        page_path: String,
        expire: DateTime<Local>,
        user_name: String,
    ) -> Self {
        Self {
            token,
            page_id,
            page_path,
            expire,
            user_name,
        }
    }
    ///
    /// ロック解除トークンへのアクセサ
    ///
    /// # 戻り値
    /// ロック解除トークンを返す。
    ///
    pub(crate) fn token(&self) -> LockToken {
        self.token.clone()
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// ページパスへのアクセサ
    ///
    /// # 戻り値
    /// ページパスを返す。
    ///
    pub(crate) fn page_path(&self) -> String {
        self.page_path.clone()
    }

    ///
    /// 有効期限へのアクセサ
    ///
    /// # 戻り値
    /// 有効期限を返す。
    ///
    pub(crate) fn expire(&self) -> DateTime<Local> {
        self.expire
    }

    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }
}

impl PageListEntry {
    ///
    /// ページ一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    /// * `latest_revision` - 最新リビジョン番号
    /// * `timestamp` - 作成日時
    /// * `user_name` - 記述ユーザ名
    /// * `deleted` - 削除済みフラグ
    /// * `draft` - ドラフトフラグ
    /// * `locked` - ロックフラグ
    ///
    /// # 戻り値
    /// PageListEntryを返す。
    ///
    pub(in crate::database) fn new(
        id: PageId,
        path: String,
        latest_revision: u64,
        timestamp: DateTime<Local>,
        user_name: String,
        deleted: bool,
        draft: bool,
        locked: bool,
    ) -> Self {
        Self {
            id,
            path,
            latest_revision,
            timestamp,
            user_name,
            deleted,
            draft,
            locked,
        }
    }
    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn id(&self) -> PageId {
        self.id.clone()
    }

    ///
    /// ページパスへのアクセサ
    ///
    /// # 戻り値
    /// ページパスを返す。
    ///
    pub(crate) fn path(&self) -> String {
        self.path.clone()
    }

    ///
    /// 最新リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// 最新リビジョン番号を返す。
    ///
    pub(crate) fn latest_revision(&self) -> u64 {
        self.latest_revision
    }

    ///
    /// 作成日時へのアクセサ
    ///
    /// # 戻り値
    /// 作成日時を返す。
    ///
    pub(crate) fn timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    ///
    /// 記述したユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 記述したユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 削除済みフラグへのアクセサ
    ///
    /// # 戻り値
    /// 削除済みの場合は`true`を返す。
    ///
    pub(crate) fn deleted(&self) -> bool {
        self.deleted
    }

    ///
    /// ドラフト状態の判定
    ///
    /// # 戻り値
    /// ドラフト状態の場合は`true`を返す。
    ///
    pub(crate) fn is_draft(&self) -> bool {
        self.draft
    }

    ///
    /// ロック状態の判定
    ///
    /// # 戻り値
    /// ロック中の場合は`true`を返す。
    ///
    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    #[cfg(test)]
    ///
    /// テスト用のページ一覧情報を生成
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    /// * `latest_revision` - 最新リビジョン番号
    /// * `timestamp` - 作成日時
    /// * `user_name` - 記述ユーザ名
    /// * `deleted` - 削除済みフラグ
    /// * `draft` - ドラフトフラグ
    /// * `locked` - ロックフラグ
    ///
    /// # 戻り値
    /// テスト用のPageListEntryを返す。
    ///
    pub(crate) fn new_for_test(
        id: PageId,
        path: String,
        latest_revision: u64,
        timestamp: DateTime<Local>,
        user_name: String,
        deleted: bool,
        draft: bool,
        locked: bool,
    ) -> Self {
        Self {
            id,
            path,
            latest_revision,
            timestamp,
            user_name,
            deleted,
            draft,
            locked,
        }
    }
}

impl TemplateCandidateListEntry {
    ///
    /// テンプレート候補一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `current_path` - current path
    /// * `name` - テンプレート表示名
    /// * `description` - テンプレート説明
    /// * `macro_expand` - マクロ即時展開可否
    ///
    /// # 戻り値
    /// TemplateCandidateListEntry を返す。
    ///
    pub(in crate::database) fn new(
        page_id: PageId,
        current_path: String,
        name: String,
        description: Option<String>,
        macro_expand: Option<bool>,
    ) -> Self {
        Self {
            page_id,
            current_path,
            name,
            description,
            macro_expand,
        }
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// current path へのアクセサ
    ///
    /// # 戻り値
    /// current path を返す。
    ///
    pub(crate) fn current_path(&self) -> &str {
        &self.current_path
    }

    ///
    /// テンプレート表示名へのアクセサ
    ///
    /// # 戻り値
    /// テンプレート表示名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// テンプレート説明へのアクセサ
    ///
    /// # 戻り値
    /// テンプレート説明を返す。
    ///
    pub(crate) fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    ///
    /// マクロ即時展開可否へのアクセサ
    ///
    /// # 戻り値
    /// マクロ即時展開可否を返す。
    ///
    pub(crate) fn macro_expand(&self) -> Option<bool> {
        self.macro_expand
    }
}

impl PromptCandidateListEntry {
    ///
    /// prompt候補一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `current_path` - current path
    /// * `name` - prompt名
    /// * `description` - prompt説明
    /// * `system` - system情報
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// PromptCandidateListEntryを返す。
    ///
    pub(in crate::database) fn new(
        page_id: PageId,
        current_path: String,
        name: String,
        description: String,
        system: Option<String>,
        arguments: Vec<PromptArgumentEntry>,
    ) -> Self {
        Self {
            page_id,
            current_path,
            name,
            description,
            system,
            arguments,
        }
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// current pathへのアクセサ
    ///
    /// # 戻り値
    /// current pathを返す。
    ///
    pub(crate) fn current_path(&self) -> &str {
        &self.current_path
    }

    ///
    /// prompt名へのアクセサ
    ///
    /// # 戻り値
    /// prompt名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// prompt説明へのアクセサ
    ///
    /// # 戻り値
    /// prompt説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// system情報へのアクセサ
    ///
    /// # 戻り値
    /// system情報が存在する場合はその値を返す。
    ///
    pub(crate) fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    ///
    /// prompt引数へのアクセサ
    ///
    /// # 戻り値
    /// prompt引数を定義順で返す。
    ///
    pub(crate) fn arguments(&self) -> &[PromptArgumentEntry] {
        &self.arguments
    }
}

impl ResourceCandidateListEntry {
    ///
    /// ページ由来resource候補一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `current_path` - current path
    /// * `resource_id` - resource 識別子
    /// * `name` - resource 名
    /// * `description` - resource 説明
    /// * `mime_type` - MIME type
    ///
    /// # 戻り値
    /// ResourceCandidateListEntryを返す。
    ///
    pub(in crate::database) fn new(
        page_id: PageId,
        current_path: String,
        resource_id: String,
        name: String,
        description: String,
        mime_type: String,
    ) -> Self {
        Self {
            page_id,
            current_path,
            resource_id,
            name,
            description,
            mime_type,
        }
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// current pathへのアクセサ
    ///
    /// # 戻り値
    /// current pathを返す。
    ///
    pub(crate) fn current_path(&self) -> &str {
        &self.current_path
    }

    ///
    /// resource 識別子へのアクセサ
    ///
    /// # 戻り値
    /// resource 識別子を返す。
    ///
    pub(crate) fn resource_id(&self) -> &str {
        &self.resource_id
    }

    ///
    /// resource 名へのアクセサ
    ///
    /// # 戻り値
    /// resource 名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// resource 説明へのアクセサ
    ///
    /// # 戻り値
    /// resource 説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// MIME typeへのアクセサ
    ///
    /// # 戻り値
    /// MIME typeを返す。
    ///
    pub(crate) fn mime_type(&self) -> &str {
        &self.mime_type
    }
}

impl ResourceSourceEntry {
    ///
    /// resource URIから解決した最新ページソースを生成する。
    ///
    /// # 引数
    /// * `current_path` - current path
    /// * `revision` - 最新リビジョン番号
    /// * `source` - 最新ページソース
    ///
    /// # 戻り値
    /// ResourceSourceEntryを返す。
    ///
    pub(in crate::database) fn new(
        current_path: String,
        revision: u64,
        source: String,
    ) -> Self {
        Self {
            current_path,
            revision,
            source,
        }
    }

    ///
    /// current pathへのアクセサ
    ///
    /// # 戻り値
    /// current pathを返す。
    ///
    pub(crate) fn current_path(&self) -> &str {
        &self.current_path
    }

    ///
    /// 最新リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// 最新リビジョン番号を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 最新ページソースへのアクセサ
    ///
    /// # 戻り値
    /// 最新ページソースを返す。
    ///
    pub(crate) fn source(&self) -> &str {
        &self.source
    }
}

impl ResourceListEntry {
    ///
    /// resource一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `uri` - MCP公開URI
    /// * `name` - resource 名
    /// * `description` - resource 説明
    /// * `mime_type` - MIME type
    /// * `source` - resourceの由来
    /// * `page_id` - ページID
    /// * `current_path` - current path
    ///
    /// # 戻り値
    /// ResourceListEntryを返す。
    ///
    pub(in crate::database) fn new(
        uri: String,
        name: String,
        description: String,
        mime_type: String,
        source: ResourceListSource,
        page_id: Option<PageId>,
        current_path: Option<String>,
    ) -> Self {
        Self {
            uri,
            name,
            description,
            mime_type,
            source,
            page_id,
            current_path,
        }
    }

    ///
    /// MCP公開URIへのアクセサ
    ///
    /// # 戻り値
    /// MCP公開URIを返す。
    ///
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    ///
    /// resource 名へのアクセサ
    ///
    /// # 戻り値
    /// resource 名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// resource 説明へのアクセサ
    ///
    /// # 戻り値
    /// resource 説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// MIME typeへのアクセサ
    ///
    /// # 戻り値
    /// MIME typeを返す。
    ///
    pub(crate) fn mime_type(&self) -> &str {
        &self.mime_type
    }

    ///
    /// resourceの由来へのアクセサ
    ///
    /// # 戻り値
    /// resourceの由来を返す。
    ///
    pub(crate) fn source(&self) -> ResourceListSource {
        self.source
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDが存在する場合はその値を返す。
    ///
    pub(crate) fn page_id(&self) -> Option<PageId> {
        self.page_id.clone()
    }

    ///
    /// current pathへのアクセサ
    ///
    /// # 戻り値
    /// current pathが存在する場合はその値を返す。
    ///
    pub(crate) fn current_path(&self) -> Option<&str> {
        self.current_path.as_deref()
    }
}

impl PromptSourceEntry {
    ///
    /// prompt名から解決した最新ページソースを生成する
    ///
    /// # 引数
    /// * `revision` - 最新リビジョン番号
    /// * `source` - 最新ページソース
    ///
    /// # 戻り値
    /// prompt最新ページソースを返す。
    ///
    pub(in crate::database) fn new(
        revision: u64,
        source: String,
    ) -> Self {
        Self {
            revision,
            source,
        }
    }

    ///
    /// 最新リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// 最新リビジョン番号を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// 最新ページソースへのアクセサ
    ///
    /// # 戻り値
    /// 最新ページソースを返す。
    ///
    pub(crate) fn source(&self) -> &str {
        &self.source
    }
}

impl PageIndexEntry {
    ///
    /// ページインデックス一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `index` - ページインデックス
    ///
    /// # 戻り値
    /// PageIndexEntryを返す。
    ///
    pub(in crate::database) fn new(id: PageId, index: PageIndex) -> Self {
        Self { id, index }
    }
    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.id.clone()
    }

    ///
    /// ページインデックスへのアクセサ
    ///
    /// # 戻り値
    /// ページインデックスを返す。
    ///
    pub(crate) fn index(&self) -> PageIndex {
        self.index.clone()
    }
}

impl PageSourceEntry {
    ///
    /// ページソース一覧用の情報を生成する。
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `revision` - リビジョン番号
    /// * `source` - ページソース
    ///
    /// # 戻り値
    /// PageSourceEntryを返す。
    ///
    pub(in crate::database) fn new(
        page_id: PageId,
        revision: u64,
        source: PageSource,
    ) -> Self {
        Self {
            page_id,
            revision,
            source,
        }
    }
    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// リビジョン番号を返す。
    ///
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// ページソースへのアクセサ
    ///
    /// # 戻り値
    /// ページソースを返す。
    ///
    pub(crate) fn source(&self) -> PageSource {
        self.source.clone()
    }
}
