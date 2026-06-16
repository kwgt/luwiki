/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP primitive共通名前索引のtransaction内操作
//!

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use redb::{ReadableTable, WriteTransaction};

use crate::database::schema::{
    DbError,
    MCP_PRIMITIVE_NAME_STATE_TABLE,
    MCP_PRIMITIVE_NAME_TABLE,
    PAGE_INDEX_TABLE,
    PAGE_SOURCE_TABLE,
};
use crate::database::types::{
    McpPrimitiveKind,
    McpPrimitiveNameKey,
    PageId,
};
use crate::markdown_source::front_matter::{
    extract_prompt_page_front_matter,
};

pub(in crate::database) const PRIMITIVE_NAME_STATE_KEY: u8 = 0;
pub(in crate::database) const PRIMITIVE_NAME_STATE_VERSION: u8 = 1;

///
/// 既存ページからMCP primitive名前索引を初期構築する
///
/// # 引数
/// * `txn` - DB初期化と同じwrite transaction
///
/// # 戻り値
/// 構築済みまたは初期構築成功時は`Ok(())`を返す。
///
pub(in crate::database) fn initialize_mcp_primitive_names_in_txn(
    txn: &WriteTransaction,
) -> Result<()> {
    let mut state_table =
        txn.open_table(MCP_PRIMITIVE_NAME_STATE_TABLE)?;
    if let Some(state) = state_table.get(PRIMITIVE_NAME_STATE_KEY)? {
        if state.value() == PRIMITIVE_NAME_STATE_VERSION {
            return Ok(());
        }
        return Err(anyhow!("unsupported MCP primitive name state"));
    }

    let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
    let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
    let mut entries = Vec::new();
    let mut owners = HashMap::new();

    /*
     * 最新ページソースを解析して重複を検査する
     */
    for item in index_table.iter()? {
        let (page_id, index) = item?;
        let page_id = page_id.value();
        let index = index.value();
        if index.is_draft() {
            continue;
        }
        let source = source_table
            .get((page_id.clone(), index.latest()))?
            .ok_or_else(|| anyhow!("latest page source missing"))?
            .value()
            .source();
        let Some(prompt) = extract_prompt_page_front_matter(&source)? else {
            continue;
        };
        let key = McpPrimitiveNameKey::new(
            McpPrimitiveKind::Prompt,
            prompt.name().to_string(),
        );
        if let Some(existing) = owners.insert(key.clone(), page_id.clone()) {
            return Err(anyhow!(
                "duplicate MCP primitive name during initialization: \
                 primitive={} name={} page_ids={},{}",
                key.primitive().as_str(),
                key.name(),
                existing,
                page_id,
            ));
        }
        entries.push((key, page_id));
    }

    /*
     * 検証済みの名前索引と構築済みマーカーを反映する
     */
    let mut name_table = txn.open_table(MCP_PRIMITIVE_NAME_TABLE)?;
    let mut old_keys = Vec::new();
    for item in name_table.iter()? {
        old_keys.push(item?.0.value());
    }
    for key in old_keys {
        let _ = name_table.remove(key)?;
    }
    for (key, page_id) in entries {
        name_table.insert(key, page_id)?;
    }
    state_table.insert(
        PRIMITIVE_NAME_STATE_KEY,
        PRIMITIVE_NAME_STATE_VERSION,
    )?;

    Ok(())
}

///
/// ページソースに対応するprimitive名前索引を同期する
///
/// # 引数
/// * `txn` - ページ正本と同じwrite transaction
/// * `page_id` - 同期対象ページID
/// * `source` - 保存予定の最新ページソース
///
/// # 戻り値
/// 同期に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn sync_mcp_primitive_name_for_source_in_txn(
    txn: &WriteTransaction,
    page_id: &PageId,
    source: &str,
) -> Result<()> {
    let next_key = extract_prompt_page_front_matter(source)?
        .map(|prompt| {
            McpPrimitiveNameKey::new(
                McpPrimitiveKind::Prompt,
                prompt.name().to_string(),
            )
        });
    let mut table = txn.open_table(MCP_PRIMITIVE_NAME_TABLE)?;
    let mut owned_keys = Vec::new();

    /*
     * 現在の所有キーと新しい名前の競合を確認する
     */
    for entry in table.iter()? {
        let (key, owner) = entry?;
        let key = key.value();
        let owner = owner.value();
        if owner == *page_id {
            owned_keys.push(key);
        } else if next_key.as_ref() == Some(&key) {
            return Err(anyhow!(DbError::McpPrimitiveNameAlreadyExists {
                primitive: key.primitive(),
                name: key.name().to_string(),
            }));
        }
    }

    /*
     * 旧キーを解放して新しいキーを登録する
     */
    for key in owned_keys {
        if next_key.as_ref() != Some(&key) {
            let _ = table.remove(key)?;
        }
    }
    if let Some(key) = next_key {
        table.insert(key, page_id.clone())?;
    }

    Ok(())
}

///
/// 指定ページ群が所有するprimitive名前索引を除去する
///
/// # 引数
/// * `txn` - ページ正本と同じwrite transaction
/// * `page_ids` - 名前を解放するページID群
///
/// # 戻り値
/// 除去に成功した場合は`Ok(())`を返す。
///
pub(in crate::database) fn remove_mcp_primitive_names_by_page_ids_in_txn(
    txn: &WriteTransaction,
    page_ids: &[PageId],
) -> Result<()> {
    if page_ids.is_empty() {
        return Ok(());
    }

    let target_ids: HashSet<PageId> = page_ids.iter().cloned().collect();
    let mut table = txn.open_table(MCP_PRIMITIVE_NAME_TABLE)?;
    let mut keys = Vec::new();
    for entry in table.iter()? {
        let (key, owner) = entry?;
        if target_ids.contains(&owner.value()) {
            keys.push(key.value());
        }
    }
    for key in keys {
        let _ = table.remove(key)?;
    }

    Ok(())
}

///
/// promptのprimitive名前索引を検証済みの内容へ置換する
///
/// # 引数
/// * `txn` - 再構成処理と同じwrite transaction
/// * `entries` - 検証済みのprompt名前索引
///
/// # 戻り値
/// 置換と構築済み状態の更新に成功した場合は
/// `Ok(())`を返す。
///
pub(in crate::database) fn replace_prompt_primitive_names_in_txn(
    txn: &WriteTransaction,
    entries: &[(McpPrimitiveNameKey, PageId)],
) -> Result<()> {
    let mut name_table = txn.open_table(MCP_PRIMITIVE_NAME_TABLE)?;
    let mut prompt_keys = Vec::new();

    /*
     * 既存のprompt名前だけを収集して除去する
     */
    for entry in name_table.iter()? {
        let (key, _) = entry?;
        let key = key.value();
        if key.primitive() == McpPrimitiveKind::Prompt {
            prompt_keys.push(key);
        }
    }
    for key in prompt_keys {
        let _ = name_table.remove(key)?;
    }

    /*
     * 検証済み索引と構築済み状態を反映する
     */
    for (key, page_id) in entries {
        name_table.insert(key.clone(), page_id.clone())?;
    }
    drop(name_table);
    let mut state_table =
        txn.open_table(MCP_PRIMITIVE_NAME_STATE_TABLE)?;
    state_table.insert(
        PRIMITIVE_NAME_STATE_KEY,
        PRIMITIVE_NAME_STATE_VERSION,
    )?;

    Ok(())
}
