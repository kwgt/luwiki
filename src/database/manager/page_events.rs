/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ブラウザの現在認可・ページ状態とcommit確定通知
//!

use anyhow::{Result, anyhow};
use chrono::Local;
use redb::{ReadTransaction, ReadableDatabase, WriteTransaction};
use serde::Serialize;

use super::DatabaseManager;
use super::page_state::StateAccessDenied;
use crate::auth::{AuthContext, PageAccessPolicy};
use crate::database::schema::{
    BEARER_TOKEN_ID_TABLE, BEARER_TOKEN_TABLE, PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE, PAGE_SOURCE_TABLE, USER_INFO_TABLE,
};
use crate::database::types::{BearerScope, PageId, PathPrefixSet};

///
/// 閲覧対象を認可したsnapshotだけで生成する通知
///
#[derive(Clone, PartialEq, Serialize)]
pub(crate) struct BrowserPageState {
    /// 表示基準からの変化
    pub(crate) kind: &'static str,
    /// 通常ページの状態だけを含む
    pub(crate) page_state: Option<BrowserSourceState>,
    /// 認可できた移動先・先祖
    pub(crate) navigation: Option<BrowserNavigation>,
    /// 認証済み不変主体の比較値
    pub(crate) viewer_key: String,
}

///
/// ブラウザが比較する通常ページ情報
///
#[derive(Clone, PartialEq, Serialize)]
pub(crate) struct BrowserSourceState {
    /// 既存RESTと同じページ識別子
    id: String,
    /// 最新revision
    revision: u64,
    /// 旧sourceでは欠落可能
    instance_id: Option<String>,
    /// 本文と同じsnapshotの比較値
    state_token: String,
}

///
/// 閲覧画面の移動候補
///
#[derive(Clone, PartialEq, Serialize)]
pub(crate) struct BrowserNavigation {
    /// renamed又はancestor
    kind: &'static str,
    /// 認可した正規形path
    path: String,
}

// 同一txnの状態読取と通知を専用moduleへ分離する。
impl DatabaseManager {
    ///
    /// commitが成功した場合だけ購読を起床する
    ///
    /// # 引数
    /// * `txn` - 関連操作のwrite transaction
    ///
    /// # 戻り値
    /// 既存と同じcommit成否。
    ///
    pub(super) fn commit_page_change(
        &self,
        txn: WriteTransaction,
    ) -> std::result::Result<(), redb::CommitError> {
        txn.commit()?;
        self.page_changes.send_modify(|value| {
            *value = value.wrapping_add(1);
        });
        Ok(())
    }

    ///
    /// 初回読取前に起床receiverを登録する
    ///
    /// # 戻り値
    /// 再照合用の最新印。印の値をクライアントへ送らない。
    ///
    pub(crate) fn subscribe_page_changes(
        &self,
    ) -> tokio::sync::watch::Receiver<u64> {
        self.page_changes.subscribe()
    }

    ///
    /// heartbeat時に主体の現在有効性だけを確認する
    ///
    /// # 引数
    /// * `auth` - 入口で認証した不変主体
    /// * `credential` - Basicの同snapshot識別値
    ///
    /// # 戻り値
    /// 許可又は拒否。TTLを延長しない。
    ///
    pub(crate) fn check_page_event_identity(
        &self,
        auth: &AuthContext,
        credential: Option<&str>,
    ) -> Result<()> {
        let txn = self.db.begin_read()?;
        event_prefixes(&txn, auth, credential).map(|_| ())
    }

    ///
    /// 同じread snapshotで現在認可・表示対象・導線を確定する
    ///
    /// # 引数
    /// * `auth` - 入口の不変主体
    /// * `credential` - Basic資格情報の比較値
    /// * `path` - 正規形の表示path
    /// * `displayed` - 現在表示しているID
    /// * `expected` - 実際の表示sourceのtoken
    ///
    /// # 戻り値
    /// 認可した通知内容又は固定分類可能なエラー。
    ///
    /// # 概要
    /// 主体・path・IDを認可した後で移動・再作成・取得不能を判定する。
    ///
    pub(crate) fn read_browser_page_state(
        &self,
        auth: &AuthContext,
        credential: Option<&str>,
        path: &str,
        displayed: &PageId,
        expected: &str,
    ) -> Result<BrowserPageState> {
        /* 現在の主体と入力pathを存在情報より先に認可する。 */
        let txn = self.db.begin_read()?;
        let prefixes = event_prefixes(&txn, auth, credential)?;
        let policy = PageAccessPolicy::new(&prefixes);
        if !policy.allows(path) {
            return Err(StateAccessDenied::Prefix.into());
        }
        let mut result = BrowserPageState {
            kind: "unavailable",
            page_state: None,
            navigation: None,
            viewer_key: crate::page_state::digest(
                b"luwiki:viewer:v1",
                &[auth.actor_id().unwrap().to_string().as_bytes()],
            ),
        };
        let mut claimed_here = false;
        let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
        let paths = txn.open_table(PAGE_PATH_TABLE)?;

        /* 表示中IDの現在pathも認可し、確認できるrenameを優先する。 */
        if let Some(index) = indexes.get(displayed.clone())? {
            let index = index.value();
            let current = index.authorization_path()?;
            if !policy.allows(current.as_str()) {
                return Err(StateAccessDenied::Prefix.into());
            }
            claimed_here = !index.deleted()
                && !index.is_draft()
                && current.as_str() == path;
            if !index.deleted() && !index.is_draft() && current.as_str() != path
            {
                result.kind = "renamed";
                result.navigation = Some(BrowserNavigation {
                    kind: "renamed",
                    path: current.into_string(),
                });
                return Ok(result);
            }
        }

        /* pathの現在割当とsourceを同じsnapshotから比較する。 */
        if let Some(id) = paths.get(path.to_string())? {
            let id = id.value();
            if claimed_here && id != *displayed {
                return Err(anyhow!("page event identity inconsistent"));
            }
            let index = indexes
                .get(id.clone())?
                .ok_or_else(|| anyhow!("page event index missing"))?
                .value();
            if index.deleted() || index.authorization_path()?.as_str() != path {
                return Err(anyhow!("page event path inconsistent"));
            }
            if !index.is_draft() {
                let sources = txn.open_table(PAGE_SOURCE_TABLE)?;
                let source = sources
                    .get((id.clone(), index.latest()))?
                    .ok_or_else(|| anyhow!("page event source missing"))?
                    .value();
                let token = crate::page_state::state_token(
                    path,
                    &id,
                    index.latest(),
                    &source,
                );
                result.kind = if id != *displayed {
                    "replaced"
                } else if token == expected {
                    "unchanged"
                } else {
                    "updated"
                };
                result.page_state = Some(BrowserSourceState {
                    id: id.to_string(),
                    revision: index.latest(),
                    instance_id: source.instance_id().map(|id| id.to_string()),
                    state_token: token,
                });
                return Ok(result);
            }
        }

        if claimed_here {
            return Err(anyhow!("page event assignment missing"));
        }
        /* hard delete後もpathから最も近い通常先祖を探す。 */
        let mut ancestor = path.to_string();
        while ancestor != "/" {
            ancestor = ancestor
                .rsplit_once('/')
                .map(|(p, _)| if p.is_empty() { "/" } else { p })
                .unwrap_or("/")
                .to_string();
            if let Some(id) = paths.get(ancestor.clone())? {
                let index = indexes
                    .get(id.value())?
                    .ok_or_else(|| anyhow!("ancestor index missing"))?
                    .value();
                if index.deleted()
                    || index.authorization_path()?.as_str() != ancestor
                {
                    return Err(anyhow!("ancestor path inconsistent"));
                }
                if index.is_draft() {
                    continue;
                }
                if policy.allows(&ancestor) {
                    result.navigation = Some(BrowserNavigation {
                        kind: "ancestor",
                        path: ancestor,
                    });
                }
                break;
            }
        }
        Ok(result)
    }
}

///
/// 認証済み主体の現在情報を同じtxnから再確認する
///
/// # 引数
/// * `txn` - 状態と共有するsnapshot
/// * `auth` - 入口の主体
/// * `credential` - Basicの認証済み比較値
///
/// # 戻り値
/// 現在のpath制約。認証時の古いprefixを使わない。
///
/// # 概要
/// Basicは不変ID・credential、Bearerは管理ID・期限・scopeを照合する。
///
fn event_prefixes(
    txn: &ReadTransaction,
    auth: &AuthContext,
    credential: Option<&str>,
) -> Result<PathPrefixSet> {
    let actor = auth.actor_id().ok_or(StateAccessDenied::Authentication)?;
    let users = txn.open_table(USER_INFO_TABLE)?;
    let user = users
        .get(actor.clone())?
        .ok_or(StateAccessDenied::Authentication)?
        .value();
    if user.id() != *actor {
        return Err(StateAccessDenied::Authentication.into());
    }
    if let Some(token_id) = auth.token_id() {
        /* 失効・期限と現在scopeを確認し、TTLは変更しない。 */
        let ids = txn.open_table(BEARER_TOKEN_ID_TABLE)?;
        let key = ids
            .get(token_id.clone())?
            .ok_or(StateAccessDenied::Authentication)?
            .value();
        let tokens = txn.open_table(BEARER_TOKEN_TABLE)?;
        let token = tokens
            .get(key)?
            .ok_or(StateAccessDenied::Authentication)?
            .value();
        if token.token_id() != *token_id
            || token.user_id() != *actor
            || token.revoked()
            || token.expire_at() <= Local::now()
        {
            return Err(StateAccessDenied::Authentication.into());
        }
        if !token.scopes().allows(BearerScope::Read) {
            return Err(StateAccessDenied::Scope.into());
        }
        Ok(token.path_prefixes())
    } else {
        if !user.allows_basic_auth()
            || credential != Some(user.credential_tag().as_str())
        {
            return Err(StateAccessDenied::Authentication.into());
        }
        Ok(PathPrefixSet::new())
    }
}

#[cfg(test)]
mod tests;
