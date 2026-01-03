/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user delete"の実装
//!

use anyhow::Result;

use crate::cmd_args::{UserDeleteOpts, Options};
use crate::database::DatabaseManager;
use super::CommandContext;

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

// トレイトCommandContextの実装
impl CommandContext for UserDeleteCommandContext {
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
    fn delete_user_succeeds() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");
        manager.add_user(TEST_USERNAME, TEST_PASSWORD, None)
            .expect("add failed");

        manager.delete_user(TEST_USERNAME).expect("delete failed");
        let deleted = manager.verify_user(TEST_USERNAME, TEST_PASSWORD)
            .expect("verify failed");
        assert!(!deleted);

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    #[test]
    fn delete_user_fails_when_missing() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");

        let result = manager.delete_user("missing_user");
        assert!(result.is_err());

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
        let base = PathBuf::from("tests")
            .join("tmp")
            .join(unique_suffix());
        let db_dir = base.join("db");
        let assets_dir = base.join("assets");
        fs::create_dir_all(&db_dir).expect("create db dir failed");
        fs::create_dir_all(&assets_dir).expect("create assets dir failed");

        let db_path = db_dir.join("database.redb");
        (base, db_path, assets_dir)
    }

    fn unique_suffix() -> String {
        Ulid::new().to_string()
    }
}
