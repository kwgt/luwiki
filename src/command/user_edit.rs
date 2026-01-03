/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user edit"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{Options, UserEditOpts};
use crate::database::DatabaseManager;
use super::CommandContext;
use super::common::read_password_with_confirm;

///
/// "user edit"サブコマンドのコンテキスト情報をパックした構造体
///
struct UserEditCommandContext {
    manager: DatabaseManager,
    username: String,
    display_name: Option<String>,
    change_password: bool,
}

impl UserEditCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &UserEditOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            username: sub_opts.user_name(),
            display_name: sub_opts.display_name(),
            change_password: sub_opts.is_password_change(),
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for UserEditCommandContext {
    fn exec(&self) -> Result<()> {
        if self.display_name.is_none() && !self.change_password {
            return Err(anyhow!("no update options specified"));
        }

        let password = if self.change_password {
            Some(read_password_with_confirm()?)
        } else {
            None
        };

        self.manager.update_user(
            &self.username,
            self.display_name.as_deref(),
            password.as_deref(),
        )
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &UserEditOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(UserEditCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use ulid::Ulid;

    const TEST_USERNAME: &str = "edit_user";
    const TEST_PASSWORD: &str = "password123";

    #[test]
    fn update_display_name_succeeds() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");
        manager.add_user(TEST_USERNAME, TEST_PASSWORD, None)
            .expect("add failed");

        manager.update_user(TEST_USERNAME, Some("new"), None)
            .expect("update failed");
        let users = manager.list_users().expect("list failed");
        let user = users
            .iter()
            .find(|user| user.username() == TEST_USERNAME)
            .expect("user missing");
        assert_eq!(user.display_name(), "new");

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    #[test]
    fn update_password_succeeds() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");
        manager.add_user(TEST_USERNAME, TEST_PASSWORD, None)
            .expect("add failed");

        manager
            .update_user(TEST_USERNAME, None, Some("newpass123"))
            .expect("update failed");
        let ok = manager
            .verify_user(TEST_USERNAME, "newpass123")
            .expect("verify failed");
        let old_ok = manager
            .verify_user(TEST_USERNAME, TEST_PASSWORD)
            .expect("verify failed");
        assert!(ok);
        assert!(!old_ok);

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    #[test]
    fn update_user_fails_when_missing() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");

        let result = manager.update_user("missing", Some("x"), None);
        assert!(result.is_err());

        fs::remove_dir_all(db_dir).expect("cleanup failed");
    }

    #[test]
    fn update_user_fails_when_no_changes() {
        let (db_dir, db_path, assets_dir) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &assets_dir)
            .expect("open failed");
        manager.add_user(TEST_USERNAME, TEST_PASSWORD, None)
            .expect("add failed");

        let result = manager.update_user(TEST_USERNAME, None, None);
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
