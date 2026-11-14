/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 現在のBearer認可とページ状態を同一snapshotで取得する
//!

use std::fmt;

use anyhow::{Result, anyhow};
use chrono::Local;
use redb::{ReadTransaction, ReadableDatabase};

use super::DatabaseManager;
use crate::auth::{AuthContext, PageAccessPolicy};
use crate::database::schema::{
    BEARER_TOKEN_ID_TABLE, BEARER_TOKEN_TABLE, PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE, PAGE_SOURCE_TABLE, USER_INFO_TABLE,
};
use crate::database::types::{BearerScope, PageId, PageSource};

///
/// 状態参照の公開可能な拒否分類
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StateAccessDenied {
    /// 認証時の主体が現在は利用不能
    Authentication,
    /// 現在のread scopeが不足
    Scope,
    /// 現在のpath prefix制約違反
    Prefix,
}

impl fmt::Display for StateAccessDenied {
    ///
    /// 固定の拒否名を出力する
    ///
    /// # 引数
    /// * `f` - 書込先
    ///
    /// # 戻り値
    /// 書込結果。
    ///
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("page state access denied")
    }
}

impl std::error::Error for StateAccessDenied {}

///
/// 同一snapshotから取り出した内部状態
///
pub(crate) struct PageStateSnapshot {
    /// 外部に出さないページ識別子
    pub(crate) page_id: PageId,
    /// 最新revision
    pub(crate) revision: u64,
    /// 最新source。外部へ本文を返すためには使用しない
    pub(crate) source: PageSource,
}

// 状態参照専用の同一snapshot処理を分離して可読性を確保する。
impl DatabaseManager {
    ///
    /// 現在の主体・read権限とpathの最新状態を同時に確定する
    ///
    /// # 引数
    /// * `auth` - HTTP入口で認証済みの不変主体
    /// * `path` - 正規化済み絶対path
    ///
    /// # 戻り値
    /// 通常ページ、取得不能ならNone、または拒否・内部不整合。
    ///
    /// # 概要
    /// 再確認はread txnだけで行い、TTLや保存データを書き換えない。
    ///
    pub(crate) fn read_mcp_page_state(
        &self,
        auth: &AuthContext,
        path: &str,
    ) -> Result<Option<PageStateSnapshot>> {
        let txn = self.db.begin_read()?;
        self.mcp_state_in_txn(&txn, auth, path)
    }

    ///
    /// 指定snapshotだけを使って認可・状態を取得する
    ///
    /// # 引数
    /// * `txn` - 一回のread snapshot
    /// * `auth` - 認証済み主体
    /// * `path` - 正規化済みpath
    ///
    /// # 戻り値
    /// 認可結果と同じ世代の状態。
    ///
    /// # 概要
    /// 現在の主体を確認してから、同じtxnのpath・index・sourceを読む。
    ///
    fn mcp_state_in_txn(
        &self,
        txn: &ReadTransaction,
        auth: &AuthContext,
        path: &str,
    ) -> Result<Option<PageStateSnapshot>> {
        /* 認証済みtokenと不変主体を同じsnapshotで再確認する。 */
        let token_id = auth.token_id()
            .ok_or(StateAccessDenied::Authentication)?;
        let actor_id = auth.actor_id()
            .ok_or(StateAccessDenied::Authentication)?;
        let token_ids = txn.open_table(BEARER_TOKEN_ID_TABLE)?;
        let token_hash = token_ids.get(token_id.clone())?
            .ok_or(StateAccessDenied::Authentication)?.value();
        let tokens = txn.open_table(BEARER_TOKEN_TABLE)?;
        let token = tokens.get(token_hash)?
            .ok_or(StateAccessDenied::Authentication)?.value();
        if token.token_id() != *token_id || token.user_id() != *actor_id
            || token.revoked() || token.expire_at() <= Local::now()
        {
            return Err(StateAccessDenied::Authentication.into());
        }
        let users = txn.open_table(USER_INFO_TABLE)?;
        let user = users.get(actor_id.clone())?
            .ok_or(StateAccessDenied::Authentication)?.value();
        if user.id() != *actor_id {
            return Err(StateAccessDenied::Authentication.into());
        }

        /* 最新scopeと入力pathを先に認可し、存在情報を保護する。 */
        if !token.scopes().allows(BearerScope::Read) {
            return Err(StateAccessDenied::Scope.into());
        }
        let prefixes = token.path_prefixes();
        let policy = PageAccessPolicy::new(&prefixes);
        if !policy.allows(path) {
            return Err(StateAccessDenied::Prefix.into());
        }

        /* path割当・index・sourceを一つのsnapshotで取得する。 */
        let paths = txn.open_table(PAGE_PATH_TABLE)?;
        let Some(page_id) = paths.get(path.to_string())? else {
            return Ok(None);
        };
        let page_id = page_id.value();
        let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
        let index = indexes.get(page_id.clone())?
            .ok_or_else(|| anyhow!("page state index missing"))?.value();
        if index.deleted() || index.authorization_path()?.as_str() != path {
            return Err(anyhow!("page state path index inconsistent"));
        }
        if index.is_draft() {
            return Ok(None);
        }
        let revision = index.latest();
        let sources = txn.open_table(PAGE_SOURCE_TABLE)?;
        let source = sources.get((page_id.clone(), revision))?
            .ok_or_else(|| anyhow!("page state source missing"))?.value();
        Ok(Some(PageStateSnapshot { page_id, revision, source }))
    }
}

#[cfg(test)]
mod tests;
