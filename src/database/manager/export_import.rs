/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export/import 用の低水準 DB API を提供するモジュール
//!

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use chrono::Local;
use redb::{ReadableDatabase, ReadableTable, Table};

use super::DatabaseManager;
use crate::database::schema::{
    ASSET_GROUP_TABLE,
    ASSET_INFO_TABLE,
    ASSET_LOOKUP_TABLE,
    DELETED_PAGE_PATH_TABLE,
    LOCK_INFO_TABLE,
    PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE,
    PAGE_SOURCE_TABLE,
    USER_ID_TABLE,
    USER_INFO_TABLE,
};
use crate::database::txn_helpers::{delete_draft_in_txn, delete_page_hard_in_txn};
use crate::database::types::{AssetId, AssetInfo, Id, PageId, PageIndex, PageSource, RenameInfo, UserInfo};
use crate::export_import::MigrateExportPageSnapshot;
use crate::export_import::model::{ExportBundle, ExportRevision, ExportRevisionRename};

///
/// export 用のページ収集結果
///
#[derive(Clone, Debug)]
pub(crate) struct ExportPageReadRecord {
    pub(crate) page_id: PageId,
    pub(crate) path: String,
    pub(crate) index: PageIndex,
}

///
/// export 用のリビジョン収集結果
///
#[derive(Clone, Debug)]
pub(crate) struct ExportRevisionReadRecord {
    pub(crate) page_id: PageId,
    pub(crate) revision: u64,
    pub(crate) source: PageSource,
}

///
/// export 用のアセット収集結果
///
#[derive(Clone, Debug)]
pub(crate) struct ExportAssetReadRecord {
    pub(crate) asset: AssetInfo,
    pub(crate) data: Vec<u8>,
}

///
/// export 用の低水準読取結果
///
#[derive(Clone, Debug)]
pub(crate) struct ExportReadSet {
    pub(crate) pages: Vec<ExportPageReadRecord>,
    pub(crate) revisions: Vec<ExportRevisionReadRecord>,
    pub(crate) users: Vec<UserInfo>,
    pub(crate) assets: Vec<ExportAssetReadRecord>,
    pub(crate) draft_page_ids: Vec<PageId>,
}

impl DatabaseManager {
    ///
    /// export 用の低水準読取
    ///
    /// # 概要
    /// 単一 read transaction 内で対象ページ、関連リビジョン、ユーザ、
    /// アセット、ドラフト削除対象をまとめて収集する。
    ///
    /// # 引数
    /// * `base_path` - 起点パス
    /// * `collect_drafts` - ドラフト削除対象も収集する場合は true
    ///
    /// # 戻り値
    /// 収集結果を返す。
    ///
    pub(crate) fn collect_export_read_set(
        &self,
        base_path: &str,
        collect_drafts: bool,
    ) -> Result<ExportReadSet> {
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
        let asset_table = txn.open_table(ASSET_INFO_TABLE)?;
        let mut pages = Vec::new();
        let mut revisions = Vec::new();
        let mut assets = Vec::new();
        let mut draft_page_ids = Vec::new();
        let mut user_ids = HashSet::new();

        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            let page_id = page_id.value().clone();
            let index = index.value();
            let path = match index.current_path() {
                Some(path) => path.to_string(),
                None => continue,
            };

            if !is_target_path(base_path, &path) {
                continue;
            }

            if index.is_draft() {
                if collect_drafts {
                    draft_page_ids.push(page_id);
                }
                continue;
            }

            if index.deleted() {
                continue;
            }

            pages.push(ExportPageReadRecord {
                page_id: page_id.clone(),
                path,
                index: index.clone(),
            });

            for source_entry in source_table
                .range((page_id.clone(), 0u64)..=(page_id.clone(), u64::MAX))?
            {
                let (key, source) = source_entry?;
                let (source_page_id, revision) = key.value();
                let source = source.value();
                user_ids.insert(source.user());
                revisions.push(ExportRevisionReadRecord {
                    page_id: source_page_id,
                    revision,
                    source,
                });
            }

            for asset_entry in group_table.get(page_id.clone())? {
                let asset_id = asset_entry?.value();
                let asset_info = match asset_table.get(asset_id.clone())? {
                    Some(info) => info.value(),
                    None => continue,
                };

                if asset_info.deleted() || asset_info.is_zombie() {
                    continue;
                }

                user_ids.insert(asset_info.user());
                assets.push(ExportAssetReadRecord {
                    data: self.read_asset_data(&asset_id)?,
                    asset: asset_info,
                });
            }
        }

        let mut users = Vec::new();
        for entry in user_table.iter()? {
            let (_, info) = entry?;
            let info = info.value();
            if user_ids.contains(&info.id()) {
                users.push(info);
            }
        }

        Ok(ExportReadSet {
            pages,
            revisions,
            users,
            assets,
            draft_page_ids,
        })
    }

    ///
    /// import 用の低水準 DB 投入
    ///
    /// # 概要
    /// 検証済み bundle を変換せずに DB 永続化型へ写像して一括反映する。
    ///
    /// # 引数
    /// * `bundle` - 反映対象 bundle
    ///
    /// # 戻り値
    /// 反映に成功した場合は `Ok(())` を返す。
    ///
    pub(crate) fn insert_import_bundle(
        &self,
        bundle: &ExportBundle,
    ) -> Result<()> {
        let revision_map = build_revision_map(&bundle.revisions);
        let txn = self.db.begin_write()?;

        {
            let mut user_id_table = txn.open_table(USER_ID_TABLE)?;
            let mut user_info_table = txn.open_table(USER_INFO_TABLE)?;
            let mut page_path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut page_index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut page_source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut asset_info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut asset_lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut asset_group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            for user in &bundle.users {
                let user_info = UserInfo::new_import(
                    user.id.clone(),
                    user.username.clone(),
                    user.password.clone(),
                    user.salt,
                    user.display_name.clone(),
                    crate::database::types::UserAttributeSet::new(),
                    Local::now(),
                );
                user_id_table.insert(user.username.clone(), user.id.clone())?;
                user_info_table.insert(user.id.clone(), user_info)?;
            }

            for page in &bundle.pages {
                let final_path = rebuild_absolute_path(
                    &bundle.manifest.export_root,
                    &page.path,
                );
                let rename_revisions =
                    page.rename_revisions.clone().unwrap_or_default();
                let page_index = PageIndex::new_page_import(
                    page.id.clone(),
                    final_path.clone(),
                    page.latest,
                    page.earliest,
                    rename_revisions,
                );

                page_path_table.insert(final_path, page.id.clone())?;
                page_index_table.insert(page.id.clone(), page_index)?;

                if let Some(revisions) = revision_map.get(&page.id) {
                    for revision in revisions {
                        let page_source = PageSource::new_import(
                            revision.revision,
                            Some(Id::new()),
                            revision.timestamp,
                            revision.user.clone(),
                            convert_export_rename(revision.rename.as_ref()),
                            revision.source.clone(),
                        );
                        page_source_table.insert(
                            (revision.page.clone(), revision.revision),
                            page_source,
                        )?;
                    }
                }
            }

            for asset in &bundle.assets {
                let asset_info = AssetInfo::new_import(
                    asset.id.clone(),
                    Some(Id::new()),
                    Some(asset.page.clone()),
                    asset.file_name.clone(),
                    asset.mime.clone(),
                    asset.size,
                    asset.user.clone(),
                    asset.timestamp,
                    false,
                );
                asset_info_table.insert(asset.id.clone(), asset_info)?;
                asset_lookup_table.insert(
                    (asset.page.clone(), asset.file_name.clone()),
                    asset.id.clone(),
                )?;
                let _ = asset_group_table.insert(asset.page.clone(), asset.id.clone())?;
            }
        }

        txn.commit()?;
        Ok(())
    }

    ///
    /// migrate export 用の削除連動
    ///
    /// # 概要
    /// 収集済みページ、ドラフト、ロック対象を単一成功条件で削除する。
    ///
    /// # 引数
    /// * `exported_page_ids` - export 済み通常ページID群
    /// * `draft_page_ids` - 同一サブツリー配下ドラフトID群
    /// * `lock_page_ids` - 削除対象ロックのページID群
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    pub(crate) fn delete_for_migrate_export(
        &self,
        base_path: &str,
        exported_pages: &[MigrateExportPageSnapshot],
        draft_page_ids: &[PageId],
        lock_page_ids: &[PageId],
    ) -> Result<()> {
        let txn = self.db.begin_write()?;
        let mut removed_asset_ids = Vec::new();

        {
            let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            verify_migrate_export_targets(
                &index_table,
                base_path,
                exported_pages,
                draft_page_ids,
            )?;
        }

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut page_index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut page_source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut asset_lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut asset_group_table =
                txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            for page in exported_pages {
                delete_page_hard_in_txn(
                    &page.page_id,
                    &mut path_table,
                    &mut deleted_path_table,
                    &mut page_index_table,
                    &mut page_source_table,
                    &mut lock_table,
                    &mut asset_info_table,
                    &mut asset_lookup_table,
                    &mut asset_group_table,
                    &mut removed_asset_ids,
                )?;
            }
        }

        {
            for page_id in draft_page_ids {
                removed_asset_ids.extend(delete_draft_in_txn(&txn, page_id)?);
            }
        }

        {
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            remove_locks_by_page_ids(&mut lock_table, lock_page_ids)?;
        }

        txn.commit()?;

        for asset_id in removed_asset_ids {
            let _ = fs::remove_file(self.asset_file_path(&asset_id));
        }

        Ok(())
    }

    ///
    /// アセット実体の一時配置
    ///
    /// # 引数
    /// * `asset_id` - 対象アセットID
    /// * `data` - アセットデータ
    ///
    /// # 戻り値
    /// 一時ファイルパスを返す。
    ///
    pub(crate) fn stage_asset_blob(
        &self,
        asset_id: &AssetId,
        data: &[u8],
    ) -> Result<PathBuf> {
        let staged_path = self.staged_asset_file_path(asset_id)?;
        if let Some(parent) = staged_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&staged_path, data)?;
        Ok(staged_path)
    }

    ///
    /// 一時配置済みアセットの確定配置
    ///
    /// # 引数
    /// * `staged_path` - 一時ファイルパス
    /// * `asset_id` - 対象アセットID
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    pub(crate) fn commit_staged_asset_blob<P>(
        &self,
        staged_path: P,
        asset_id: &AssetId,
    ) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let final_path = self.asset_file_path(asset_id);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(staged_path.as_ref(), final_path)?;
        Ok(())
    }

    ///
    /// 一時配置済みアセットの破棄
    ///
    /// # 引数
    /// * `staged_path` - 一時ファイルパス
    ///
    /// # 戻り値
    /// 成功時は `Ok(())` を返す。
    ///
    pub(crate) fn discard_staged_asset_blob<P>(
        &self,
        staged_path: P,
    ) -> Result<()>
    where
        P: AsRef<Path>,
    {
        match fs::remove_file(staged_path.as_ref()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    fn staged_asset_file_path(
        &self,
        asset_id: &AssetId,
    ) -> Result<PathBuf> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| anyhow!(err))?
            .as_nanos();
        Ok(
            self.asset_path
                .join(".staging")
                .join(format!("{}-{}.tmp", asset_id, unique)),
        )
    }
}

fn build_revision_map(
    revisions: &[ExportRevision],
) -> BTreeMap<PageId, Vec<&ExportRevision>> {
    let mut map = BTreeMap::new();
    for revision in revisions {
        map.entry(revision.page.clone())
            .or_insert_with(Vec::new)
            .push(revision);
    }
    map
}

fn convert_export_rename(
    rename: Option<&ExportRevisionRename>,
) -> RenameInfo {
    match rename {
        None => RenameInfo::none(),
        Some(ExportRevisionRename::Active(active)) => RenameInfo::new(
            active.from.clone(),
            active.to.clone(),
            active.link_refs.clone(),
        ),
        Some(ExportRevisionRename::RemovedByMigrate(_)) => {
            RenameInfo::removed_by_migrate()
        }
    }
}

fn verify_migrate_export_targets(
    index_table: &Table<'_, PageId, PageIndex>,
    base_path: &str,
    exported_pages: &[MigrateExportPageSnapshot],
    draft_page_ids: &[PageId],
) -> Result<()> {
    let expected_pages: BTreeMap<PageId, (&str, u64)> = exported_pages
        .iter()
        .map(|page| (page.page_id.clone(), (page.path.as_str(), page.latest)))
        .collect();
    let expected_drafts: BTreeSet<PageId> =
        draft_page_ids.iter().cloned().collect();
    let mut actual_pages = BTreeMap::new();
    let mut actual_drafts = BTreeSet::new();

    for entry in index_table.iter()? {
        let (page_id, index) = entry?;
        let page_id = page_id.value().clone();
        let index = index.value();
        let Some(path) = index.current_path() else {
            continue;
        };
        if !is_target_path(base_path, path) {
            continue;
        }

        if index.is_draft() {
            actual_drafts.insert(page_id);
            continue;
        }

        if index.deleted() {
            continue;
        }

        actual_pages.insert(page_id, (path.to_string(), index.latest()));
    }

    if actual_pages.len() != expected_pages.len() {
        return Err(anyhow!(
            "migrate export target pages changed before delete"
        ));
    }

    for (page_id, (expected_path, expected_latest)) in expected_pages {
        let Some((actual_path, actual_latest)) = actual_pages.get(&page_id) else {
            return Err(anyhow!(
                "migrate export target page disappeared before delete: page_id={}",
                page_id
            ));
        };

        if actual_path != expected_path || *actual_latest != expected_latest {
            return Err(anyhow!(
                "migrate export target page changed before delete: page_id={}",
                page_id
            ));
        }
    }

    if actual_drafts != expected_drafts {
        return Err(anyhow!(
            "migrate export target drafts changed before delete"
        ));
    }

    Ok(())
}

fn rebuild_absolute_path(export_root: &str, rel_path: &str) -> String {
    if export_root == "/" {
        if rel_path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", rel_path.trim_start_matches('/'))
        }
    } else if rel_path.is_empty() {
        export_root.to_string()
    } else {
        format!(
            "{}/{}",
            export_root.trim_end_matches('/'),
            rel_path.trim_start_matches('/'),
        )
    }
}

fn remove_locks_by_page_ids(
    lock_table: &mut Table<
        '_,
        crate::database::types::LockToken,
        crate::database::types::LockInfo,
    >,
    page_ids: &[PageId],
) -> Result<()> {
    let target_ids: HashSet<PageId> = page_ids.iter().cloned().collect();
    let mut tokens = Vec::new();
    for entry in lock_table.iter()? {
        let (token, info) = entry?;
        if target_ids.contains(&info.value().page()) {
            tokens.push(token.value().clone());
        }
    }
    for token in tokens {
        let _ = lock_table.remove(token)?;
    }
    Ok(())
}

fn is_target_path(base_path: &str, path: &str) -> bool {
    if base_path == "/" {
        return path.starts_with('/');
    }

    if path == base_path {
        return true;
    }

    let prefix = format!("{}/", base_path.trim_end_matches('/'));
    path.starts_with(&prefix)
}
