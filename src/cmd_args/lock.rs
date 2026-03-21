/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"lock"のコマンドライン定義
//!

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;

#[derive(Clone, Args, Debug)]
pub(crate) struct LockCommand {
    #[command(subcommand)]
    pub(crate) subcommand: LockSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum LockSubCommand {
    /// ロック情報の一覧表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(LockListOpts),

    /// ロック情報の削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(LockDeleteOpts),
}

///
/// lock_listサブコマンドのソート順
///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ValueEnum)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum LockListSortMode {
    /// デフォルト（ロックID順）
    Default,

    /// 有効期限でソート
    Expire,

    /// ユーザ名でソート
    UserName,

    /// ページパスでソート
    PagePath,
}

///
/// サブコマンドlock_listのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct LockListOpts {
    /// 一覧のソート方法
    #[arg(long = "sort-by", value_name = "MODE")]
    sort_by: Option<LockListSortMode>,

    /// ソートを逆順で行う
    #[arg(short = 'r', long = "reverse-sort")]
    reverse_sort: bool,

    /// 詳細情報で表示
    #[arg(short = 'l', long = "long-info")]
    long_info: bool,
}

impl LockListOpts {
    ///
    /// ソートモードへのアクセサ
    ///
    /// # 戻り値
    /// ソートモードを返す
    ///
    pub(crate) fn sort_mode(&self) -> LockListSortMode {
        self.sort_by.unwrap_or(LockListSortMode::Default)
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

    ///
    /// 詳細表示指定へのアクセサ
    ///
    /// # 戻り値
    /// 詳細表示が指定されている場合はtrue
    ///
    pub(crate) fn is_long_info(&self) -> bool {
        self.long_info
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for LockListOpts {
    ///
    /// lock list サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * ソート設定を未指定項目へ補完
         */
        if self.sort_by.is_none() {
            if let Some(mode) = config.lock_list_sort_mode() {
                self.sort_by = Some(mode);
            }
        }

        if !self.reverse_sort {
            if let Some(reverse) = config.lock_list_reverse_sort() {
                self.reverse_sort = reverse;
            }
        }

        if !self.long_info {
            if let Some(long_info) = config.lock_list_long_info() {
                self.long_info = long_info;
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for LockListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for LockListOpts {
    fn show_options(&self) {
        println!("lock list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
        println!("   long_info:    {:?}", self.is_long_info());
    }
}

///
/// サブコマンドlock_deleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct LockDeleteOpts {
    /// 削除するロックID
    #[arg()]
    lock_id: String,
}

impl LockDeleteOpts {
    ///
    /// ロックIDへのアクセサ
    ///
    /// # 戻り値
    /// ロックIDを返す
    ///
    pub(crate) fn lock_id(&self) -> String {
        self.lock_id.clone()
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for LockDeleteOpts {
    fn show_options(&self) {
        println!("lock delete command options");
        println!("   lock_id: {}", self.lock_id());
    }
}
