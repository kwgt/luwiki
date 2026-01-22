/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ユーザ情報の操作を提供するモジュール
//!

use anyhow::{anyhow, Result};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};

use crate::database::schema::{USER_ID_TABLE, USER_INFO_TABLE};
use crate::database::types::{UserId, UserInfo};
use super::DatabaseManager;

impl DatabaseManager {
    ///
    /// ユーザ情報の追加
    ///
    /// # 引数
    /// * `username` - 登録するユーザ名
    /// * `password` - 登録するパスワード
    /// * `display_name` - 表示名
    ///
    /// # 戻り値
    /// 登録に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn add_user<S>(
        &self,
        username: S,
        password: S,
        display_name: Option<S>,
    ) -> Result<()>
    where
        S: AsRef<str> + Copy,
    {
        /*
         * 事前情報の整形
         */
        let key = username.as_ref().to_string();

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * 登録処理
         */
        {
            let mut id_table = txn.open_table(USER_ID_TABLE)?;

            /*
             * 既存ユーザの確認
             */
            if id_table.get(&key)?.is_some() {
                return Err(anyhow!(
                    "user already exists: {}",
                    username.as_ref()
                ));
            }

            /*
             * ユーザ情報の生成
             */
            let user_info = UserInfo::new(username, password, display_name);
            let user_id = user_info.id();

            /*
             * ユーザ情報の登録
             */
            let mut info_table = txn.open_table(USER_INFO_TABLE)?;
            info_table.insert(user_id.clone(), user_info)?;
            id_table.insert(&key, user_id)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ユーザ認証の検証
    ///
    /// # 引数
    /// * `username` - ユーザ名
    /// * `password` - パスワード
    ///
    /// # 戻り値
    /// 認証に成功した場合は`Ok(true)`を返す。
    ///
    pub(crate) fn verify_user(&self, username: &str, password: &str)
        -> Result<bool>
    {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;

        /*
         * ユーザID取得
         */
        let id_table = txn.open_table(USER_ID_TABLE)?;
        let key = username.to_string();
        let user_id = match id_table.get(&key)? {
            Some(id) => id.value(),
            None => return Ok(false),
        };

        /*
         * ユーザ情報取得
         */
        let info_table = txn.open_table(USER_INFO_TABLE)?;
        let user_info = match info_table.get(user_id)? {
            Some(info) => info.value(),
            None => return Ok(false),
        };

        /*
         * パスワード検証
         */
        Ok(user_info.verify_password(password))
    }

    ///
    /// ユーザIDからユーザ名を取得
    ///
    /// # 引数
    /// * `user_id` - ユーザID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(ユーザ名))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_user_name_by_id(
        &self,
        user_id: &UserId,
    ) -> Result<Option<String>> {
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(USER_INFO_TABLE)?;
        let info = match info_table.get(user_id.clone())? {
            Some(info) => info.value(),
            None => return Ok(None),
        };

        Ok(Some(info.username()))
    }

    ///
    /// ユーザ名からユーザIDを取得
    ///
    /// # 引数
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(UserId))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_user_id_by_name(
        &self,
        user_name: &str,
    ) -> Result<Option<UserId>> {
        let txn = self.db.begin_read()?;
        let id_table = txn.open_table(USER_ID_TABLE)?;
        let key = user_name.to_string();
        Ok(id_table.get(&key)?.map(|entry| entry.value()))
    }

    ///
    /// ユーザ情報の一覧取得
    ///
    /// # 戻り値
    /// ユーザ情報の一覧を返す。
    ///
    pub(crate) fn list_users(&self) -> Result<Vec<UserInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(USER_INFO_TABLE)?;
        let mut users = Vec::new();

        /*
         * ユーザ情報の収集
         */
        for entry in info_table.iter()? {
            let (_, info) = entry?;
            users.push(info.value());
        }

        Ok(users)
    }

    ///
    /// ユーザ登録の有無の確認
    ///
    /// # 戻り値
    /// ユーザが一人でも登録されている場合は`Ok(true)`を登録されていない場合は
    /// `Ok(false)`を返す。データベースアクセス時にエラーが発生した場合はエラー
    /// 情報を`Err()`でラップして返す。
    ///
    pub(crate) fn is_users_registered(&self) -> Result<bool> {
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(USER_INFO_TABLE)?;

        Ok(!info_table.is_empty()?)
    }

    ///
    /// ユーザ情報の削除
    ///
    /// # 引数
    /// * `username` - 削除対象のユーザ名
    ///
    /// # 戻り値
    /// 削除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn delete_user(&self, username: &str) -> Result<()> {
        /*
         * 事前情報の整形
         */
        let key = username.to_string();

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * ユーザ情報の削除
         */
        {
            let mut id_table = txn.open_table(USER_ID_TABLE)?;
            let user_id = match id_table.get(&key)? {
                Some(id) => id.value(),
                None => {
                    return Err(anyhow!(
                        "user not found: {}",
                        username
                    ));
                }
            };

            let mut info_table = txn.open_table(USER_INFO_TABLE)?;
            let _ = info_table.remove(user_id)?;
            let _ = id_table.remove(&key)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ユーザ情報の更新
    ///
    /// # 引数
    /// * `username` - 更新対象のユーザ名
    /// * `display_name` - 表示名
    /// * `password` - パスワード
    ///
    /// # 戻り値
    /// 更新に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn update_user(
        &self,
        username: &str,
        display_name: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        /*
         * 引数の妥当性チェック
         */
        if display_name.is_none() && password.is_none() {
            return Err(anyhow!("no update fields specified"));
        }

        /*
         * 事前情報の整形
         */
        let key = username.to_string();

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * ユーザ情報の更新
         */
        {
            let id_table = txn.open_table(USER_ID_TABLE)?;
            let user_id = match id_table.get(&key)? {
                Some(id) => id.value(),
                None => {
                    return Err(anyhow!("user not found: {}", username));
                }
            };

            let mut info_table = txn.open_table(USER_INFO_TABLE)?;
            let mut user_info = match info_table.get(user_id.clone())? {
                Some(info) => info.value(),
                None => {
                    return Err(anyhow!("user not found: {}", username));
                }
            };

            if let Some(name) = display_name {
                user_info.set_display_name(name);
            }

            if let Some(password) = password {
                user_info.set_password(password);
            }

            info_table.insert(user_id.clone(), user_info)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }
}
