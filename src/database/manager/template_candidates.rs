/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! テンプレート候補派生データの操作を提供するモジュール
//!

use anyhow::{Result, anyhow};
use redb::{ReadableDatabase, ReadableTable, WriteTransaction};

use super::DatabaseManager;
use crate::database::entries::TemplateCandidateListEntry;
use crate::database::schema::{
    PAGE_INDEX_TABLE,
    PAGE_SOURCE_TABLE,
    TEMPLATE_CANDIDATE_TABLE,
};
use crate::database::template_candidates::{
    build_legacy_template_candidate_entry,
    build_template_candidate_entry_from_source,
    is_direct_child_template_path,
};
use crate::database::types::{
    PageId,
    TemplateCandidateEntry,
    TemplateCandidateSource,
};

impl DatabaseManager {
    ///
    /// ページIDに対応するテンプレート候補派生データを取得する
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// 候補が存在する場合は `Ok(Some(...))` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn get_template_candidate_by_page_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<TemplateCandidateEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(TEMPLATE_CANDIDATE_TABLE)?;

        Ok(match table.get(page_id.clone())? {
            Some(entry) => Some(entry.value()),
            None => None,
        })
    }

    ///
    /// 単一ページのテンプレート候補派生データを最新ソースから同期する
    ///
    /// # 引数
    /// * `page_id` - 同期対象ページID
    ///
    /// # 戻り値
    /// 同期後の候補が存在する場合は `Ok(Some(...))` を返す。
    /// 通常ページまたは draft 等で候補を持たない場合は `Ok(None)` を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn sync_template_candidate_for_page(
        &self,
        page_id: &PageId,
    ) -> Result<Option<TemplateCandidateEntry>> {
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
                let source = match source_table.get((page_id.clone(), revision))? {
                    Some(entry) => Some(entry.value()),
                    None => None,
                };
                match source {
                    Some(source) => build_template_candidate_entry_from_source(
                        &source.source(),
                    )?,
                    None => None,
                }
            }
        };

        /*
         * 候補テーブルへ upsert / remove を反映する
         */
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(TEMPLATE_CANDIDATE_TABLE)?;
            let existing = table
                .get(page_id.clone())?
                .map(|entry| entry.value());
            match &next_entry {
                Some(entry) => {
                    table.insert(page_id.clone(), entry.clone())?;
                }
                None => {
                    let should_remove = match existing {
                        Some(entry) => matches!(
                            entry.source(),
                            TemplateCandidateSource::FrontMatter
                        ),
                        None => true,
                    };
                    if should_remove {
                        let _ = table.remove(page_id.clone())?;
                    }
                }
            }
        }
        txn.commit()?;

        Ok(next_entry)
    }

    ///
    /// 単一ページのテンプレート候補派生データを除去する
    ///
    /// # 引数
    /// * `page_id` - 除去対象ページID
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    pub(crate) fn remove_template_candidate_by_page_id(
        &self,
        page_id: &PageId,
    ) -> Result<()> {
        self.remove_template_candidates_by_page_ids(&[page_id.clone()])
    }

    ///
    /// 複数ページのテンプレート候補派生データを除去する
    ///
    /// # 引数
    /// * `page_ids` - 除去対象ページID一覧
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    pub(crate) fn remove_template_candidates_by_page_ids(
        &self,
        page_ids: &[PageId],
    ) -> Result<()> {
        if page_ids.is_empty() {
            return Ok(());
        }

        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(TEMPLATE_CANDIDATE_TABLE)?;
            for page_id in page_ids {
                let _ = table.remove(page_id.clone())?;
            }
        }
        txn.commit()?;

        Ok(())
    }

    ///
    /// current path と合流済みのテンプレート候補一覧を取得する
    ///
    /// # 戻り値
    /// 削除済みページおよび draft を除外した候補一覧を返す。
    ///
    pub(crate) fn list_template_candidates(
        &self,
    ) -> Result<Vec<TemplateCandidateListEntry>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(TEMPLATE_CANDIDATE_TABLE)?;
        let mut raw_entries = Vec::new();
        let mut page_ids = Vec::new();

        for entry in table.iter()? {
            let (page_id, candidate) = entry?;
            let page_id = page_id.value().clone();
            page_ids.push(page_id.clone());
            raw_entries.push((page_id, candidate.value()));
        }

        drop(table);
        drop(txn);

        let current_paths = self.get_current_page_paths_by_ids(&page_ids)?;
        let mut entries = Vec::new();
        for (page_id, candidate) in raw_entries {
            let Some(path_info) = current_paths.get(&page_id) else {
                continue;
            };
            if path_info.deleted() || path_info.draft() {
                continue;
            }

            entries.push(TemplateCandidateListEntry::new(
                page_id,
                path_info.current_path().to_string(),
                candidate.name().to_string(),
                candidate.description().map(str::to_string),
                candidate.macro_expand(),
            ));
        }

        Ok(entries)
    }

    ///
    /// 全ページの最新ソースからテンプレート候補テーブルを再構成する
    ///
    /// # 戻り値
    /// 再構成後に投入された候補件数を返す。
    ///
    pub(crate) fn rebuild_template_candidates(&self) -> Result<usize> {
        self.rebuild_template_candidates_with_legacy(None)
    }

    ///
    /// 全ページの最新ソースからテンプレート候補テーブルを再構成する
    ///
    /// # 引数
    /// * `template_root` - legacy 候補取り込み元
    ///
    /// # 戻り値
    /// 再構成後に投入された候補件数を返す。
    ///
    pub(crate) fn rebuild_template_candidates_with_legacy(
        &self,
        template_root: Option<&str>,
    ) -> Result<usize> {
        let txn = self.db.begin_write()?;
        let count =
            rebuild_template_candidates_in_txn(&txn, template_root)?;
        txn.commit()?;

        Ok(count)
    }
}

///
/// 再構成用の検証済みtemplate候補一覧
///
pub(in crate::database) type TemplateRebuildEntries =
    Vec<(PageId, TemplateCandidateEntry)>;

///
/// write transaction内でtemplate候補を収集・検証する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
/// * `template_root` - legacy候補取り込み元
///
/// # 戻り値
/// 検証済みのtemplate候補一覧を返す。
///
pub(in crate::database) fn collect_template_candidates_in_txn(
    txn: &WriteTransaction,
    template_root: Option<&str>,
) -> Result<TemplateRebuildEntries> {
    let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
    let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
    let mut entries = Vec::new();

    /*
     * 最新ソースから候補を再抽出する
     */
    for entry in index_table.iter()? {
        let (page_id, index) = entry?;
        let page_id = page_id.value();
        let index = index.value();
        if index.is_draft() {
            continue;
        }

        let page_path = index.path();
        let source = source_table
            .get((page_id.clone(), index.latest()))?
            .ok_or_else(|| anyhow!("latest page source missing"))?
            .value()
            .source();
        let candidate =
            build_template_candidate_entry_from_source(&source)?;
        if let Some(candidate) = candidate {
            entries.push((page_id, candidate));
            continue;
        }

        if let Some(template_root) = template_root {
            if is_direct_child_template_path(template_root, &page_path) {
                entries.push((
                    page_id,
                    build_legacy_template_candidate_entry(&page_path),
                ));
            }
        }
    }

    Ok(entries)
}

///
/// write transaction内でtemplate候補テーブルを置換する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
/// * `entries` - 検証済みのtemplate候補一覧
///
/// # 戻り値
/// 置換に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn replace_template_candidates_in_txn(
    txn: &WriteTransaction,
    entries: &TemplateRebuildEntries,
) -> Result<()> {
    let mut table = txn.open_table(TEMPLATE_CANDIDATE_TABLE)?;
    let mut existing_page_ids = Vec::new();
    for entry in table.iter()? {
        existing_page_ids.push(entry?.0.value());
    }
    for page_id in existing_page_ids {
        let _ = table.remove(page_id)?;
    }
    for (page_id, candidate) in entries {
        table.insert(page_id.clone(), candidate.clone())?;
    }

    Ok(())
}

///
/// write transaction内でtemplate候補を再構成する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
/// * `template_root` - legacy候補取り込み元
///
/// # 戻り値
/// 再構成後のtemplate候補件数を返す。
///
pub(in crate::database) fn rebuild_template_candidates_in_txn(
    txn: &WriteTransaction,
    template_root: Option<&str>,
) -> Result<usize> {
    let entries =
        collect_template_candidates_in_txn(txn, template_root)?;
    replace_template_candidates_in_txn(txn, &entries)?;

    Ok(entries.len())
}
