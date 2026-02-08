/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセット関連の操作を提供するモジュール
//!

use std::fs;

use anyhow::{anyhow, Result};
use redb::{ReadableDatabase, ReadableTable};

use crate::database::entries::{AssetListEntry, AssetMoveResult};
use crate::database::schema::{
    ASSET_GROUP_TABLE, ASSET_INFO_TABLE, ASSET_LOOKUP_TABLE, PAGE_INDEX_TABLE,
    USER_ID_TABLE, USER_INFO_TABLE,
};
use crate::database::types::{AssetId, AssetInfo, PageId};
use super::DatabaseManager;

impl DatabaseManager {
    ///
    /// アセット情報の一覧取得
    ///
    /// # 戻り値
    /// アセット情報の一覧を返す。
    ///
    pub(crate) fn list_assets(&self) -> Result<Vec<AssetListEntry>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(ASSET_INFO_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let mut assets = Vec::new();

        /*
         * ページパスマップの構築
         */
        let mut page_map = std::collections::HashMap::new();
        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            page_map.insert(page_id.value().clone(), index.value().path());
        }

        /*
         * アセット情報の収集
         */
        for entry in info_table.iter()? {
            let (asset_id, asset_info) = entry?;
            let asset_id = asset_id.value().clone();
            let asset_info = asset_info.value();
            let user_id = asset_info.user();
            let user_info = user_table
                .get(user_id.clone())?
                .ok_or_else(|| anyhow!("user not found"))?
                .value();
            let page_path = asset_info
                .page_id()
                .and_then(|page_id| page_map.get(&page_id).cloned());

            assets.push(AssetListEntry::new(
                asset_id,
                asset_info.file_name(),
                asset_info.mime(),
                asset_info.size(),
                asset_info.timestamp(),
                user_info.username(),
                page_path,
                asset_info.deleted(),
            ));
        }

        Ok(assets)
    }

    ///
    /// ページ内のアセットIDを取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `file_name` - ファイル名
    ///
    /// # 戻り値
    /// 解決できたアセットIDを返す。存在しない場合は`None`を返す。
    ///
    pub(crate) fn get_asset_id_by_page_file(
        &self,
        page_id: &PageId,
        file_name: &str,
    ) -> Result<Option<AssetId>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ASSET_LOOKUP_TABLE)?;
        let key = (page_id.clone(), file_name.to_string());
        Ok(table.get(key)?.map(|entry| entry.value()))
    }

    ///
    /// アセット情報の取得
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(AssetInfo))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_asset_info_by_id(
        &self,
        asset_id: &AssetId,
    ) -> Result<Option<AssetInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ASSET_INFO_TABLE)?;
        Ok(table.get(asset_id.clone())?.map(|entry| entry.value()))
    }

    ///
    /// ページ所属アセット情報の一覧取得
    ///
    /// # 概要
    /// ページに紐付くアセット情報を収集する。
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// アセット情報の一覧を返す。
    ///
    pub(crate) fn list_page_assets(
        &self,
        page_id: &PageId,
    ) -> Result<Vec<AssetInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
        let info_table = txn.open_table(ASSET_INFO_TABLE)?;
        let mut assets = Vec::new();

        /*
         * アセット情報の収集
         */
        for entry in group_table.get(page_id.clone())? {
            let asset_id = entry?.value();
            let asset_info = info_table
                .get(asset_id.clone())?
                .ok_or_else(|| anyhow!("asset info not found"))?
                .value();
            assets.push(asset_info);
        }

        Ok(assets)
    }

    ///
    /// 削除済みアセットの存在確認
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `file_name` - ファイル名
    ///
    /// # 戻り値
    /// 削除済みアセットが存在する場合は`true`を返す。
    ///
    pub(crate) fn has_deleted_asset_by_page_file(
        &self,
        page_id: &PageId,
        file_name: &str,
    ) -> Result<bool> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
        let info_table = txn.open_table(ASSET_INFO_TABLE)?;

        for entry in group_table.get(page_id.clone())? {
            let asset_id = entry?.value();
            let asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => continue,
            };

            if !asset_info.deleted() {
                continue;
            }

            if asset_info.file_name() == file_name {
                return Ok(true);
            }
        }

        Ok(false)
    }

    ///
    /// アセットデータの取得
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// アセットデータを返す。
    ///
    pub(crate) fn read_asset_data(
        &self,
        asset_id: &AssetId,
    ) -> Result<Vec<u8>> {
        /*
         * アセットパスの生成
         */
        let path = self.asset_file_path(asset_id);
        Ok(fs::read(path)?)
    }

    ///
    /// アセットの作成
    ///
    /// # 概要
    /// アセット情報の登録とファイル保存を行う。
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `file_name` - ファイル名
    /// * `mime` - MIME種別
    /// * `user_name` - 登録ユーザ名
    /// * `data` - アセットデータ
    ///
    /// # 戻り値
    /// 作成したアセットIDを返す。
    ///
    pub(crate) fn create_asset(
        &self,
        page_id: &PageId,
        file_name: &str,
        mime: &str,
        user_name: &str,
        data: &[u8],
    ) -> Result<AssetId> {
        /*
         * 事前情報の整形
         */
        let file_name = file_name.to_string();
        let mime = mime.to_string();
        let size = data.len() as u64;

        /*
         * アセットファイルの保存
         */
        let asset_id = AssetId::new();
        let asset_path = self.asset_file_path(&asset_id);
        if let Some(parent) = asset_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&asset_path, data)?;

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let insert_result = (|| -> Result<()> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let index_table = txn.open_table(PAGE_INDEX_TABLE)?;

            /*
             * ページ存在確認
             */
            if index_table.get(page_id.clone())?.is_none() {
                return Err(anyhow!(crate::database::DbError::PageNotFound));
            }

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(crate::database::DbError::UserNotFound)),
                }
            };

            /*
             * 既存アセットの確認
             */
            let lookup_key = (page_id.clone(), file_name.clone());
            let conflict_id = match lookup_table.get(&lookup_key)? {
                Some(entry) => Some(entry.value().clone()),
                None => None,
            };

            if let Some(conflict_id) = conflict_id {
                let deleted_conflict = match info_table.get(conflict_id.clone())? {
                    Some(info) => info.value().deleted(),
                    None => false,
                };

                if deleted_conflict {
                    let _ = lookup_table.remove(lookup_key.clone());
                } else {
                    return Err(anyhow!(crate::database::DbError::AssetAlreadyExists));
                }
            }

            /*
             * アセット情報の登録
             */
            let asset_info = AssetInfo::new(
                asset_id.clone(),
                page_id.clone(),
                file_name.clone(),
                mime,
                size,
                user_id,
            );
            info_table.insert(asset_id.clone(), asset_info)?;
            lookup_table.insert(lookup_key, asset_id.clone())?;
            let _ = group_table.insert(page_id.clone(), asset_id.clone())?;

            Ok(())
        })();

        /*
         * 登録失敗時の巻き戻し
         */
        if let Err(err) = insert_result {
            let _ = fs::remove_file(&asset_path);
            return Err(err);
        }

        /*
         * コミット
         */
        if let Err(err) = txn.commit() {
            let _ = fs::remove_file(&asset_path);
            return Err(err.into());
        }

        Ok(asset_id)
    }

    ///
    /// アセットの削除
    ///
    /// # 概要
    /// アセット情報の削除フラグを更新する。
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 削除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn delete_asset(&self, asset_id: &AssetId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let delete_result = (|| -> Result<()> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(crate::database::DbError::AssetNotFound)),
            };

            /*
             * 削除済み判定
             */
            if asset_info.deleted() {
                return Err(anyhow!(crate::database::DbError::AssetDeleted));
            }

            /*
             * 削除フラグ更新
             */
            let page_id = asset_info.page_id();
            let file_name = asset_info.file_name();
            asset_info.set_deleted(true);
            info_table.insert(asset_id.clone(), asset_info)?;

            /*
             * lookup解除
             */
            if let Some(page_id) = page_id {
                let lookup_key = (page_id, file_name);
                let _ = lookup_table.remove(lookup_key);
            } else {
                let mut remove_keys = Vec::new();
                for entry in lookup_table.iter()? {
                    let (key, value) = entry?;
                    if value.value() == asset_id.clone() {
                        remove_keys.push(key.value());
                    }
                }
                for (page_id, file_name) in remove_keys {
                    let _ = lookup_table.remove((page_id, file_name));
                }
            }

            Ok(())
        })();

        if let Err(err) = delete_result {
            return Err(err);
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// アセットのハードデリート
    ///
    /// # 概要
    /// アセット情報と関連テーブルを削除し、ファイルも削除する。
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 削除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn delete_asset_hard(&self, asset_id: &AssetId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let delete_result = (|| -> Result<Option<(PageId, String)>> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * 対象アセットの取得
             */
            let asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(crate::database::DbError::AssetNotFound)),
            };

            /*
             * 参照の削除
             */
            let page_id = asset_info.page_id();
            let file_name = asset_info.file_name();
            if let Some(page_id) = page_id.clone() {
                let lookup_key = (page_id.clone(), file_name.clone());
                let _ = lookup_table.remove(lookup_key);
                let _ = group_table.remove(page_id, asset_id.clone());
            } else {
                let mut remove_keys = Vec::new();
                for entry in lookup_table.iter()? {
                    let (key, value) = entry?;
                    if value.value() == asset_id.clone() {
                        remove_keys.push(key.value());
                    }
                }
                for (page_id, file_name) in remove_keys {
                    let _ = lookup_table.remove((page_id.clone(), file_name));
                    let _ = group_table.remove(page_id, asset_id.clone());
                }
            }

            /*
             * 情報の削除
             */
            let _ = info_table.remove(asset_id.clone())?;

            Ok(page_id.map(|id| (id, file_name)))
        })();

        if let Err(err) = delete_result {
            return Err(err);
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        let asset_path = self.asset_file_path(asset_id);
        let _ = fs::remove_file(asset_path);

        Ok(())
    }

    ///
    /// アセットの復帰
    ///
    /// # 概要
    /// アセット情報の削除フラグを解除する。
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 復帰に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn undelete_asset(
        &self,
        asset_id: &AssetId,
        new_name: Option<&str>,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let update_result = (|| -> Result<()> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(crate::database::DbError::AssetNotFound)),
            };

            if !asset_info.deleted() {
                return Err(anyhow!(crate::database::DbError::AssetAlreadyExists));
            }

            let target_name = new_name
                .unwrap_or(asset_info.file_name().as_str())
                .to_string();

            if let Some(page_id) = asset_info.page_id() {
                let lookup_key = (page_id.clone(), target_name.clone());
                if lookup_table.get(&lookup_key)?.is_some() {
                    return Err(anyhow!(crate::database::DbError::AssetAlreadyExists));
                }

                let current_name = asset_info.file_name();
                if current_name != target_name {
                    let _ = lookup_table.remove((page_id.clone(), current_name));
                }
                asset_info.set_file_name(target_name.clone());
                lookup_table.insert(lookup_key, asset_id.clone())?;
            } else if asset_info.file_name() != target_name {
                asset_info.set_file_name(target_name);
            }

            asset_info.set_deleted(false);
            info_table.insert(asset_id.clone(), asset_info)?;
            Ok(())
        })();

        if let Err(err) = update_result {
            return Err(err);
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// アセットの移動
    ///
    /// # 概要
    /// アセットの所属ページを移動する。
    ///
    /// # 引数
    /// * `asset_id` - 移動対象のアセットID
    /// * `dst_page_id` - 移動先ページID
    /// * `force` - 移動先競合を許可する場合はtrue
    ///
    /// # 戻り値
    /// 移動に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn move_asset(
        &self,
        asset_id: &AssetId,
        dst_page_id: &PageId,
        force: bool,
    ) -> Result<AssetMoveResult> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut conflict_asset: Option<AssetId> = None;

        let move_result = (|| -> Result<AssetMoveResult> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let index_table = txn.open_table(PAGE_INDEX_TABLE)?;

            /*
             * 移動先ページの存在確認
             */
            let dst_index = match index_table.get(dst_page_id.clone())? {
                Some(index) => index.value(),
                None => return Ok(AssetMoveResult::PageNotFound),
            };

            if dst_index.deleted() {
                if !force {
                    return Ok(AssetMoveResult::PageDeleted);
                }
            }

            /*
             * 対象アセットの取得
             */
            let mut asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(crate::database::DbError::AssetNotFound)),
            };

            let file_name = asset_info.file_name();
            let lookup_key = (dst_page_id.clone(), file_name.clone());

            let conflict_id = lookup_table
                .get(&lookup_key)?
                .map(|entry| entry.value());
            if let Some(conflict_id) = conflict_id {
                if conflict_id == asset_id.clone() {
                    // 同一アセットの再指定は競合として扱わない
                } else if !force {
                    return Ok(AssetMoveResult::NameConflict);
                } else {
                    conflict_asset = Some(conflict_id.clone());
                    let _ = lookup_table.remove(lookup_key.clone());
                    let _ = group_table.remove(dst_page_id.clone(), conflict_id.clone());
                    let _ = info_table.remove(conflict_id)?;
                }
            }

            /*
             * 旧所属の参照解除
             */
            if let Some(src_page_id) = asset_info.page_id() {
                let src_key = (src_page_id.clone(), file_name.clone());
                let _ = lookup_table.remove(src_key);
                let _ = group_table.remove(src_page_id, asset_id.clone());
            }

            /*
             * 新所属の登録
             */
            asset_info.set_page_id(dst_page_id.clone());
            info_table.insert(asset_id.clone(), asset_info)?;
            lookup_table.insert(lookup_key, asset_id.clone())?;
            let _ = group_table.insert(dst_page_id.clone(), asset_id.clone())?;

            Ok(AssetMoveResult::Moved)
        })()?;

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * 競合アセットの削除
         */
        if let Some(conflict) = conflict_asset {
            let asset_path = self.asset_file_path(&conflict);
            let _ = fs::remove_file(asset_path);
        }

        Ok(move_result)
    }
}
