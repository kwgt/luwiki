/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! resource候補派生データの操作を提供するモジュール
//!

use std::collections::HashSet;

#[cfg(test)]
use anyhow::anyhow;
use anyhow::Result;
use redb::{ReadableDatabase, ReadableTable, WriteTransaction};

use super::DatabaseManager;
use crate::database::entries::{
    ResourceCandidateListEntry,
    ResourceSourceEntry,
    ResourceSourceLookupResult,
};
use crate::database::resource_candidates::{
    build_resource_candidate_entry_from_source,
};
use crate::database::resource_list::DEFAULT_RESOURCE_MIME_TYPE;
use crate::database::resource_uris::{
    RESOURCE_URI_INDEX_STATE_KEY,
    RESOURCE_URI_INDEX_STATE_VERSION,
    replace_resource_uris_in_txn,
};
use crate::database::schema::{
    DbError,
    PAGE_INDEX_TABLE,
    PAGE_SOURCE_TABLE,
    RESOURCE_CANDIDATE_TABLE,
    RESOURCE_URI_INDEX_TABLE,
    RESOURCE_URI_INDEX_STATE_TABLE,
};
use crate::database::types::{
    PageId,
    ResourceCandidateEntry,
};
#[cfg(test)]
use crate::database::types::{
    PageSource,
    RenameInfo,
    UserId,
};

impl DatabaseManager {
    ///
    /// resource URI逆引き索引が利用可能か確認する
    ///
    /// # 戻り値
    /// 対応versionの構築済みマーカーが存在する場合は
    /// `Ok(true)`を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_resource_uri_index_ready(&self) -> Result<bool> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RESOURCE_URI_INDEX_STATE_TABLE)?;

        Ok(match table.get(RESOURCE_URI_INDEX_STATE_KEY)? {
            Some(state) => state.value() == RESOURCE_URI_INDEX_STATE_VERSION,
            None => false,
        })
    }

    ///
    /// ページIDに対応するresource候補派生データを取得する
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// 候補が存在する場合は `Ok(Some(...))` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn get_resource_candidate_by_page_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<ResourceCandidateEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RESOURCE_CANDIDATE_TABLE)?;

        Ok(match table.get(page_id.clone())? {
            Some(entry) => Some(entry.value()),
            None => None,
        })
    }

    ///
    /// 単一ページのresource候補派生データを最新ソースから同期する
    ///
    /// # 引数
    /// * `page_id` - 同期対象ページID
    ///
    /// # 戻り値
    /// 同期後の候補が存在する場合は `Ok(Some(...))` を返す。
    /// 通常ページまたは draft 等で候補を持たない場合は `Ok(None)` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn sync_resource_candidate_for_page(
        &self,
        page_id: &PageId,
    ) -> Result<Option<ResourceCandidateEntry>> {
        /*
         * 最新ソースから候補生成可否を判定する
         */
        let next_entry = {
            let txn = self.db.begin_read()?;
            let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;

            let index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Ok(None),
            };

            if index.is_draft() {
                None
            } else {
                let revision = index.latest();
                match source_table.get((page_id.clone(), revision))? {
                    Some(entry) => {
                        let source = entry.value().source();
                        let current_path = index.path();
                        build_resource_candidate_entry_from_source(
                            &current_path,
                            &source,
                        )?
                    }
                    None => None,
                }
            }
        };

        #[cfg(test)]
        if self.resource_candidate_sync_failure_for_test() {
            return Err(anyhow!("resource candidate sync failure for test"));
        }

        /*
         * 候補テーブルへ upsert / remove を反映する
         */
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(RESOURCE_CANDIDATE_TABLE)?;
            match &next_entry {
                Some(entry) => {
                    table.insert(page_id.clone(), entry.clone())?;
                }
                None => {
                    let _ = table.remove(page_id.clone())?;
                }
            }
        }
        txn.commit()?;

        Ok(next_entry)
    }

    ///
    /// 複数ページのresource候補を最新ソースから同期する
    ///
    /// # 引数
    /// * `page_ids` - 同期対象ページID一覧
    ///
    /// # 戻り値
    /// 全ページの同期に成功した場合は `Ok(())` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn sync_resource_candidates_for_page_ids(
        &self,
        page_ids: &[PageId],
    ) -> Result<()> {
        for page_id in page_ids {
            self.sync_resource_candidate_for_page(page_id)?;
        }

        Ok(())
    }

    ///
    /// 単一ページのresource候補派生データを除去する
    ///
    /// # 引数
    /// * `page_id` - 除去対象ページID
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn remove_resource_candidate_by_page_id(
        &self,
        page_id: &PageId,
    ) -> Result<()> {
        self.remove_resource_candidates_by_page_ids(&[page_id.clone()])
    }

    ///
    /// 複数ページのresource候補派生データを除去する
    ///
    /// # 引数
    /// * `page_ids` - 除去対象ページID一覧
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn remove_resource_candidates_by_page_ids(
        &self,
        page_ids: &[PageId],
    ) -> Result<()> {
        if page_ids.is_empty() {
            return Ok(());
        }

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(RESOURCE_CANDIDATE_TABLE)?;
            for page_id in page_ids {
                let _ = table.remove(page_id.clone())?;
            }
        }
        txn.commit()?;

        Ok(())
    }

    ///
    /// 最新ページ状態と合流済みのresource候補一覧を取得する
    ///
    /// # 戻り値
    /// 削除済みページおよびdraftを除外した候補一覧を返す。
    ///
    pub(crate) fn list_resource_candidates(
        &self,
    ) -> Result<Vec<ResourceCandidateListEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(RESOURCE_CANDIDATE_TABLE)?;
        let mut raw_entries = Vec::new();
        let mut page_ids = Vec::new();

        /*
         * resource候補とページIDを収集する
         */
        for entry in table.iter()? {
            let (page_id, candidate) = entry?;
            let page_id = page_id.value().clone();
            page_ids.push(page_id.clone());
            raw_entries.push((page_id, candidate.value()));
        }

        drop(table);
        drop(txn);

        /*
         * 最新ページ状態と合流して公開可能な候補へ変換する
         */
        let current_paths = self.get_current_page_paths_by_ids(&page_ids)?;
        let mut entries = Vec::new();
        for (page_id, candidate) in raw_entries {
            let Some(path_info) = current_paths.get(&page_id) else {
                continue;
            };
            if path_info.deleted() || path_info.draft() {
                continue;
            }
            let mime_type = candidate
                .mime_type()
                .unwrap_or(DEFAULT_RESOURCE_MIME_TYPE)
                .to_string();

            entries.push(ResourceCandidateListEntry::new(
                page_id,
                path_info.current_path().to_string(),
                candidate.resource_path().to_string(),
                candidate.name().to_string(),
                candidate.description().to_string(),
                mime_type,
            ));
        }

        Ok(entries)
    }

    ///
    /// resource_pathから公開可能な最新ページソースを取得する
    ///
    /// # 引数
    /// * `resource_path` - resource path
    ///
    /// # 戻り値
    /// URI索引と最新ページ状態の解決結果を分類して返す。
    ///
    pub(crate) fn get_resource_source_by_path(
        &self,
        resource_path: &str,
    ) -> Result<ResourceSourceLookupResult> {
        let txn = self.db.begin_read()?;
        let uri_table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;

        /*
         * URI索引から公開可能な最新ページ状態を解決する
         */
        let page_id = match uri_table.get(resource_path.to_string())? {
            Some(owner) => owner.value(),
            None => return Ok(ResourceSourceLookupResult::NotFound),
        };
        let index = match index_table.get(page_id.clone())? {
            Some(index) => index.value(),
            None => return Ok(ResourceSourceLookupResult::Inconsistent),
        };
        if index.is_draft() || index.deleted() {
            return Ok(ResourceSourceLookupResult::Unavailable);
        }
        let current_path = match index.current_path() {
            Some(path) => path.to_string(),
            None => return Ok(ResourceSourceLookupResult::Inconsistent),
        };

        /*
         * 同じread transactionから最新ソースを取得する
         */
        let revision = index.latest();
        let source = match source_table
            .get((page_id.clone(), revision))?
        {
            Some(source) => source.value().source(),
            None => return Ok(ResourceSourceLookupResult::Inconsistent),
        };

        Ok(ResourceSourceLookupResult::Found(ResourceSourceEntry::new(
            current_path,
            revision,
            source,
        )))
    }

    ///
    /// 全ページの最新ソースからresource派生データを再構成する
    ///
    /// # 戻り値
    /// 再構成後に投入されたresource候補件数を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn rebuild_resource_candidates(&self) -> Result<usize> {
        let txn = self.db.begin_write()?;
        let count = rebuild_resource_candidates_in_txn(&txn)?;
        txn.commit()?;

        Ok(count)
    }

    ///
    /// テスト用にresource候補を直接投入する
    ///
    /// # 引数
    /// * `page_id` - 投入対象ページID
    /// * `candidate` - resource候補
    ///
    /// # 戻り値
    /// 投入に成功した場合は`Ok(())`を返す。
    ///
    #[cfg(test)]
    pub(crate) fn insert_resource_candidate_for_test(
        &self,
        page_id: &PageId,
        candidate: &ResourceCandidateEntry,
    ) -> Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(RESOURCE_CANDIDATE_TABLE)?;
            table.insert(page_id.clone(), candidate.clone())?;
        }
        txn.commit()?;

        Ok(())
    }

    ///
    /// テスト用にページのlatest sourceだけを差し替える
    ///
    /// # 引数
    /// * `page_id` - 差し替え対象ページID
    /// * `source` - 差し替え後のページソース
    ///
    /// # 戻り値
    /// latest sourceの差し替えに成功した場合は
    /// `Ok(())`を返す。
    ///
    #[cfg(test)]
    pub(crate) fn replace_latest_page_source_for_resource_rebuild_test(
        &self,
        page_id: &PageId,
        source: String,
    ) -> Result<()> {
        let txn = self.db.begin_write()?;
        let revision = {
            let table = txn.open_table(PAGE_INDEX_TABLE)?;
            let index = table
                .get(page_id.clone())?
                .ok_or_else(|| anyhow!("page index missing"))?
                .value();
            index.latest()
        };
        {
            let mut table = txn.open_table(PAGE_SOURCE_TABLE)?;
            table.insert(
                (page_id.clone(), revision),
                PageSource::new_revision(
                    revision,
                    source,
                    UserId::new(),
                    RenameInfo::none(),
                ),
            )?;
        }
        txn.commit()?;

        Ok(())
    }
}

///
/// resource再構成用の検証済みデータ
///
pub(in crate::database) struct ResourceRebuildData {
    candidates: Vec<(PageId, ResourceCandidateEntry)>,
    uri_entries: Vec<(String, PageId)>,
}

impl ResourceRebuildData {
    ///
    /// resource候補件数を返す
    ///
    /// # 戻り値
    /// 検証済みのresource候補件数を返す。
    ///
    pub(in crate::database) fn len(&self) -> usize {
        self.candidates.len()
    }
}

///
/// write transaction内でresource派生データを収集・検証する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
///
/// # 戻り値
/// 検証済みのresource候補とURI索引エントリを返す。
///
pub(in crate::database) fn collect_resource_candidates_in_txn(
    txn: &WriteTransaction,
) -> Result<ResourceRebuildData> {
    let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
    let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
    let mut candidates = Vec::new();
    let mut uri_entries = Vec::new();
    let mut resource_paths = HashSet::new();

    /*
     * 最新ページソースから候補とURI索引を生成する
     */
    for entry in index_table.iter()? {
        let (page_id, index) = entry?;
        let page_id = page_id.value();
        let index = index.value();
        if index.is_draft() {
            continue;
        }
        let source = source_table
            .get((page_id.clone(), index.latest()))?
            .ok_or_else(|| anyhow::anyhow!("latest page source missing"))?
            .value()
            .source();
        let Some(candidate) = build_resource_candidate_entry_from_source(
            &index.path(),
            &source,
        )? else {
            continue;
        };
        let resource_path = candidate.resource_path().to_string();
        if !resource_paths.insert(resource_path.clone()) {
            return Err(anyhow::anyhow!(DbError::ResourceUriAlreadyExists {
                resource_path,
            }));
        }
        uri_entries.push((resource_path, page_id.clone()));
        candidates.push((page_id, candidate));
    }

    Ok(ResourceRebuildData {
        candidates,
        uri_entries,
    })
}

///
/// write transaction内でresource派生データを置換する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
/// * `data` - 検証済みのresource候補とURI索引
///
/// # 戻り値
/// 置換に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn replace_resource_candidates_in_txn(
    txn: &WriteTransaction,
    data: &ResourceRebuildData,
) -> Result<()> {
    let mut table = txn.open_table(RESOURCE_CANDIDATE_TABLE)?;
    let mut existing_page_ids = Vec::new();
    for entry in table.iter()? {
        existing_page_ids.push(entry?.0.value());
    }
    for page_id in existing_page_ids {
        let _ = table.remove(page_id)?;
    }
    for (page_id, candidate) in &data.candidates {
        table.insert(page_id.clone(), candidate.clone())?;
    }
    drop(table);
    replace_resource_uris_in_txn(txn, &data.uri_entries)?;

    Ok(())
}

///
/// write transaction内でresource派生データを再構成する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
///
/// # 戻り値
/// 再構成後に投入されたresource候補件数を返す。
///
pub(in crate::database) fn rebuild_resource_candidates_in_txn(
    txn: &WriteTransaction,
) -> Result<usize> {
    let data = collect_resource_candidates_in_txn(txn)?;
    let count = data.len();
    replace_resource_candidates_in_txn(txn, &data)?;

    Ok(count)
}
