/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ移動・復元・削除・履歴操作を認可済みwrite txnへ接続する
//! DatabaseManagerの操作別実装を読みやすく分離する。
//!

use anyhow::{Result, anyhow};
use redb::{ReadableMultimapTable, ReadableTable, WriteTransaction};

use super::DatabaseManager;
use super::mutation::{
    AuthorizedMutation, MutationAccess, MutationLock, authorize_create_path,
    mutation_outcome_in_txn,
};
use crate::database::DbError;
use crate::database::schema::{
    DELETED_PAGE_PATH_TABLE, PAGE_INDEX_TABLE, PAGE_PATH_TABLE, USER_ID_TABLE,
};
use crate::database::types::PageId;
use crate::wiki_path::{DestinationPathInput, NormalizedWikiPath};

///
/// REST専用末尾指定とMCP最終値を区別する配置先
///
pub(crate) enum PageDestination<'a> {
    /// 元basenameを同txnのindexから一度だけ解決する
    Rest(&'a DestinationPathInput),
    /// MCP等の入力解決済み最終値
    Final(&'a NormalizedWikiPath),
}

///
/// 既存restoreの非削除状態を区別する
///
#[derive(Debug)]
pub(crate) struct PageNotDeleted;

impl std::fmt::Display for PageNotDeleted {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("page not deleted")
    }
}

impl std::error::Error for PageNotDeleted {}

///
/// 同txnで拒否した移動側
///
#[derive(Clone, Copy, Debug)]
pub(crate) enum MoveDeniedSide {
    /// 元pathの拒否
    Source,
    /// 最終配置先の拒否
    Destination,
}

impl std::fmt::Display for MoveDeniedSide {
    ///
    /// 拒否側の固定診断を返す
    ///
    /// # 引数
    /// * `formatter` - 診断出力先
    ///
    /// # 戻り値
    /// 書込結果。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>)
        -> std::fmt::Result
    {
        formatter.write_str("move path authorization side")
    }
}

impl std::error::Error for MoveDeniedSide {}

///
/// 既存revision validatorで確定した指定対象
///
#[derive(Clone, Copy, Debug)]
pub(crate) struct RevisionAccessTarget {
    /// 検証成立した指定revision
    pub(crate) revision: u64,
}

impl std::fmt::Display for RevisionAccessTarget {
    ///
    /// revision値を漏らさず固定診断を返す
    ///
    /// # 引数
    /// * `formatter` - 診断出力先
    ///
    /// # 戻り値
    /// 書込結果。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>)
        -> std::fmt::Result
    {
        formatter.write_str("validated revision target")
    }
}

impl std::error::Error for RevisionAccessTarget {}

impl DatabaseManager {
    ///
    /// 同write txnで最新元・最終先を確定し移動/復元する
    ///
    /// # 引数
    /// * `page_id` - 固定起点ID
    /// * `destination` - 意図を保持する配置先
    /// * `restore` - 削除状態から復元するか
    /// * `recursive` - 配下も操作するか
    /// * `access` - policy/既存lock条件
    /// * `check_noop_lock` - REST no-opの既存lock条件を維持する指定
    ///
    /// # 戻り値
    /// 要求変更の有無と確定対象/commit。
    ///
    /// # 概要
    /// 両側認可を状態・lock・no-opより先に行い、全件検証後commitする。
    ///
    pub(crate) fn move_page_authorized(
        &self,
        page_id: &PageId,
        destination: PageDestination<'_>,
        restore: bool,
        recursive: bool,
        access: &MutationAccess<'_>,
        check_noop_lock: bool,
    ) -> Result<AuthorizedMutation<bool>> {
        /*
         * 同じ最新indexから元と最終配置先を確定し両側認可する
         */
        let txn = self.db.begin_write()?;
        let resolve_access = MutationAccess {
            policy: access.policy,
            lock: MutationLock::Unchecked,
            allow_draft: true,
            allow_deleted: true,
            cleanup_draft: false,
            expected_version: None,
        };
        let initial =
            self.prepare_mutation_in_txn(&txn, page_id, &resolve_access)
                .map_err(|err| {
                    if err.downcast_ref::<crate::database::PathPrefixDenied>()
                        .is_some()
                    { err.context(MoveDeniedSide::Source) } else { err }
                })?;
        let index = initial
            .index
            .ok_or_else(|| anyhow!(DbError::PageNotFound))?;
        let initial_target =
            super::authorized_pages::PageAccessTarget::from_index(&index)?;
        let source = index.authorization_path()
            .map_err(|err| err.context(initial_target.clone()))?;
        if source.as_str() == "/" {
            return Err(anyhow!(DbError::RootPageProtected)
                .context(initial_target));
        }
        let target = match destination {
            PageDestination::Rest(input) => input.resolve(&source)
                .map_err(|err| err.context(initial_target.clone()))?,
            PageDestination::Final(path) => path.clone(),
        };
        authorize_create_path(target.as_str(), &access.policy)
            .map_err(|err| err.context(MoveDeniedSide::Destination)
                .context(initial_target.clone()))?;
        let mut attempted = mutation_outcome_in_txn(
            &txn,
            page_id,
            source.as_str(),
            vec![page_id.clone()],
        ).map_err(|err| err.context(initial_target))?;
        attempted.path = target.as_str().to_string();

        /*
         * 認可後に既存状態条件とno-opの操作固有条件を適用する
         */
        if index.is_draft() {
            return Err(anyhow!(DbError::DraftPage).context(attempted));
        }
        if restore && !index.deleted() {
            return Err(anyhow!(PageNotDeleted).context(attempted));
        }
        if !restore && index.deleted() {
            return Err(anyhow!(DbError::PageDeleted).context(attempted));
        }
        if recursive {
            validate_recursive_targets(&txn, page_id, source.as_str(), restore)
                .map_err(|err| err.context(attempted.clone()))?;
        }
        let no_op = !restore && source == target;
        if no_op && !check_noop_lock {
            attempted.no_op = true;
            return Ok(AuthorizedMutation {
                value: false,
                outcome: attempted,
            });
        }
        let prepared = self
            .prepare_mutation_in_txn(&txn, page_id, access)
            .map_err(|err| err.context(attempted.clone()))?;
        if prepared.index.is_none() {
            return self.complete_expired_mutation(txn, page_id, prepared);
        }

        /*
         * 全件条件/relative mappingと本体更新を同txnで行う
         */
        let affected = (|| -> Result<Vec<PageId>> {
            if no_op {
                return Ok(vec![page_id.clone()]);
            }
            if restore {
                if recursive {
                    self.restore_recursive_in_txn(
                        &txn,
                        page_id,
                        target.as_str(),
                        true,
                        false,
                    )
                } else {
                    self.restore_page_in_txn(
                        &txn,
                        page_id,
                        target.as_str(),
                        true,
                        false,
                    )
                }
            } else if recursive {
                self.rename_recursive_in_txn(
                    &txn,
                    page_id,
                    target.as_str(),
                    false,
                )
            } else {
                self.rename_page_in_txn(
                    &txn,
                    source.as_str(),
                    target.as_str(),
                    Some(page_id),
                )?;
                Ok(vec![page_id.clone()])
            }
        })()
        .map_err(|err| err.context(attempted))?;
        let mut outcome =
            mutation_outcome_in_txn(&txn, page_id, source.as_str(), affected)?;
        txn.commit()
            .map_err(|err| anyhow!(err).context(outcome.clone()))?;
        outcome.committed = true;
        outcome.no_op = no_op;
        outcome.cleanup_only = no_op && prepared.cleanup_changed;
        self.finish_mutation_assets(&prepared.asset_cleanup, &outcome)?;
        Ok(AuthorizedMutation {
            value: !no_op,
            outcome,
        })
    }

    ///
    /// 同write txnの最新対象認可後にrollback/compactionする
    ///
    /// # 引数
    /// * `page_id` - 固定対象
    /// * `revision` - 指定revision
    /// * `rollback` - rollbackかcompactionか
    /// * `access` - 認可/lock条件
    ///
    /// # 戻り値
    /// 確定対象/commit情報。
    ///
    /// # 概要
    /// 既存状態/revision/lockと派生同期順を維持する。
    ///
    pub(crate) fn revise_page_authorized(
        &self,
        page_id: &PageId,
        revision: u64,
        rollback: bool,
        access: &MutationAccess<'_>,
    ) -> Result<AuthorizedMutation<()>> {
        let txn = self.db.begin_write()?;
        let prepared = self.prepare_mutation_in_txn(&txn, page_id, access)?;
        let attempted = mutation_outcome_in_txn(
            &txn,
            page_id,
            &prepared.source_path,
            vec![page_id.clone()],
        )?;
        /*
         * 認可後に同txnで履歴条件と更新を実行する
         */
        let result = if rollback {
            self.rollback_page_in_txn(&txn, page_id, revision)
        } else {
            self.compact_page_in_txn(&txn, page_id, revision)
        };
        result.map_err(|err| err.context(attempted))?;
        let mut outcome = mutation_outcome_in_txn(
            &txn,
            page_id,
            &prepared.source_path,
            vec![page_id.clone()],
        )?;
        txn.commit()
            .map_err(|err| anyhow!(err).context(outcome.clone())
                .context(RevisionAccessTarget { revision }))?;
        outcome.committed = true;
        self.finish_mutation_assets(&prepared.asset_cleanup, &outcome)
            .map_err(|err| err.context(RevisionAccessTarget { revision }))?;
        if rollback {
            self.sync_page_candidates_after_commit(page_id, Some(&outcome))
                .map_err(|err| err.context(RevisionAccessTarget { revision }))?;
        }
        Ok(AuthorizedMutation { value: (), outcome })
    }

    ///
    /// 単独/再帰削除を固定IDと最新認可で実行する
    ///
    /// # 引数
    /// * `page_id` - 固定起点
    /// * `recursive` - 配下も削除するか
    /// * `user_name` - REST操作ユーザ
    /// * `access` - 認可/操作固有lock条件
    ///
    /// # 戻り値
    /// 確定対象とcommit済み情報。
    ///
    /// # 概要
    /// draft/assetのFS除去は本体commit成功後にだけ実行する。
    ///
    pub(crate) fn delete_page_authorized(
        &self,
        page_id: &PageId,
        recursive: bool,
        user_name: &str,
        access: &MutationAccess<'_>,
    ) -> Result<AuthorizedMutation<()>> {
        let txn = self.db.begin_write()?;
        let prepared = self.prepare_mutation_in_txn(&txn, page_id, access)?;
        let attempted = mutation_outcome_in_txn(
            &txn,
            page_id,
            &prepared.source_path,
            vec![page_id.clone()],
        )?;
        /*
         * 既存削除条件と対象全件を同じtxnで処理する
         */
        let (affected, mut assets) = (|| -> Result<_> {
            if recursive {
                validate_recursive_targets(
                    &txn,
                    page_id,
                    &prepared.source_path,
                    false,
                )?;
                self.delete_recursive_in_txn(&txn, page_id, false)
            } else {
                let users = txn.open_table(USER_ID_TABLE)?;
                let user = users
                    .get(user_name.to_string())?
                    .ok_or_else(|| anyhow!(DbError::UserNotFound))?
                    .value();
                drop(users);
                let token = prepared.lock.as_ref().map(|lock| lock.token());
                let assets = self.delete_page_with_token_in_txn(
                    &txn,
                    page_id,
                    &user,
                    token.as_ref(),
                )?;
                Ok((vec![page_id.clone()], assets))
            }
        })()
        .map_err(|err| err.context(attempted))?;
        assets.extend(prepared.asset_cleanup);
        let mut outcome = mutation_outcome_in_txn(
            &txn,
            page_id,
            &prepared.source_path,
            affected,
        )?;
        txn.commit()
            .map_err(|err| anyhow!(err).context(outcome.clone()))?;
        outcome.committed = true;
        let mut committed = outcome.clone();
        committed.post_processing_failure = true;
        self.remove_template_candidate_by_page_id(page_id)
            .map_err(|err| err.context(committed))?;
        self.finish_mutation_assets(&assets, &outcome)?;
        Ok(AuthorizedMutation { value: (), outcome })
    }
}

///
/// 認可付き再帰候補と最新indexの対象一致を検証する
///
/// # 引数
/// * `txn` - 更新と同じtxn
/// * `root` - 認可済み固定起点
/// * `base` - 最新起点path
/// * `deleted` - 削除index候補を走査するか
///
/// # 戻り値
/// 全候補一致なら成功。不整合は内部エラー。
///
/// # 概要
/// legacy collectorへ渡す前に別pathのID追跡を防ぎ、起点行も確認する。
///
fn validate_recursive_targets(
    txn: &WriteTransaction,
    root: &PageId,
    base: &str,
    deleted: bool,
) -> Result<()> {
    let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
    let prefix = format!("{}/", base.trim_end_matches('/'));
    let mut found_root = false;
    let mut validate = |path: &str, id: PageId| -> Result<()> {
        let index = indexes
            .get(id.clone())?
            .ok_or_else(|| anyhow!("recursive target index missing"))?
            .value();
        let actual = if deleted {
            index.last_deleted_path()
        } else {
            index.current_path()
        };
        if index.id() != id
            || index.deleted() != deleted
            || actual != Some(path)
        {
            return Err(anyhow!("recursive target index mismatch"));
        }
        found_root |= id == *root && path == base;
        Ok(())
    };
    /*
     * path候補を同txnの最新indexと全件照合する
     */
    if deleted {
        let paths = txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
        for entry in paths.range(base.to_string()..)? {
            let (path, ids) = entry?;
            let path = path.value();
            if path != base && !path.starts_with(&prefix) {
                break;
            }
            for id in ids {
                validate(&path, id?.value())?;
            }
        }
    } else {
        let paths = txn.open_table(PAGE_PATH_TABLE)?;
        for entry in paths.range(base.to_string()..)? {
            let (path, id) = entry?;
            let path = path.value();
            if path != base && !path.starts_with(&prefix) {
                break;
            }
            validate(&path, id.value())?;
        }
    }
    if !found_root {
        return Err(anyhow!("recursive root path missing"));
    }
    Ok(())
}
