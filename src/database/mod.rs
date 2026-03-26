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
pub(crate) use manager::bearer_tokens::VerifyBearerTokenFailureReason;
pub(crate) use manager::pages_read::AppendConflictState;
pub(crate) use manager::pages_write::{AppendPageRequest, AppendPageResult};
pub(crate) use schema::DbError;

use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Local};
use redb::{Database, ReadableTable};

use crate::database::schema::{
    BEARER_TOKEN_ID_TABLE,
    BEARER_TOKEN_TABLE,
    USER_ID_TABLE,
    USER_INFO_TABLE,
};
use crate::database::types::PageId;
use crate::database::types::{
    BearerScope,
    BearerScopeSet,
    PathPrefixSet,
    TokenId,
};

///
/// テスト用の Bearer トークン管理情報スナップショット
///
#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub struct BearerTokenSnapshotForTest {
    /// Bearer トークンID
    pub token_id: String,

    /// 付与スコープ
    pub scopes: Vec<String>,

    /// TTL 秒数
    pub ttl_seconds: i64,

    /// path prefix 制約
    pub path_prefixes: Vec<String>,

    /// 失効状態
    pub revoked: bool,

    /// 任意名
    pub name: Option<String>,
}

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

///
/// テスト用に Bearer トークンを発行する
///
/// # 引数
/// * `db_path` - データベースファイルパス
/// * `asset_path` - アセットディレクトリパス
/// * `user_name` - 発行対象ユーザ名
/// * `ttl_seconds` - TTL 秒数
///
/// # 戻り値
/// `(token_id, token_plaintext)` を返す。
///
#[allow(dead_code)]
pub fn create_bearer_token_for_test<P>(
    db_path: P,
    asset_path: P,
    user_name: &str,
    ttl_seconds: i64,
) -> Result<(String, String)>
where
    P: AsRef<Path>,
{
    let manager = DatabaseManager::open(db_path, asset_path)?;
    let (plaintext, info) = manager.create_bearer_token(
        user_name,
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::new(),
        Duration::seconds(ttl_seconds),
        Some("integration test token".to_string()),
    )?;

    Ok((info.token_id().to_string(), plaintext.expose().to_string()))
}

///
/// テスト用に Bearer トークンを失効する
///
/// # 引数
/// * `db_path` - データベースファイルパス
/// * `asset_path` - アセットディレクトリパス
/// * `token_id` - 失効対象トークンID
///
/// # 戻り値
/// 成功時は `Ok(())` を返す。
///
#[allow(dead_code)]
pub fn revoke_bearer_token_for_test<P>(
    db_path: P,
    asset_path: P,
    token_id: &str,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let manager = DatabaseManager::open(db_path, asset_path)?;
    let token_id = TokenId::from_string(token_id)
        .map_err(|_| anyhow!("invalid token id: {}", token_id))?;
    manager.revoke_bearer_token_by_id(&token_id)?;
    Ok(())
}

///
/// テスト用に Bearer トークン管理情報を取得する
///
/// # 引数
/// * `db_path` - データベースファイルパス
/// * `asset_path` - アセットディレクトリパス
/// * `token_id` - 取得対象トークンID
///
/// # 戻り値
/// 対象トークンが存在する場合はスナップショットを返す。
///
#[allow(dead_code)]
pub fn get_bearer_token_snapshot_for_test<P>(
    db_path: P,
    asset_path: P,
    token_id: &str,
) -> Result<Option<BearerTokenSnapshotForTest>>
where
    P: AsRef<Path>,
{
    let manager = DatabaseManager::open(db_path, asset_path)?;
    let token_id = TokenId::from_string(token_id)
        .map_err(|_| anyhow!("invalid token id: {}", token_id))?;
    let info = match manager.get_bearer_token_info_by_id(&token_id)? {
        Some(info) => info,
        None => return Ok(None),
    };

    Ok(Some(BearerTokenSnapshotForTest {
        token_id: info.token_id().to_string(),
        scopes: info
            .scopes()
            .iter()
            .map(|scope| scope.as_str().to_string())
            .collect(),
        ttl_seconds: info.ttl().num_seconds(),
        path_prefixes: info
            .path_prefixes()
            .iter()
            .map(str::to_string)
            .collect(),
        revoked: info.revoked(),
        name: info.name(),
    }))
}

///
/// テスト用に Bearer トークンの日時項目を調整する
///
/// # 引数
/// * `db_path` - データベースファイルパス
/// * `asset_path` - アセットディレクトリパス
/// * `token_id` - 調整対象トークンID
/// * `created_at` - 作成日時
/// * `updated_at` - 最終更新日時
/// * `expire_at` - 有効期限
///
/// # 戻り値
/// 成功時は `Ok(())` を返す。
///
#[allow(dead_code)]
pub fn rewrite_bearer_token_timestamps_for_test<P>(
    db_path: P,
    asset_path: P,
    token_id: &str,
    created_at: DateTime<Local>,
    updated_at: DateTime<Local>,
    expire_at: DateTime<Local>,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let _ = asset_path.as_ref();
    let db_path_buf = db_path.as_ref().to_path_buf();
    let token_id = TokenId::from_string(token_id)
        .map_err(|_| anyhow!("invalid token id: {}", token_id))?;
    let db = Database::create(&db_path_buf)?;
    let txn = db.begin_write()?;
    {
        let token_hash = {
            let token_id_table = txn.open_table(BEARER_TOKEN_ID_TABLE)?;
            token_id_table
                .get(token_id.clone())?
                .ok_or_else(|| anyhow!("token not found: {}", token_id))?
                .value()
        };
        let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
        let mut token_info = token_table
            .get(token_hash)?
            .ok_or_else(|| anyhow!("token not found: {}", token_id))?
            .value();
        token_info.overwrite_timestamps_for_test(
            created_at,
            updated_at,
            expire_at,
        );
        token_table.insert(token_hash, token_info)?;
    }
    txn.commit()?;

    Ok(())
}

///
/// テスト用に Bearer トークンの紐付けユーザだけを削除する
///
/// # 注記
/// Bearer トークン自体は残し、照合後のユーザ解決失敗を再現するために使う。
///
/// # 引数
/// * `db_path` - データベースファイルパス
/// * `asset_path` - アセットディレクトリパス
/// * `user_name` - 削除対象ユーザ名
///
/// # 戻り値
/// 成功時は `Ok(())` を返す。
///
#[allow(dead_code)]
pub fn delete_user_only_for_bearer_test<P>(
    db_path: P,
    _asset_path: P,
    user_name: &str,
) -> Result<()>
where
    P: AsRef<Path>,
{
    let db_path_buf = db_path.as_ref().to_path_buf();
    let user_name = user_name.to_string();
    let db = Database::create(&db_path_buf)?;
    let txn = db.begin_write()?;
    {
        let mut id_table = txn.open_table(USER_ID_TABLE)?;
        let user_id = match id_table.get(&user_name)? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!("user not found: {}", user_name)),
        };
        let mut info_table = txn.open_table(USER_INFO_TABLE)?;
        let _ = info_table.remove(user_id)?;
        let _ = id_table.remove(&user_name)?;
    }
    txn.commit()?;

    Ok(())
}

#[cfg(test)]
mod tests;
