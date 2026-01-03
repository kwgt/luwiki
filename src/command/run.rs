/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンドrunの実装
//!

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::cmd_args::{FrontendConfig, RunOpts, Options};
use crate::database::DatabaseManager;
use crate::http_server;
use super::CommandContext;

///
/// addサブコマンドのコンテキスト情報をパックした構造体
///
struct RunCommandContext {
    /// バインド先のアドレス
    bind_addr: String,

    /// バインド先のポート番号
    bind_port: u16,

    /// データベースファイルへのパス
    db_path: PathBuf,

    /// アセットデータ格納ディレクトリへのパス
    asset_path: PathBuf,

    /// frontend設定
    frontend_config: FrontendConfig,

    /// 起動時にブラウザを開くか否かのフラグ
    #[allow(dead_code)]
    open_browser: bool,
}

impl RunCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &RunOpts) -> Result<Self> {
        Ok(Self {
            db_path: opts.db_path(),
            asset_path: opts.assets_path(),
            bind_addr: sub_opts.bind_addr(),
            bind_port: sub_opts.bind_port(),
            frontend_config: opts.frontend_config()?,
            open_browser: sub_opts.is_browser_open(),
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for RunCommandContext {
    fn exec(&self) -> Result<()> {
        let manager = DatabaseManager::open(&self.db_path, &self.asset_path)?;

        if !manager.is_users_registered()? {
            return Err(anyhow!("no users registered"));
        }

        http_server::run(
            self.bind_addr.clone(),
            self.bind_port,
            manager,
            self.frontend_config.clone(),
        )
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(opts: &Options, sub_opts: &RunOpts)
    -> Result<Box<dyn CommandContext>>
{
    Ok(Box::new(RunCommandContext::new(opts, sub_opts)?))
}
