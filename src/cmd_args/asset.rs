/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset"のコマンドライン定義
//!

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;
use crate::database::types::{AssetId, PageId};
use crate::rest_api::{validate_asset_file_name, validate_page_path};

#[derive(Clone, Args, Debug)]
pub(crate) struct AssetCommand {
    #[command(subcommand)]
    pub(crate) subcommand: AssetSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum AssetSubCommand {
    /// アセットの追加
    #[command(name = "add", alias = "a")]
    Add(AssetAddOpts),

    /// アセット一覧の表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(AssetListOpts),

    /// アセットの削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(AssetDeleteOpts),

    /// アセット削除のパージ
    #[command(name = "purge", alias = "p")]
    Purge(AssetPurgeOpts),

    /// アセットの回復
    #[command(name = "undelete", alias = "ud")]
    Undelete(AssetUndeleteOpts),

    /// アセットの移動
    #[command(name = "move_to", alias = "m", alias = "mv")]
    MoveTo(AssetMoveToOpts),
}

///
/// サブコマンドasset_addのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct AssetAddOpts {
    /// 登録ユーザ名
    #[arg(short = 'u', long = "user", value_name = "USER-NAME")]
    user_name: Option<String>,

    /// MIME種別の指定
    #[arg(short = 't', long = "mime-type", value_name = "TYPE")]
    mime_type: Option<String>,

    /// 取り込むアセットファイルのパス
    #[arg()]
    file_path: PathBuf,

    /// 所属ページIDまたはページパス
    #[arg()]
    target: String,
}

impl AssetAddOpts {
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
    /// MIME種別へのアクセサ
    ///
    /// # 戻り値
    /// MIME種別を返す
    ///
    pub(crate) fn mime_type(&self) -> Option<String> {
        self.mime_type.clone()
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
    /// 対象ページ指定へのアクセサ
    ///
    /// # 戻り値
    /// 対象ページ指定を返す
    ///
    pub(crate) fn target(&self) -> String {
        self.target.clone()
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for AssetAddOpts {
    ///
    /// asset add サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        if self.user_name.is_none() {
            if let Some(user_name) = config.asset_add_default_user() {
                self.user_name = Some(user_name);
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for AssetAddOpts {
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

        fs::metadata(path)?;

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("file name is invalid"))?;
        if let Err(message) = validate_asset_file_name(file_name) {
            return Err(anyhow!("invalid file name: {}", message));
        }

        if PageId::from_string(&self.target).is_ok() {
            return Ok(());
        }

        if let Err(message) = validate_page_path(&self.target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetAddOpts {
    fn show_options(&self) {
        println!("asset add command options");
        println!("   user_name: {:?}", self.user_name.as_deref());
        println!("   mime_type: {:?}", self.mime_type());
        println!("   file_path: {}", self.file_path.display());
        println!("   target:    {}", self.target());
    }
}

///
/// asset listサブコマンドのソート順
///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ValueEnum)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssetListSortMode {
    /// デフォルト（アセットID順）
    Default,

    /// アップロード日時でソート
    Upload,

    /// アップロードユーザ名でソート
    UserName,

    /// MIME種別でソート
    MimeType,

    /// サイズでソート
    Size,

    /// ページパスでソート
    Path,
}

///
/// サブコマンドasset_listのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct AssetListOpts {
    /// 一覧のソート方法
    #[arg(long = "sort-by", value_name = "MODE")]
    sort_by: Option<AssetListSortMode>,

    /// ソートを逆順で行う
    #[arg(short = 'r', long = "reverse-sort")]
    reverse_sort: bool,

    /// 詳細情報で表示
    #[arg(short = 'l', long = "long-info")]
    long_info: bool,
}

impl AssetListOpts {
    ///
    /// ソートモードへのアクセサ
    ///
    /// # 戻り値
    /// ソートモードを返す
    ///
    pub(crate) fn sort_mode(&self) -> AssetListSortMode {
        self.sort_by.unwrap_or(AssetListSortMode::Default)
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
impl ApplyConfig for AssetListOpts {
    ///
    /// asset list サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * ソート設定を未指定項目へ補完
         */
        if self.sort_by.is_none() {
            if let Some(mode) = config.asset_list_sort_mode() {
                self.sort_by = Some(mode);
            }
        }

        if !self.reverse_sort {
            if let Some(reverse) = config.asset_list_reverse_sort() {
                self.reverse_sort = reverse;
            }
        }

        if !self.long_info {
            if let Some(long_info) = config.asset_list_long_info() {
                self.long_info = long_info;
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for AssetListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetListOpts {
    fn show_options(&self) {
        println!("asset list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
        println!("   long_info:    {:?}", self.is_long_info());
    }
}

///
/// サブコマンドasset_deleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct AssetDeleteOpts {
    /// ハードデリートを行う
    #[arg(short = 'H', long = "hard-delete")]
    hard_delete: bool,

    /// 削除対象のアセットIDまたはアセットパス
    #[arg()]
    target: String,
}

impl AssetDeleteOpts {
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
impl Validate for AssetDeleteOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("asset id or path is empty"));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetDeleteOpts {
    fn show_options(&self) {
        println!("asset delete command options");
        println!("   hard_delete: {:?}", self.is_hard_delete());
        println!("   target:      {}", self.target());
    }
}

///
/// サブコマンドasset_purgeのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct AssetPurgeOpts {
    /// 削除済みアセットを削除する対象ページ
    #[arg()]
    target: Option<String>,
}

impl AssetPurgeOpts {
    ///
    /// 削除対象指定へのアクセサ
    ///
    /// # 戻り値
    /// 削除対象指定を返す
    ///
    pub(crate) fn target(&self) -> Option<String> {
        self.target.clone()
    }
}

// Validateトレイトの実装
impl Validate for AssetPurgeOpts {
    fn validate(&mut self) -> Result<()> {
        if let Some(target) = &self.target {
            if target.trim().is_empty() {
                return Err(anyhow!("page id or path is empty"));
            }

            if PageId::from_string(target).is_ok() {
                return Ok(());
            }

            if let Err(message) = validate_page_path(target) {
                return Err(anyhow!("invalid page path: {}", message));
            }
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetPurgeOpts {
    fn show_options(&self) {
        println!("asset purge command options");
        println!("   target: {:?}", self.target());
    }
}

///
/// サブコマンドasset_undeleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct AssetUndeleteOpts {
    /// 復帰対象のアセットID
    #[arg()]
    target: String,

    /// 復帰時のアセット名
    #[arg()]
    rename_to: Option<String>,
}

impl AssetUndeleteOpts {
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
    /// 復帰時のアセット名へのアクセサ
    ///
    /// # 戻り値
    /// 復帰時のアセット名を返す
    ///
    pub(crate) fn rename_to(&self) -> Option<String> {
        self.rename_to.clone()
    }
}

// Validateトレイトの実装
impl Validate for AssetUndeleteOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("asset id is empty"));
        }

        if AssetId::from_string(&self.target).is_err() {
            return Err(anyhow!("invalid asset id"));
        }

        if let Some(name) = &self.rename_to {
            if let Err(message) = validate_asset_file_name(name) {
                return Err(anyhow!("invalid file name: {}", message));
            }
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetUndeleteOpts {
    fn show_options(&self) {
        println!("asset undelete command options");
        println!("   target: {}", self.target());
        println!("   rename_to: {:?}", self.rename_to());
    }
}

///
/// サブコマンドasset_move_toのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct AssetMoveToOpts {
    /// 強制的に移動を行う
    #[arg(short = 'f', long = "force")]
    force: bool,

    /// 移動対象のアセットID
    #[arg()]
    asset_id: String,

    /// 移動先のページIDまたはページパス
    #[arg()]
    dst_target: String,
}

impl AssetMoveToOpts {
    ///
    /// 強制移動指定へのアクセサ
    ///
    /// # 戻り値
    /// 強制移動が指定されている場合はtrue
    ///
    pub(crate) fn is_force(&self) -> bool {
        self.force
    }

    ///
    /// 移動対象アセットIDへのアクセサ
    ///
    /// # 戻り値
    /// 移動対象アセットIDを返す
    ///
    pub(crate) fn asset_id(&self) -> String {
        self.asset_id.clone()
    }

    ///
    /// 移動先指定へのアクセサ
    ///
    /// # 戻り値
    /// 移動先指定を返す
    ///
    pub(crate) fn dst_target(&self) -> String {
        self.dst_target.clone()
    }
}

// Validateトレイトの実装
impl Validate for AssetMoveToOpts {
    fn validate(&mut self) -> Result<()> {
        if self.asset_id.trim().is_empty() {
            return Err(anyhow!("asset id is empty"));
        }

        if AssetId::from_string(&self.asset_id).is_err() {
            return Err(anyhow!("invalid asset id"));
        }

        if PageId::from_string(&self.dst_target).is_ok() {
            return Ok(());
        }

        if let Err(message) = validate_page_path(&self.dst_target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetMoveToOpts {
    fn show_options(&self) {
        println!("asset move_to command options");
        println!("   force:      {:?}", self.is_force());
        println!("   asset_id:   {}", self.asset_id());
        println!("   dst_target: {}", self.dst_target());
    }
}
