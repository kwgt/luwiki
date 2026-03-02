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
use super::schema::{
    DEFAULT_ROOT_SOURCE, DEFAULT_SANDBOX_SOURCE, DbError, ROOT_PAGE_PATH, SANDBOX_PAGE_PATH,
    SANDBOX_SAMPLE_CODE_FILE_NAME, SANDBOX_SAMPLE_CODE_SOURCE, SANDBOX_SAMPLE_CSV_FILE_NAME,
    SANDBOX_SAMPLE_CSV_SOURCE,
};
use super::types::{AssetId, PageId};

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

        Ok(Self {
            db,
            asset_path: asset_path.as_ref().into(),
        })
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
        self.create_page_if_missing(ROOT_PAGE_PATH, DEFAULT_ROOT_SOURCE, user_name)?;
        self.create_page_if_missing(SANDBOX_PAGE_PATH, DEFAULT_SANDBOX_SOURCE, user_name)?;

        let sandbox_page_id = match self.get_page_id_by_path(SANDBOX_PAGE_PATH)? {
            Some(id) => id,
            None => return Err(anyhow::anyhow!(DbError::PageNotFound)),
        };

        self.create_asset_if_missing(
            &sandbox_page_id,
            SANDBOX_SAMPLE_CODE_FILE_NAME,
            "text/x-rust",
            user_name,
            SANDBOX_SAMPLE_CODE_SOURCE.as_bytes(),
        )?;
        self.create_asset_if_missing(
            &sandbox_page_id,
            SANDBOX_SAMPLE_CSV_FILE_NAME,
            "text/csv",
            user_name,
            SANDBOX_SAMPLE_CSV_SOURCE.as_bytes(),
        )?;

        Ok(())
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
    /// ページが存在しない場合にページを作成する。
    ///
    fn create_page_if_missing(&self, path: &str, source: &str, user_name: &str) -> Result<()> {
        if self.page_exists(path)? {
            return Ok(());
        }

        match self.create_page(path, user_name, source.to_string()) {
            Ok(_) => Ok(()),
            Err(err) => {
                if let Some(DbError::PageAlreadyExists) = err.downcast_ref::<DbError>() {
                    return Ok(());
                }

                Err(err)
            }
        }
    }

    ///
    /// アセットが存在しない場合にアセットを作成する。
    ///
    fn create_asset_if_missing(
        &self,
        page_id: &PageId,
        file_name: &str,
        mime: &str,
        user_name: &str,
        data: &[u8],
    ) -> Result<()> {
        if self
            .get_asset_id_by_page_file(page_id, file_name)?
            .is_some()
        {
            return Ok(());
        }

        match self.create_asset(page_id, file_name, mime, user_name, data) {
            Ok(_) => Ok(()),
            Err(err) => {
                if let Some(DbError::AssetAlreadyExists) = err.downcast_ref::<DbError>() {
                    return Ok(());
                }

                Err(err)
            }
        }
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
