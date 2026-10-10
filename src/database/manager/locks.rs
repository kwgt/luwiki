/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ロック関連の操作を提供するモジュール
//!

use std::fs;

use anyhow::{anyhow, Result};
use chrono::Local;
use redb::{ReadableDatabase, ReadableTable, WriteTransaction};

use super::DatabaseManager;
use super::authorized_pages::{page_in_txn, AuthorizedPage, PageSourceNotFound};
use crate::auth::PageAccessPolicy;
use super::authorized_pages::PageAccessTarget;
use crate::database::entries::LockListEntry;
use crate::database::schema::{
    LOCK_INFO_TABLE,
    PAGE_INDEX_TABLE,
    USER_ID_TABLE,
    USER_INFO_TABLE,
};
use crate::database::txn_helpers::{delete_draft_in_txn, find_lock_by_page};
use crate::database::types::{AssetId, LockInfo, LockToken, PageId, PageIndex};

///
/// 同一write txnで確定したlock操作結果と認可対象
///
#[derive(Debug)]
pub(crate) struct AuthorizedLockResult<T> {
    /// 既存lock操作の返却値
    pub(crate) value: T,
    /// 操作txnで認可した対象index。MS4の成功対象情報に利用する。
    pub(crate) target: PageIndex,
}

///
/// 公開層で遅延判定するlock入力不正
///
#[derive(Clone, Copy, Debug)]
pub(crate) struct LockTokenInputError {
    /// 既存公開エラー理由
    pub(crate) reason: &'static str,
}

impl std::fmt::Display for LockTokenInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason)
    }
}

impl std::error::Error for LockTokenInputError {}

impl DatabaseManager {
    ///
    /// meta/lock参照を最新対象認可とcleanupと同じtxnで取得する
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `revision` - 履歴revision
    /// * `with_source` - meta用ソース取得
    /// * `allow_deleted` - metaの削除履歴参照を許すか
    /// * `policy` - prefix制約
    ///
    /// # 戻り値
    /// 同一txnのページ・有効lock・lock所有者名。
    ///
    /// # 概要
    /// 認可前の副作用を防ぎ、cleanup後の対象を再取得して返す。
    ///
    pub(crate) fn read_authorized_page_meta(
        &self,
        page_id: &PageId,
        revision: Option<u64>,
        with_source: bool,
        allow_deleted: bool,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<(AuthorizedPage, Option<LockInfo>, Option<String>)> {
        /*
         * cleanup前に最新対象を認可し状態条件を確認する
         */
        let txn = self.db.begin_write()?;
        let before = page_in_txn(&txn, page_id, revision, with_source, policy)?;
        let target = PageAccessTarget::from_page(&before)?;

        let result = (|| -> Result<_> {
            if !allow_deleted && before.index.deleted() {
                return Err(anyhow!(crate::database::DbError::PageDeleted));
            }
            if with_source && !before.index.is_draft()
                && before.source.is_none()
            {
                return Err(PageSourceNotFound.into());
            }
            let (lock, asset_ids) = cleanup_page_lock_in_txn(&txn, page_id)?;

            /*
             * cleanup後の同じtxnから返却情報を取得する
             */
            let result =
                page_in_txn(&txn, page_id, revision, with_source, policy);
            let lock_user = if let Some(lock) = &lock {
                let users = txn.open_table(USER_INFO_TABLE)?;
                users.get(lock.user())?.map(|entry| entry.value().username())
            } else { None };
            #[cfg(test)]
            if self.fail_meta_commit_for_test.load(
                std::sync::atomic::Ordering::SeqCst,
            ) {
                return Err(anyhow!("injected meta commit checkpoint failure"));
            }
            txn.commit()?;
            for asset_id in asset_ids {
                let _ = fs::remove_file(self.asset_file_path(&asset_id));
            }
            Ok((result?, lock, lock_user))
        })();
        result.map_err(|err| err.context(target))
    }

    ///
    /// 隔離fixtureのlock期限を過去へ設定する
    ///
    /// # 引数
    /// * `page_id` - fixture対象ID
    ///
    /// # 戻り値
    /// 同txnの期限書換結果。製品ビルドには含めない。
    ///
    /// # 概要
    /// 既存serde表現を使い運用設定やglobal clockを変更しない。
    ///
    #[cfg(test)]
    pub(crate) fn expire_page_lock_for_test(&self, page_id: &PageId)
        -> Result<()>
    {
        /*
         * 同txnのlockだけをfixture用の過去時刻へ差し替える
         */
        let txn = self.db.begin_write()?;
        {
            let mut locks = txn.open_table(LOCK_INFO_TABLE)?;
            let (_, lock) = find_lock_by_page(&locks, page_id)?
                .ok_or_else(|| anyhow!("fixture lock missing"))?;
            let mut value = serde_json::to_value(&lock)?;
            value["expire"] = serde_json::json!(
                Local::now() - chrono::Duration::minutes(1)
            );
            let expired: LockInfo = serde_json::from_value(value)?;
            locks.insert(lock.token(), expired)?;
        }
        txn.commit()?;
        Ok(())
    }

    ///
    /// ロック情報の取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// ロック情報を取得できた場合は`Ok(Some(LockInfo))`を返す。
    /// ロックが存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_page_lock_info(
        &self,
        page_id: &PageId,
    ) -> Result<Option<LockInfo>> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let (result, asset_ids) = cleanup_page_lock_in_txn(&txn, page_id)?;
        txn.commit()?;
        for asset_id in asset_ids {
            let _ = fs::remove_file(self.asset_file_path(&asset_id));
        }
        Ok(result)
    }

    ///
    /// ロックの取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// 取得したロック情報を返す。
    ///
    pub(crate) fn acquire_page_lock(
        &self,
        page_id: &PageId,
        user_name: &str,
    ) -> Result<LockInfo> {
        self.acquire_page_lock_with_policy(page_id, user_name, None)
            .map(|(value, _)| value)
    }

    ///
    /// 最新対象を認可して既存ロック操作を実施する
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `user_name` - 操作ユーザ
    /// * `policy` - 保存prefix制約
    ///
    /// # 戻り値
    /// 操作結果、または認可/既存条件エラー。
    ///
    pub(crate) fn acquire_page_lock_authorized(
        &self,
        page_id: &PageId,
        user_name: &str,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<AuthorizedLockResult<LockInfo>> {
        self.acquire_page_lock_with_policy(page_id, user_name, Some(policy))
            .and_then(|(value, target)| {
                let target = target.ok_or_else(|| anyhow!(
                    "authorized lock target is missing"))?;
                Ok(AuthorizedLockResult { value, target })
            })
    }

    ///
    /// CLI互換と認可入口の共通ロック処理
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `user_name` - 操作ユーザ
    /// * `policy` - REST認可制約、CLIの場合はNone
    ///
    /// # 戻り値
    /// 同一txnの操作結果を返す。
    ///
    /// # 概要
    /// 同txnの確定情報を保持し既存検証・副作用の順序を維持する。
    ///
    fn acquire_page_lock_with_policy(
        &self,
        page_id: &PageId,
        user_name: &str,
        policy: Option<&PageAccessPolicy<'_>>,
    ) -> Result<(LockInfo, Option<PageIndex>)> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        /*
         * 最新対象の認可はlock条件や副作用より先に実施する
         */
        let target = if let Some(policy) = policy {
            let page = page_in_txn(&txn, page_id, None, false, policy)?;
            if page.index.deleted() {
                return Err(anyhow!(crate::database::DbError::PageDeleted)
                    .context(PageAccessTarget::from_page(&page)?));
            }
            Some(page.index)
        } else { None };
        let audit_target = target.as_ref()
            .map(PageAccessTarget::from_index).transpose()?;
        let result = (|| -> Result<_> {
        let lock_info = {
                let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
                let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
                let now = Local::now();

                /*
                 * ユーザIDの解決
                 */
                let user_id = {
                    let id_table = txn.open_table(USER_ID_TABLE)?;
                    let key = user_name.to_string();
                    match id_table.get(&key)? {
                        Some(id) => id.value(),
                        None => {
                            return Err(anyhow!(
                                crate::database::DbError::UserNotFound
                            ));
                        }
                    }
                };

                let mut index = match index_table.get(page_id.clone())? {
                    Some(entry) => entry.value(),
                    None => {
                        return Err(anyhow!(
                            crate::database::DbError::LockNotFound,
                        ));
                    }
                };

                /*
                 * 既存ロックの確認
                 */
                if index.is_draft() {
                    if let Some((token, existing)) =
                        find_lock_by_page(&lock_table, page_id)?
                    {
                        if existing.expire() > now {
                            return Err(anyhow!(
                                crate::database::DbError::PageLocked
                            ));
                        }
                        lock_table.remove(token)?;
                    }
                } else if let Some(token) = index.lock_token() {
                    let existing = lock_table
                        .get(token.clone())?
                        .map(|entry| entry.value());
                    if let Some(lock_info) = existing {
                        if lock_info.expire() > now {
                            return Err(anyhow!(
                                crate::database::DbError::PageLocked
                            ));
                        }
                        lock_table.remove(token)?;
                    }
                    index.set_lock_token(None);
                }

                /*
                 * ロック情報の生成と登録
                 */
                let lock_info = LockInfo::new(page_id, &user_id);
                let token = lock_info.token();
                if !index.is_draft() {
                    index.set_lock_token(Some(token.clone()));
                    index_table.insert(page_id.clone(), index)?;
                }
                lock_table.insert(token, lock_info.clone())?;

                lock_info
            };

            /*
             * コミット
             */
            txn.commit()?;

            Ok((lock_info, target))
        })();
        result.map_err(|err| match audit_target {
            Some(target) => err.context(target),
            None => err,
        })
    }

    ///
    /// ロックの延長
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `user_name` - ユーザ名
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 更新したロック情報を返す。
    ///
    pub(crate) fn renew_page_lock(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: &LockToken,
    ) -> Result<LockInfo> {
        self.renew_page_lock_with_policy(page_id, user_name, Ok(token), None)
            .map(|(value, _)| value)
    }

    ///
    /// 最新対象を認可して既存ロック操作を実施する
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `user_name` - 操作ユーザ
    /// * `token` - ロック解除token
    /// * `policy` - 保存prefix制約
    ///
    /// # 戻り値
    /// 操作結果、または認可/既存条件エラー。
    ///
    pub(crate) fn renew_page_lock_authorized(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: Result<&LockToken, LockTokenInputError>,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<AuthorizedLockResult<LockInfo>> {
        self.renew_page_lock_with_policy(
            page_id, user_name, token, Some(policy),
        )
            .and_then(|(value, target)| {
                let target = target.ok_or_else(|| anyhow!(
                    "authorized lock target is missing"))?;
                Ok(AuthorizedLockResult { value, target })
            })
    }

    ///
    /// CLI互換と認可入口の共通ロック処理
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `user_name` - 操作ユーザ
    /// * `token` - ロック解除token
    /// * `policy` - REST認可制約、CLIの場合はNone
    ///
    /// # 戻り値
    /// 同一txnの操作結果を返す。
    ///
    /// # 概要
    /// 同txnの確定情報を保持し既存検証・副作用の順序を維持する。
    ///
    fn renew_page_lock_with_policy(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: Result<&LockToken, LockTokenInputError>,
        policy: Option<&PageAccessPolicy<'_>>,
    ) -> Result<(LockInfo, Option<PageIndex>)> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        /*
         * 最新対象の認可はlock条件や副作用より先に実施する
         */
        let target = if let Some(policy) = policy {
            let page = page_in_txn(&txn, page_id, None, false, policy)?;
            if page.index.deleted() {
                return Err(anyhow!(crate::database::DbError::PageDeleted)
                    .context(PageAccessTarget::from_page(&page)?));
            }
            Some(page.index)
        } else { None };
        let audit_target = target.as_ref()
            .map(PageAccessTarget::from_index).transpose()?;
        let result = (|| -> Result<_> {
        let token = token?;

            let lock_info = {
                let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
                let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
                let now = Local::now();

                /*
                 * ユーザIDの解決
                 */
                let user_id = {
                    let id_table = txn.open_table(USER_ID_TABLE)?;
                    let key = user_name.to_string();
                    match id_table.get(&key)? {
                        Some(id) => id.value(),
                        None => {
                            return Err(anyhow!(
                                crate::database::DbError::UserNotFound
                            ));
                        }
                    }
                };

                let mut index = match index_table.get(page_id.clone())? {
                    Some(entry) => entry.value(),
                    None => {
                        return Err(anyhow!(
                            crate::database::DbError::LockNotFound,
                        ));
                    }
                };

                /*
                 * トークン整合性の確認
                 */
                if index.is_draft() {
                    /*
                     * ドラフトページのロック更新
                     */
                    let mut lock_info = match lock_table.get(token.clone())? {
                        Some(lock_info) => lock_info.value(),
                        None => {
                            return Err(anyhow!(
                                crate::database::DbError::LockNotFound
                            ));
                        }
                    };

                    if lock_info.page() != *page_id {
                        return Err(anyhow!(
                            crate::database::DbError::LockForbidden
                        ));
                    }

                    if lock_info.expire() <= now {
                        lock_table.remove(token.clone())?;
                        return Err(anyhow!(
                            crate::database::DbError::LockNotFound,
                        ));
                    }

                    if lock_info.user() != user_id {
                        return Err(anyhow!(
                            crate::database::DbError::LockForbidden
                        ));
                    }

                    lock_info.renew();
                    let new_token = lock_info.token();
                    lock_table.remove(token.clone())?;
                    lock_table.insert(new_token.clone(), lock_info.clone())?;
                    lock_info
                } else {
                    let current = match index.lock_token() {
                        Some(current) => current,
                        None => token.clone(),
                    };

                    if current != *token {
                        return Err(anyhow!(
                            crate::database::DbError::LockForbidden
                        ));
                    }

                    /*
                     * ロック情報の取得と検証
                     */
                    let mut lock_info = match lock_table.get(token.clone())? {
                        Some(lock_info) => lock_info.value(),
                        None => {
                            index.set_lock_token(None);
                            index_table.insert(page_id.clone(), index)?;
                            return Err(anyhow!(
                                crate::database::DbError::LockNotFound
                            ));
                        }
                    };

                    if lock_info.expire() <= now {
                        lock_table.remove(token.clone())?;
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index)?;
                        return Err(anyhow!(
                            crate::database::DbError::LockNotFound,
                        ));
                    }

                    if lock_info.user() != user_id {
                        return Err(anyhow!(
                            crate::database::DbError::LockForbidden
                        ));
                    }

                    /*
                     * ロック情報の更新
                     */
                    lock_info.renew();
                    let new_token = lock_info.token();
                    lock_table.remove(token.clone())?;
                    lock_table.insert(new_token.clone(), lock_info.clone())?;
                    index.set_lock_token(Some(new_token));
                    index_table.insert(page_id.clone(), index)?;

                    lock_info
                }
            };

            /*
             * コミット
             */
            txn.commit()?;

            Ok((lock_info, target))
        })();
        result.map_err(|err| match audit_target {
            Some(target) => err.context(target),
            None => err,
        })
    }

    ///
    /// ロックの解除
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `user_name` - ユーザ名
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 解除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn release_page_lock(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: &LockToken,
    ) -> Result<()> {
        self.release_page_lock_with_policy(page_id, user_name, Ok(token), None)
            .map(|(value, _)| value)
    }

    ///
    /// 最新対象を認可して既存ロック操作を実施する
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `user_name` - 操作ユーザ
    /// * `token` - ロック解除token
    /// * `policy` - 保存prefix制約
    ///
    /// # 戻り値
    /// 操作結果、または認可/既存条件エラー。
    ///
    pub(crate) fn release_page_lock_authorized(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: Result<&LockToken, LockTokenInputError>,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<AuthorizedLockResult<()>> {
        self.release_page_lock_with_policy(
            page_id, user_name, token, Some(policy),
        )
            .and_then(|(value, target)| {
                let target = target.ok_or_else(|| anyhow!(
                    "authorized lock target is missing"))?;
                Ok(AuthorizedLockResult { value, target })
            })
    }

    ///
    /// CLI互換と認可入口の共通ロック処理
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `user_name` - 操作ユーザ
    /// * `token` - ロック解除token
    /// * `policy` - REST認可制約、CLIの場合はNone
    ///
    /// # 戻り値
    /// 同一txnの操作結果を返す。
    ///
    /// # 概要
    /// 同txnの確定情報を保持し既存検証・副作用の順序を維持する。
    ///
    fn release_page_lock_with_policy(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: Result<&LockToken, LockTokenInputError>,
        policy: Option<&PageAccessPolicy<'_>>,
    ) -> Result<((), Option<PageIndex>)> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        /*
         * 最新対象の認可はlock条件や副作用より先に実施する
         */
        let target = if let Some(policy) = policy {
            let page = page_in_txn(&txn, page_id, None, false, policy)?;
            if page.index.deleted() {
                return Err(anyhow!(crate::database::DbError::PageDeleted)
                    .context(PageAccessTarget::from_page(&page)?));
            }
            Some(page.index)
        } else { None };
        let audit_target = target.as_ref()
            .map(PageAccessTarget::from_index).transpose()?;
        let result = (|| -> Result<_> {
        let token = token?;

            let mut asset_ids: Vec<AssetId> = Vec::new();
            let mut delete_draft = false;
            let mut needs_commit = false;
            let mut result: Result<()> = Ok(());

            {
                let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
                let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
                let now = Local::now();

                /*
                 * ユーザIDの解決
                 */
                let user_id = {
                    let id_table = txn.open_table(USER_ID_TABLE)?;
                    let key = user_name.to_string();
                    match id_table.get(&key)? {
                        Some(id) => id.value(),
                        None => {
                            return Err(anyhow!(
                                crate::database::DbError::UserNotFound
                            ));
                        }
                    }
                };

                let mut index = match index_table.get(page_id.clone())? {
                    Some(entry) => entry.value(),
                    None => {
                        return Err(anyhow!(
                            crate::database::DbError::LockNotFound,
                        ));
                    }
                };

                /*
                 * ロック情報の検証
                 */
                let mut lock_info: Option<LockInfo> = None;
                match lock_table.get(token.clone())? {
                    Some(info) => lock_info = Some(info.value()),
                    None => {
                        if !index.is_draft() {
                            index.set_lock_token(None);
                            index_table.insert(page_id.clone(), index.clone())?;
                            needs_commit = true;
                        }
                        result = Err(anyhow!(
                            crate::database::DbError::LockNotFound
                        ));
                    }
                }

                if result.is_ok() {
                    let lock_info = lock_info.expect("lock info");
                    if lock_info.page() != *page_id {
                        result = Err(anyhow!(
                            crate::database::DbError::LockForbidden
                        ));
                    } else if lock_info.expire() <= now {
                        lock_table.remove(token.clone())?;
                        needs_commit = true;
                        if index.is_draft() {
                            delete_draft = true;
                        } else {
                            index.set_lock_token(None);
                            index_table.insert(page_id.clone(), index)?;
                        }
                        result = Err(anyhow!(
                            crate::database::DbError::LockNotFound
                        ));
                    } else if lock_info.user() != user_id {
                        result = Err(anyhow!(
                            crate::database::DbError::LockForbidden
                        ));
                    } else {
                        /*
                         * ロック情報の削除
                         */
                        lock_table.remove(token.clone())?;
                        needs_commit = true;
                        if index.is_draft() {
                            delete_draft = true;
                        } else {
                            index.set_lock_token(None);
                            index_table.insert(page_id.clone(), index)?;
                        }
                    }
                }
            }

            if delete_draft {
                asset_ids = delete_draft_in_txn(&txn, page_id)?;
                needs_commit = true;
            }

            if needs_commit {
                txn.commit()?;
            }

            for asset_id in asset_ids {
                let asset_path = self.asset_file_path(&asset_id);
                let _ = fs::remove_file(asset_path);
            }

            result.map(|_| ((), target))
        })();
        result.map_err(|err| match audit_target {
            Some(target) => err.context(target),
            None => err,
        })
    }

    ///
    /// ロック期限切れの掃除
    ///
    /// # 戻り値
    /// 削除したロック情報の件数を返す。
    ///
    pub(crate) fn cleanup_expired_locks(&self) -> Result<usize> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut draft_pages: Vec<PageId> = Vec::new();
        let (removed, mut needs_commit) = {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let now = Local::now();

            /*
             * 期限切れロックの収集
             */
            let mut expired = Vec::new();
            for entry in lock_table.iter()? {
                let (token, info) = entry?;
                let info = info.value();
                if info.expire() <= now {
                    expired.push((token.value().clone(), info.page()));
                }
            }

            /*
             * 期限切れが無ければ終了
             */
            if expired.is_empty() {
                Ok::<(usize, bool), anyhow::Error>((0, false))
            } else {
                /*
                 * 期限切れロックの削除
                 */
                let mut removed = 0usize;
                for (token, page_id) in expired {
                    let _ = lock_table.remove(token.clone())?;

                    let index = match index_table.get(page_id.clone())? {
                        Some(entry) => Some(entry.value()),
                        None => None,
                    };

                    if let Some(mut index) = index {
                        if index.is_draft() {
                            draft_pages.push(page_id.clone());
                        } else if index.lock_token() == Some(token.clone()) {
                            index.set_lock_token(None);
                            index_table.insert(page_id.clone(), index)?;
                        }
                    }

                    removed += 1;
                }

                Ok::<(usize, bool), anyhow::Error>((removed, true))
            }
        }?;

        /*
         * コミット
         */
        for page_id in draft_pages {
            let mut deleted = delete_draft_in_txn(&txn, &page_id)?;
            asset_ids.append(&mut deleted);
            needs_commit = true;
        }

        if needs_commit {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(removed)
    }

    ///
    /// ロック情報の一覧取得
    ///
    /// # 戻り値
    /// ロック情報の一覧を返す。
    ///
    pub(crate) fn list_locks(&self) -> Result<Vec<LockListEntry>> {
        /*
         * 期限切れロックの掃除
         */
        self.cleanup_expired_locks()?;

        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let lock_table = txn.open_table(LOCK_INFO_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let now = Local::now();

        /*
         * ページパスのマップ構築
         */
        let mut page_map = std::collections::HashMap::new();
        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            let index = index.value();
            page_map.insert(page_id.value().clone(), index.path());
        }

        /*
         * ユーザ名のマップ構築
         */
        let mut user_map = std::collections::HashMap::new();
        for entry in user_table.iter()? {
            let (id, info) = entry?;
            let info = info.value();
            user_map.insert(id.value().clone(), info.username());
        }

        /*
         * ロック情報の収集
         */
        let mut locks = Vec::new();
        for entry in lock_table.iter()? {
            let (token, info) = entry?;
            let info = info.value();
            if info.expire() <= now {
                continue;
            }

            let page_path = match page_map.get(&info.page()) {
                Some(path) => path.clone(),
                None => return Err(anyhow!("page not found")),
            };

            let user_name = match user_map.get(&info.user()) {
                Some(name) => name.clone(),
                None => return Err(anyhow!("user not found")),
            };

            locks.push(LockListEntry::new(
                token.value().clone(),
                info.page(),
                page_path,
                info.expire(),
                user_name,
            ));
        }

        Ok(locks)
    }

    ///
    /// ロック情報の削除
    ///
    /// # 引数
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 削除に成功した場合は`true`を返す。
    ///
    pub(crate) fn delete_lock(&self, token: &LockToken) -> Result<bool> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut removed = false;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut draft_to_delete: Option<PageId> = None;

        /*
         * ロック情報の削除とインデックス更新
         */
        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * ロック情報取得
             */
            let lock_info = match lock_table.get(token.clone())? {
                Some(info) => Some(info.value()),
                None => None,
            };

            if let Some(info) = lock_info {
                /*
                 * ロック情報の削除
                 */
                let _ = lock_table.remove(token.clone())?;
                let page_id = info.page();
                let index = match index_table.get(page_id.clone())? {
                    Some(entry) => Some(entry.value()),
                    None => None,
                };
                /*
                 * ページインデックスのロック解除
                 */
                if let Some(mut index) = index {
                    if index.is_draft() {
                        draft_to_delete = Some(page_id.clone());
                    } else if index.lock_token() == Some(token.clone()) {
                        index.set_lock_token(None);
                        index_table.insert(page_id, index)?;
                    }
                }

                removed = true;
            } else {
                /*
                 * インデックス側でロック解除を試行
                 */
                let mut target = None;
                for entry in index_table.iter()? {
                    let (page_id, index) = entry?;
                    let mut index = index.value();
                    if index.lock_token() == Some(token.clone()) {
                        index.set_lock_token(None);
                        target = Some((page_id.value().clone(), index));
                        removed = true;
                        break;
                    }
                }

                if let Some((page_id, index)) = target {
                    index_table.insert(page_id, index)?;
                }
            }
        }

        /*
         * ドラフト削除
         */
        if let Some(page_id) = draft_to_delete {
            let mut deleted = delete_draft_in_txn(&txn, &page_id)?;
            asset_ids.append(&mut deleted);
            removed = true;
        }

        /*
         * コミット
         */
        if removed {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(removed)
    }

    ///
    /// ページID指定でロックを削除する
    ///
    /// # 概要
    /// ページIDに紐付くロック情報を削除する。
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// ロック削除に成功した場合は`true`を返す。
    ///
    pub(crate) fn delete_page_lock_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<bool> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut removed = false;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut draft_to_delete: Option<PageId> = None;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Ok(false),
            };

            if let Some((token, _)) = find_lock_by_page(&lock_table, page_id)? {
                let _ = lock_table.remove(token.clone())?;
                if index.is_draft() {
                    draft_to_delete = Some(page_id.clone());
                } else if index.lock_token() == Some(token.clone()) {
                    let mut index = index;
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index)?;
                }
                removed = true;
            } else if index.is_draft() {
                draft_to_delete = Some(page_id.clone());
                removed = true;
            } else if index.lock_token().is_some() {
                let mut index = index;
                index.set_lock_token(None);
                index_table.insert(page_id.clone(), index)?;
                removed = true;
            }
        }

        if let Some(draft_page_id) = draft_to_delete {
            asset_ids = delete_draft_in_txn(&txn, &draft_page_id)?;
            removed = true;
        }

        if removed {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(removed)
    }
}

///
/// 認可済みtxn内で期限切れロックとdraftを整理する
///
/// # 引数
/// * `txn` - 認可済み対象のwrite txn
/// * `page_id` - 対象ID
///
/// # 戻り値
/// 有効lockとcommit後に除去するasset IDを返す。
///
/// # 概要
/// 呼び出し側の対象認可後にcleanupし、FS整理はcommit後へ渡す。
///
pub(super) fn cleanup_page_lock_in_txn(
    txn: &WriteTransaction,
    page_id: &PageId,
) -> Result<(Option<LockInfo>, Vec<AssetId>)> {
    /*
     * 同一txnから最新indexとlockを取得する
     */
    let mut result = None;
    let mut asset_ids: Vec<AssetId> = Vec::new();
    let mut delete_draft = false;

    {
        let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
        let now = Local::now();

        let mut index = {
            let entry = match index_table.get(page_id.clone())? {
                Some(entry) => entry,
                None => return Ok((result, asset_ids)),
            };
            entry.value()
        };

        /*
         * ロック情報の検証
         */
        if index.is_draft() {
            match find_lock_by_page(&lock_table, page_id)? {
                Some((token, lock_info)) => {
                    if lock_info.expire() <= now {
                        lock_table.remove(token)?;
                        delete_draft = true;
                    } else {
                        result = Some(lock_info);
                    }
                }
                None => {
                    delete_draft = true;
                }
            }
        } else if let Some(token) = index.lock_token() {
            let lock_info = match lock_table.get(token.clone())? {
                Some(info) => Some(info.value()),
                None => {
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index.clone())?;
                    None
                }
            };

            if let Some(lock_info) = lock_info {
                if lock_info.expire() <= now {
                    lock_table.remove(token)?;
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index.clone())?;
                } else {
                    result = Some(lock_info);
                }
            }
        }
    }

    /*
     * 必要なdraft除去を同一txnに含める
     */
    if delete_draft {
        asset_ids = delete_draft_in_txn(txn, page_id)?;
    }

    Ok((result, asset_ids))
}
