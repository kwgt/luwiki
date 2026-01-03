/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user add"の実装
//!

use std::cell::RefCell;

use anyhow::Result;

use crate::cmd_args::{UserAddOpts, Options};
use crate::database::DatabaseManager;
use super::CommandContext;
use super::common::read_password_with_confirm;

///
/// "user add"サブコマンドのコンテキスト情報をパックした構造体
///
struct UserAddCommandContext {
    /// データベースマネージャオブジェクト
    manager: RefCell<DatabaseManager>,

    /// ユーザ名
    username: String,

    /// 表示名
    display_name: Option<String>,
}

impl UserAddCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &UserAddOpts) -> Result<Self> {
        Ok(Self {
            manager: RefCell::new(opts.open_database()?),
            username: sub_opts.user_name(),
            display_name: sub_opts.display_name(),
        })
    }
}

// CommandContextの実装
impl CommandContext for UserAddCommandContext {
    fn exec(&self) -> Result<()> {
        let manager = self.manager.borrow_mut();
        manager.add_user(
            &self.username,
            &read_password_with_confirm()?,
            self.display_name.as_ref(),
        )?;
        manager.ensure_default_root(&self.username)
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &UserAddOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(UserAddCommandContext::new(opts, sub_opts)?))
}
