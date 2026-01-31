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

use crate::database::entries::{
    PageIndexEntry, PageListEntry, PageSourceEntry,
};
use crate::database::schema::{
    DELETED_PAGE_PATH_TABLE, LOCK_INFO_TABLE, PAGE_INDEX_TABLE, PAGE_PATH_TABLE,
    PAGE_SOURCE_TABLE, USER_INFO_TABLE,
};
use crate::database::types::{
    PageId, PageIndex, PageSource, UserId, UserInfo,
};
use super::DatabaseManager;

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
        Ok(table.get((page_id.clone(), revision))?.map(|entry| entry.value()))
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
            let deleted_table = txn.open_multimap_table(
                DELETED_PAGE_PATH_TABLE,
            )?;
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
}

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
    let prefix = build_recursive_prefix(base_path);
    let mut iter = path_table.range(base_path.to_string()..)?;

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
    let prefix = build_recursive_prefix(base_path);
    let mut iter = deleted_table.range(base_path.to_string()..)?;

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
