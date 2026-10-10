/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 同一write txnの認可・lock条件・commit情報を扱う
//! DatabaseManagerの操作別実装を読みやすく分離する。
//!

use std::fmt;
use std::fs;

use anyhow::{Result, anyhow};
use chrono::Local;
use redb::{ReadableTable, WriteTransaction};

use super::DatabaseManager;
use super::authorized_pages::authorize_index;
use super::locks::{LockTokenInputError, cleanup_page_lock_in_txn};
use crate::auth::PageAccessPolicy;
use crate::database::DbError;
use crate::database::schema::{
    LOCK_INFO_TABLE, PAGE_INDEX_TABLE, PAGE_SOURCE_TABLE, USER_ID_TABLE,
};
use crate::database::txn_helpers::find_lock_by_page;
use crate::database::types::{AssetId, LockInfo, LockToken, PageId, PageIndex};

///
/// lock条件を操作ごとに維持する指定
///
pub(crate) enum MutationLock<'a> {
    /// 両起点解決など、操作条件判定前の認可だけを行う
    Unchecked,
    /// MCP/移動/再帰操作の有効lock拒否
    Unlocked,
    /// RESTの有効lockに対するtoken/owner確認
    Token {
        /// 操作ユーザ名
        user_name: &'a str,
        /// 入力形式エラーも認可後まで遅延する
        token: Result<Option<&'a LockToken>, LockTokenInputError>,
        /// lockなしでも入力形式を確認する既存DELETE条件
        validate_unlocked: bool,
        /// token不一致の既存公開理由
        invalid_token_reason: &'static str,
    },
}

///
/// 認可付きwrite入口の既存状態条件
///
pub(crate) struct MutationAccess<'a> {
    /// 保存prefixの認可policy
    pub(crate) policy: PageAccessPolicy<'a>,
    /// 操作固有のlock条件
    pub(crate) lock: MutationLock<'a>,
    /// draftを操作対象にできるか
    pub(crate) allow_draft: bool,
    /// restore等で削除済み対象を許すか
    pub(crate) allow_deleted: bool,
    /// 既存操作条件に従って期限切れdraftを除去するか
    pub(crate) cleanup_draft: bool,
    /// 明示したMCP editのrevision/instanceだけを再照合する
    pub(crate) expected_version: Option<(u64, &'a str)>,
}

///
/// 同txnで確定した成功対象とcommit状態
///
#[derive(Clone, Debug)]
pub(crate) struct MutationOutcome {
    /// asset書込時の確定ID（page操作はNone）
    pub(crate) asset_id: Option<AssetId>,
    /// 固定の起点ID
    pub(crate) page_id: PageId,
    /// 認可時の元path
    pub(crate) source_path: String,
    /// 更新後の最終path（削除時は削除元）
    pub(crate) path: String,
    /// 確定revision。draftのみの場合はNone
    pub(crate) revision: Option<u64>,
    /// 確定sourceのinstance ID
    pub(crate) instance_id: Option<String>,
    /// 更新した対象ID（no-opは起点のみ）
    pub(crate) affected_page_ids: Vec<PageId>,
    /// commit成功を確認したか（falseは副作用不在を断言しない）
    pub(crate) committed: bool,
    /// commit後の派生同期/FS失敗か（cleanup後業務404と区別する）
    pub(crate) post_processing_failure: bool,
    /// 要求した移動が同値で変更不要だったか
    pub(crate) no_op: bool,
    /// 要求変更なしで期限切れlock/draft整理だけをcommitしたか
    pub(crate) cleanup_only: bool,
}

impl fmt::Display for MutationOutcome {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mutation committed={} at {}", self.committed, self.path)
    }
}

///
/// 既存返却値に同txn対象情報を添える
///
pub(crate) struct AuthorizedMutation<T> {
    /// 既存操作の値
    pub(crate) value: T,
    /// MS4へ渡す確定対象/commit
    pub(crate) outcome: MutationOutcome,
}

///
/// 認可とcleanupを終えた同txn対象
///
pub(super) struct PreparedMutation {
    /// cleanup後index。期限切れdraftの除去後はNone
    pub(super) index: Option<PageIndex>,
    /// 認可時の対象path
    pub(super) source_path: String,
    /// commit後に除去するdraft asset
    pub(super) asset_cleanup: Vec<AssetId>,
    /// 有効lock（保存後解除に使う）
    pub(super) lock: Option<LockInfo>,
    /// cleanupがindex/lock/draftを変更したか
    pub(super) cleanup_changed: bool,
}

///
/// 型付きのMCP edit整合性拒否
///
#[derive(Debug)]
pub(crate) enum MutationVersionError {
    /// revision不一致
    Revision,
    /// instance ID不一致
    Instance,
}

impl fmt::Display for MutationVersionError {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Revision => "revision is not latest",
            Self::Instance => "instance_id does not match latest content",
        })
    }
}

impl std::error::Error for MutationVersionError {}

impl DatabaseManager {
    ///
    /// 最新対象を認可し、既存state/lock/cleanupを同txnで処理する
    ///
    /// # 引数
    /// * `txn` - 操作write txn
    /// * `page_id` - read時点から固定したID
    /// * `access` - 認可と操作条件
    ///
    /// # 戻り値
    /// cleanup後対象、または副作用前の拒否。
    ///
    /// # 概要
    /// active lockの成立する403を区別し、状態確認後にcleanupする。
    ///
    pub(super) fn prepare_mutation_in_txn(
        &self,
        txn: &WriteTransaction,
        page_id: &PageId,
        access: &MutationAccess<'_>,
    ) -> Result<PreparedMutation> {
        /*
         * 最新indexの認可は全条件/cleanupより先に行う
         */
        let index = {
            let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
            indexes
                .get(page_id.clone())?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?
                .value()
        };
        if index.id() != *page_id {
            return Err(anyhow!("page embedded ID mismatch"));
        }
        authorize_index(&index, &access.policy)?;
        let source_path = index.authorization_path()?.as_str().to_string();
        let attempted = mutation_outcome_in_txn(
            txn,
            page_id,
            &source_path,
            vec![page_id.clone()],
        )?;
        let result = (|| -> Result<PreparedMutation> {
            let active = {
                let locks = txn.open_table(LOCK_INFO_TABLE)?;
                let lock = if index.is_draft() {
                    find_lock_by_page(&locks, page_id)?.map(|(_, lock)| lock)
                } else if let Some(token) = index.lock_token() {
                    locks.get(token)?.map(|entry| entry.value())
                } else {
                    None
                };
                lock.filter(|lock| lock.expire() > Local::now())
            };
            let mut missing_token = false;

            /*
             * 実在するactive lockのtoken/owner拒否を判定する
             */
            if let MutationLock::Token {
                user_name,
                token,
                invalid_token_reason,
                ..
            } = &access.lock
            {
                if let Some(lock) = &active {
                    match token.as_ref().map_err(|err| *err)? {
                        Some(token) => {
                            if *token != &lock.token() {
                                return Err(LockTokenInputError {
                                    reason: invalid_token_reason,
                                }
                                .into());
                            }
                            let users = txn.open_table(USER_ID_TABLE)?;
                            let user = users
                                .get(user_name.to_string())?
                                .ok_or_else(|| anyhow!(DbError::UserNotFound))?
                                .value();
                            if lock.user() != user {
                                return Err(anyhow!(DbError::LockForbidden));
                            }
                        }
                        None => missing_token = true,
                    }
                }
            }

            /*
             * 既存状態条件と明示editの整合性を維持する
             */
            if !access.allow_deleted && index.deleted() {
                return Err(anyhow!(DbError::PageDeleted));
            }
            if !access.allow_draft && index.is_draft() {
                return Err(anyhow!(DbError::DraftPage));
            }
            if let Some((revision, instance_id)) = access.expected_version {
                if index.latest() != revision {
                    return Err(MutationVersionError::Revision.into());
                }
                let sources = txn.open_table(PAGE_SOURCE_TABLE)?;
                let source = sources
                    .get((page_id.clone(), revision))?
                    .ok_or_else(|| anyhow!("page source not found"))?
                    .value();
                if source.instance_id().map(|id| id.to_string()).as_deref()
                    != Some(instance_id)
                {
                    return Err(MutationVersionError::Instance.into());
                }
            }
            if missing_token
                || (matches!(access.lock, MutationLock::Unlocked)
                    && active.is_some())
            {
                return Err(anyhow!(DbError::PageLocked));
            }
            if let MutationLock::Token {
                token,
                validate_unlocked: true,
                ..
            } = &access.lock
            {
                token.as_ref().map_err(|err| *err)?;
            }
            if matches!(access.lock, MutationLock::Unchecked) {
                return Ok(PreparedMutation {
                    index: Some(index),
                    source_path,
                    asset_cleanup: Vec::new(),
                    lock: active,
                    cleanup_changed: false,
                });
            }

            /*
             * 認可後のcleanupと対象再取得を同じtxnで行う
             */
            if index.is_draft() && !access.cleanup_draft {
                return Ok(PreparedMutation {
                    index: Some(index),
                    source_path,
                    asset_cleanup: Vec::new(),
                    lock: active,
                    cleanup_changed: false,
                });
            }
            let old_lock_token = index.lock_token();
            let (lock, asset_cleanup) = cleanup_page_lock_in_txn(txn, page_id)?;
            let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
            let index =
                indexes.get(page_id.clone())?.map(|entry| entry.value());
            let cleanup_changed = index.is_none()
                || !asset_cleanup.is_empty()
                || index
                    .as_ref()
                    .is_some_and(|index| index.lock_token() != old_lock_token);
            Ok(PreparedMutation {
                index,
                source_path,
                asset_cleanup,
                lock,
                cleanup_changed,
            })
        })();
        result.map_err(|err| err.context(attempted))
    }

    ///
    /// 認可後の期限切れdraft除去をcommitし既存404へ進む
    ///
    /// # 引数
    /// * `txn` - draftを除去した同txn
    /// * `page_id` - 固定対象
    /// * `prepared` - cleanup結果
    ///
    /// # 戻り値
    /// cleanup commit情報付きPageNotFound、または後処理500。
    ///
    pub(super) fn complete_expired_mutation<T>(
        &self,
        txn: WriteTransaction,
        page_id: &PageId,
        prepared: PreparedMutation,
    ) -> Result<AuthorizedMutation<T>> {
        let mut outcome = mutation_outcome_in_txn(
            &txn,
            page_id,
            &prepared.source_path,
            vec![page_id.clone()],
        )?;
        txn.commit()
            .map_err(|err| anyhow!(err).context(outcome.clone()))?;
        outcome.committed = true;
        outcome.cleanup_only = true;
        self.finish_mutation_assets(&prepared.asset_cleanup, &outcome)?;
        Err(anyhow!(DbError::PageNotFound).context(outcome))
    }

    ///
    /// commit後のdraft asset整理を行う
    ///
    /// # 引数
    /// * `assets` - 当該txnで除去したasset ID
    /// * `outcome` - commit情報
    ///
    /// # 戻り値
    /// 成功またはcommit済み情報を保持した後処理失敗。
    ///
    pub(super) fn finish_mutation_assets(
        &self,
        assets: &[AssetId],
        outcome: &MutationOutcome,
    ) -> Result<()> {
        for asset in assets {
            if let Err(err) = fs::remove_file(self.asset_file_path(asset)) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    let mut committed = outcome.clone();
                    committed.post_processing_failure = true;
                    return Err(anyhow!(err).context(committed));
                }
            }
        }
        Ok(())
    }
}

///
/// 同txnの最終index/sourceから返却情報を作る
///
/// # 引数
/// * `txn` - 操作txn
/// * `page_id` - 起点ID
/// * `source_path` - 認可した元path
/// * `affected` - 操作対象ID
///
/// # 戻り値
/// commit前に確定する対象情報。commit成功時にflagを立てる。
///
pub(super) fn mutation_outcome_in_txn(
    txn: &WriteTransaction,
    page_id: &PageId,
    source_path: &str,
    affected: Vec<PageId>,
) -> Result<MutationOutcome> {
    let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
    let index = indexes.get(page_id.clone())?.map(|entry| entry.value());
    let (path, revision, instance_id) = if let Some(index) = index {
        let revision = if index.is_draft() {
            None
        } else {
            Some(index.latest())
        };
        let instance = if let Some(revision) = revision {
            let sources = txn.open_table(PAGE_SOURCE_TABLE)?;
            sources
                .get((page_id.clone(), revision))?
                .and_then(|entry| entry.value().instance_id())
                .map(|id| id.to_string())
        } else {
            None
        };
        (index.path(), revision, instance)
    } else {
        (source_path.to_string(), None, None)
    };
    Ok(MutationOutcome {
        asset_id: None,
        page_id: page_id.clone(),
        source_path: source_path.to_string(),
        path,
        revision,
        instance_id,
        affected_page_ids: affected,
        committed: false,
        post_processing_failure: false,
        no_op: false,
        cleanup_only: false,
    })
}

///
/// 同txnで扱う最終作成先を認可する
///
/// # 引数
/// * `path` - 保存する最終path
/// * `policy` - 保存prefix制約
///
/// # 戻り値
/// 許可または入力不整合/実path付き拒否。
///
pub(super) fn authorize_create_path(
    path: &str,
    policy: &PageAccessPolicy<'_>,
) -> Result<()> {
    crate::wiki_path::NormalizedWikiPath::from_stored(path)?;
    if !policy.allows(path) {
        return Err(super::authorized_pages::PathPrefixDenied {
            path: path.to_string(),
            parent_candidate: false,
        }
        .into());
    }
    Ok(())
}
