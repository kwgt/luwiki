/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ情報の参照系操作を提供するモジュール
//!

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use chrono::Local;
use redb::{ReadableDatabase, ReadableMultimapTable, ReadableTable};

use super::DatabaseManager;
use crate::database::entries::{PageIndexEntry, PageListEntry, PageSourceEntry};
use crate::database::schema::{
    DELETED_PAGE_PATH_TABLE,
    LOCK_INFO_TABLE,
    PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE,
    PAGE_SOURCE_TABLE,
    USER_INFO_TABLE,
};
use crate::database::txn_helpers::find_lock_by_page;
use crate::database::types::{PageId, PageIndex, PageSource, UserId, UserInfo};

///
/// current path から解決した現在ページ状態
///
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct CurrentPageState {
    /// ページID
    page_id: PageId,

    /// ページインデックス
    page_index: PageIndex,

    /// 最新 revision
    latest_revision: Option<u64>,

    /// 最新ソース
    latest_source: Option<PageSource>,

    /// current path
    current_path: String,
}

#[cfg_attr(not(test), allow(dead_code))]
impl CurrentPageState {
    ///
    /// 現在ページ状態の生成
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `page_index` - ページインデックス
    /// * `latest_revision` - 最新 revision
    /// * `latest_source` - 最新ソース
    /// * `current_path` - current path
    ///
    /// # 戻り値
    /// 生成した現在ページ状態を返す。
    ///
    fn new(
        page_id: PageId,
        page_index: PageIndex,
        latest_revision: Option<u64>,
        latest_source: Option<PageSource>,
        current_path: String,
    ) -> Self {
        Self {
            page_id,
            page_index,
            latest_revision,
            latest_source,
            current_path,
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
    /// ページインデックスへのアクセサ
    ///
    /// # 戻り値
    /// ページインデックスを返す。
    ///
    pub(crate) fn page_index(&self) -> PageIndex {
        self.page_index.clone()
    }

    ///
    /// 最新 revision へのアクセサ
    ///
    /// # 戻り値
    /// 最新 revision を返す。draft の場合は `None` を返す。
    ///
    pub(crate) fn latest_revision(&self) -> Option<u64> {
        self.latest_revision
    }

    ///
    /// 最新ソースへのアクセサ
    ///
    /// # 戻り値
    /// 最新ソースを返す。draft の場合は `None` を返す。
    ///
    pub(crate) fn latest_source(&self) -> Option<PageSource> {
        self.latest_source.clone()
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
}

///
/// search 結果整形用の current path 情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CurrentPathInfo {
    /// current path
    current_path: String,

    /// 削除済み状態
    deleted: bool,

    /// draft 状態
    draft: bool,
}

impl CurrentPathInfo {
    ///
    /// current path 情報の生成
    ///
    /// # 引数
    /// * `current_path` - current path
    /// * `deleted` - 削除済み状態
    /// * `draft` - draft 状態
    ///
    /// # 戻り値
    /// 生成した current path 情報を返す。
    ///
    fn new(current_path: String, deleted: bool, draft: bool) -> Self {
        Self {
            current_path,
            deleted,
            draft,
        }
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
    /// 削除済み状態へのアクセサ
    ///
    /// # 戻り値
    /// 削除済みの場合は `true` を返す。
    ///
    pub(crate) fn deleted(&self) -> bool {
        self.deleted
    }

    ///
    /// draft 状態へのアクセサ
    ///
    /// # 戻り値
    /// draft の場合は `true` を返す。
    ///
    pub(crate) fn draft(&self) -> bool {
        self.draft
    }

    ///
    /// 短縮URL対象として利用可能かどうかを返す
    ///
    /// # 戻り値
    /// 通常ページとして短縮URL対象にできる場合は `true` を返す。
    ///
    pub(crate) fn short_url_available(&self) -> bool {
        !self.deleted && !self.draft
    }
}

///
/// page path から解決したページ状態
///
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum PagePathResolveState {
    /// 通常ページとして解決できた状態
    Current { page_id: PageId, draft: bool },

    /// 削除済みページが存在する状態
    Deleted,

    /// 対応するページが存在しない状態
    NotFound,
}

#[cfg_attr(not(test), allow(dead_code))]
impl PagePathResolveState {
    /*
     * path 解決結果の補助アクセサ群は、
     * 今後の呼び出し側追加を見込んで保持する。
     * 現時点では一部経路で未使用のため dead_code を抑止する。
     */
    #[allow(dead_code)]
    ///
    /// 通常ページとして解決したページIDを返す
    ///
    /// # 戻り値
    /// 通常ページまたは draft として解決した場合はページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> Option<PageId> {
        match self {
            Self::Current { page_id, .. } => Some(page_id.clone()),
            Self::Deleted | Self::NotFound => None,
        }
    }

    #[allow(dead_code)]
    ///
    /// draft 状態かどうかを返す
    ///
    /// # 戻り値
    /// draft として解決した場合は `true` を返す。
    ///
    pub(crate) fn draft(&self) -> bool {
        match self {
            Self::Current { draft, .. } => *draft,
            Self::Deleted | Self::NotFound => false,
        }
    }

    #[allow(dead_code)]
    ///
    /// 短縮URL対象として返却可能なページIDを返す
    ///
    /// # 戻り値
    /// 通常ページとして短縮URL対象にできる場合はページIDを返す。
    ///
    pub(crate) fn short_url_page_id(&self) -> Option<PageId> {
        match self {
            Self::Current { page_id, draft } if !draft => Some(page_id.clone()),
            Self::Current { .. } | Self::Deleted | Self::NotFound => None,
        }
    }
}

///
/// `append` 競合確認用の現在状態
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppendConflictState {
    /// 最新 revision
    latest_revision: Option<u64>,

    /// 最新 source の更新者
    latest_user_id: Option<UserId>,

    /// 有効ロック有無
    locked: bool,

    /// draft 状態
    draft: bool,
}

impl AppendConflictState {
    ///
    /// `append` 競合確認状態の生成
    ///
    /// # 引数
    /// * `latest_revision` - 最新 revision
    /// * `latest_user_id` - 最新 source の更新者
    /// * `locked` - 有効ロック有無
    /// * `draft` - draft 状態
    ///
    /// # 戻り値
    /// 生成した状態を返す。
    ///
    fn new(
        latest_revision: Option<u64>,
        latest_user_id: Option<UserId>,
        locked: bool,
        draft: bool,
    ) -> Self {
        Self {
            latest_revision,
            latest_user_id,
            locked,
            draft,
        }
    }

    ///
    /// 最新 revision へのアクセサ
    ///
    /// # 戻り値
    /// 最新 revision を返す。draft の場合は `None` を返す。
    ///
    pub(crate) fn latest_revision(&self) -> Option<u64> {
        self.latest_revision
    }

    ///
    /// 最新更新者へのアクセサ
    ///
    /// # 戻り値
    /// 最新 source の更新者を返す。draft の場合は `None` を返す。
    ///
    pub(crate) fn latest_user_id(&self) -> Option<UserId> {
        self.latest_user_id.clone()
    }

    ///
    /// ロック有無へのアクセサ
    ///
    /// # 戻り値
    /// 有効ロックが存在する場合は `true` を返す。
    ///
    pub(crate) fn locked(&self) -> bool {
        self.locked
    }

    ///
    /// draft 状態へのアクセサ
    ///
    /// # 戻り値
    /// draft の場合は `true` を返す。
    ///
    pub(crate) fn draft(&self) -> bool {
        self.draft
    }
}

impl DatabaseManager {
    ///
    /// ページIDからページインデックスとパスを取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some((path, PageIndex)))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn get_page_index_entry_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<(String, PageIndex)>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_INDEX_TABLE)?;
        match table.get(page_id.clone())? {
            Some(entry) => {
                let index = entry.value();
                Ok(Some((index.path(), index)))
            }
            None => Ok(None),
        }
    }

    ///
    /// ページIDからページインデックスを取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(PageIndex))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_page_index_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<PageIndex>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_INDEX_TABLE)?;
        Ok(table.get(page_id.clone())?.map(|entry| entry.value()))
    }

    ///
    /// ページソースの取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `revision` - リビジョン番号
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(PageSource))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_page_source(
        &self,
        page_id: &PageId,
        revision: u64,
    ) -> Result<Option<PageSource>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_SOURCE_TABLE)?;
        Ok(table
            .get((page_id.clone(), revision))?
            .map(|entry| entry.value()))
    }

    ///
    /// ページが存在するかを確認する（テスト用）
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    /// * `revision` - リビジョン番号
    ///
    /// # 戻り値
    /// 存在する場合はtrueを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn has_page_source_for_test(
        &self,
        page_id: &PageId,
        revision: u64,
    ) -> Result<bool> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_SOURCE_TABLE)?;
        Ok(table.get((page_id.clone(), revision))?.is_some())
    }

    ///
    /// ページ情報の一覧取得
    ///
    /// # 戻り値
    /// ページ情報の一覧を返す。
    ///
    pub(crate) fn list_pages(&self) -> Result<Vec<PageListEntry>> {
        /*
         * 期限切れロックの掃除
         */
        self.cleanup_expired_locks()?;

        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let lock_table = txn.open_table(LOCK_INFO_TABLE)?;
        let mut pages = Vec::new();
        let now = Local::now();

        /*
         * ロック状態の収集
         */
        let mut locked_pages = HashMap::new();
        for entry in lock_table.iter()? {
            let (_, info) = entry?;
            let info = info.value();
            if info.expire() <= now {
                continue;
            }
            locked_pages.insert(info.page(), true);
        }

        /*
         * ページ情報の収集
         */
        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            let index = index.value();
            let page_id = page_id.value().clone();
            let locked = locked_pages.contains_key(&page_id);

            if index.is_draft() {
                pages.push(PageListEntry::new(
                    page_id,
                    index.path(),
                    0,
                    Local::now(),
                    String::new(),
                    index.deleted(),
                    true,
                    locked,
                ));
                continue;
            }

            let revision = index.latest();
            let source = source_table
                .get((page_id.clone(), revision))?
                .ok_or_else(|| anyhow!("page source not found"))?
                .value();
            let user_id = source.user();

            let user_info = user_table
                .get(user_id.clone())?
                .ok_or_else(|| anyhow!("user not found"))?
                .value();

            pages.push(PageListEntry::new(
                page_id,
                index.path(),
                revision,
                source.timestamp(),
                user_info.username(),
                index.deleted(),
                false,
                locked,
            ));
        }

        Ok(pages)
    }

    ///
    /// パスプレフィックスに対するページ一覧取得
    ///
    /// # 引数
    /// * `base_path` - 起点パス
    /// * `with_deleted` - 削除済みページの取得有無
    ///
    /// # 戻り値
    /// ページパス一覧を返す。
    ///
    ///
    /// パスプレフィックスに対するページ一覧取得
    ///
    /// # 引数
    /// * `base_path` - 起点パス
    /// * `with_deleted` - 削除済みページの取得有無
    ///
    /// # 戻り値
    /// ページ情報の一覧を返す。
    ///
    pub(crate) fn list_page_entries_by_prefix(
        &self,
        base_path: &str,
        with_deleted: bool,
    ) -> Result<Vec<PageListEntry>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let path_table = txn.open_table(PAGE_PATH_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let mut entries = Vec::new();

        /*
         * 通常ページの収集
         */
        collect_page_list_entries(
            &path_table,
            &index_table,
            &source_table,
            &user_table,
            base_path,
            &mut entries,
        )?;

        /*
         * 削除済みページの収集
         */
        if with_deleted {
            let deleted_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            collect_deleted_page_list_entries(
                &deleted_table,
                &index_table,
                &source_table,
                &user_table,
                base_path,
                &mut entries,
            )?;
        }

        Ok(entries)
    }

    ///
    /// FTS用にページインデックスの一覧を取得する
    ///
    pub(crate) fn list_page_index_entries(
        &self,
    ) -> Result<Vec<PageIndexEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_INDEX_TABLE)?;
        let mut entries = Vec::new();

        for entry in table.iter()? {
            let (page_id, index) = entry?;
            entries.push(PageIndexEntry::new(
                page_id.value().clone(),
                index.value(),
            ));
        }

        Ok(entries)
    }

    ///
    /// FTS用にページソースの一覧を取得する
    ///
    pub(crate) fn list_page_source_entries(
        &self,
    ) -> Result<Vec<PageSourceEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let mut entries = Vec::new();

        for entry in table.iter()? {
            let (key, source) = entry?;
            let (page_id, revision) = key.value();
            entries.push(PageSourceEntry::new(
                page_id,
                revision,
                source.value(),
            ));
        }

        Ok(entries)
    }

    ///
    /// FTS用にページソースの一覧を取得する
    ///
    /// # 引数
    /// * `page_id` - 取得対象のページID
    ///
    /// # 戻り値
    /// ページソースの一覧を返す。
    ///
    pub(crate) fn list_page_source_entries_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Vec<PageSourceEntry>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let mut entries = Vec::new();

        /*
         * 対象ページのソース収集
         */
        let start = (page_id.clone(), 0u64);
        let end = (page_id.clone(), u64::MAX);
        for entry in table.range(start..=end)? {
            let (key, source) = entry?;
            let (page_id, revision) = key.value();
            entries.push(PageSourceEntry::new(
                page_id,
                revision,
                source.value(),
            ));
        }

        Ok(entries)
    }

    ///
    /// パスからページIDを取得
    ///
    /// # 引数
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 解決できたページIDを返す。存在しない場合は`None`を返す。
    ///
    pub(crate) fn get_page_id_by_path(
        &self,
        path: &str,
    ) -> Result<Option<PageId>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_PATH_TABLE)?;
        let key = path.to_string();
        Ok(table.get(&key)?.map(|entry| entry.value()))
    }

    ///
    /// current path から現在ページ状態を取得
    ///
    /// # 引数
    /// * `path` - current path
    ///
    /// # 戻り値
    /// current path に一致するページが存在する場合は
    /// `Ok(Some(CurrentPageState))` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn get_current_page_state_by_path(
        &self,
        path: &str,
    ) -> Result<Option<CurrentPageState>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let path_table = txn.open_table(PAGE_PATH_TABLE)?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let key = path.to_string();

        /*
         * current path からページIDを解決
         */
        let page_id = match path_table.get(&key)? {
            Some(entry) => entry.value(),
            None => return Ok(None),
        };

        /*
         * ページ状態を構築
         */
        let page_index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!("page index not found")),
        };
        let current_path = match page_index.current_path() {
            Some(current_path) => current_path.to_string(),
            None => return Err(anyhow!("current path not found")),
        };
        if current_path != path {
            return Err(anyhow!("current path mismatch"));
        }

        let latest_revision = if page_index.is_draft() {
            None
        } else {
            Some(page_index.latest())
        };
        let latest_source = match latest_revision {
            Some(revision) => source_table
                .get((page_id.clone(), revision))?
                .map(|entry| entry.value()),
            None => None,
        };

        Ok(Some(CurrentPageState::new(
            page_id,
            page_index,
            latest_revision,
            latest_source,
            current_path,
        )))
    }

    ///
    /// 複数ページIDの current path 情報を取得
    ///
    /// # 引数
    /// * `page_ids` - 取得対象のページID一覧
    ///
    /// # 戻り値
    /// current path 情報の写像を返す。
    ///
    pub(crate) fn get_current_page_paths_by_ids(
        &self,
        page_ids: &[PageId],
    ) -> Result<HashMap<PageId, CurrentPathInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let mut paths = HashMap::new();

        /*
         * current path 情報の収集
         */
        for page_id in page_ids {
            let Some(entry) = index_table.get(page_id.clone())? else {
                continue;
            };
            let index = entry.value();
            let Some(current_path) = index.current_path() else {
                continue;
            };

            paths.insert(
                page_id.clone(),
                CurrentPathInfo::new(
                    current_path.to_string(),
                    index.deleted(),
                    index.is_draft(),
                ),
            );
        }

        Ok(paths)
    }

    ///
    /// ページIDから current path 情報を取得
    ///
    /// # 引数
    /// * `page_id` - 取得対象のページID
    ///
    /// # 戻り値
    /// 対象ページが存在し、current path を解決できる場合は
    /// `Ok(Some(CurrentPathInfo))` を返す。
    /// 存在しない場合は `Ok(None)` を返す。
    ///
    pub(crate) fn get_current_page_path_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<CurrentPathInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;

        /*
         * ページインデックスの解決
         */
        let index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Ok(None),
        };

        /*
         * current path 情報の構築
         */
        let current_path = match index.current_path() {
            Some(path) => path.to_string(),
            None => return Ok(None),
        };

        Ok(Some(CurrentPathInfo::new(
            current_path,
            index.deleted(),
            index.is_draft(),
        )))
    }

    ///
    /// `append` 競合確認用の現在状態を取得
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// 対象ページが存在する場合は現在状態を返す。
    /// 存在しない場合は `None` を返す。
    ///
    pub(crate) fn get_append_conflict_state_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<AppendConflictState>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let lock_table = txn.open_table(LOCK_INFO_TABLE)?;
        let now = Local::now();

        let index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Ok(None),
        };

        /*
         * draft / 通常ページの状態を組み立てる
         */
        if index.is_draft() {
            let locked = match find_lock_by_page(&lock_table, page_id)? {
                Some((_, info)) => info.expire() > now,
                None => false,
            };
            return Ok(Some(AppendConflictState::new(
                None,
                None,
                locked,
                true,
            )));
        }

        let latest_revision = index.latest();
        let latest_source = match source_table
            .get((page_id.clone(), latest_revision))?
        {
            Some(entry) => entry.value(),
            None => return Err(anyhow!("page source not found")),
        };
        let locked = match index.lock_token() {
            Some(token) => match lock_table.get(token)? {
                Some(info) => info.value().expire() > now,
                None => false,
            },
            None => false,
        };

        Ok(Some(AppendConflictState::new(
            Some(latest_revision),
            Some(latest_source.user()),
            locked,
            false,
        )))
    }

    ///
    /// 削除済みページ候補の取得
    ///
    /// # 引数
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 対象となるページIDの一覧を返す。
    ///
    pub(crate) fn get_deleted_page_ids_by_path(
        &self,
        path: &str,
    ) -> Result<Vec<PageId>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
        let key = path.to_string();
        let mut page_ids = Vec::new();

        for entry in table.get(key)? {
            page_ids.push(entry?.value());
        }

        Ok(page_ids)
    }

    ///
    /// page path からページ状態を解決
    ///
    /// # 引数
    /// * `path` - 正規化済み page path
    ///
    /// # 戻り値
    /// 通常ページ、draft、削除済み、未存在のいずれかの状態を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn resolve_page_state_by_path(
        &self,
        path: &str,
    ) -> Result<PagePathResolveState> {
        /*
         * current path としての解決
         */
        if let Some(page_id) = self.get_page_id_by_path(path)? {
            let page_index = match self.get_page_index_by_id(&page_id)? {
                Some(index) => index,
                None => return Err(anyhow!("page index not found")),
            };

            return Ok(PagePathResolveState::Current {
                page_id,
                draft: page_index.is_draft(),
            });
        }

        /*
         * 削除済みページの解決
         */
        if !self.get_deleted_page_ids_by_path(path)?.is_empty() {
            return Ok(PagePathResolveState::Deleted);
        }

        Ok(PagePathResolveState::NotFound)
    }
}

///
/// 指定パス配下の通常ページ一覧を収集
///
/// # 引数
/// * `path_table` - ページパステーブル
/// * `index_table` - ページインデックステーブル
/// * `source_table` - ページソーステーブル
/// * `user_table` - ユーザ情報テーブル
/// * `base_path` - 起点パス
/// * `entries` - 収集結果の格納先
///
/// # 戻り値
/// 収集に成功した場合は`Ok(())`を返す。
///
fn collect_page_list_entries<T1, T2, T3, T4>(
    path_table: &T1,
    index_table: &T2,
    source_table: &T3,
    user_table: &T4,
    base_path: &str,
    entries: &mut Vec<PageListEntry>,
) -> Result<()>
where
    T1: ReadableTable<String, PageId>,
    T2: ReadableTable<PageId, PageIndex>,
    T3: ReadableTable<(PageId, u64), PageSource>,
    T4: ReadableTable<UserId, UserInfo>,
{
    /*
     * レンジ走査の準備
     */
    let prefix = build_recursive_prefix(base_path);
    let mut iter = path_table.range(base_path.to_string()..)?;

    /*
     * 配下ページの収集
     */
    for entry in &mut iter {
        let (path, page_id) = entry?;
        let path = path.value();
        if path != base_path && !path.starts_with(&prefix) {
            break;
        }

        let page_id = page_id.value().clone();
        let index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!("page index not found")),
        };

        if index.is_draft() {
            continue;
        }

        if index.deleted() {
            continue;
        }

        let latest_revision = index.latest();
        let source = source_table
            .get((page_id.clone(), latest_revision))?
            .ok_or_else(|| anyhow!("page source not found"))?
            .value();
        let user_id = source.user();
        let user_info = user_table
            .get(user_id.clone())?
            .ok_or_else(|| anyhow!("user not found"))?
            .value();

        entries.push(PageListEntry::new(
            page_id,
            path.to_string(),
            latest_revision,
            source.timestamp(),
            user_info.username(),
            false,
            false,
            false,
        ));
    }

    Ok(())
}

///
/// 指定パス配下の削除済みページ一覧を収集
///
/// # 引数
/// * `deleted_table` - 削除済みページパステーブル
/// * `index_table` - ページインデックステーブル
/// * `source_table` - ページソーステーブル
/// * `user_table` - ユーザ情報テーブル
/// * `base_path` - 起点パス
/// * `entries` - 収集結果の格納先
///
/// # 戻り値
/// 収集に成功した場合は`Ok(())`を返す。
///
fn collect_deleted_page_list_entries<T1, T2, T3, T4>(
    deleted_table: &T1,
    index_table: &T2,
    source_table: &T3,
    user_table: &T4,
    base_path: &str,
    entries: &mut Vec<PageListEntry>,
) -> Result<()>
where
    T1: ReadableMultimapTable<String, PageId>,
    T2: ReadableTable<PageId, PageIndex>,
    T3: ReadableTable<(PageId, u64), PageSource>,
    T4: ReadableTable<UserId, UserInfo>,
{
    /*
     * レンジ走査の準備
     */
    let prefix = build_recursive_prefix(base_path);
    let mut iter = deleted_table.range(base_path.to_string()..)?;

    /*
     * 配下ページの収集
     */
    for entry in &mut iter {
        let (path, page_ids) = entry?;
        let path = path.value();
        if path != base_path && !path.starts_with(&prefix) {
            break;
        }

        for page_id in page_ids {
            let page_id = page_id?.value().clone();
            let index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!("page index not found")),
            };

            if index.is_draft() {
                continue;
            }

            if !index.deleted() {
                continue;
            }

            let latest_revision = index.latest();
            let source = source_table
                .get((page_id.clone(), latest_revision))?
                .ok_or_else(|| anyhow!("page source not found"))?
                .value();
            let user_id = source.user();
            let user_info = user_table
                .get(user_id.clone())?
                .ok_or_else(|| anyhow!("user not found"))?
                .value();

            entries.push(PageListEntry::new(
                page_id,
                path.to_string(),
                latest_revision,
                source.timestamp(),
                user_info.username(),
                true,
                false,
                false,
            ));
        }
    }

    Ok(())
}

fn build_recursive_prefix(base_path: &str) -> String {
    let base = base_path.trim_end_matches('/');
    format!("{}/", base)
}
