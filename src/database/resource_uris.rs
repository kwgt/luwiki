/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP resource URI逆引き索引のtransaction内操作
//!

use anyhow::{Result, anyhow};
use redb::{ReadableTable, WriteTransaction};
use std::collections::HashSet;

use crate::database::schema::{
    DbError,
    RESOURCE_URI_INDEX_STATE_TABLE,
    RESOURCE_URI_INDEX_TABLE,
};
use crate::database::types::PageId;
use crate::markdown_source::front_matter::{
    extract_resource_page_front_matter,
    validate_resource_path_shape,
};

/// resource URI逆引き索引の構築状態キー
pub(in crate::database) const RESOURCE_URI_INDEX_STATE_KEY: u8 = 0;

/// resource URI逆引き索引の現行構築状態version
pub(in crate::database) const RESOURCE_URI_INDEX_STATE_VERSION: u8 = 1;

///
/// ページpathからresource_pathを導出する
///
/// # 引数
/// * `path` - 対象ページのcurrent path
///
/// # 戻り値
/// 導出したresource_pathを返す。
///
pub(in crate::database) fn derive_resource_path_from_path(
    path: &str,
) -> Result<String> {
    let path_without_root = path.strip_prefix('/').unwrap_or(path);
    let resource_path = format!("/pages/{}", path_without_root);
    validate_resource_path_shape(&resource_path)?;

    Ok(resource_path)
}

///
/// resource URI逆引き索引用のエントリ群を置換する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
/// * `entries` - 検証済みresource_pathと所有ページID
///
/// # 戻り値
/// 置換に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn replace_resource_uris_in_txn(
    txn: &WriteTransaction,
    entries: &[(String, PageId)],
) -> Result<()> {
    let mut table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;
    let mut existing_resource_paths = Vec::new();
    for entry in table.iter()? {
        existing_resource_paths.push(entry?.0.value());
    }
    for resource_path in existing_resource_paths {
        let _ = table.remove(resource_path)?;
    }
    for (resource_path, page_id) in entries {
        table.insert(resource_path.clone(), page_id.clone())?;
    }
    drop(table);

    let mut state_table = txn.open_table(RESOURCE_URI_INDEX_STATE_TABLE)?;
    state_table.insert(
        RESOURCE_URI_INDEX_STATE_KEY,
        RESOURCE_URI_INDEX_STATE_VERSION,
    )?;

    Ok(())
}

///
/// ページソースに対応するresource URI逆引き索引を同期する
///
/// # 引数
/// * `txn` - ページ正本と同じwrite transaction
/// * `page_id` - 同期対象ページID
/// * `current_path` - 同期対象ページのcurrent path
/// * `source` - 保存予定の最新ページソース
///
/// # 戻り値
/// 同期に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn sync_resource_uri_for_source_in_txn(
    txn: &WriteTransaction,
    page_id: &PageId,
    current_path: &str,
    source: &str,
) -> Result<()> {
    let next_resource_path = match extract_resource_page_front_matter(source)? {
        Some(resource) => Some(match resource.resource_path() {
            Some(resource_path) => resource_path.to_string(),
            None => derive_resource_path_from_path(current_path)?,
        }),
        None => None,
    };
    let mut table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;
    let mut owned_resource_paths = Vec::new();

    /*
     * 現在の所有resource_pathと新しいresource_pathの競合を確認する
     */
    for entry in table.iter()? {
        let (resource_path, owner) = entry?;
        let resource_path = resource_path.value();
        let owner = owner.value();
        if owner == *page_id {
            owned_resource_paths.push(resource_path);
        } else if next_resource_path.as_ref() == Some(&resource_path) {
            return Err(anyhow!(DbError::ResourceUriAlreadyExists {
                resource_path,
            }));
        }
    }

    /*
     * 旧resource_pathを解放して新しいresource_pathを登録する
     */
    for resource_path in owned_resource_paths {
        if next_resource_path.as_ref() != Some(&resource_path) {
            let _ = table.remove(resource_path)?;
        }
    }
    if let Some(resource_path) = next_resource_path {
        table.insert(resource_path, page_id.clone())?;
    }

    Ok(())
}

///
/// 指定ページ群が所有するresource URI逆引き索引を除去する
///
/// # 引数
/// * `txn` - ページ正本と同じwrite transaction
/// * `page_ids` - URIを解放するページID群
///
/// # 戻り値
/// 除去に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn remove_resource_uris_by_page_ids_in_txn(
    txn: &WriteTransaction,
    page_ids: &[PageId],
) -> Result<()> {
    if page_ids.is_empty() {
        return Ok(());
    }

    let target_ids: HashSet<PageId> = page_ids.iter().cloned().collect();
    let mut table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;
    let mut resource_paths = Vec::new();
    for entry in table.iter()? {
        let (resource_path, owner) = entry?;
        if target_ids.contains(&owner.value()) {
            resource_paths.push(resource_path.value());
        }
    }
    for resource_path in resource_paths {
        let _ = table.remove(resource_path)?;
    }

    Ok(())
}

///
/// ページソースに対応するresource URI予約の所有者を確認する
///
/// # 引数
/// * `txn` - ページ正本と同じwrite transaction
/// * `page_id` - 確認対象ページID
/// * `current_path` - 確認対象ページの現在または削除時path
/// * `source` - 確認対象ページの最新ページソース
///
/// # 戻り値
/// resourceでないページまたは予約所有者が対象ページ自身の場合は`Ok(())`を返す。
///
pub(in crate::database) fn verify_resource_uri_owner_for_source_in_txn(
    txn: &WriteTransaction,
    page_id: &PageId,
    current_path: &str,
    source: &str,
) -> Result<()> {
    let Some(resource) = extract_resource_page_front_matter(source)? else {
        return Ok(());
    };
    let resource_path = match resource.resource_path() {
        Some(resource_path) => resource_path.to_string(),
        None => derive_resource_path_from_path(current_path)?,
    };
    let table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;

    match table.get(resource_path.clone())? {
        Some(owner) if owner.value() == *page_id => Ok(()),
        Some(_) => Err(anyhow!(DbError::ResourceUriAlreadyExists {
            resource_path,
        })),
        None => Err(anyhow!(
            "resource URI reservation missing: resource_path={}",
            resource_path,
        )),
    }
}
