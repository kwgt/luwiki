/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! コマンドライン引数を取り扱うモジュール
//!

mod config;
mod logger;

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use directories::BaseDirs;
use pulldown_cmark::Parser as MarkdownParser;
use serde::{Deserialize, Serialize};

use crate::command::{
    CommandContext, asset_add, asset_list, asset_delete, asset_move_to,
    asset_undelete, commands, fts_merge, fts_rebuild, fts_search, help_all,
    lock_delete, lock_list, page_add, page_delete, page_list, page_move_to,
    page_undelete, page_unlock, run, user_add, user_delete, user_edit,
    user_list,
};
use crate::database::DatabaseManager;
use crate::database::types::{AssetId, PageId};
use crate::rest_api::{validate_asset_file_name, validate_page_path};
use config::Config;
pub(crate) use config::FrontendConfig;


/// デフォルトのコンフィギュレーションパス
static DEFAULT_CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    BaseDirs::new()
        .unwrap()
        .config_local_dir()
        .join(env!("CARGO_PKG_NAME"))
        .to_path_buf()
});

/// デフォルトのデータパス
static DEFAULT_DATA_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    BaseDirs::new()
        .unwrap()
        .data_local_dir()
        .join(env!("CARGO_PKG_NAME"))
        .to_path_buf()
});

///
/// デフォルトのコンフィグレーションファイルのパス情報を生成
///
/// # 戻り値
/// コンフィギュレーションファイルのパス情報
///
fn default_config_path() -> PathBuf {
    DEFAULT_CONFIG_PATH.join("config.toml")
}

///
/// デフォルトのログ出力先のパスを生成
///
/// # 戻り値
/// ログ出力先ディレクトリのパス情報
///
fn default_log_path() -> PathBuf {
    DEFAULT_DATA_PATH.join("log")
}

///
/// デフォルトのデータベースファイルのパス情報を生成
///
/// # 戻り値
/// データベースファイルのパス情報
///
fn default_db_path() -> PathBuf {
    DEFAULT_DATA_PATH.join("database.redb")
}

///
/// デフォルトのアセットデータ格納ディレクトリのパス情報を生成
///
/// # 戻り値
/// アセットデータ格納ディレクトリのパス情報
///
fn default_assets_path() -> PathBuf {
    DEFAULT_DATA_PATH.join("assets")
}

///
/// デフォルトの全文検索インデックス格納ディレクトリのパス情報を生成
///
/// # 戻り値
/// 全文検索インデックス格納ディレクトリのパス情報
///
fn default_fts_index_path() -> PathBuf {
    DEFAULT_DATA_PATH.join("index")
}

///
/// デフォルトのサーバ証明書ファイルのパス情報を生成
///
/// # 戻り値
/// サーバ証明書ファイルのパス情報
///
fn default_cert_path() -> PathBuf {
    DEFAULT_DATA_PATH.join("server.pem")
}

/// アセットの最大サイズ(10MiB)
const MAX_ASSET_SIZE: u64 = 10 * 1024 * 1024;

///
/// show_options()実装を要求するトレイト
///
trait ShowOptions {
    ///
    /// オプション設定内容の表示
    ///
    fn show_options(&self);
}

///
/// validate()実装を要求するトレイト
///
trait Validate {
    ///
    /// オプション設定内容の表示
    ///
    fn validate(&mut self) -> Result<()>;
}

///
/// apply_config()実装を要求するトレイト
///
trait ApplyConfig {
    ///
    /// オプション設定へのコンフィギュレーションの反映
    ///
    fn apply_config(&mut self, config: &Config);
}

///
/// ログレベルを指し示す列挙子
///
#[derive(Debug, Clone, Copy, PartialEq, ValueEnum, Deserialize, Serialize)]
#[clap(rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "UPPERCASE")]
enum LogLevel {
    /// ログを記録しない
    #[serde(alias = "off", alias = "OFF")]
    #[value(alias = "off")]
    None,

    /// エラー情報以上のレベルを記録
    Error,

    /// 警告情報以上のレベルを記録
    Warn,

    /// 一般情報以上のレベルを記録
    Info,

    /// デバッグ情報以上のレベルを記録
    Debug,

    /// トレース情報以上のレベルを記録
    Trace,
}

// Intoトレイトの実装
impl Into<log::LevelFilter> for LogLevel {
    fn into(self) -> log::LevelFilter {
        match self {
            Self::None => log::LevelFilter::Off,
            Self::Error => log::LevelFilter::Error,
            Self::Warn => log::LevelFilter::Warn,
            Self::Info => log::LevelFilter::Info,
            Self::Debug => log::LevelFilter::Debug,
            Self::Trace => log::LevelFilter::Trace,
        }
    }
}

// AsRefトレイトの実装
impl AsRef<str> for LogLevel {
    fn as_ref(&self) -> &str {
        match self {
            Self::None => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

///
/// グローバルオプション情報を格納する構造体
///
#[derive(Parser, Debug, Clone)]
#[command(
    name = env!("CARGO_PKG_NAME"),
    about = "ローカル運用向けWikiシステム",
    version,
    long_about = None,
    subcommand_required = false,
    arg_required_else_help = true,
)]
pub struct Options {
    /// config.tomlを使用する場合のパス
    #[arg(short = 'c', long = "config-path")]
    config_path: Option<PathBuf>,

    /// 記録するログレベルの指定
    #[arg(short = 'l', long = "log-level", value_name = "LEVEL",
        ignore_case = true)]
    log_level: Option<LogLevel>,

    /// ログの出力先の指定
    #[arg(short = 'L', long = "log-output", value_name = "PATH")]
    log_output: Option<PathBuf>,

    /// ログを標準出力にも同時出力するか否か
    #[arg(long = "log-tee")]
    log_tee: bool,

    /// データベースファイルのパス
    #[arg(short = 'd', long = "db-path")]
    db_path: Option<PathBuf>,

    /// 全文検索インデックスの格納パス
    #[arg(short = 'I', long = "fts-index")]
    fts_index: Option<PathBuf>,

    /// アセットデータ格納ディレクトリのパス
    #[arg(short = 'a', long = "assets-path")]
    assets_path: Option<PathBuf>,

    /// テンプレートページの格納パス
    #[arg(short = 't', long = "template-root", value_name = "PATH")]
    template_root: Option<String>,

    /// 設定情報の表示
    #[arg(long = "show-options")]
    show_options: bool,

    /// 設定情報の保存
    #[arg(long = "save-config")]
    save_config: bool,

    /// 実行するサブコマンド
    #[command(subcommand)]
    command: Option<Command>,
}

impl Options {
    ///
    /// ログレベルへのアクセサ
    ///
    /// # 戻り値
    /// 設定されたログレベルを返す
    fn log_level(&self) -> LogLevel {
        if let Some(level) = self.log_level {
            level
        } else {
            LogLevel::Info
        }
    }

    ///
    /// ログの出力先へのアクセサ
    ///
    /// # 戻り値
    /// ログの出力先として設定されたパス情報を返す。未設定の場合はデフォルトの
    /// パスを返す。
    ///
    fn log_output(&self) -> PathBuf {
        if let Some(path) = &self.log_output {
            path.clone()
        } else {
            default_log_path()
        }
    }

    ///
    /// ログの標準出力同時出力フラグへのアクセサ
    ///
    /// # 戻り値
    /// ログの標準出力同時出力が有効であればtrueを返す
    ///
    fn log_tee(&self) -> bool {
        self.log_tee
    }

    ///
    /// データベースパスへのアクセサ
    ///
    /// # 戻り値
    /// オプションで指定されたデータベースファイルへのパスを返す。オプションで未
    /// 定義の場合はデフォルトのパスを返す。
    ///
    pub(crate) fn db_path(&self) -> PathBuf {
        if let Some(path) = &self.db_path {
            path.clone()
        } else {
            default_db_path()
        }
    }

    ///
    /// 全文検索インデックス格納ディレクトリのパスへのアクセサ
    ///
    /// # 戻り値
    /// オプションで指定された全文検索インデックス格納ディレクトリへのパスを返
    /// す。オプションで未定義の場合はデフォルトのパスを返す。
    ///
    pub(crate) fn fts_index_path(&self) -> PathBuf {
        if let Some(path) = &self.fts_index {
            path.clone()
        } else {
            default_fts_index_path()
        }
    }

    ///
    /// アセットデータ格納ディレクトリのパスへのアクセサ
    ///
    /// # 戻り値
    /// オプションで指定されたアセットデータ格納ディレクトリへのパスを返す。オプ
    /// ションで未定義の場合はデフォルトのパスを返す。
    ///
    pub(crate) fn assets_path(&self) -> PathBuf {
        if let Some(path) = &self.assets_path {
            path.clone()
        } else {
            default_assets_path()
        }
    }

    ///
    /// frontend設定情報へのアクセサ
    ///
    pub(crate) fn frontend_config(&self) -> Result<FrontendConfig> {
        let path = if let Some(path) = &self.config_path {
            path.clone()
        } else {
            default_config_path()
        };

        if !path.exists() {
            return Ok(Config::default().frontend_config());
        }

        if !path.is_file() {
            return Err(anyhow!("{} is not file", path.display()));
        }

        let config = config::load(&path)?;
        Ok(config.frontend_config())
    }

    ///
    /// データベースのオープン
    ///
    /// # 戻り値
    /// オープンに成功した場合はデータベースオブジェクトを`Ok()`でラップして返
    /// す。失敗した場合はエラー情報を`Err()`でラップして返す。
    ///
    pub(crate) fn open_database(&self) -> Result<DatabaseManager> {
        match DatabaseManager::open(self.db_path(), self.assets_path()) {
            Ok(mgr) => Ok(mgr),
            Err(err) => Err(
                anyhow!("open failed: {}", err).context("database open")
            )
        }
    }

    ///
    /// テンプレートルートへのアクセサ
    ///
    /// # 戻り値
    /// テンプレートルートが設定されている場合は`Some()`で返す。
    ///
    pub(crate) fn template_root(&self) -> Option<String> {
        self.template_root.clone()
    }

    ///
    /// コンフィギュレーションファイルの適用
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`を返す。
    ///
    /// # 注記
    /// config.tomlを読み込みオプション情報に反映する。
    ///
    fn apply_config(&mut self) -> Result<()> {
        let path = if let Some(path) = &self.config_path {
            // オプションでコンフィギュレーションファイルのパスが指定されて
            // いる場合、そのパスに何もなければエラー
            if !path.exists() {
                return Err(anyhow!("{} is not exists", path.display()));
            }

            // 指定されたパスを返す
            path.clone()

        } else {
            default_config_path()
        };

        // この時点でパスに何も無い場合はそのまま何もせず正常終了
        if !path.exists() {
            return Ok(());
        }

        // 指定されたパスにあるのがファイルでなければエラー
        if !path.is_file() {
            return Err(anyhow!("{} is not file", path.display()));
        }

        // そのパスからコンフィギュレーションを読み取る
        match config::load(&path) {
            // コンフィギュレーションファイルを読み取れた場合は内容をオプション
            // 情報に反映する。
            Ok(config) => {
                if self.log_level.is_none() {
                    if let Some(level) = config.log_level() {
                        self.log_level = Some(level);
                    }
                }

                if self.log_output.is_none() {
                    if let Some(path) = &config.log_output() {
                        self.log_output = Some(path.clone());
                    }
                }

                if self.db_path.is_none() {
                    if let Some(path) = &config.db_path() {
                        self.db_path = Some(path.clone());
                    }
                }

                if self.fts_index.is_none() {
                    if let Some(path) = &config.fts_index() {
                        self.fts_index = Some(path.clone());
                    }
                }

                if self.assets_path.is_none() {
                    if let Some(path) = &config.assets_path() {
                        self.assets_path = Some(path.clone());
                    }
                }

                if self.template_root.is_none() {
                    if let Some(path) = config.template_root() {
                        self.template_root = Some(path);
                    }
                }

                // コマンド毎のオプション情報へもコンフィギュレーションの内容を
                // 反映する。
                let opts: Option<&mut dyn ApplyConfig> = match
                    &mut self.command
                {
                    Some(Command::Run(opts)) => Some(opts),
                    Some(Command::User(user)) => match &mut user.subcommand {
                        UserSubCommand::List(opts) => Some(opts),
                        _ => None,
                    }
                    Some(Command::Page(page)) => match &mut page.subcommand {
                        PageSubCommand::Add(opts) => Some(opts),
                        PageSubCommand::Delete(_) => None,
                        PageSubCommand::List(opts) => Some(opts),
                        PageSubCommand::MoveTo(_) => None,
                        PageSubCommand::Undelete(opts) => Some(opts),
                        PageSubCommand::Unlock(_) => None,
                    }
                    Some(Command::Lock(lock)) => match &mut lock.subcommand {
                        LockSubCommand::List(opts) => Some(opts),
                        _ => None,
                    }
                    Some(Command::Asset(asset)) => match &mut asset.subcommand {
                        AssetSubCommand::Add(opts) => Some(opts),
                        AssetSubCommand::List(opts) => Some(opts),
                        AssetSubCommand::Delete(_) => None,
                        AssetSubCommand::Undelete(_) => None,
                        AssetSubCommand::MoveTo(_) => None,
                    }
                    Some(Command::Fts(fts)) => match &mut fts.subcommand {
                        FtsSubCommand::Search(opts) => Some(opts),
                        _ => None,
                    }
                    _ => None,
                };

                if let Some(opts) = opts {
                    opts.apply_config(&config);
                }

                Ok(())
            }

            // エラーが出たらそのままエラー
            Err(err) => Err(anyhow!("{}", err))
        }
    }

    ///
    /// オプション情報のバリデート
    ///
    /// # 戻り値
    /// オプション情報に矛盾が無い場合は`Ok(())`を返す。
    ///
    fn validate(&mut self) -> Result<()> {
        if self.show_options && self.save_config {
            return Err(anyhow!(
                "--show-options and --save-config can't be specified mutually"
            ));
        }

        if let Some(command) = &mut self.command {
            let opts: Option<&mut dyn Validate> = match command {
                Command::Run(opts) => Some(opts),
                Command::User(user) => match &mut user.subcommand {
                    UserSubCommand::Delete(opts) => Some(opts),
                    UserSubCommand::Edit(opts) => Some(opts),
                    UserSubCommand::List(opts) => Some(opts),
                    _ => None
                }
                Command::Page(page) => match &mut page.subcommand {
                    PageSubCommand::Add(opts) => Some(opts),
                    PageSubCommand::Delete(opts) => Some(opts),
                    PageSubCommand::List(opts) => Some(opts),
                    PageSubCommand::MoveTo(opts) => Some(opts),
                    PageSubCommand::Undelete(opts) => Some(opts),
                    PageSubCommand::Unlock(opts) => Some(opts),
                }
                Command::Lock(lock) => match &mut lock.subcommand {
                    LockSubCommand::List(opts) => Some(opts),
                    _ => None
                }
                Command::Asset(asset) => match &mut asset.subcommand {
                    AssetSubCommand::Add(opts) => Some(opts),
                    AssetSubCommand::List(opts) => Some(opts),
                    AssetSubCommand::Delete(opts) => Some(opts),
                    AssetSubCommand::Undelete(opts) => Some(opts),
                    AssetSubCommand::MoveTo(opts) => Some(opts),
                }
                Command::Fts(fts) => match &mut fts.subcommand {
                    FtsSubCommand::Search(opts) => Some(opts),
                    _ => None,
                }
                Command::Commands => None,
                Command::HelpAll => None,
            };

            if let Some(opts) = opts {
                opts.validate()?;
            }
        }

        Ok(())
    }

    ///
    /// オプション設定内容の表示
    ///
    fn show_options(&self) {
        let config_path = if let Some(path) = &self.config_path {
            path.display().to_string()
        } else {
            let path = default_config_path();

            if path.exists() {
                path.display().to_string()
            } else {
                "(none)".to_string()
            }
        };

        println!("global options");
        println!("   config path:      {}", config_path);
        println!("   database path:    {}", self.db_path().display());
        println!("   fts index path:   {}", self.fts_index_path().display());
        println!("   log level:        {}", self.log_level().as_ref());
        println!("   log output:       {}", self.log_output().display());
        println!("   log tee:          {}", self.log_tee());
        println!("   assets directory: {}", self.assets_path().display());
        println!(
            "   template root:    {}",
            self.template_root
                .as_deref()
                .unwrap_or("(none)")
        );

        // サブコマンドが指定されており、そのサブコマンドがオプションを持つなら
        // そのオプションも表示する。
        if let Some(command) = &self.command {
            let opts: Option<&dyn ShowOptions> = match command {
                Command::Run(opts) => Some(opts),
                Command::User(user) => match &user.subcommand {
                    UserSubCommand::Add(opts) => Some(opts),
                    UserSubCommand::Delete(opts) => Some(opts),
                    UserSubCommand::Edit(opts) => Some(opts),
                    UserSubCommand::List(opts) => Some(opts),
                }
                Command::Page(page) => match &page.subcommand {
                    PageSubCommand::Add(opts) => Some(opts),
                    PageSubCommand::Delete(opts) => Some(opts),
                    PageSubCommand::List(opts) => Some(opts),
                    PageSubCommand::MoveTo(opts) => Some(opts),
                    PageSubCommand::Undelete(opts) => Some(opts),
                    PageSubCommand::Unlock(opts) => Some(opts),
                }
                Command::Lock(lock) => match &lock.subcommand {
                    LockSubCommand::List(opts) => Some(opts),
                    LockSubCommand::Delete(opts) => Some(opts),
                }
                Command::Asset(asset) => match &asset.subcommand {
                    AssetSubCommand::Add(opts) => Some(opts),
                    AssetSubCommand::List(opts) => Some(opts),
                    AssetSubCommand::Delete(opts) => Some(opts),
                    AssetSubCommand::Undelete(opts) => Some(opts),
                    AssetSubCommand::MoveTo(opts) => Some(opts),
                }
                Command::Fts(fts) => match &fts.subcommand {
                    FtsSubCommand::Search(opts) => Some(opts),
                    _ => None,
                }
                Command::Commands => None,
                Command::HelpAll => None,
            };

            if let Some(opts) = opts {
                println!("");
                opts.show_options();
            }
        }
    }

    ///
    /// サブコマンドのコマンドコンテキストの生成
    ///
    pub(crate) fn build_context(&self) -> Result<Box<dyn CommandContext>> {
        match &self.command {
            Some(Command::Run(opts)) => run::build_context(self, opts),
            Some(Command::User(user)) => match &user.subcommand {
                UserSubCommand::Add(opts) => user_add::build_context(self, opts),
                UserSubCommand::Delete(opts) => user_delete::build_context(self, opts),
                UserSubCommand::Edit(opts) => user_edit::build_context(self, opts),
                UserSubCommand::List(opts) => user_list::build_context(self, opts),
            }
            Some(Command::Page(page)) => match &page.subcommand {
                PageSubCommand::Add(opts) => page_add::build_context(self, opts),
                PageSubCommand::Delete(opts) => page_delete::build_context(self, opts),
                PageSubCommand::List(opts) => {
                    page_list::build_context(self, opts)
                }
                PageSubCommand::MoveTo(opts) => {
                    page_move_to::build_context(self, opts)
                }
                PageSubCommand::Undelete(opts) => {
                    page_undelete::build_context(self, opts)
                }
                PageSubCommand::Unlock(opts) => {
                    page_unlock::build_context(self, opts)
                }
            }
            Some(Command::Lock(lock)) => match &lock.subcommand {
                LockSubCommand::List(opts) => {
                    lock_list::build_context(self, opts)
                }
                LockSubCommand::Delete(opts) => {
                    lock_delete::build_context(self, opts)
                }
            }
            Some(Command::Asset(asset)) => match &asset.subcommand {
                AssetSubCommand::Add(opts) => asset_add::build_context(self, opts),
                AssetSubCommand::List(opts) => asset_list::build_context(self, opts),
                AssetSubCommand::Delete(opts) => asset_delete::build_context(self, opts),
                AssetSubCommand::Undelete(opts) => {
                    asset_undelete::build_context(self, opts)
                }
                AssetSubCommand::MoveTo(opts) => {
                    asset_move_to::build_context(self, opts)
                }
            }
            Some(Command::Fts(fts)) => match &fts.subcommand {
                FtsSubCommand::Rebuild => fts_rebuild::build_context(self),
                FtsSubCommand::Merge => fts_merge::build_context(self),
                FtsSubCommand::Search(opts) => fts_search::build_context(self, opts),
            }
            Some(Command::Commands) => commands::build_context(self),
            Some(Command::HelpAll) => help_all::build_context(self),
            None => Err(anyhow!("command not specified")),
        }
    }
}

///
/// サブコマンドの定義
///
#[derive(Clone, Debug, Subcommand)]
enum Command {
    /// サーバの起動
    #[command(name = "run", alias = "r")]
    Run(RunOpts),

    /// ユーザ管理コマンド一覧の表示
    #[command(name = "user", alias = "u")]
    User(UserCommand),

    /// ページ管理コマンド一覧の表示
    #[command(name = "page", alias = "p")]
    Page(PageCommand),

    /// ロック管理コマンド一覧の表示
    #[command(name = "lock", alias = "l")]
    Lock(LockCommand),

    /// アセット管理コマンド一覧の表示
    #[command(name = "asset", alias = "a")]
    Asset(AssetCommand),

    /// 全文検索管理コマンド一覧の表示
    #[command(name = "fts", alias = "i", alias = "index")]
    Fts(FtsCommand),

    /// サブコマンド一覧の表示
    #[command(name = "commands")]
    Commands,

    /// 全サブコマンドのヘルプ出力
    #[command(name = "help-all")]
    HelpAll,
}

#[derive(Clone, Args, Debug)]
pub(crate) struct UserCommand {
    #[command(subcommand)]
    subcommand: UserSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum UserSubCommand {
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
    List(UserListOpts)
}

#[derive(Clone, Args, Debug)]
pub(crate) struct PageCommand {
    #[command(subcommand)]
    subcommand: PageSubCommand,
}

#[derive(Clone, Args, Debug)]
pub(crate) struct LockCommand {
    #[command(subcommand)]
    subcommand: LockSubCommand,
}

#[derive(Clone, Args, Debug)]
pub(crate) struct AssetCommand {
    #[command(subcommand)]
    subcommand: AssetSubCommand,
}

#[derive(Clone, Args, Debug)]
pub(crate) struct FtsCommand {
    #[command(subcommand)]
    subcommand: FtsSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PageSubCommand {
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

#[derive(Clone, Debug, Subcommand)]
enum LockSubCommand {
    /// ロック情報の一覧表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(LockListOpts),

    /// ロック情報の削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(LockDeleteOpts),
}

#[derive(Clone, Debug, Subcommand)]
enum AssetSubCommand {
    /// アセットの追加
    #[command(name = "add", alias = "a")]
    Add(AssetAddOpts),

    /// アセット一覧の表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(AssetListOpts),

    /// アセットの削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(AssetDeleteOpts),

    /// アセットの回復
    #[command(name = "undelete", alias = "ud")]
    Undelete(AssetUndeleteOpts),

    /// アセットの移動
    #[command(name = "move_to", alias = "m", alias = "mv")]
    MoveTo(AssetMoveToOpts),
}

#[derive(Clone, Debug, Subcommand)]
enum FtsSubCommand {
    /// 全文検索インデックスの再構築
    #[command(name = "rebuild", alias = "r")]
    Rebuild,

    /// 全文検索インデックスのマージ
    #[command(name = "merge", alias = "m")]
    Merge,

    /// 全文検索
    #[command(name = "search", alias = "s")]
    Search(FtsSearchOpts),
}

///
/// サブコマンドrunのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct RunOpts {
    /// バインド先のアドレス
    #[arg(short = 'b', long = "open-browser", help = "ブラウザを起動する")]
    open_browser: bool,

    /// TLSでの通信を有効にする
    #[arg(short = 'T', long = "tls")]
    use_tls: bool,

    /// TLS用のサーバ証明書ファイルのパス
    #[arg(short = 'C', long = "cert", value_name = "FILE")]
    cert_path: Option<PathBuf>,

    /// サーバのバインド先
    #[arg()]
    bind_addr: Option<String>,

    /// サーバのバインド先ポート
    #[arg(skip)]
    bind_port: Option<u16>,
}

impl RunOpts {
    ///
    /// 検索キーへのアクセサ
    ///
    /// # 戻り値
    /// キー文字列を返す
    ///
    pub(crate) fn is_browser_open(&self) -> bool {
        self.open_browser
    }

    ///
    /// TLS有効フラグへのアクセサ
    ///
    /// # 戻り値
    /// TLSが有効ならtrueを返す。
    ///
    pub(crate) fn use_tls(&self) -> bool {
        self.use_tls
    }

    ///
    /// サーバ証明書ファイルのパスへのアクセサ
    ///
    /// # 戻り値
    /// オプションで指定された証明書パスを返す。未指定の場合はデフォルトのパス
    /// を返す。
    ///
    pub(crate) fn cert_path(&self) -> PathBuf {
        if let Some(path) = &self.cert_path {
            path.clone()
        } else {
            default_cert_path()
        }
    }

    ///
    /// サーバ証明書パスの明示指定フラグへのアクセサ
    ///
    /// # 戻り値
    /// 証明書パスが明示指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_cert_path_explicit(&self) -> bool {
        self.cert_path.is_some()
    }

    ///
    /// バインド先のアドレスへのアクセサ
    ///
    pub(crate) fn bind_addr(&self) -> String {
        if let Some(addr) = &self.bind_addr {
            addr.clone()
        } else {
            "0.0.0.0".to_string()
        }
    }

    ///
    /// バインド先のポート番号へのアクセサ
    ///
    pub(crate) fn bind_port(&self) -> u16 {
        if let Some(port) = self.bind_port {
            port
        } else {
            8080
        }
    }
}

// Validateトレイトの実装
impl Validate for RunOpts {
    fn validate(&mut self) -> Result<()> {
        if let Some(value) = &self.bind_addr {
            let (addr, port) = parse_bind_value(value)?;

            if let Some(current_port) = self.bind_port {
                if let Some(parsed_port) = port {
                    if current_port != parsed_port {
                        return Err(anyhow!(
                            "bind port is inconsistent: {} vs {}",
                            current_port,
                            parsed_port
                        ));
                    }
                }
            }

            self.bind_addr = Some(addr);
            if self.bind_port.is_none() {
                self.bind_port = port;
            }
        }

        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for RunOpts {
    fn apply_config(&mut self, config: &Config) {
        if let Some(value) = &self.bind_addr {
            if self.bind_port.is_none() {
                if let Ok((addr, port)) = parse_bind_value(value) {
                    self.bind_addr = Some(addr);
                    self.bind_port = port;
                }
            }
        } else if let Some(addr) = config.run_bind_addr() {
            self.bind_addr = Some(addr);
        }

        if self.bind_port.is_none() {
            if let Some(port) = config.run_bind_port() {
                self.bind_port = Some(port);
            }
        }

        if !self.use_tls {
            if let Some(use_tls) = config.use_tls() {
                self.use_tls = use_tls;
            }
        }

        if self.cert_path.is_none() {
            if let Some(path) = config.server_cert() {
                self.cert_path = Some(path);
            }
        }
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for UserListOpts {
    fn apply_config(&mut self, config: &Config) {
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

// ApplyConfigトレイトの実装
impl ApplyConfig for PageListOpts {
    fn apply_config(&mut self, config: &Config) {
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

// ApplyConfigトレイトの実装
impl ApplyConfig for PageAddOpts {
    fn apply_config(&mut self, config: &Config) {
        if self.user_name.is_none() {
            if let Some(user_name) = config.page_add_default_user() {
                self.user_name = Some(user_name);
            }
        }
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for PageUndeleteOpts {
    fn apply_config(&mut self, config: &Config) {
        if !self.without_assets {
            if let Some(with_assets) = config.page_undelete_with_assets() {
                if !with_assets {
                    self.without_assets = true;
                }
            }
        }
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for AssetAddOpts {
    fn apply_config(&mut self, config: &Config) {
        if self.user_name.is_none() {
            if let Some(user_name) = config.asset_add_default_user() {
                self.user_name = Some(user_name);
            }
        }
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for AssetListOpts {
    fn apply_config(&mut self, config: &Config) {
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

// ApplyConfigトレイトの実装
impl ApplyConfig for LockListOpts {
    fn apply_config(&mut self, config: &Config) {
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
impl Validate for UserListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// Validateトレイトの実装
impl Validate for PageListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
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

// Validateトレイトの実装
impl Validate for PageMoveToOpts {
    fn validate(&mut self) -> Result<()> {
        if let Err(message) = validate_page_path(&self.dst_path) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
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

// Validateトレイトの実装
impl Validate for PageDeleteOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("page id or path is empty"));
        }

        Ok(())
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

        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_ASSET_SIZE {
            return Err(anyhow!("asset size exceeds limit"));
        }

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("file name is invalid"))?;
        if let Err(message) = validate_asset_file_name(file_name) {
            return Err(anyhow!("invalid file name: {}", message));
        }

        if let Ok(_) = PageId::from_string(&self.target) {
            return Ok(());
        }

        if let Err(message) = validate_page_path(&self.target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
    }
}

// Validateトレイトの実装
impl Validate for AssetListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
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

// Validateトレイトの実装
impl Validate for AssetUndeleteOpts {
    fn validate(&mut self) -> Result<()> {
        if self.target.trim().is_empty() {
            return Err(anyhow!("asset id is empty"));
        }

        if AssetId::from_string(&self.target).is_err() {
            return Err(anyhow!("invalid asset id"));
        }

        Ok(())
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

        if let Ok(_) = PageId::from_string(&self.dst_target) {
            return Ok(());
        }

        if let Err(message) = validate_page_path(&self.dst_target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        Ok(())
    }
}

// Validateトレイトの実装
impl Validate for LockListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// Validateトレイトの実装
impl Validate for UserDeleteOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
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

///
/// BIND-ADDR[:PORT]形式の値を解析する
///
/// # 概要
/// IPv6の`[ADDR]:PORT`形式、`ADDR:PORT`形式、`ADDR`のみの形式を解析し、
/// バインド先のアドレスとポートを返す。
///
/// # 引数
/// * `value` - 解析対象の文字列
///
/// # 戻り値
/// 解析に成功した場合は`(bind_addr, bind_port)`を返す。ポートが指定されて
/// いない場合は`None`を返す。
///
fn parse_bind_value(value: &str) -> Result<(String, Option<u16>)> {
    /*
     * 入力の事前チェック
     */
    if value.is_empty() {
        return Err(anyhow!("bind address is empty"));
    }

    /*
     * IPv6角括弧形式の解析
     */
    if let Some(rest) = value.strip_prefix('[') {
        let close_pos = rest.find(']')
            .ok_or_else(|| anyhow!("invalid bind address: {}", value))?;
        let addr = &rest[..close_pos];
        if addr.is_empty() {
            return Err(anyhow!("bind address is empty"));
        }

        let tail = &rest[close_pos + 1..];
        if tail.is_empty() {
            return Ok((addr.to_string(), None));
        }

        if let Some(port_str) = tail.strip_prefix(':') {
            if port_str.is_empty() {
                return Err(anyhow!("bind port is empty"));
            }

            return Ok((addr.to_string(), Some(port_str.parse()?)));
        }

        return Err(anyhow!("invalid bind address: {}", value));
    }

    /*
     * IPv4/ホスト名形式の解析
     */
    let colon_count = value.matches(':').count();
    if colon_count == 0 {
        return Ok((value.to_string(), None));
    }

    if colon_count == 1 {
        let mut iter = value.splitn(2, ':');
        let addr = iter.next().unwrap_or_default();
        let port_str = iter.next().unwrap_or_default();

        if addr.is_empty() {
            return Err(anyhow!("bind address is empty"));
        }
        if port_str.is_empty() {
            return Err(anyhow!("bind port is empty"));
        }

        return Ok((addr.to_string(), Some(port_str.parse()?)));
    }

    /*
     * IPv6リテラル形式の解析
     */
    Ok((value.to_string(), None))
}

// ShowOptionsトレイトの実装
impl ShowOptions for RunOpts {
    fn show_options(&self) {
        println!("run command options");
        println!("   browser_open:   {:?}", self.is_browser_open());
        println!("   tls enabled:    {}", self.use_tls());
        println!("   cert path:      {}", self.cert_path().display());
        println!("   bind:  {}:{}", self.bind_addr(), self.bind_port());
    }
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

// ShowOptionsトレイトの実装
impl ShowOptions for UserDeleteOpts {
    fn show_options(&self) {
        println!("user delete command options");
        println!("   user_name: {}", self.user_name());
    }
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

// ShowOptionsトレイトの実装
impl ShowOptions for PageUnlockOpts {
    fn show_options(&self) {
        println!("page unlock command options");
        println!("   target: {}", self.target());
    }
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

    /// ソフトデリート済みのアセットをハードデリートする
    #[arg(short = 'p', long = "purge")]
    purge: bool,

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
    /// パージ指定へのアクセサ
    ///
    /// # 戻り値
    /// パージが指定されている場合はtrue
    ///
    pub(crate) fn is_purge(&self) -> bool {
        self.purge
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

// ShowOptionsトレイトの実装
impl ShowOptions for AssetDeleteOpts {
    fn show_options(&self) {
        println!("asset delete command options");
        println!("   hard_delete: {:?}", self.is_hard_delete());
        println!("   purge:       {:?}", self.is_purge());
        println!("   target:      {}", self.target());
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
}

// ShowOptionsトレイトの実装
impl ShowOptions for AssetUndeleteOpts {
    fn show_options(&self) {
        println!("asset undelete command options");
        println!("   target: {}", self.target());
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

// ShowOptionsトレイトの実装
impl ShowOptions for AssetMoveToOpts {
    fn show_options(&self) {
        println!("asset move_to command options");
        println!("   force:      {:?}", self.is_force());
        println!("   asset_id:   {}", self.asset_id());
        println!("   dst_target: {}", self.dst_target());
    }
}

///
/// fts searchサブコマンドの検索対象
///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ValueEnum)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum FtsSearchTarget {
    /// 見出し
    Headings,

    /// 本文
    Body,

    /// コードブロック
    Code,
}

///
/// サブコマンドfts searchのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct FtsSearchOpts {
    /// 検索対象
    #[arg(short = 't', long = "target", value_name = "TARGET")]
    target: Option<FtsSearchTarget>,

    /// 削除済みページを検索対象に含める
    #[arg(short = 'd', long = "with-deleted")]
    with_deleted: bool,

    /// 全リビジョンを検索対象に含める
    #[arg(short = 'a', long = "all-revision")]
    all_revision: bool,

    /// 検索式
    #[arg()]
    expression: String,
}

impl FtsSearchOpts {
    ///
    /// 検索対象のアクセサ
    ///
    pub(crate) fn target(&self) -> FtsSearchTarget {
        self.target.unwrap_or(FtsSearchTarget::Body)
    }

    ///
    /// 削除済み対象を含めるか否か
    ///
    /// # 戻り値
    /// 削除済みページを含める場合は`true`
    ///
    pub(crate) fn with_deleted(&self) -> bool {
        self.with_deleted
    }

    ///
    /// 全リビジョン対象か否か
    ///
    /// # 戻り値
    /// 全リビジョンを対象に含める場合は`true`
    ///
    pub(crate) fn all_revision(&self) -> bool {
        self.all_revision
    }

    ///
    /// 検索式のアクセサ
    ///
    pub(crate) fn expression(&self) -> String {
        self.expression.clone()
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for FtsSearchOpts {
    fn show_options(&self) {
        println!("fts search command options");
        println!("   target:     {:?}", self.target());
        println!("   deleted:    {:?}", self.with_deleted());
        println!("   revision:   {:?}", self.all_revision());
        println!("   expression: {}", self.expression());
    }
}

// Validateトレイトの実装
impl Validate for FtsSearchOpts {
    fn validate(&mut self) -> Result<()> {
        if self.expression.trim().is_empty() {
            return Err(anyhow!("search expression is empty"));
        }
        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for FtsSearchOpts {
    fn apply_config(&mut self, config: &Config) {
        if self.target.is_none() {
            if let Some(target) = config.fts_search_target() {
                self.target = Some(target);
            }
        }

        if !self.with_deleted {
            if let Some(with_deleted) = config.fts_search_with_deleted() {
                self.with_deleted = with_deleted;
            }
        }

        if !self.all_revision {
            if let Some(all_revision) = config.fts_search_all_revision() {
                self.all_revision = all_revision;
            }
        }
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

// ShowOptionsトレイトの実装
impl ShowOptions for UserListOpts {
    fn show_options(&self) {
        println!("user list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
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

// ShowOptionsトレイトの実装
impl ShowOptions for PageListOpts {
    fn show_options(&self) {
        println!("page list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
        println!("   long_info:    {:?}", self.is_long_info());
    }
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

///
/// コマンドライン引数のパース処理
///
/// # 戻り値
/// オプション情報をまとめたオブジェクトを返す。
///
pub(crate) fn parse() -> Result<Arc<Options>> {
    let mut opts = Options::parse();

    /*
     * デフォルトデータパスの作成
     */
    std::fs::create_dir_all(DEFAULT_DATA_PATH.clone())?;

    /*
     * コンフィギュレーションファイルの適用
     */
    opts.apply_config()?;

    /*
     * 設定情報のバリデーション
     */
    opts.validate()?;

    /*
     * ログ機能の初期化
     */
    logger::init(&opts)?;

    /*
     * 設定情報の表示
     */
    if opts.show_options {
        opts.show_options();
        std::process::exit(0);
    }

    /*
     * 設定の保存
     */
    if opts.save_config {
        save_config(&opts)?;
        std::process::exit(0);
    }

    /*
     * 設定情報の返却
     */
    Ok(Arc::new(opts))
}

///
/// 設定保存が必要であればconfig.tomlへ書き込みを行う
///
/// # 概要
/// 既存の設定ファイルがある場合は読み込み、現在の設定内容で更新した上で保存
/// する。設定ファイルが存在しない場合はデフォルト設定を基準に更新して保存す
/// る。
///
/// # 引数
/// * `opts` - コマンドラインとコンフィグ適用後の設定情報
///
/// # 戻り値
/// 保存処理に成功した場合は`Ok(())`を返す。
///
fn save_config(opts: &Options) -> Result<()> {
    /*
     * 保存先パスの決定
     */
    let path = if let Some(path) = &opts.config_path {
        path.clone()
    } else {
        default_config_path()
    };

    /*
     * 既存ファイルの上書き確認
     */
    if path.exists() {
        if !confirm_overwrite(&path)? {
            return Ok(());
        }
    }

    /*
     * 保存先ディレクトリの作成
     */
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    /*
     * 現在の設定内容を反映
     */
    let mut config = if path.exists() {
        config::load(&path)?
    } else {
        Config::default()
    };

    config.set_log_level(opts.log_level());
    config.set_log_output(opts.log_output());
    config.set_db_path(opts.db_path());
    config.set_fts_index(opts.fts_index_path());
    config.set_assets_path(opts.assets_path());
    config.set_template_root(opts.template_root());

    match &opts.command {
        Some(Command::Run(opts)) => {
            config.set_run_bind_addr(opts.bind_addr());
            config.set_run_bind_port(opts.bind_port());
            config.set_run_use_tls(opts.use_tls());
            config.set_run_server_cert(opts.cert_path());
        }

        Some(Command::User(user)) => match &user.subcommand {
            UserSubCommand::List(opts) => {
                config.set_user_list_sort_mode(opts.sort_mode());
                config.set_user_list_reverse_sort(opts.is_reverse_sort());
            }

            _ => {}
        }

        Some(Command::Page(page)) => match &page.subcommand {
            PageSubCommand::Add(opts) => {
                if let Some(user_name) = opts.user_name.as_ref() {
                    config.set_page_add_default_user(user_name.clone());
                }
            }
            PageSubCommand::Delete(_) => {}
            PageSubCommand::List(opts) => {
                config.set_page_list_sort_mode(opts.sort_mode());
                config.set_page_list_reverse_sort(opts.is_reverse_sort());
                config.set_page_list_long_info(opts.is_long_info());
            }
            PageSubCommand::MoveTo(_) => {}
            PageSubCommand::Undelete(opts) => {
                config.set_page_undelete_with_assets(!opts.is_without_assets());
            }
            PageSubCommand::Unlock(_) => {}
        }

        Some(Command::Lock(lock)) => match &lock.subcommand {
            LockSubCommand::List(opts) => {
                config.set_lock_list_sort_mode(opts.sort_mode());
                config.set_lock_list_reverse_sort(opts.is_reverse_sort());
                config.set_lock_list_long_info(opts.is_long_info());
            }
            _ => {}
        }

        Some(Command::Asset(asset)) => match &asset.subcommand {
            AssetSubCommand::Add(opts) => {
                if let Some(user_name) = opts.user_name.as_ref() {
                    config.set_asset_add_default_user(user_name.clone());
                }
            }
            AssetSubCommand::List(opts) => {
                config.set_asset_list_sort_mode(opts.sort_mode());
                config.set_asset_list_reverse_sort(opts.is_reverse_sort());
                config.set_asset_list_long_info(opts.is_long_info());
            }
            AssetSubCommand::Delete(_) => {}
            AssetSubCommand::Undelete(_) => {}
            AssetSubCommand::MoveTo(_) => {}
        }

        Some(Command::Fts(fts)) => match &fts.subcommand {
            FtsSubCommand::Search(opts) => {
                config.set_fts_search_target(opts.target());
                config.set_fts_search_with_deleted(opts.with_deleted());
                config.set_fts_search_all_revision(opts.all_revision());
            }
            _ => {}
        }

        _ => {}
    }

    /*
     * 保存処理の実行
     */
    config.save(&path)?;

    Ok(())
}

///
/// config.tomlの上書き可否を標準入出力で問い合わせる
///
/// # 引数
/// * `path` - 対象となるパス
///
/// # 戻り値
/// 上書きを許可する場合は`true`、拒否された場合は`false`を返す。
///
fn confirm_overwrite(path: &Path) -> Result<bool> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut input = stdin.lock();
    let mut output = stdout.lock();

    confirm_overwrite_with_io(path, &mut input, &mut output)
}

///
/// 任意の入出力を使ってconfig.tomlの上書き可否を問い合わせる
///
/// # 引数
/// * `path` - 対象となるパス
/// * `input` - 入力ストリーム（質問への回答を受け取る）
/// * `output` - 出力ストリーム（質問を表示する）
///
/// # 戻り値
/// 上書きを許可する場合は`true`、拒否された場合は`false`を返す。
///
fn confirm_overwrite_with_io<R, W>(path: &Path, input: &mut R, output: &mut W,)
    -> Result<bool>
where
    R: BufRead,
    W: Write,
{
    write!(
        output,
        "{} は既に存在します。上書きしますか？ [y/N]: ",
        path.display()
    )?;
    output.flush()?;

    let mut buf = String::new();
    input.read_line(&mut buf)?;

    let ans = buf.trim().to_lowercase();
    Ok(ans == "y" || ans == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tempfile::TempDir;

    #[test]
    fn parse_tls_and_cert_options() {
        let dir = TempDir::new().expect("temp dir");
        let cert_path = dir.path().join("server.pem");
        let cert_arg = cert_path.to_string_lossy().to_string();
        let args = ["luwiki", "run", "--tls", "--cert", &cert_arg];

        let opts = Options::try_parse_from(args).expect("parse failed");
        let run_opts = match opts.command {
            Some(Command::Run(run_opts)) => run_opts,
            _ => panic!("run options missing"),
        };
        assert!(run_opts.use_tls());
        assert!(run_opts.is_cert_path_explicit());
        assert_eq!(run_opts.cert_path(), cert_path);
    }

    #[test]
    fn save_config_writes_tls_settings() {
        let dir = TempDir::new().expect("temp dir");
        let config_path = dir.path().join("config.toml");
        let config_arg = config_path.to_string_lossy().to_string();
        let cert_path = dir.path().join("certs/server.pem");
        let cert_arg = cert_path.to_string_lossy().to_string();
        let args = [
            "luwiki",
            "--config-path",
            &config_arg,
            "run",
            "--tls",
            "--cert",
            &cert_arg,
        ];

        let opts = Options::try_parse_from(args).expect("parse failed");
        save_config(&opts).expect("save failed");

        let config = config::load(&config_path).expect("load failed");
        assert_eq!(config.use_tls(), Some(true));
        assert_eq!(config.server_cert(), Some(cert_path));
    }
}
