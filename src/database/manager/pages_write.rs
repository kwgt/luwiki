/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ情報の更新系操作を提供するモジュール
//!

use std::collections::HashSet;
use std::fs;

use anyhow::{anyhow, Result};
use chrono::Local;
use redb::{ReadableMultimapTable, ReadableTable};

use crate::database::link_refs::{build_link_refs, build_link_refs_with_table};
use crate::database::schema::{
    is_root_path, ASSET_GROUP_TABLE, ASSET_INFO_TABLE, ASSET_LOOKUP_TABLE,
    DELETED_PAGE_PATH_TABLE, DbError, LOCK_INFO_TABLE, PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE, PAGE_SOURCE_TABLE, USER_ID_TABLE,
};
use crate::database::txn_helpers::{
    collect_recursive_deleted_page_targets_in_txn,
    collect_recursive_page_ids_in_txn, collect_recursive_page_targets_in_txn,
    delete_draft_in_txn, delete_page_hard_in_txn, delete_page_soft_in_txn,
    find_lock_by_page, verify_page_lock_in_txn,
};
use crate::database::types::{
    AssetId, LockInfo, LockToken, PageId, PageIndex, PageSource, RenameInfo,
    UserId,
};
use super::DatabaseManager;

impl DatabaseManager {
    ///
    /// ページの作成
    ///
    /// # 引数
    /// * `path` - ページのパス
    /// * `user_id` - ページを作成したユーザID
    /// * `source` - ページソース
    ///
    /// # 戻り値
    /// 作成したページIDを返す。
    ///
    pub(crate) fn create_page<P, U>(
        &self,
        path: P,
        user_name: U,
        source: String,
    ) -> Result<PageId>
    where
        P: AsRef<str>,
        U: AsRef<str>,
    {
        let path = path.as_ref().to_string();
        let user_name = user_name.as_ref().to_string();
        let revision = 1u64;

        let txn = self.db.begin_write()?;
        let page_id = PageId::new();

        {
            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;

            /*
             * パス重複の確認
             */
            if path_table.get(&path)?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * 初期リネーム情報の生成
             */
            let link_refs = build_link_refs_with_table(&path_table, &path, &source)?;
            let rename_info = RenameInfo::new(
                None,
                path.clone(),
                link_refs,
            );
            let page_index = PageIndex::new_page(page_id.clone(), path.clone());
            let page_source = PageSource::new(source, user_id, rename_info);

            /*
             * インデックスとソースの登録
             */
            path_table.insert(&path, page_id.clone())?;
            index_table.insert(page_id.clone(), page_index)?;

            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            source_table.insert((page_id.clone(), revision), page_source)?;
        }

        txn.commit()?;

        Ok(page_id)
    }

    ///
    /// ドラフトページの作成(ロック取得を含む)
    ///
    /// # 概要
    /// ドラフトページを作成し、同時にロックを取得する。
    ///
    /// # 引数
    /// * `path` - ページパス
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// 作成したページIDとロック情報を返す。
    ///
    pub(crate) fn create_draft_page<P, U>(
        &self,
        path: P,
        user_name: U,
    ) -> Result<(PageId, LockInfo)>
    where
        P: AsRef<str>,
        U: AsRef<str>,
    {
        let path = path.as_ref().to_string();
        let user_name = user_name.as_ref().to_string();

        let txn = self.db.begin_write()?;
        let page_id = PageId::new();
        let lock_info = {
            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * パス重複の確認
             */
            if path_table.get(&path)?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * ドラフト情報の登録
             */
            let page_index = PageIndex::new_draft(page_id.clone(), path.clone());
            path_table.insert(&path, page_id.clone())?;
            index_table.insert(page_id.clone(), page_index)?;

            /*
             * ロック情報の登録
             */
            let lock_info = LockInfo::new(&page_id, &user_id);
            let token = lock_info.token();
            lock_table.insert(token, lock_info.clone())?;

            lock_info
        };

        txn.commit()?;

        Ok((page_id, lock_info))
    }

    ///
    /// ページの書き込み
    ///
    /// # 引数
    /// * `path` - ページのパス
    /// * `user` - ページを編集したユーザの名前
    /// * `source` - ページソース
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn put_page(
        &self,
        page_id: &PageId,
        user_name: &str,
        source: String,
        amend: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                if amend {
                    return Err(anyhow!(DbError::AmendForbidden));
                }

                let path = index.path();
                let link_refs = build_link_refs(&txn, &path, &source)?;
                let rename_info = RenameInfo::new(
                    None,
                    path.clone(),
                    link_refs,
                );
                let mut page_index = PageIndex::new_page(
                    page_id.clone(),
                    path,
                );
                if let Some((token, _)) =
                    find_lock_by_page(&lock_table, page_id)?
                {
                    page_index.set_lock_token(Some(token));
                }
                let page_source = PageSource::new(source, user_id, rename_info);

                index_table.insert(page_id.clone(), page_index)?;
                source_table.insert((page_id.clone(), 1), page_source)?;
            } else if amend {
                /*
                 * 最新リビジョンの更新
                 */
                let revision = index.latest();
                let mut page_source = match source_table.get(
                    (page_id.clone(), revision)
                )? {
                    Some(entry) => entry.value(),
                    None => {
                        return Err(anyhow!(
                            "page source not found"
                        ));
                    }
                };

                if page_source.user() != user_id {
                    return Err(anyhow!(DbError::AmendForbidden));
                }

                page_source.update_source(source);
                source_table.insert((page_id.clone(), revision), page_source)?;
            } else {
                /*
                 * 新規リビジョンの追加
                 */
                let revision = index.latest() + 1;
                let page_source = PageSource::new_revision(
                    revision,
                    source,
                    user_id,
                    None,
                );

                index.set_latest(revision);
                index_table.insert(page_id.clone(), index)?;
                source_table.insert((page_id.clone(), revision), page_source)?;
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの再帰的削除
    ///
    /// # 概要
    /// パスインデックスのレンジ走査で対象ページ配下を収集し、トランザクション
    /// 内で一括削除を行う。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    /// * `hard_delete` - ハードデリートを行う場合はtrue
    ///
    /// # 戻り値
    /// 成功時は対象ページID一覧を`Ok()`でラップして返す。
    ///
    pub(crate) fn delete_pages_recursive_by_id(
        &self,
        page_id: &PageId,
        hard_delete: bool,
    ) -> Result<Vec<PageId>> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids = Vec::new();

        let target_ids = {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * 起点ページの取得と検証
             */
            let mut base_index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if base_index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if base_index.deleted() && !hard_delete {
                return Err(anyhow!("page already deleted"));
            }

            let base_path = if let Some(path) = base_index.current_path() {
                path.to_string()
            } else if hard_delete {
                match base_index.last_deleted_path() {
                    Some(path) => path.to_string(),
                    None => return Err(anyhow!("page path not found")),
                }
            } else {
                return Err(anyhow!("page path not found"));
            };

            if is_root_path(&base_path) {
                return Err(anyhow!(DbError::RootPageProtected));
            }

            /*
             * 起点ページのロック検証
             */
            let now = Local::now();
            verify_page_lock_in_txn(
                page_id,
                &mut base_index,
                &mut index_table,
                &mut lock_table,
                &now,
            )?;

            /*
             * 再帰対象の収集
             */
            let mut targets = collect_recursive_page_ids_in_txn(
                &mut path_table,
                &mut index_table,
                &mut lock_table,
                &base_path,
            )?;

            if targets.iter().all(|id| id != page_id) {
                targets.insert(0, page_id.clone());
            }

            let target_ids = targets.clone();

            /*
             * ページ削除の実行
             */
            if hard_delete {
                let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
                for target_id in targets.iter() {
                    delete_page_hard_in_txn(
                        target_id,
                        &mut path_table,
                        &mut deleted_path_table,
                        &mut index_table,
                        &mut source_table,
                        &mut lock_table,
                        &mut asset_table,
                        &mut lookup_table,
                        &mut group_table,
                        &mut asset_ids,
                    )?;
                }
            } else {
                for target_id in targets.iter() {
                    delete_page_soft_in_txn(
                        target_id,
                        &mut path_table,
                        &mut deleted_path_table,
                        &mut index_table,
                        &mut lock_table,
                        &mut asset_table,
                        &mut lookup_table,
                        &mut group_table,
                    )?;
                }
            }

            target_ids
        };

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        if hard_delete {
            for asset_id in asset_ids {
                let asset_path = self.asset_file_path(&asset_id);
                let _ = fs::remove_file(asset_path);
            }
        }

        Ok(target_ids)
    }

    ///
    /// ページソースのロールバック(ソースのみ)
    ///
    /// # 概要
    /// 指定リビジョンより新しいソースを削除し、最新リビジョン番号を更新する。
    /// パスやリネーム履歴は変更しない。
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    /// * `rollback_to` - ロールバック先のリビジョン番号
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn rollback_page_source_only(
        &self,
        page_id: &PageId,
        rollback_to: u64,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * ページ情報取得と検証
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if index.deleted() {
                return Err(anyhow!(DbError::PageDeleted));
            }

            let latest = index.latest();
            let earliest = index.earliest();
            if rollback_to < earliest || rollback_to > latest {
                return Err(anyhow!(DbError::InvalidRevision));
            }

            /*
             * ロック検証
             */
            let now = Local::now();
            verify_page_lock_in_txn(
                page_id,
                &mut index,
                &mut index_table,
                &mut lock_table,
                &now,
            )?;

            /*
             * ソース削除
             */
            for revision in (rollback_to + 1)..=latest {
                let _ = source_table.remove((page_id.clone(), revision))?;
            }

            /*
             * 最新リビジョン更新
             */
            index.set_latest(rollback_to);
            index_table.insert(page_id.clone(), index)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページソースのコンパクション
    ///
    /// # 概要
    /// 指定リビジョンより過去のソースを削除し、最古リビジョン番号を更新する。
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    /// * `keep_from` - 保持する下限リビジョン番号
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn compact_page_source(
        &self,
        page_id: &PageId,
        keep_from: u64,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * ページ情報取得と検証
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if index.deleted() {
                return Err(anyhow!(DbError::PageDeleted));
            }

            let latest = index.latest();
            let earliest = index.earliest();
            if keep_from < earliest || keep_from > latest {
                return Err(anyhow!(DbError::InvalidRevision));
            }

            /*
             * ロック検証
             */
            let now = Local::now();
            verify_page_lock_in_txn(
                page_id,
                &mut index,
                &mut index_table,
                &mut lock_table,
                &now,
            )?;

            /*
             * ソース削除
             */
            if keep_from > earliest {
                for revision in earliest..=(keep_from - 1) {
                    let _ = source_table.remove((page_id.clone(), revision))?;
                }
            }

            /*
             * 最古リビジョン更新
             */
            index.set_earliest(keep_from);
            index_table.insert(page_id.clone(), index)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの再帰的リネーム
    ///
    /// # 概要
    /// 指定ページと配下ページのパスを一括で更新する。
    ///
    /// # 引数
    /// * `page_id` - 起点となるページID
    /// * `rename_to` - 移動先のパス
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn rename_pages_recursive_by_id(
        &self,
        page_id: &PageId,
        rename_to: &str,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * 起点ページの取得と検証
             */
            let base_index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if base_index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            let rename_to = if
                let Some(suffix) = Self::get_suffix(base_index.path())
            {
                if rename_to.ends_with('/') {
                    format!("{}{}", rename_to, suffix)
                } else {
                    rename_to.to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            };

            let base_path = match base_index.current_path() {
                Some(path) => path.to_string(),
                None => return Err(anyhow!("page path not found")),
            };

            if is_root_path(&base_path) {
                return Err(anyhow!(DbError::RootPageProtected));
            }

            if base_path == rename_to {
                return Ok(());
            }

            let base_prefix = Self::build_recursive_prefix(&base_path);
            if rename_to.starts_with(&base_prefix) {
                return Err(anyhow!(DbError::InvalidMoveDestination));
            }

            /*
             * 再帰対象の収集
             */
            let targets = collect_recursive_page_targets_in_txn(
                &mut path_table,
                &mut index_table,
                &mut lock_table,
                &base_path,
            )?;

            let target_ids: HashSet<PageId> =
                targets.iter().map(|item| item.page_id.clone()).collect();

            /*
             * 衝突チェックと移動パス生成
             */
            let mut new_paths = HashSet::new();
            let mut mappings: Vec<(PageId, String, String)> = Vec::new();
            for target in &targets {
                let new_path = if target.path == base_path {
                    rename_to.clone()
                } else {
                    let suffix = target
                        .path
                        .strip_prefix(&base_prefix)
                        .unwrap_or("");

                    format!("{}/{}", rename_to, suffix)
                };

                if !new_paths.insert(new_path.clone()) {
                    return Err(anyhow!(DbError::PageAlreadyExists));
                }

                if let Some(existing) = path_table.get(&new_path)? {
                    let existing_id = existing.value();
                    if !target_ids.contains(&existing_id) {
                        return Err(anyhow!(DbError::PageAlreadyExists));
                    }
                }

                mappings.push((
                    target.page_id.clone(),
                    target.path.clone(),
                    new_path,
                ));
            }

            /*
             * リネーム実行
             */
            for (target_id, src_path, new_path) in mappings {
                let mut index = match index_table.get(target_id.clone())? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!(DbError::PageNotFound)),
                };

                if index.is_draft() {
                    return Err(anyhow!(DbError::PageNotFound));
                }

                let latest = index.latest();
                let latest_source = match source_table.get(
                    (target_id.clone(), latest)
                )? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!("page source not found")),
                };

                let link_refs = {
                    let path_table = &path_table;
                    build_link_refs_with_table(
                        path_table,
                        &src_path,
                        &latest_source.source(),
                    )?
                };
                let rename_info = RenameInfo::new(
                    Some(src_path.clone()),
                    new_path.clone(),
                    link_refs,
                );
                let revision = latest + 1;
                let page_source = PageSource::new_revision(
                    revision,
                    latest_source.source(),
                    latest_source.user(),
                    Some(rename_info),
                );

                index.set_latest(revision);
                index.set_path(new_path.clone());
                index.push_rename_revision(revision);
                index_table.insert(target_id.clone(), index)?;
                source_table.insert((target_id.clone(), revision), page_source)?;

                path_table.remove(&src_path)?;
                path_table.insert(&new_path, target_id.clone())?;
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの削除
    ///
    /// # 概要
    /// ページをソフトデリートし、関連アセットも削除状態にする。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn delete_page_by_id(&self, page_id: &PageId) -> Result<()>
    {
        if let Some(index) = self.get_page_index_by_id(page_id)? {
            if index.is_draft() {
                return self.delete_draft_by_id(page_id);
            }
        }

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            delete_page_soft_in_txn(
                page_id,
                &mut path_table,
                &mut deleted_path_table,
                &mut index_table,
                &mut lock_table,
                &mut asset_table,
                &mut lookup_table,
                &group_table,
            )?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ロック解除トークン付きページ削除
    ///
    /// # 概要
    /// ロック検証とページ削除を単一トランザクション内で実行する。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    /// * `user_id` - 操作ユーザID
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn delete_page_by_id_with_lock_token(
        &self,
        page_id: &PageId,
        user_id: &UserId,
        token: Option<&LockToken>,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids = Vec::new();

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let now = Local::now();

            /*
             * ページ情報取得
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            /*
             * ロック検証
             */
            if index.is_draft() {
                if let Some((lock_token, lock_info)) =
                    find_lock_by_page(&lock_table, page_id)?
                {
                    if lock_info.expire() > now {
                        let provided = match token {
                            Some(token) => token,
                            None => return Err(anyhow!(DbError::PageLocked)),
                        };
                        if *provided != lock_token {
                            return Err(anyhow!(DbError::LockForbidden));
                        }
                        if lock_info.user() != *user_id {
                            return Err(anyhow!(DbError::LockForbidden));
                        }
                    }
                    lock_table.remove(lock_token)?;
                }
            } else if let Some(lock_token) = index.lock_token() {
                let mut lock_user = None;
                let mut lock_expired = false;
                let mut lock_exists = false;
                if let Some(lock_info) = lock_table.get(lock_token.clone())? {
                    let lock_info = lock_info.value();
                    lock_exists = true;
                    lock_expired = lock_info.expire() <= now;
                    lock_user = Some(lock_info.user());
                }

                if lock_exists && !lock_expired {
                    let provided = match token {
                        Some(token) => token,
                        None => return Err(anyhow!(DbError::PageLocked)),
                    };
                    if *provided != lock_token {
                        return Err(anyhow!(DbError::LockForbidden));
                    }
                    if lock_user != Some(user_id.clone()) {
                        return Err(anyhow!(DbError::LockForbidden));
                    }
                } else if lock_exists && lock_expired {
                    lock_table.remove(lock_token.clone())?;
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index.clone())?;
                } else {
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index.clone())?;
                }
            }

            /*
             * ページ削除
             */
            if index.is_draft() {
                asset_ids = delete_draft_in_txn(&txn, page_id)?;
            } else {
                delete_page_soft_in_txn(
                    page_id,
                    &mut path_table,
                    &mut deleted_path_table,
                    &mut index_table,
                    &mut lock_table,
                    &mut asset_table,
                    &mut lookup_table,
                    &group_table,
                )?;
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(())
    }

    ///
    /// ドラフトページの削除
    ///
    /// # 概要
    /// ドラフトページを削除し、関連アセットも削除する。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn delete_draft_by_id(&self, page_id: &PageId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let asset_ids: Vec<AssetId>;

        {
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut tokens = Vec::new();
            for entry in lock_table.iter()? {
                let (token, info) = entry?;
                let info = info.value();
                if info.page() == *page_id {
                    tokens.push(token.value().clone());
                }
            }

            for token in tokens {
                let _ = lock_table.remove(token);
            }
        }

        asset_ids = delete_draft_in_txn(&txn, page_id)?;

        txn.commit()?;

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(())
    }

    ///
    /// ページの復帰
    ///
    /// # 概要
    /// ページを削除状態から復帰し、必要であれば付随アセットを復帰する。
    ///
    /// # 引数
    /// * `page_id` - 復帰対象のページID
    /// * `with_assets` - 付随アセットの復帰を行う場合はtrue
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn undelete_page_by_id(
        &self,
        page_id: &PageId,
        restore_to: &str,
        with_assets: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * ページ情報取得と削除状態の判定
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if !index.deleted() {
                return Err(anyhow!("page not deleted"));
            }

            if path_table.get(&restore_to.to_string())?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            let restore_to = if
                let Some(suffix) = Self::get_suffix(index.path())
            {
                if restore_to.ends_with('/') {
                    format!("{}{}", restore_to, suffix)
                } else {
                    restore_to.to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            };

            /*
             * 削除フラグの更新
             */
            let deleted_path = match index.last_deleted_path() {
                Some(path) => path.to_string(),
                None => return Err(anyhow!("deleted path not found")),
            };
            index.set_path(restore_to.to_string());
            index_table.insert(page_id.clone(), index)?;
            let _ = deleted_path_table.remove(deleted_path, page_id.clone())?;
            let _ = path_table.insert(restore_to, page_id.clone())?;

            if with_assets {
                /*
                 * 付随アセットの復帰
                 */
                for entry in group_table.get(page_id.clone())? {
                    let asset_id = entry?.value();
                    let mut asset_info = match asset_table.get(asset_id.clone())? {
                        Some(info) => info.value(),
                        None => return Err(anyhow!("asset info not found")),
                    };

                    if !asset_info.deleted() || asset_info.page_id().is_some() {
                        continue;
                    }

                    let file_name = asset_info.file_name();
                    asset_info.set_deleted(false);
                    asset_info.set_page_id(page_id.clone());
                    asset_table.insert(asset_id.clone(), asset_info)?;
                    let _ = lookup_table.insert(
                        (page_id.clone(), file_name),
                        asset_id.clone(),
                    )?;
                }
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの再帰的復帰
    ///
    /// # 概要
    /// 指定ページと配下ページを削除状態から復帰する。
    ///
    /// # 引数
    /// * `page_id` - 復帰対象のページID
    /// * `restore_to` - 復帰先のパス
    /// * `with_assets` - 付随アセットの復帰を行う場合はtrue
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn undelete_pages_recursive_by_id(
        &self,
        page_id: &PageId,
        restore_to: &str,
        with_assets: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * 起点ページの取得と検証
             */
            let base_index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if base_index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if !base_index.deleted() {
                return Err(anyhow!("page not deleted"));
            }

            let restore_to = if
                let Some(suffix) = Self::get_suffix(base_index.path())
            {
                if restore_to.ends_with('/') {
                    format!("{}{}", restore_to, suffix)
                } else {
                    restore_to.to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            };

            let base_deleted_path = match base_index.last_deleted_path() {
                Some(path) => path.to_string(),
                None => return Err(anyhow!("deleted path not found")),
            };

            if path_table.get(&restore_to.to_string())?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * 再帰対象の収集
             */
            let targets = collect_recursive_deleted_page_targets_in_txn(
                &mut deleted_path_table,
                &mut index_table,
                &mut lock_table,
                &base_deleted_path,
            )?;

            /*
             * 衝突チェックと復帰先生成
             */
            let base_prefix = Self::build_recursive_prefix(&base_deleted_path);
            let mut new_paths = HashSet::new();
            let mut mappings: Vec<(PageId, String, String)> = Vec::new();

            for target in &targets {
                let new_path = if target.path == base_deleted_path {
                    restore_to.clone()
                } else {
                    let suffix = target
                        .path
                        .strip_prefix(&base_prefix)
                        .unwrap_or("");

                    format!("{}/{}", restore_to, suffix)
                };

                if !new_paths.insert(new_path.clone()) {
                    return Err(anyhow!(DbError::PageAlreadyExists));
                }

                if path_table.get(&new_path)?.is_some() {
                    return Err(anyhow!(DbError::PageAlreadyExists));
                }

                mappings.push((
                    target.page_id.clone(),
                    target.path.clone(),
                    new_path,
                ));
            }

            /*
             * 復帰実行
             */
            for (target_id, deleted_path, new_path) in mappings {
                let mut index = match index_table.get(target_id.clone())? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!(DbError::PageNotFound)),
                };

                if !index.deleted() {
                    return Err(anyhow!("page not deleted"));
                }

                index.set_path(new_path.to_string());
                index_table.insert(target_id.clone(), index)?;
                let _ = deleted_path_table.remove(deleted_path, target_id.clone())?;
                let _ = path_table.insert(new_path, target_id.clone())?;

                if with_assets {
                    /*
                     * 付随アセットの復帰
                     */
                    for entry in group_table.get(target_id.clone())? {
                        let asset_id = entry?.value();
                        let mut asset_info = match asset_table.get(asset_id.clone())? {
                            Some(info) => info.value(),
                            None => return Err(anyhow!("asset info not found")),
                        };

                        if !asset_info.deleted() || asset_info.page_id().is_some() {
                            continue;
                        }

                        let file_name = asset_info.file_name();
                        asset_info.set_deleted(false);
                        asset_info.set_page_id(target_id.clone());
                        asset_table.insert(asset_id.clone(), asset_info)?;
                        let _ = lookup_table.insert(
                            (target_id.clone(), file_name),
                            asset_id.clone(),
                        )?;
                    }
                }
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページのハードデリート
    ///
    /// # 概要
    /// ページの全リビジョンとインデックスを削除し、関連アセットの参照を解消する。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn delete_page_by_id_hard(
        &self,
        page_id: &PageId,
    ) -> Result<()> {
        if let Some(index) = self.get_page_index_by_id(page_id)? {
            if index.is_draft() {
                return self.delete_draft_by_id(page_id);
            }
        }

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids = Vec::new();

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            delete_page_hard_in_txn(
                page_id,
                &mut path_table,
                &mut deleted_path_table,
                &mut index_table,
                &mut source_table,
                &mut lock_table,
                &mut asset_table,
                &mut lookup_table,
                &mut group_table,
                &mut asset_ids,
            )?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(())
    }

    ///
    /// ページの書き込み
    ///
    /// # 引数
    /// * `path` - 削除対象のパス
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn remove_page<S>(path: S) -> Result<()>
    where
        S: AsRef<str>,
    {
        if is_root_path(path.as_ref()) {
            return Err(anyhow!(DbError::RootPageProtected));
        }

        todo!()
    }

    ///
    /// ページのリネーム
    ///
    /// # 引数
    /// * `path` - リネーム対象のページのパス
    /// * `dst_path` - リネーム後のパス
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn rename_page<S>(&self, path: S, dst_path: S) -> Result<()>
    where
        S: AsRef<str>,
    {
        if is_root_path(path.as_ref()) {
            return Err(anyhow!(DbError::RootPageProtected));
        }

        let path = path.as_ref().to_string();

        let dst_path = {
            if let Some(suffix) = Self::get_suffix(&path) {
                if dst_path.as_ref().ends_with("/") {
                    format!("{}{}", dst_path.as_ref(), suffix)
                } else {
                    dst_path.as_ref().to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            }
        };

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;

            /*
             * リネーム対象の解決と競合チェック
             */
            let page_id = match path_table.get(&path)? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if path_table.get(&dst_path)?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * 最新リビジョンとソース取得
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageNotFound));
            }

            let latest = index.latest();
            let latest_source = match source_table.get(
                (page_id.clone(), latest)
            )? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!("page source not found")),
            };

            /*
             * リネーム情報と新リビジョン生成
             */
            let link_refs = build_link_refs_with_table(
                &path_table,
                &path,
                &latest_source.source(),
            )?;
            let rename_info = RenameInfo::new(
                Some(path.clone()),
                dst_path.clone(),
                link_refs,
            );

            let revision = latest + 1;
            let page_source = PageSource::new_revision(
                revision,
                latest_source.source(),
                latest_source.user(),
                Some(rename_info),
            );

            /*
             * インデックス更新とパスインデックス書き換え
             */
            index.set_latest(revision);
            index.set_path(dst_path.clone());
            index.push_rename_revision(revision);
            index_table.insert(page_id.clone(), index)?;
            source_table.insert((page_id.clone(), revision), page_source)?;

            path_table.remove(&path)?;
            path_table.insert(&dst_path, page_id)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// 再帰判定用のパスプレフィックス生成
    ///
    /// # 引数
    /// * `base_path` - 起点パス
    ///
    /// # 戻り値
    /// 配下判定用のプレフィックスを返す。
    ///
    fn build_recursive_prefix(base_path: &str) -> String {
        let base = base_path.trim_end_matches('/');
        format!("{}/", base)
    }

    ///
    /// パス文字列の終端ページ名(サフィックスエレメント)を取得する
    ///
    /// # 引数
    /// * `path` - 対象のパス
    ///
    /// # 戻り値
    /// 取得できた終端ページ名を`Some()`でラップして返す。引数`path`で渡された
    /// パスの終端がパスセパレータ("/")の場合は`None`を返す。
    ///
    fn get_suffix<S>(path: S) -> Option<String>
    where
        S: AsRef<str>,
    {
        path.as_ref().rsplit('/').find(|s| !s.is_empty()).map(|s| s.to_string())
    }
}
