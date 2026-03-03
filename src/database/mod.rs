/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース関連処理をまとめたモジュール
//!

pub(crate) mod types;

mod entries;
mod init;
mod link_refs;
mod manager;
mod schema;
mod txn_helpers;

#[allow(unused_imports)]
pub(crate) use entries::{
    AssetListEntry,
    AssetMoveResult,
    LockListEntry,
    PageListEntry,
};
pub(crate) use manager::DatabaseManager;
pub(crate) use schema::DbError;

use std::path::Path;

use anyhow::{anyhow, Result};

use crate::database::types::PageId;

#[allow(dead_code)]
///
/// テスト用にページソース存在有無を確認
///
/// # 引数
/// * `db_path` - データベースファイルパス
/// * `asset_path` - アセットディレクトリパス
/// * `page_id` - ページID文字列
/// * `revision` - リビジョン番号
///
/// # 戻り値
/// ページソースが存在する場合は`true`を返す。
///
pub fn page_source_exists_for_test<P>(
    db_path: P,
    asset_path: P,
    page_id: &str,
    revision: u64,
) -> Result<bool>
where
    P: AsRef<Path>,
{
    let manager = DatabaseManager::open(db_path, asset_path)?;
    let page_id = PageId::from_string(page_id)
        .map_err(|_| anyhow!(DbError::PageNotFound))?;
    manager.has_page_source_for_test(&page_id, revision)
}

#[cfg(test)]
mod tests;
