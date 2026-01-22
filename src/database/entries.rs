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
    AssetId, LockToken, PageId, PageIndex, PageSource,
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
    pub(in crate::database) fn new(
        id: PageId,
        index: PageIndex,
    ) -> Self {
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
