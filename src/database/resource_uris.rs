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
    validate_resource_id,
};

/// resource URI逆引き索引の構築状態キー
pub(in crate::database) const RESOURCE_URI_INDEX_STATE_KEY: u8 = 0;

/// resource URI逆引き索引の現行構築状態version
pub(in crate::database) const RESOURCE_URI_INDEX_STATE_VERSION: u8 = 1;

///
/// ページpathからresource_idを導出する
///
/// # 引数
/// * `path` - 対象ページのcurrent path
///
/// # 戻り値
/// 導出したresource_idを返す。
///
pub(in crate::database) fn derive_resource_id_from_path(
    path: &str,
) -> Result<String> {
    let resource_id = path.strip_prefix('/').unwrap_or(path).to_string();
    validate_resource_id(&resource_id)?;

    Ok(resource_id)
}

///
/// resource URI逆引き索引用のエントリ群を置換する
///
/// # 引数
/// * `txn` - 再構成全体を所有するwrite transaction
/// * `entries` - 検証済みresource_idと所有ページID
///
/// # 戻り値
/// 置換に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn replace_resource_uris_in_txn(
    txn: &WriteTransaction,
    entries: &[(String, PageId)],
) -> Result<()> {
    let mut table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;
    let mut existing_resource_ids = Vec::new();
    for entry in table.iter()? {
        existing_resource_ids.push(entry?.0.value());
    }
    for resource_id in existing_resource_ids {
        let _ = table.remove(resource_id)?;
    }
    for (resource_id, page_id) in entries {
        table.insert(resource_id.clone(), page_id.clone())?;
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
    let next_resource_id = match extract_resource_page_front_matter(source)? {
        Some(resource) => Some(match resource.resource_id() {
            Some(resource_id) => resource_id.to_string(),
            None => derive_resource_id_from_path(current_path)?,
        }),
        None => None,
    };
    let mut table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;
    let mut owned_resource_ids = Vec::new();

    /*
     * 現在の所有resource_idと新しいresource_idの競合を確認する
     */
    for entry in table.iter()? {
        let (resource_id, owner) = entry?;
        let resource_id = resource_id.value();
        let owner = owner.value();
        if owner == *page_id {
            owned_resource_ids.push(resource_id);
        } else if next_resource_id.as_ref() == Some(&resource_id) {
            return Err(anyhow!(DbError::ResourceUriAlreadyExists {
                resource_id,
            }));
        }
    }

    /*
     * 旧resource_idを解放して新しいresource_idを登録する
     */
    for resource_id in owned_resource_ids {
        if next_resource_id.as_ref() != Some(&resource_id) {
            let _ = table.remove(resource_id)?;
        }
    }
    if let Some(resource_id) = next_resource_id {
        table.insert(resource_id, page_id.clone())?;
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
    let mut resource_ids = Vec::new();
    for entry in table.iter()? {
        let (resource_id, owner) = entry?;
        if target_ids.contains(&owner.value()) {
            resource_ids.push(resource_id.value());
        }
    }
    for resource_id in resource_ids {
        let _ = table.remove(resource_id)?;
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
    let resource_id = match resource.resource_id() {
        Some(resource_id) => resource_id.to_string(),
        None => derive_resource_id_from_path(current_path)?,
    };
    let table = txn.open_table(RESOURCE_URI_INDEX_TABLE)?;

    match table.get(resource_id.clone())? {
        Some(owner) if owner.value() == *page_id => Ok(()),
        Some(_) => Err(anyhow!(DbError::ResourceUriAlreadyExists {
            resource_id,
        })),
        None => Err(anyhow!(
            "resource URI reservation missing: resource_id={}",
            resource_id,
        )),
    }
}
