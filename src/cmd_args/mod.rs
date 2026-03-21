/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! コマンドライン引数を取り扱うモジュール
//!

mod config;
mod export;
mod fts;
mod asset;
mod import;
mod lock;
mod logger;
mod page;
mod run;
mod token;
mod user;

use std::io::{BufRead, self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::command::{
    asset_add, asset_delete, asset_list, asset_move_to, asset_purge,
    asset_undelete, commands, export as export_command, fts_merge,
    fts_rebuild, fts_search, help_all, import as import_command,
    lock_delete, lock_list, page_add, page_delete, page_list, page_move_to,
    page_undelete, page_unlock, run as run_command, token_create,
    token_list, token_purge, token_revoke,
    user_add, user_delete, user_edit, user_list, CommandContext,
};
use crate::database::DatabaseManager;
use config::Config;
pub(crate) use asset::{
    AssetAddOpts,
    AssetCommand,
    AssetDeleteOpts,
    AssetListOpts,
    AssetListSortMode,
    AssetMoveToOpts,
    AssetPurgeOpts,
    AssetSubCommand,
    AssetUndeleteOpts,
};
pub(crate) use config::FrontendConfig;
pub(crate) use export::ExportOpts;
pub(crate) use fts::{
    FtsCommand,
    FtsSearchOpts,
    FtsSearchTarget,
    FtsSubCommand,
};
pub(crate) use import::ImportOpts;
pub(crate) use lock::{
    LockCommand,
    LockDeleteOpts,
    LockListOpts,
    LockListSortMode,
    LockSubCommand,
};
pub(crate) use page::{
    PageAddOpts,
    PageCommand,
    PageDeleteOpts,
    PageListOpts,
    PageListSortMode,
    PageMoveToOpts,
    PageSubCommand,
    PageUndeleteOpts,
    PageUnlockOpts,
};
pub(crate) use run::RunOpts;
pub(crate) use token::{
    TokenCommand,
    TokenCreateOpts,
    TokenListOpts,
    TokenPurgeOpts,
    TokenRevokeOpts,
    TokenSubCommand,
};
pub(crate) use user::{
    UserAddOpts,
    UserCommand,
    UserDeleteOpts,
    UserEditOpts,
    UserListOpts,
    UserListSortMode,
    UserSubCommand,
};

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

/// デフォルトのWikiタイトル
const DEFAULT_WIKI_TITLE: &str = "LUWIKI";

/// アセットサイズ単位(KiB)
const ASSET_SIZE_KIB: u64 = 1024;

/// アセットサイズ単位(MiB)
const ASSET_SIZE_MIB: u64 = ASSET_SIZE_KIB * 1024;

/// アセットサイズ上限のデフォルト値
const DEFAULT_ASSET_LIMIT_SIZE_TEXT: &str = "10M";

/// アセットサイズ上限のデフォルト値(10MiB)
const DEFAULT_ASSET_LIMIT_SIZE: u64 = 10 * ASSET_SIZE_MIB;

/// アセットサイズ上限として許可する最大値(100MiB)
const MAX_ASSET_LIMIT_SIZE: u64 = 100 * ASSET_SIZE_MIB;

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
    ///
    /// ログレベルを `log::LevelFilter` へ変換
    ///
    /// # 戻り値
    /// 対応する `log::LevelFilter`
    ///
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
    ///
    /// ログレベルの文字列表現を返す
    ///
    /// # 戻り値
    /// 設定ファイルやCLI表示で使用するログレベル文字列
    ///
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

    /// Wikiタイトル
    #[arg(short = 'T', long = "wiki-title", value_name = "TITLE")]
    wiki_title: Option<String>,

    /// アセットサイズ上限
    #[arg(short = 'S', long = "asset-limit-size", value_name = "SIZE")]
    asset_limit_size: Option<String>,

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
        /*
         * 設定ファイルパスの決定
         */
        let path = if let Some(path) = &self.config_path {
            path.clone()
        } else {
            default_config_path()
        };

        /*
         * 設定ファイルの存在確認と読込
         */
        if !path.exists() {
            return Ok(Config::default().frontend_config());
        }

        if !path.is_file() {
            return Err(anyhow!("{} is not file", path.display()));
        }

        /*
         * frontend設定の返却
         */
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
            ),
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
    /// Wikiタイトルへのアクセサ
    ///
    /// # 戻り値
    /// Wikiタイトルが設定されている場合はその値を返す。未設定または空白のみの
    /// 場合はデフォルト値を返す。
    ///
    pub(crate) fn wiki_title(&self) -> String {
        self.wiki_title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| DEFAULT_WIKI_TITLE.to_string())
    }

    ///
    /// アセットサイズ上限へのアクセサ
    ///
    /// # 戻り値
    /// アップロード可能なアセットサイズの上限値(バイト単位)を返す。
    ///
    pub(crate) fn asset_limit_size(&self) -> Result<u64> {
        let raw = self
            .asset_limit_size
            .as_deref()
            .unwrap_or(DEFAULT_ASSET_LIMIT_SIZE_TEXT);
        parse_asset_limit_size(raw)
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
        /*
         * 使用する設定ファイルパスの決定
         */
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

        /*
         * 設定ファイルの存在確認
         */
        // この時点でパスに何も無い場合はそのまま何もせず正常終了
        if !path.exists() {
            return Ok(());
        }

        // 指定されたパスにあるのがファイルでなければエラー
        if !path.is_file() {
            return Err(anyhow!("{} is not file", path.display()));
        }

        /*
         * 設定値を読み込みグローバル・サブコマンドへ反映
         */
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

                if self.wiki_title.is_none() {
                    if let Some(title) = config.wiki_title() {
                        self.wiki_title = Some(title);
                    }
                }

                if self.asset_limit_size.is_none() {
                    if let Some(size) = config.asset_limit_size() {
                        self.asset_limit_size = Some(size);
                    }
                }

                if let Some(opts) = self
                    .command
                    .as_mut()
                    .and_then(Command::apply_config_target_mut)
                {
                    opts.apply_config(&config);
                }

                Ok(())
            }

            // エラーが出たらそのままエラー
            Err(err) => Err(anyhow!("{}", err)),
        }
    }

    ///
    /// オプション情報のバリデート
    ///
    /// # 戻り値
    /// オプション情報に矛盾が無い場合は`Ok(())`を返す。
    ///
    fn validate(&mut self) -> Result<()> {
        /*
         * グローバルオプションの整合性確認
         */
        if self.show_options && self.save_config {
            return Err(anyhow!(
                "--show-options and --save-config can't be specified mutually"
            ));
        }

        if let Some(value) = self.asset_limit_size.as_deref() {
            parse_asset_limit_size(value)?;
        }

        /*
         * サブコマンド固有オプションの検証
         */
        if let Some(opts) = self
            .command
            .as_mut()
            .and_then(Command::validate_target_mut)
        {
                opts.validate()?;
        }

        Ok(())
    }

    ///
    /// オプション設定内容の表示
    ///
    fn show_options(&self) {
        /*
         * グローバルオプション表示用の値を組み立て
         */
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

        /*
         * グローバルオプションを表示
         */
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
            self.template_root.as_deref().unwrap_or("(none)")
        );
        println!("   wiki title:       {}", self.wiki_title());
        println!(
            "   asset limit size: {}",
            self.asset_limit_size().unwrap_or(DEFAULT_ASSET_LIMIT_SIZE),
        );

        // サブコマンドが指定されており、そのサブコマンドがオプションを持つなら
        // そのオプションも表示する。
        if let Some(opts) = self
            .command
            .as_ref()
            .and_then(Command::show_options_target)
        {
                println!("");
                opts.show_options();
        }
    }

    ///
    /// サブコマンドのコマンドコンテキストの生成
    ///
    pub(crate) fn build_context(&self) -> Result<Box<dyn CommandContext>> {
        /*
         * サブコマンドに応じた実行コンテキストを構築
         */
        self.command
            .as_ref()
            .map(|command| command.build_context(self))
            .unwrap_or_else(|| Err(anyhow!("command not specified")))
    }
}

///
/// サブコマンドの定義
///
#[derive(Clone, Debug, Subcommand)]
enum Command {
    /// サーバの起動
    #[command(name = "run", alias = "r")]
    Run(run::RunOpts),

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
    #[command(name = "fts", alias = "f", alias = "index")]
    Fts(FtsCommand),

    /// Bearerトークン管理コマンド一覧の表示
    #[command(name = "token", alias = "t")]
    Token(TokenCommand),

    /// バックアップ／マイグレート用データのエクスポート
    #[command(name = "export", alias = "e")]
    Export(export::ExportOpts),

    /// エクスポートデータのインポート
    #[command(name = "import", alias = "i")]
    Import(import::ImportOpts),

    /// サブコマンド一覧の表示
    #[command(name = "commands")]
    Commands,

    /// 全サブコマンドのヘルプ出力
    #[command(name = "help-all")]
    HelpAll,
}

impl Command {
    ///
    /// 設定反映対象のサブコマンドオプションを返す
    ///
    fn apply_config_target_mut(&mut self) -> Option<&mut dyn ApplyConfig> {
        match self {
            Self::Run(opts) => Some(opts),
            Self::User(user) => match &mut user.subcommand {
                UserSubCommand::List(opts) => Some(opts),
                _ => None,
            },
            Self::Page(page) => match &mut page.subcommand {
                PageSubCommand::Add(opts) => Some(opts),
                PageSubCommand::List(opts) => Some(opts),
                PageSubCommand::Undelete(opts) => Some(opts),
                _ => None,
            },
            Self::Lock(lock) => match &mut lock.subcommand {
                LockSubCommand::List(opts) => Some(opts),
                _ => None,
            },
            Self::Asset(asset) => match &mut asset.subcommand {
                AssetSubCommand::Add(opts) => Some(opts),
                AssetSubCommand::List(opts) => Some(opts),
                _ => None,
            },
            Self::Fts(fts) => match &mut fts.subcommand {
                FtsSubCommand::Search(opts) => Some(opts),
                _ => None,
            },
            Self::Token(token) => match &mut token.subcommand {
                TokenSubCommand::Create(opts) => Some(opts),
                TokenSubCommand::Revoke(opts) => Some(opts),
                TokenSubCommand::Purge(opts) => Some(opts),
                TokenSubCommand::List(opts) => Some(opts),
            },
            Self::Export(_) => None,
            Self::Import(_) => None,
            Self::Commands => None,
            Self::HelpAll => None,
        }
    }

    ///
    /// 検証対象のサブコマンドオプションを返す
    ///
    fn validate_target_mut(&mut self) -> Option<&mut dyn Validate> {
        match self {
            Self::Run(opts) => Some(opts),
            Self::User(user) => match &mut user.subcommand {
                UserSubCommand::Delete(opts) => Some(opts),
                UserSubCommand::Edit(opts) => Some(opts),
                UserSubCommand::List(opts) => Some(opts),
                _ => None,
            },
            Self::Page(page) => match &mut page.subcommand {
                PageSubCommand::Add(opts) => Some(opts),
                PageSubCommand::Delete(opts) => Some(opts),
                PageSubCommand::List(opts) => Some(opts),
                PageSubCommand::MoveTo(opts) => Some(opts),
                PageSubCommand::Undelete(opts) => Some(opts),
                PageSubCommand::Unlock(opts) => Some(opts),
            },
            Self::Lock(lock) => match &mut lock.subcommand {
                LockSubCommand::List(opts) => Some(opts),
                _ => None,
            },
            Self::Asset(asset) => match &mut asset.subcommand {
                AssetSubCommand::Add(opts) => Some(opts),
                AssetSubCommand::List(opts) => Some(opts),
                AssetSubCommand::Delete(opts) => Some(opts),
                AssetSubCommand::Purge(opts) => Some(opts),
                AssetSubCommand::Undelete(opts) => Some(opts),
                AssetSubCommand::MoveTo(opts) => Some(opts),
            },
            Self::Fts(fts) => match &mut fts.subcommand {
                FtsSubCommand::Search(opts) => Some(opts),
                _ => None,
            },
            Self::Token(token) => match &mut token.subcommand {
                TokenSubCommand::Create(opts) => Some(opts),
                TokenSubCommand::Revoke(_) => None,
                TokenSubCommand::Purge(_) => None,
                TokenSubCommand::List(_) => None,
            },
            Self::Export(opts) => Some(opts),
            Self::Import(opts) => Some(opts),
            Self::Commands => None,
            Self::HelpAll => None,
        }
    }

    ///
    /// 表示対象のサブコマンドオプションを返す
    ///
    fn show_options_target(&self) -> Option<&dyn ShowOptions> {
        match self {
            Self::Run(opts) => Some(opts),
            Self::User(user) => match &user.subcommand {
                UserSubCommand::Add(opts) => Some(opts),
                UserSubCommand::Delete(opts) => Some(opts),
                UserSubCommand::Edit(opts) => Some(opts),
                UserSubCommand::List(opts) => Some(opts),
            },
            Self::Page(page) => match &page.subcommand {
                PageSubCommand::Add(opts) => Some(opts),
                PageSubCommand::Delete(opts) => Some(opts),
                PageSubCommand::List(opts) => Some(opts),
                PageSubCommand::MoveTo(opts) => Some(opts),
                PageSubCommand::Undelete(opts) => Some(opts),
                PageSubCommand::Unlock(opts) => Some(opts),
            },
            Self::Lock(lock) => match &lock.subcommand {
                LockSubCommand::List(opts) => Some(opts),
                LockSubCommand::Delete(opts) => Some(opts),
            },
            Self::Asset(asset) => match &asset.subcommand {
                AssetSubCommand::Add(opts) => Some(opts),
                AssetSubCommand::List(opts) => Some(opts),
                AssetSubCommand::Delete(opts) => Some(opts),
                AssetSubCommand::Purge(opts) => Some(opts),
                AssetSubCommand::Undelete(opts) => Some(opts),
                AssetSubCommand::MoveTo(opts) => Some(opts),
            },
            Self::Fts(fts) => match &fts.subcommand {
                FtsSubCommand::Search(opts) => Some(opts),
                _ => None,
            },
            Self::Token(token) => match &token.subcommand {
                TokenSubCommand::Create(opts) => Some(opts),
                TokenSubCommand::Revoke(opts) => Some(opts),
                TokenSubCommand::Purge(opts) => Some(opts),
                TokenSubCommand::List(opts) => Some(opts),
            },
            Self::Export(opts) => Some(opts),
            Self::Import(opts) => Some(opts),
            Self::Commands => None,
            Self::HelpAll => None,
        }
    }

    ///
    /// サブコマンドに応じた実行コンテキストを構築する
    ///
    fn build_context(&self, opts: &Options) -> Result<Box<dyn CommandContext>> {
        match self {
            Self::Run(sub_opts) => run_command::build_context(opts, sub_opts),
            Self::User(user) => match &user.subcommand {
                UserSubCommand::Add(sub_opts) => {
                    user_add::build_context(opts, sub_opts)
                }
                UserSubCommand::Delete(sub_opts) => {
                    user_delete::build_context(opts, sub_opts)
                }
                UserSubCommand::Edit(sub_opts) => {
                    user_edit::build_context(opts, sub_opts)
                }
                UserSubCommand::List(sub_opts) => {
                    user_list::build_context(opts, sub_opts)
                }
            },
            Self::Page(page) => match &page.subcommand {
                PageSubCommand::Add(sub_opts) => {
                    page_add::build_context(opts, sub_opts)
                }
                PageSubCommand::Delete(sub_opts) => {
                    page_delete::build_context(opts, sub_opts)
                }
                PageSubCommand::List(sub_opts) => {
                    page_list::build_context(opts, sub_opts)
                }
                PageSubCommand::MoveTo(sub_opts) => {
                    page_move_to::build_context(opts, sub_opts)
                }
                PageSubCommand::Undelete(sub_opts) => {
                    page_undelete::build_context(opts, sub_opts)
                }
                PageSubCommand::Unlock(sub_opts) => {
                    page_unlock::build_context(opts, sub_opts)
                }
            },
            Self::Lock(lock) => match &lock.subcommand {
                LockSubCommand::List(sub_opts) => {
                    lock_list::build_context(opts, sub_opts)
                }
                LockSubCommand::Delete(sub_opts) => {
                    lock_delete::build_context(opts, sub_opts)
                }
            },
            Self::Asset(asset) => match &asset.subcommand {
                AssetSubCommand::Add(sub_opts) => {
                    asset_add::build_context(opts, sub_opts)
                }
                AssetSubCommand::List(sub_opts) => {
                    asset_list::build_context(opts, sub_opts)
                }
                AssetSubCommand::Delete(sub_opts) => {
                    asset_delete::build_context(opts, sub_opts)
                }
                AssetSubCommand::Purge(sub_opts) => {
                    asset_purge::build_context(opts, sub_opts)
                }
                AssetSubCommand::Undelete(sub_opts) => {
                    asset_undelete::build_context(opts, sub_opts)
                }
                AssetSubCommand::MoveTo(sub_opts) => {
                    asset_move_to::build_context(opts, sub_opts)
                }
            },
            Self::Fts(fts) => match &fts.subcommand {
                FtsSubCommand::Rebuild => fts_rebuild::build_context(opts),
                FtsSubCommand::Merge => fts_merge::build_context(opts),
                FtsSubCommand::Search(sub_opts) => {
                    fts_search::build_context(opts, sub_opts)
                }
            },
            Self::Token(token) => match &token.subcommand {
                TokenSubCommand::Create(sub_opts) => {
                    token_create::build_context(opts, sub_opts)
                }
                TokenSubCommand::Revoke(sub_opts) => {
                    token_revoke::build_context(opts, sub_opts)
                }
                TokenSubCommand::Purge(sub_opts) => {
                    token_purge::build_context(opts, sub_opts)
                }
                TokenSubCommand::List(sub_opts) => {
                    token_list::build_context(opts, sub_opts)
                }
            },
            Self::Export(sub_opts) => {
                export_command::build_context(opts, sub_opts)
            }
            Self::Import(sub_opts) => {
                import_command::build_context(opts, sub_opts)
            }
            Self::Commands => commands::build_context(opts),
            Self::HelpAll => help_all::build_context(opts),
        }
    }

    ///
    /// サブコマンド固有の設定内容を `Config` へ保存する
    ///
    fn save_config(&self, config: &mut Config) {
        match self {
            Self::Run(opts) => {
                config.set_run_bind_addr(opts.bind_addr());
                config.set_run_bind_port(opts.bind_port());
                config.set_run_use_tls(opts.use_tls());
                config.set_run_server_cert(opts.cert_path());
            }
            Self::User(user) => {
                if let UserSubCommand::List(opts) = &user.subcommand {
                    config.set_user_list_sort_mode(opts.sort_mode());
                    config.set_user_list_reverse_sort(opts.is_reverse_sort());
                }
            }
            Self::Page(page) => match &page.subcommand {
                PageSubCommand::Add(opts) => {
                    if let Some(user_name) = opts.raw_user_name() {
                        config.set_page_add_default_user(user_name);
                    }
                }
                PageSubCommand::List(opts) => {
                    config.set_page_list_sort_mode(opts.sort_mode());
                    config.set_page_list_reverse_sort(opts.is_reverse_sort());
                    config.set_page_list_long_info(opts.is_long_info());
                }
                PageSubCommand::Undelete(opts) => {
                    config.set_page_undelete_with_assets(!opts.is_without_assets());
                }
                _ => {}
            },
            Self::Lock(lock) => {
                if let LockSubCommand::List(opts) = &lock.subcommand {
                    config.set_lock_list_sort_mode(opts.sort_mode());
                    config.set_lock_list_reverse_sort(opts.is_reverse_sort());
                    config.set_lock_list_long_info(opts.is_long_info());
                }
            }
            Self::Asset(asset) => match &asset.subcommand {
                AssetSubCommand::Add(opts) => {
                    if let Some(user_name) = opts.raw_user_name() {
                        config.set_asset_add_default_user(user_name);
                    }
                }
                AssetSubCommand::List(opts) => {
                    config.set_asset_list_sort_mode(opts.sort_mode());
                    config.set_asset_list_reverse_sort(opts.is_reverse_sort());
                    config.set_asset_list_long_info(opts.is_long_info());
                }
                _ => {}
            },
            Self::Fts(fts) => {
                if let FtsSubCommand::Search(opts) = &fts.subcommand {
                    config.set_fts_search_target(opts.target());
                    config.set_fts_search_with_deleted(opts.with_deleted());
                    config.set_fts_search_all_revision(opts.all_revision());
                }
            }
            Self::Token(_) => {}
            Self::Export(_) => {}
            Self::Import(_) => {}
            Self::Commands => {}
            Self::HelpAll => {}
        }
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
    config.set_wiki_title(opts.wiki_title.clone());
    config.set_asset_limit_size(opts.asset_limit_size.clone());

    if let Some(command) = &opts.command {
        command.save_config(&mut config);
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

///
/// アセットサイズ指定文字列の解析
///
/// # 引数
/// * `raw` - 解析対象の文字列
///
/// # 戻り値
/// 解析後のバイト数を返す。
///
fn parse_asset_limit_size(raw: &str) -> Result<u64> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(anyhow!("asset limit size is empty"));
    }

    let (number_text, unit) = match value.chars().last() {
        Some(last) if last.is_ascii_alphabetic() => {
            (&value[..value.len() - last.len_utf8()], Some(last))
        }
        Some(_) => (value, None),
        None => return Err(anyhow!("asset limit size is empty")),
    };

    if number_text.is_empty() {
        return Err(anyhow!("asset limit size format is invalid"));
    }

    let number = number_text
        .parse::<u64>()
        .map_err(|_| anyhow!("asset limit size format is invalid"))?;
    let multiplier = match unit {
        None => 1_u64,
        Some('k') | Some('K') => ASSET_SIZE_KIB,
        Some('m') | Some('M') => ASSET_SIZE_MIB,
        _ => return Err(anyhow!("asset limit size unit is invalid")),
    };

    let bytes = number
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("asset limit size is too large"))?;

    if bytes == 0 {
        return Err(anyhow!("asset limit size must be greater than zero"));
    }
    if bytes > MAX_ASSET_LIMIT_SIZE {
        return Err(anyhow!(
            "asset limit size exceeds maximum ({})",
            MAX_ASSET_LIMIT_SIZE
        ));
    }

    Ok(bytes)
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

    #[test]
    fn parse_asset_limit_size_with_k_or_m() {
        assert_eq!(
            parse_asset_limit_size("10M").expect("parse failed"),
            10 * 1024 * 1024,
        );
        assert_eq!(
            parse_asset_limit_size("10m").expect("parse failed"),
            10 * 1024 * 1024,
        );
        assert_eq!(
            parse_asset_limit_size("10K").expect("parse failed"),
            10 * 1024,
        );
        assert_eq!(
            parse_asset_limit_size("10k").expect("parse failed"),
            10 * 1024,
        );
    }

    #[test]
    fn parse_asset_limit_size_rejects_invalid_unit() {
        assert!(parse_asset_limit_size("10Mi").is_err());
        assert!(parse_asset_limit_size("10G").is_err());
    }

    #[test]
    fn parse_asset_limit_size_rejects_over_limit() {
        assert_eq!(
            parse_asset_limit_size("100M").expect("parse failed"),
            100 * 1024 * 1024,
        );
        assert!(parse_asset_limit_size("101M").is_err());
    }

    #[test]
    fn parse_export_command_options() {
        let mut opts = Options::try_parse_from([
            "luwiki",
            "export",
            "--subtree",
            "/docs",
            "--dry-run",
            "--password",
            "secret",
            "--yes",
            "--strict-mode",
            "out.zip",
        ])
        .expect("parse failed");

        opts.validate().expect("validate failed");
        let export_opts = match opts.command {
            Some(Command::Export(export_opts)) => export_opts,
            _ => panic!("export options missing"),
        };

        assert_eq!(export_opts.subtree(), Some("/docs".to_string()));
        assert!(export_opts.is_dry_run());
        assert_eq!(export_opts.password(), Some("secret".to_string()));
        assert!(export_opts.is_yes());
        assert!(export_opts.is_strict_mode());
        assert_eq!(export_opts.output(), "out.zip".to_string());
    }

    #[test]
    fn export_validate_rejects_root_subtree() {
        let mut opts = Options::try_parse_from([
            "luwiki",
            "export",
            "--subtree",
            "/",
            "out.zip",
        ])
        .expect("parse failed");

        let err = opts.validate().expect_err("root subtree must be rejected");
        assert!(err.to_string().contains("--subtree /"));
    }

    #[test]
    fn parse_import_command_options() {
        let mut opts = Options::try_parse_from([
            "luwiki",
            "import",
            "--migrate",
            "/dst",
            "--user-map",
            "alice=bob",
            "--user-list",
            "--dry-run",
            "--fix-broken-link",
            "--yes",
            "--password",
            "secret",
            "--strict-mode",
            "in.zip",
        ])
        .expect("parse failed");

        opts.validate().expect("validate failed");
        let import_opts = match opts.command {
            Some(Command::Import(import_opts)) => import_opts,
            _ => panic!("import options missing"),
        };

        assert_eq!(import_opts.migrate(), Some("/dst".to_string()));
        assert_eq!(import_opts.user_map(), vec!["alice=bob".to_string()]);
        assert!(import_opts.is_user_list());
        assert!(import_opts.is_dry_run());
        assert!(import_opts.is_fix_broken_link());
        assert!(import_opts.is_yes());
        assert_eq!(import_opts.password(), Some("secret".to_string()));
        assert!(import_opts.is_strict_mode());
        assert_eq!(import_opts.input(), "in.zip".to_string());
    }

    #[test]
    fn import_validate_rejects_fix_broken_link_without_migrate() {
        let mut opts = Options::try_parse_from([
            "luwiki",
            "import",
            "--fix-broken-link",
            "in.zip",
        ])
        .expect("parse failed");

        let err = opts.validate().expect_err(
            "fix-broken-link without migrate must be rejected",
        );
        assert!(err.to_string().contains("--fix-broken-link requires --migrate"));
    }

    #[test]
    fn import_validate_rejects_relative_migrate_prefix() {
        let mut opts = Options::try_parse_from([
            "luwiki",
            "import",
            "--migrate",
            "dst",
            "in.zip",
        ])
        .expect("parse failed");

        let err = opts
            .validate()
            .expect_err("relative migrate prefix must be rejected");
        assert!(err.to_string().contains("invalid page path"));
    }
}
