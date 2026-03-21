/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page"のコマンドライン定義
//!

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};
use pulldown_cmark::Parser as MarkdownParser;
use serde::{Deserialize, Serialize};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;
use crate::database::types::PageId;
use crate::rest_api::validate_page_path;

#[derive(Clone, Args, Debug)]
pub(crate) struct PageCommand {
    #[command(subcommand)]
    pub(crate) subcommand: PageSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum PageSubCommand {
    /// ページの追加
    #[command(name = "add", alias = "a")]
    Add(PageAddOpts),

    /// ページの削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(PageDeleteOpts),

    /// ページのロック解除
    #[command(name = "unlock", alias = "ul")]
    Unlock(PageUnlockOpts),

    /// ページの移動
    #[command(name = "move_to", alias = "m", alias = "mv")]
    MoveTo(PageMoveToOpts),

    /// ページの回復
    #[command(name = "undelete", alias = "ud")]
    Undelete(PageUndeleteOpts),

    /// ページ情報の一覧表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(PageListOpts),
}

///
/// サブコマンドpage_addのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct PageAddOpts {
    /// 登録ユーザ名
    #[arg(short = 'u', long = "user", value_name = "USER-NAME")]
    user_name: Option<String>,

    /// 取り込むMarkdownファイルのパス
    #[arg()]
    file_path: PathBuf,

    /// ページパス
    #[arg()]
    page_path: String,
}

impl PageAddOpts {
    ///
    /// 登録ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 登録ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name
            .clone()
            .expect("user_name must be resolved")
    }

    ///
    /// 設定前の登録ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// CLI入力値として指定された登録ユーザ名を返す
    ///
    pub(crate) fn raw_user_name(&self) -> Option<String> {
        self.user_name.clone()
    }

    ///
    /// ファイルパスへのアクセサ
    ///
    /// # 戻り値
    /// ファイルパスを返す
    ///
    pub(crate) fn file_path(&self) -> PathBuf {
        self.file_path.clone()
    }

    ///
    /// ページパスへのアクセサ
    ///
    /// # 戻り値
    /// ページパスを返す
    ///
    pub(crate) fn page_path(&self) -> String {
        self.page_path.clone()
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for PageAddOpts {
    ///
    /// page add サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        if self.user_name.is_none() {
            if let Some(user_name) = config.page_add_default_user() {
                self.user_name = Some(user_name);
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for PageAddOpts {
    fn validate(&mut self) -> Result<()> {
        if self.user_name.is_none() {
            return Err(anyhow!("user name is required"));
        }

        let path = &self.file_path;
        if !path.exists() {
            return Err(anyhow!("{} is not exists", path.display()));
        }

        if !path.is_file() {
            return Err(anyhow!("{} is not file", path.display()));
        }

        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !extension.eq_ignore_ascii_case("md") {
            return Err(anyhow!("file extension must be .md"));
        }

        if let Err(message) = validate_page_path(&self.page_path) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        let source = fs::read_to_string(path)?;
        let parser = MarkdownParser::new(&source);
        for _ in parser {
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for PageAddOpts {
    fn show_options(&self) {
        println!("page add command options");
        println!("   user_name: {}", self.user_name());
        println!("   file_path: {}", self.file_path.display());
        println!("   page_path: {}", self.page_path());
    }
}

///
/// サブコマンドpage_move_toのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct PageMoveToOpts {
    /// ロック中でも強制的に移動を行う
    #[arg(short = 'f', long = "force")]
    force: bool,

    /// 配下ページを含めて移動する
    #[arg(short = 'r', long = "recursive")]
    recursive: bool,

    /// 移動元のページパスまたはページID
    #[arg()]
    src_path: String,

    /// 移動先のページパス
    #[arg()]
    dst_path: String,
}

impl PageMoveToOpts {
    ///
    /// ロック無視の指定有無へのアクセサ
    ///
    /// # 戻り値
    /// 強制移動が指定されている場合はtrue
    ///
    pub(crate) fn is_force(&self) -> bool {
        self.force
    }

    ///
    /// 再帰移動指定へのアクセサ
    ///
    /// # 戻り値
    /// 再帰移動が指定されている場合はtrue
    ///
    pub(crate) fn is_recursive(&self) -> bool {
        self.recursive
    }

    ///
    /// 移動元指定へのアクセサ
    ///
    /// # 戻り値
    /// 移動元指定を返す
    ///
    pub(crate) fn src_path(&self) -> String {
        self.src_path.clone()
    }

    ///
    /// 移動先パスへのアクセサ
    ///
    /// # 戻り値
    /// 移動先パスを返す
    ///
    pub(crate) fn dst_path(&self) -> String {
        self.dst_path.clone()
    }
}

// Validateトレイトの実装
impl Validate for PageMoveToOpts {
    fn validate(&mut self) -> Result<()> {
        if let Err(message) = validate_page_path(&self.dst_path) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for PageMoveToOpts {
    fn show_options(&self) {
        println!("page move_to command options");
        println!("   force:    {:?}", self.is_force());
        println!("   recursive: {:?}", self.is_recursive());
        println!("   src_path: {}", self.src_path());
        println!("   dst_path: {}", self.dst_path());
    }
}

///
/// サブコマンドpage_undeleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct PageUndeleteOpts {
    /// アセットの復旧を行わない
    #[arg(long = "without-assets")]
    without_assets: bool,

    /// 配下ページを含めて復帰する
    #[arg(short = 'r', long = "recursive")]
    recursive: bool,

    /// 復帰対象のページID
    #[arg()]
    target: String,

    /// 復帰先のページパス
    #[arg()]
    restore_to: String,
}

impl PageUndeleteOpts {
    ///
    /// アセット復旧無効化指定へのアクセサ
    ///
    /// # 戻り値
    /// アセット復旧無効化が指定されている場合はtrue
    ///
    pub(crate) fn is_without_assets(&self) -> bool {
        self.without_assets
    }

    ///
    /// 再帰復帰指定へのアクセサ
    ///
    /// # 戻り値
    /// 再帰復帰が指定されている場合はtrue
    ///
    pub(crate) fn is_recursive(&self) -> bool {
        self.recursive
    }

    ///
    /// 復帰対象指定へのアクセサ
    ///
    /// # 戻り値
    /// 復帰対象指定を返す
    ///
    pub(crate) fn target(&self) -> String {
        self.target.clone()
    }

    ///
    /// 復帰先パスへのアクセサ
    ///
    /// # 戻り値
    /// 復帰先パスを返す
    ///
    pub(crate) fn restore_to(&self) -> String {
        self.restore_to.clone()
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for PageUndeleteOpts {
    ///
    /// page undelete サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * アセット復旧の既定値を補完
         */
        if !self.without_assets {
            if let Some(with_assets) = config.page_undelete_with_assets() {
                if !with_assets {
                    self.without_assets = true;
                }
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for PageUndeleteOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("page id is empty"));
        }

        if PageId::from_string(&self.target).is_err() {
            return Err(anyhow!("invalid page id"));
        }

        if let Err(message) = validate_page_path(&self.restore_to) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for PageUndeleteOpts {
    fn show_options(&self) {
        println!("page undelete command options");
        println!("   without_assets: {:?}", self.is_without_assets());
        println!("   recursive:      {:?}", self.is_recursive());
        println!("   target:         {}", self.target());
        println!("   restore_to:     {}", self.restore_to());
    }
}

///
/// サブコマンドpage_deleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct PageDeleteOpts {
    /// ハードデリートを行う
    #[arg(short = 'H', long = "hard-delete")]
    hard_delete: bool,

    /// 配下ページを含めて削除する
    #[arg(short = 'r', long = "recursive")]
    recursive: bool,

    /// ロック中でも強制的に削除を行う
    #[arg(short = 'f', long = "force")]
    force: bool,

    /// 削除対象のページIDまたはページパス
    #[arg()]
    target: String,
}

impl PageDeleteOpts {
    ///
    /// ハードデリート指定へのアクセサ
    ///
    /// # 戻り値
    /// ハードデリートが指定されている場合はtrue
    ///
    pub(crate) fn is_hard_delete(&self) -> bool {
        self.hard_delete
    }

    ///
    /// 再帰削除指定へのアクセサ
    ///
    /// # 戻り値
    /// 再帰削除が指定されている場合はtrue
    ///
    pub(crate) fn is_recursive(&self) -> bool {
        self.recursive
    }

    ///
    /// ロック無視の指定有無へのアクセサ
    ///
    /// # 戻り値
    /// 強制削除が指定されている場合はtrue
    ///
    pub(crate) fn is_force(&self) -> bool {
        self.force
    }

    ///
    /// 削除対象指定へのアクセサ
    ///
    /// # 戻り値
    /// 削除対象指定を返す
    ///
    pub(crate) fn target(&self) -> String {
        self.target.clone()
    }
}

// Validateトレイトの実装
impl Validate for PageDeleteOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("page id or path is empty"));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for PageDeleteOpts {
    fn show_options(&self) {
        println!("page delete command options");
        println!("   hard_delete: {:?}", self.is_hard_delete());
        println!("   recursive:   {:?}", self.is_recursive());
        println!("   force:       {:?}", self.is_force());
        println!("   target:      {}", self.target());
    }
}

///
/// サブコマンドpage_unlockのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct PageUnlockOpts {
    /// ロック解除対象のページIDまたはページパス
    #[arg()]
    target: String,
}

impl PageUnlockOpts {
    ///
    /// ロック解除対象指定へのアクセサ
    ///
    /// # 戻り値
    /// ロック解除対象指定を返す
    ///
    pub(crate) fn target(&self) -> String {
        self.target.clone()
    }
}

// Validateトレイトの実装
impl Validate for PageUnlockOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("page id or path is empty"));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for PageUnlockOpts {
    fn show_options(&self) {
        println!("page unlock command options");
        println!("   target: {}", self.target());
    }
}

///
/// page_listサブコマンドのソート順
///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ValueEnum)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum PageListSortMode {
    /// デフォルト（ページID順）
    Default,

    /// ユーザ名でソート
    UserName,

    /// ページパスでソート
    PagePath,

    /// 更新日時でソート
    LastUpdate,
}

///
/// サブコマンドpage_listのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct PageListOpts {
    /// 一覧のソート方法
    #[arg(long = "sort-by", value_name = "MODE")]
    sort_by: Option<PageListSortMode>,

    /// ソートを逆順で行う
    #[arg(short = 'r', long = "reverse-sort")]
    reverse_sort: bool,

    /// 詳細情報で表示
    #[arg(short = 'l', long = "long-info")]
    long_info: bool,
}

impl PageListOpts {
    ///
    /// ソートモードへのアクセサ
    ///
    /// # 戻り値
    /// ソートモードを返す
    ///
    pub(crate) fn sort_mode(&self) -> PageListSortMode {
        self.sort_by.unwrap_or(PageListSortMode::Default)
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
impl ApplyConfig for PageListOpts {
    ///
    /// page list サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * ソート設定を未指定項目へ補完
         */
        if self.sort_by.is_none() {
            if let Some(mode) = config.page_list_sort_mode() {
                self.sort_by = Some(mode);
            }
        }

        if !self.reverse_sort {
            if let Some(reverse) = config.page_list_reverse_sort() {
                self.reverse_sort = reverse;
            }
        }

        if !self.long_info {
            if let Some(long_info) = config.page_list_long_info() {
                self.long_info = long_info;
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for PageListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for PageListOpts {
    fn show_options(&self) {
        println!("page list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
        println!("   long_info:    {:?}", self.is_long_info());
    }
}
