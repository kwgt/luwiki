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

use super::CommandContext;
use super::common::read_password_with_confirm;
use crate::cmd_args::{Options, UserAddOpts};
use crate::database::types::UserAttributeSet;
use crate::database::DatabaseManager;

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

    /// 初期ユーザ属性
    attributes: UserAttributeSet,

    /// パスワード入力要否
    requires_password: bool,
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
            attributes: sub_opts.attributes()?,
            requires_password: sub_opts.requires_password()?,
        })
    }
}

// CommandContextの実装
impl CommandContext for UserAddCommandContext {
    fn exec(&self) -> Result<()> {
        /*
         * 必要時のみパスワード入力を取得する
         */
        let password = if self.requires_password {
            Some(read_password_with_confirm()?)
        } else {
            None
        };

        /*
         * ユーザ登録と既定ページ初期化を実行する
         */
        let manager = self.manager.borrow_mut();
        manager.add_user_with_attributes(
            &self.username,
            password.as_deref(),
            self.display_name.clone(),
            self.attributes.clone(),
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
