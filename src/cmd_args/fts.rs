/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"fts"のコマンドライン定義
//!

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;
pub(crate) use crate::fts::FtsSearchTarget;

#[derive(Clone, Args, Debug)]
pub(crate) struct FtsCommand {
    #[command(subcommand)]
    pub(crate) subcommand: FtsSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum FtsSubCommand {
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
