/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user delete"の実装
//!

use anyhow::Result;

use super::CommandContext;
use crate::cmd_args::{Options, UserDeleteOpts};
use crate::database::DatabaseManager;

///
/// "user delete"サブコマンドのコンテキスト情報をパックした構造体
///
struct UserDeleteCommandContext {
    manager: DatabaseManager,
    username: String,
}

impl UserDeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &UserDeleteOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            username: sub_opts.user_name(),
        })
    }
}

impl CommandContext for UserDeleteCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// ユーザ削除に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        self.manager.delete_user(&self.username)
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &UserDeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(UserDeleteCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use ulid::Ulid;

    const TEST_USERNAME: &str = "delete_user";
    const TEST_PASSWORD: &str = "password123";

    #[test]
    ///
    /// 既存ユーザを削除できることを確認
    ///
    /// # 注記
    /// テスト用DBにユーザを追加し、削除後に認証が失敗することを検証する。
    ///
    fn delete_user_succeeds() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");
        manager
            .add_user(TEST_USERNAME, TEST_PASSWORD, None)
            .expect("add failed");

        manager.delete_user(TEST_USERNAME).expect("delete failed");
        let deleted = manager
            .verify_user(TEST_USERNAME, TEST_PASSWORD)
            .expect("verify failed");
        assert!(!deleted);

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    #[test]
    ///
    /// 存在しないユーザ削除が失敗することを確認
    ///
    /// # 注記
    /// 空のテスト用DBで未登録ユーザを削除し、エラーになることを検証する。
    ///
    fn delete_user_fails_when_missing() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");

        let result = manager.delete_user("missing_user");
        assert!(result.is_err());

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    ///
    /// テスト用ディレクトリ群を生成
    ///
    /// # 戻り値
    /// ベースディレクトリ、DBパス、アセットディレクトリを返す。
    ///
    fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
        let base = PathBuf::from("tests").join("tmp").join(unique_suffix());
        let db_dir = base.join("db");
        let assets_dir = base.join("assets");
        fs::create_dir_all(&db_dir).expect("create db dir failed");
        fs::create_dir_all(&assets_dir).expect("create assets dir failed");

        let db_path = db_dir.join("database.redb");
        (base, db_path, assets_dir)
    }

    ///
    /// 一意なテスト用サフィックスを生成
    ///
    /// # 戻り値
    /// ULIDベースの一意文字列を返す。
    ///
    fn unique_suffix() -> String {
        Ulid::new().to_string()
    }
}
