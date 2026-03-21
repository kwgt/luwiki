/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"export"のコマンドライン定義
//!

use anyhow::{anyhow, Result};
use clap::Args;

use super::{ShowOptions, Validate};
use crate::rest_api::validate_page_path;

///
/// サブコマンドexportのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct ExportOpts {
    /// migrate export 対象のサブツリー
    #[arg(short = 's', long = "subtree", value_name = "PREFIX")]
    subtree: Option<String>,

    /// リハーサルモード
    #[arg(short = 'd', long = "dry-run")]
    dry_run: bool,

    /// ZIP パスワード
    #[arg(short = 'p', long = "password", value_name = "PASSWORD")]
    password: Option<String>,

    /// 確認プロンプトを省略
    #[arg(short = 'y', long = "yes")]
    yes: bool,

    /// strict-mode
    #[arg(short = 'S', long = "strict-mode")]
    strict_mode: bool,

    /// 出力先 ZIP パス、"-" は標準出力
    #[arg()]
    output: String,
}

impl ExportOpts {
    ///
    /// サブツリー指定へのアクセサ
    ///
    pub(crate) fn subtree(&self) -> Option<String> {
        self.subtree.clone()
    }

    ///
    /// dry-run 指定へのアクセサ
    ///
    pub(crate) fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    ///
    /// パスワード指定へのアクセサ
    ///
    pub(crate) fn password(&self) -> Option<String> {
        self.password.clone()
    }

    ///
    /// 確認省略指定へのアクセサ
    ///
    pub(crate) fn is_yes(&self) -> bool {
        self.yes
    }

    ///
    /// strict-mode 指定へのアクセサ
    ///
    pub(crate) fn is_strict_mode(&self) -> bool {
        self.strict_mode
    }

    ///
    /// 出力先へのアクセサ
    ///
    pub(crate) fn output(&self) -> String {
        self.output.clone()
    }
}

impl ShowOptions for ExportOpts {
    fn show_options(&self) {
        println!("export command options");
        println!(
            "   subtree:     {}",
            self.subtree.as_deref().unwrap_or("(none)")
        );
        println!("   dry_run:     {}", self.is_dry_run());
        println!("   password:    {}", self.password.is_some());
        println!("   yes:         {}", self.is_yes());
        println!("   strict_mode: {}", self.is_strict_mode());
        println!("   output:      {}", self.output());
    }
}

// Validateトレイトの実装
impl Validate for ExportOpts {
    fn validate(&mut self) -> Result<()> {
        if self.output.trim().is_empty() {
            return Err(anyhow!("output path is empty"));
        }

        if let Some(subtree) = self.subtree.as_deref() {
            if let Err(message) = validate_page_path(subtree) {
                return Err(anyhow!("invalid page path: {}", message));
            }

            if subtree == "/" {
                return Err(anyhow!("--subtree / is not allowed"));
            }
        }

        Ok(())
    }
}
