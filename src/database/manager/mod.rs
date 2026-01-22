/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース操作のマネージャを提供するモジュール
//!

use std::path::{Path, PathBuf};

use anyhow::Result;
use redb::{Database, ReadableDatabase};

use super::init::init_database;
use super::schema::{DbError, DEFAULT_ROOT_SOURCE, ROOT_PAGE_PATH};
use super::types::AssetId;

pub(crate) mod assets;
pub(crate) mod locks;
pub(crate) mod pages_read;
pub(crate) mod pages_write;
pub(crate) mod users;

///
/// データベース操作手順を集約する構造体
///
pub(crate) struct DatabaseManager {
    /// データベースオブジェクト
    db: Database,

    /// アセットデータ格納ディレクトリへのパス
    #[allow(dead_code)]
    asset_path: PathBuf,
}

impl DatabaseManager {
    ///
    /// データベースマネージャのオープン
    ///
    /// # 引数
    /// * `path` - データベースファイルへのパス
    ///
    /// # 戻り値
    /// データベースのオープンに成功した場合はエントリーマネージャオブジェクトを
    /// `Ok()`でラップして返す。失敗した場合はエラー情報を `Err()`でラップして返
    /// す。
    ///
    pub(crate) fn open<P>(db_path: P, asset_path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let db = match Database::create(db_path) {
            Ok(mut db) => {
                init_database(&mut db)?;
                db
            }

            Err(err) => return Err(err.into()),
        };

        Ok(Self { db, asset_path: asset_path.as_ref().into() })
    }

    ///
    /// ルートページの初期化
    ///
    /// # 引数
    /// * `user_name` - 作成者のユーザ名
    ///
    /// # 戻り値
    /// 初期化に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn ensure_default_root(&self, user_name: &str) -> Result<()> {
        if self.page_exists(ROOT_PAGE_PATH)? {
            return Ok(());
        }

        match self.create_page(
            ROOT_PAGE_PATH,
            user_name,
            DEFAULT_ROOT_SOURCE.to_string(),
        ) {
            Ok(_) => Ok(()),
            Err(err) => {
                if let Some(DbError::PageAlreadyExists) =
                    err.downcast_ref::<DbError>()
                {
                    return Ok(());
                }

                Err(err)
            }
        }
    }

    ///
    /// ページが存在するかの確認
    ///
    /// # 引数
    /// * `path` - ページのパス
    ///
    /// # 戻り値
    /// ページが存在する場合は`true`を返す。
    ///
    fn page_exists(&self, path: &str) -> Result<bool> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(super::schema::PAGE_PATH_TABLE)?;
        Ok(table.get(&path.to_string())?.is_some())
    }

    ///
    /// アセット保存パスの生成
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// アセット保存パスを返す。
    ///
    pub(crate) fn asset_file_path(&self, asset_id: &AssetId) -> PathBuf {
        let raw = asset_id.to_string();
        if raw.len() < 5 {
            return self.asset_path.join(raw);
        }

        let (dir1, rest) = raw.split_at(2);
        let (dir2, _) = rest.split_at(3);
        self.asset_path.join(dir1).join(dir2).join(raw)
    }
}
