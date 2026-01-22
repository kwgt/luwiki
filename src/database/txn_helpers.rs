/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! トランザクション内の共通処理を集約するモジュール
//!

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use redb::{
    MultimapTable, ReadableMultimapTable, ReadableTable, Table,
};

use crate::database::types::{
    AssetId, AssetInfo, LockInfo, LockToken, PageId, PageIndex, PageSource,
};

use super::schema::{
    is_root_path, ASSET_GROUP_TABLE, ASSET_INFO_TABLE, ASSET_LOOKUP_TABLE,
    DbError, PAGE_INDEX_TABLE, PAGE_PATH_TABLE,
};

///
/// 再帰処理対象のページ情報
///
pub(in crate::database) struct RecursivePageTarget {
    pub(in crate::database) page_id: PageId,
    pub(in crate::database) path: String,
}

///
/// ページIDからロック情報を探索する
///
/// # 引数
/// * `lock_table` - ロック情報テーブル
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// ロック情報を取得できた場合はOk(Some((LockToken, LockInfo)))を返す。
/// ロック情報が存在しない場合はOk(None)を返す。
///
pub(in crate::database) fn find_lock_by_page<T>(
    lock_table: &T,
    page_id: &PageId,
) -> Result<Option<(LockToken, LockInfo)>>
where
    T: ReadableTable<LockToken, LockInfo>,
{
    for entry in lock_table.iter()? {
        let (token, info) = entry?;
        let info = info.value();
        if &info.page() == page_id {
            return Ok(Some((token.value().clone(), info)));
        }
    }

    Ok(None)
}

///
/// ドラフトページの削除(トランザクション内部処理)
///
/// # 概要
/// ドラフトページと紐付くテーブル情報を削除し、アセットIDを返す。
///
/// # 引数
/// * `txn` - 書き込みトランザクション
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// 削除したアセットID一覧を返す。
///
pub(in crate::database) fn delete_draft_in_txn(
    txn: &redb::WriteTransaction,
    page_id: &PageId,
) -> Result<Vec<AssetId>> {
    /*
     * テーブルの準備
     */
    let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
    let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
    let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
    let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
    let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
    let mut asset_ids = Vec::new();

    /*
     * ページ情報の取得と検証
     */
    let index = match index_table.get(page_id.clone())? {
        Some(entry) => entry.value(),
        None => return Err(anyhow!(DbError::PageNotFound)),
    };

    if !index.is_draft() {
        return Err(anyhow!(DbError::PageNotFound));
    }

    if is_root_path(&index.path()) {
        return Err(anyhow!(DbError::RootPageProtected));
    }

    /*
     * 付随アセットの削除
     */
    for entry in group_table.remove_all(page_id.clone())? {
        let asset_id = entry?.value();
        let file_name = match asset_table.get(asset_id.clone())? {
            Some(info_guard) => info_guard.value().file_name(),
            None => {
                asset_ids.push(asset_id);
                continue;
            }
        };
        let _ = lookup_table.remove((page_id.clone(), file_name));
        let _ = asset_table.remove(asset_id.clone());
        asset_ids.push(asset_id);
    }

    /*
     * ページインデックスの削除
     */
    let _ = path_table.remove(&index.path());
    let _ = index_table.remove(page_id.clone());

    Ok(asset_ids)
}

///
/// ロック状態の検証と期限切れロックの掃除
///
/// # 概要
/// ロックが有効ならPageLockedを返し、期限切れ・欠落ロックは解消する。
///
/// # 引数
/// * `page_id` - 対象ページID
/// * `index` - ページインデックス
/// * `index_table` - ページインデックステーブル
/// * `lock_table` - ロック情報テーブル
/// * `now` - 判定基準時刻
///
/// # 戻り値
/// ロックが問題ない場合は`Ok(())`を返す。
///
pub(in crate::database) fn verify_page_lock_in_txn<'txn>(
    page_id: &PageId,
    index: &mut PageIndex,
    index_table: &mut Table<'txn, PageId, PageIndex>,
    lock_table: &mut Table<'txn, LockToken, LockInfo>,
    now: &DateTime<Local>,
) -> Result<()> {
    if let Some(token) = index.lock_token() {
        let mut remove_lock = false;
        if let Some(info) = lock_table.get(token.clone())? {
            let info = info.value();
            if info.expire() > *now {
                return Err(anyhow!(DbError::PageLocked));
            }
            remove_lock = true;
        }
        if remove_lock {
            lock_table.remove(token.clone())?;
        }
        index.set_lock_token(None);
        index_table.insert(page_id.clone(), index.clone())?;
    }

    Ok(())
}

///
/// 再帰削除対象のページID一覧を収集する
///
/// # 概要
/// ページパスインデックスのレンジイテレータを用いて指定パス配下のみを
/// 走査し、ドラフトやロックページが存在する場合はエラーとする。
///
/// # 引数
/// * `path_table` - ページパスインデックステーブル
/// * `index_table` - ページインデックステーブル
/// * `lock_table` - ロック情報テーブル
/// * `base_path` - 起点パス
///
/// # 戻り値
/// 対象ページIDの一覧を返す。
///
pub(in crate::database) fn collect_recursive_page_ids_in_txn<'txn>(
    path_table: &mut Table<'txn, String, PageId>,
    index_table: &mut Table<'txn, PageId, PageIndex>,
    lock_table: &mut Table<'txn, LockToken, LockInfo>,
    base_path: &str,
) -> Result<Vec<PageId>> {
    /*
     * 事前情報の準備
     */
    let prefix = build_recursive_prefix(base_path);
    let now = Local::now();
    let mut targets = Vec::new();

    /*
     * 配下ページの収集とロック検証
     */
    let mut iter = path_table.range(base_path.to_string()..)?;
    for entry in &mut iter {
        let (path, page_id) = entry?;
        let path = path.value();
        if path != base_path && !path.starts_with(&prefix) {
            break;
        }

        let page_id = page_id.value().clone();
        let mut index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!(DbError::PageNotFound)),
        };

        if index.is_draft() {
            return Err(anyhow!(DbError::PageLocked));
        }

        if index.deleted() {
            return Err(anyhow!("page already deleted"));
        }

        verify_page_lock_in_txn(
            &page_id,
            &mut index,
            index_table,
            lock_table,
            &now,
        )?;

        targets.push(page_id);
    }

    Ok(targets)
}

///
/// 再帰対象ページのパスとIDを収集する
///
/// # 概要
/// 配下ページのパスとIDを収集し、ドラフトやロック中が含まれる場合はエラー
/// を返す。
///
/// # 引数
/// * `path_table` - ページパスインデックステーブル
/// * `index_table` - ページインデックステーブル
/// * `lock_table` - ロック情報テーブル
/// * `base_path` - 起点パス
///
/// # 戻り値
/// 対象ページの一覧を返す。
///
pub(in crate::database) fn collect_recursive_page_targets_in_txn<'txn>(
    path_table: &mut Table<'txn, String, PageId>,
    index_table: &mut Table<'txn, PageId, PageIndex>,
    lock_table: &mut Table<'txn, LockToken, LockInfo>,
    base_path: &str,
) -> Result<Vec<RecursivePageTarget>> {
    /*
     * 事前情報の準備
     */
    let prefix = build_recursive_prefix(base_path);
    let now = Local::now();
    let mut targets = Vec::new();

    /*
     * 配下ページの収集とロック検証
     */
    let mut iter = path_table.range(base_path.to_string()..)?;
    for entry in &mut iter {
        let (path, page_id) = entry?;
        let path = path.value();
        if path != base_path && !path.starts_with(&prefix) {
            break;
        }

        let page_id = page_id.value().clone();
        let mut index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!(DbError::PageNotFound)),
        };

        if index.is_draft() {
            return Err(anyhow!(DbError::PageLocked));
        }

        if index.deleted() {
            return Err(anyhow!("page already deleted"));
        }

        verify_page_lock_in_txn(
            &page_id,
            &mut index,
            index_table,
            lock_table,
            &now,
        )?;

        targets.push(RecursivePageTarget {
            page_id,
            path: path.to_string(),
        });
    }

    Ok(targets)
}

///
/// 削除済みページの再帰対象を収集する
///
/// # 概要
/// 削除済みページパスのレンジ走査で対象パス配下を収集し、
/// ロックや未削除が混在している場合はエラーを返す。
///
/// # 引数
/// * `deleted_path_table` - 削除済みページパスインデックステーブル
/// * `index_table` - ページインデックステーブル
/// * `lock_table` - ロック情報テーブル
/// * `base_path` - 起点パス
///
/// # 戻り値
/// 対象ページの一覧を返す。
///
pub(in crate::database) fn collect_recursive_deleted_page_targets_in_txn<'txn>(
    deleted_path_table: &mut MultimapTable<'txn, String, PageId>,
    index_table: &mut Table<'txn, PageId, PageIndex>,
    lock_table: &mut Table<'txn, LockToken, LockInfo>,
    base_path: &str,
) -> Result<Vec<RecursivePageTarget>> {
    /*
     * 事前情報の準備
     */
    let prefix = build_recursive_prefix(base_path);
    let now = Local::now();
    let mut targets = Vec::new();

    /*
     * 配下ページの収集とロック検証
     */
    let mut iter = deleted_path_table.range(base_path.to_string()..)?;
    for entry in &mut iter {
        let (path, page_ids) = entry?;
        let path = path.value();
        if path != base_path && !path.starts_with(&prefix) {
            break;
        }

        for page_id in page_ids {
            let page_id = page_id?.value().clone();
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if !index.deleted() {
                return Err(anyhow!("page not deleted"));
            }

            verify_page_lock_in_txn(
                &page_id,
                &mut index,
                index_table,
                lock_table,
                &now,
            )?;

            targets.push(RecursivePageTarget {
                page_id,
                path: path.to_string(),
            });
        }
    }

    Ok(targets)
}

///
/// ページのソフトデリート(トランザクション内部処理)
///
/// # 概要
/// ページの削除フラグ更新と関連アセットの削除フラグ更新を行う。
///
/// # 引数
/// * `page_id` - 削除対象のページID
/// * `path_table` - ページパスインデックステーブル
/// * `deleted_path_table` - 削除済みページパスインデックステーブル
/// * `index_table` - ページインデックステーブル
/// * `lock_table` - ロック情報テーブル
/// * `asset_table` - アセット情報テーブル
/// * `group_table` - ページ所属アセット群取得テーブル
///
/// # 戻り値
/// 成功時は`Ok(())`を返す。
///
pub(in crate::database) fn delete_page_soft_in_txn<'txn>(
    page_id: &PageId,
    path_table: &mut Table<'txn, String, PageId>,
    deleted_path_table: &mut MultimapTable<'txn, String, PageId>,
    index_table: &mut Table<'txn, PageId, PageIndex>,
    lock_table: &mut Table<'txn, LockToken, LockInfo>,
    asset_table: &mut Table<'txn, AssetId, AssetInfo>,
    group_table: &MultimapTable<'txn, PageId, AssetId>,
) -> Result<()> {
    /*
     * ページ情報取得と保護判定
     */
    let mut index = match index_table.get(page_id.clone())? {
        Some(entry) => entry.value(),
        None => return Err(anyhow!(DbError::PageNotFound)),
    };

    if index.is_draft() {
        return Err(anyhow!(DbError::PageLocked));
    }

    if index.deleted() {
        return Err(anyhow!("page already deleted"));
    }

    if is_root_path(&index.path()) {
        return Err(anyhow!(DbError::RootPageProtected));
    }

    /*
     * ロック情報の削除
     */
    if let Some(token) = index.lock_token() {
        lock_table.remove(token)?;
        index.set_lock_token(None);
    }

    /*
     * ページ削除フラグの更新
     */
    let current_path = match index.current_path() {
        Some(path) => path.to_string(),
        None => return Err(anyhow!("page path not found")),
    };
    index.set_deleted_path(current_path.clone());
    index_table.insert(page_id.clone(), index)?;
    let _ = path_table.remove(&current_path)?;
    let _ = deleted_path_table.insert(current_path, page_id.clone())?;

    /*
     * 付随アセットの削除フラグ更新
     */
    for entry in group_table.get(page_id.clone())? {
        let asset_id = entry?.value();
        let mut asset_info = match asset_table.get(asset_id.clone())? {
            Some(info) => info.value(),
            None => return Err(anyhow!("asset info not found")),
        };

        if asset_info.deleted() {
            continue;
        }

        asset_info.set_deleted(true);
        asset_info.clear_page_id();
        asset_table.insert(asset_id.clone(), asset_info)?;
    }

    Ok(())
}

///
/// ページのハードデリート(トランザクション内部処理)
///
/// # 概要
/// ページの全リビジョンとインデックスの削除、関連アセットの参照解除を行う。
///
/// # 引数
/// * `page_id` - 削除対象のページID
/// * `path_table` - ページパスインデックステーブル
/// * `deleted_path_table` - 削除済みページパスインデックステーブル
/// * `index_table` - ページインデックステーブル
/// * `source_table` - ページソーステーブル
/// * `lock_table` - ロック情報テーブル
/// * `asset_table` - アセット情報テーブル
/// * `lookup_table` - アセットID特定テーブル
/// * `group_table` - ページ所属アセット群取得テーブル
/// * `asset_ids` - 削除対象アセットIDの収集先
///
/// # 戻り値
/// 成功時は`Ok(())`を返す。
///
pub(in crate::database) fn delete_page_hard_in_txn<'txn>(
    page_id: &PageId,
    path_table: &mut Table<'txn, String, PageId>,
    deleted_path_table: &mut MultimapTable<'txn, String, PageId>,
    index_table: &mut Table<'txn, PageId, PageIndex>,
    source_table: &mut Table<'txn, (PageId, u64), PageSource>,
    lock_table: &mut Table<'txn, LockToken, LockInfo>,
    asset_table: &mut Table<'txn, AssetId, AssetInfo>,
    lookup_table: &mut Table<'txn, (PageId, String), AssetId>,
    group_table: &mut MultimapTable<'txn, PageId, AssetId>,
    asset_ids: &mut Vec<AssetId>,
) -> Result<()> {
    /*
     * ページ情報取得と保護判定
     */
    let index = match index_table.get(page_id.clone())? {
        Some(entry) => entry.value(),
        None => return Err(anyhow!(DbError::PageNotFound)),
    };

    if index.is_draft() {
        return Err(anyhow!(DbError::PageLocked));
    }

    if is_root_path(&index.path()) {
        return Err(anyhow!(DbError::RootPageProtected));
    }

    /*
     * ロック情報の削除
     */
    if let Some(token) = index.lock_token() {
        lock_table.remove(token)?;
    }

    /*
     * 付随アセットの参照解除
     */
    for entry in group_table.remove_all(page_id.clone())? {
        let asset_id = entry?.value();
        let mut asset_info = match asset_table.get(asset_id.clone())? {
            Some(info) => info.value(),
            None => continue,
        };

        let file_name = asset_info.file_name();
        let _ = lookup_table.remove((page_id.clone(), file_name));

        asset_info.set_deleted(true);
        asset_info.clear_page_id();
        asset_table.insert(asset_id.clone(), asset_info)?;
        asset_ids.push(asset_id);
    }

    /*
     * ページソースの削除
     */
    for revision in index.earliest()..=index.latest() {
        let _ = source_table.remove((page_id.clone(), revision))?;
    }

    /*
     * パス・インデックスの削除
     */
    if let Some(path) = index.current_path() {
        let current_path = path.to_string();
        let _ = path_table.remove(&current_path)?;
    }

    if let Some(path) = index.last_deleted_path() {
        let _ = deleted_path_table.remove(path.to_string(), page_id.clone());
    }

    let _ = index_table.remove(page_id.clone())?;

    Ok(())
}

///
/// 再帰判定用のパスプレフィックスを生成する
///
/// # 引数
/// * `base_path` - 起点パス
///
/// # 戻り値
/// 配下判定用のプレフィックスを返す。
///
fn build_recursive_prefix(base_path: &str) -> String {
    let base = base_path.trim_end_matches('/');
    format!("{}/", base)
}
