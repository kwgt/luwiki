/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user"のコマンドライン定義
//!

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;

#[derive(Clone, Args, Debug)]
pub(crate) struct UserCommand {
    #[command(subcommand)]
    pub(crate) subcommand: UserSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum UserSubCommand {
    /// ユーザ追加コマンド
    #[command(name = "add", alias = "a")]
    Add(UserAddOpts),

    /// ユーザ情報の削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(UserDeleteOpts),

    /// ユーザ情報の変更
    #[command(name = "edit", alias = "e", alias = "ed")]
    Edit(UserEditOpts),

    /// ユーザ情報の一覧表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(UserListOpts),
}

///
/// サブコマンドuser_addのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserAddOpts {
    /// 表示名の指定
    #[arg(short = 'd', long = "display-name", value_name = "NAME")]
    display_name: Option<String>,

    /// 登録するユーザ名
    #[arg()]
    user_name: String,
}

impl UserAddOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 表示名へのアクセサ
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserAddOpts {
    fn show_options(&self) {
        println!("user add command options");
        println!("   user_name:    {}", self.user_name());
        println!("   display_name: {:?}", self.display_name());
    }
}

///
/// サブコマンドuser_deleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserDeleteOpts {
    /// 削除するユーザ名
    #[arg()]
    user_name: String,
}

impl UserDeleteOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }
}

// Validateトレイトの実装
impl Validate for UserDeleteOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserDeleteOpts {
    fn show_options(&self) {
        println!("user delete command options");
        println!("   user_name: {}", self.user_name());
    }
}

///
/// サブコマンドuser_editのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserEditOpts {
    /// 表示名の指定
    #[arg(short = 'd', long = "display-name", value_name = "NEW-NAME")]
    display_name: Option<String>,

    /// パスワードの指定
    #[arg(short = 'p', long = "password")]
    password: bool,

    /// 変更対象のユーザ名
    #[arg()]
    user_name: String,
}

impl UserEditOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 表示名へのアクセサ
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }

    ///
    /// パスワード変更指定へのアクセサ
    ///
    /// # 戻り値
    /// パスワード変更が指定されている場合はtrue
    ///
    pub(crate) fn is_password_change(&self) -> bool {
        self.password
    }
}

// Validateトレイトの実装
impl Validate for UserEditOpts {
    fn validate(&mut self) -> Result<()> {
        if self.display_name.is_none() && !self.password {
            return Err(anyhow!("no update options specified"));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserEditOpts {
    fn show_options(&self) {
        println!("user edit command options");
        println!("   user_name:    {}", self.user_name());
        println!("   display_name: {:?}", self.display_name());
        println!("   password:     {:?}", self.is_password_change());
    }
}

///
/// user_listサブコマンドのソート順
///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ValueEnum)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum UserListSortMode {
    /// デフォルト（ユーザID順）
    Default,

    /// ユーザ名でソート
    UserName,

    /// 表示名でソート
    DisplayName,

    /// 更新日時でソート
    LastUpdate,
}

///
/// サブコマンドuser_listのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserListOpts {
    /// 一覧のソート方法
    #[arg(long = "sort-by", value_name = "MODE")]
    sort_by: Option<UserListSortMode>,

    /// ソートを逆順で行う
    #[arg(short = 'r', long = "reverse-sort")]
    reverse_sort: bool,
}

impl UserListOpts {
    ///
    /// ソートモードへのアクセサ
    ///
    /// # 戻り値
    /// ソートモードを返す
    ///
    pub(crate) fn sort_mode(&self) -> UserListSortMode {
        self.sort_by.unwrap_or(UserListSortMode::Default)
    }

    ///
    /// 逆順ソート指定へのアクセサ
    ///
    /// # 戻り値
    /// 逆順ソートが指定されている場合はtrue
    ///
    pub(crate) fn is_reverse_sort(&self) -> bool {
        self.reverse_sort
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for UserListOpts {
    ///
    /// user list サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * ソート設定を未指定項目へ補完
         */
        if self.sort_by.is_none() {
            if let Some(mode) = config.user_list_sort_mode() {
                self.sort_by = Some(mode);
            }
        }

        if !self.reverse_sort {
            if let Some(reverse) = config.user_list_reverse_sort() {
                self.reverse_sort = reverse;
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for UserListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserListOpts {
    fn show_options(&self) {
        println!("user list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
    }
}
