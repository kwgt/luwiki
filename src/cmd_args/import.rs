/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"import"のコマンドライン定義
//!

use anyhow::{anyhow, Result};
use clap::Args;

use super::{ShowOptions, Validate};
use crate::rest_api::validate_page_path;

///
/// サブコマンドimportのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct ImportOpts {
    /// migrate import 先のプレフィクス
    #[arg(short = 'm', long = "migrate", value_name = "PREFIX")]
    migrate: Option<String>,

    /// ユーザマッピング
    #[arg(short = 'u', long = "user-map", value_name = "MAPPING")]
    user_map: Vec<String>,

    /// 編集者一覧のみを表示
    #[arg(short = 'l', long = "user-list")]
    user_list: bool,

    /// リハーサルモード
    #[arg(short = 'd', long = "dry-run")]
    dry_run: bool,

    /// 破損リンクを about:invalid へ置換
    #[arg(short = 'f', long = "fix-broken-link")]
    fix_broken_link: bool,

    /// 確認プロンプトを省略
    #[arg(short = 'y', long = "yes")]
    yes: bool,

    /// ZIP パスワード
    #[arg(short = 'p', long = "password", value_name = "PASSWORD")]
    password: Option<String>,

    /// strict-mode
    #[arg(short = 'S', long = "strict-mode")]
    strict_mode: bool,

    /// 入力 ZIP パス、"-" は標準入力
    #[arg()]
    input: String,
}

impl ImportOpts {
    ///
    /// migrate import 先へのアクセサ
    ///
    pub(crate) fn migrate(&self) -> Option<String> {
        self.migrate.clone()
    }

    ///
    /// ユーザマッピングへのアクセサ
    ///
    pub(crate) fn user_map(&self) -> Vec<String> {
        self.user_map.clone()
    }

    ///
    /// ユーザ一覧表示指定へのアクセサ
    ///
    pub(crate) fn is_user_list(&self) -> bool {
        self.user_list
    }

    ///
    /// dry-run 指定へのアクセサ
    ///
    pub(crate) fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    ///
    /// 破損リンク修正指定へのアクセサ
    ///
    pub(crate) fn is_fix_broken_link(&self) -> bool {
        self.fix_broken_link
    }

    ///
    /// 確認省略指定へのアクセサ
    ///
    pub(crate) fn is_yes(&self) -> bool {
        self.yes
    }

    ///
    /// パスワード指定へのアクセサ
    ///
    pub(crate) fn password(&self) -> Option<String> {
        self.password.clone()
    }

    ///
    /// strict-mode 指定へのアクセサ
    ///
    pub(crate) fn is_strict_mode(&self) -> bool {
        self.strict_mode
    }

    ///
    /// 入力元へのアクセサ
    ///
    pub(crate) fn input(&self) -> String {
        self.input.clone()
    }
}

impl ShowOptions for ImportOpts {
    fn show_options(&self) {
        println!("import command options");
        println!(
            "   migrate:         {}",
            self.migrate.as_deref().unwrap_or("(none)")
        );
        println!("   user_map:        {:?}", self.user_map());
        println!("   user_list:       {}", self.is_user_list());
        println!("   dry_run:         {}", self.is_dry_run());
        println!(
            "   fix_broken_link: {}",
            self.is_fix_broken_link()
        );
        println!("   yes:             {}", self.is_yes());
        println!("   password:        {}", self.password.is_some());
        println!("   strict_mode:     {}", self.is_strict_mode());
        println!("   input:           {}", self.input());
    }
}

// Validateトレイトの実装
impl Validate for ImportOpts {
    fn validate(&mut self) -> Result<()> {
        if self.input.trim().is_empty() {
            return Err(anyhow!("input path is empty"));
        }

        if let Some(prefix) = self.migrate.as_deref() {
            if let Err(message) = validate_page_path(prefix) {
                return Err(anyhow!("invalid page path: {}", message));
            }
        }

        if self.fix_broken_link && self.migrate.is_none() {
            return Err(anyhow!(
                "--fix-broken-link requires --migrate"
            ));
        }

        for mapping in &self.user_map {
            if mapping.trim().is_empty() {
                return Err(anyhow!("user mapping is empty"));
            }

            let Some((src, dst)) = mapping.split_once('=') else {
                return Err(anyhow!(
                    "invalid user mapping: {}",
                    mapping
                ));
            };

            if src.trim().is_empty() || dst.trim().is_empty() {
                return Err(anyhow!(
                    "invalid user mapping: {}",
                    mapping
                ));
            }
        }

        Ok(())
    }
}
