/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"derived"のコマンドライン定義
//!

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;

#[derive(Clone, Args, Debug)]
pub(crate) struct DerivedCommand {
    #[command(subcommand)]
    pub(crate) subcommand: DerivedSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum DerivedSubCommand {
    /// front matter 派生データの再構成
    #[command(name = "rebuild", alias = "r")]
    Rebuild(DerivedRebuildOpts),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum DerivedRebuildTarget {
    /// 全front matter由来派生データ
    All,

    /// テンプレート候補派生データ
    Templates,

    /// prompt候補派生データ
    Prompts,

    /// resource候補派生データ
    Resources,
}

#[derive(Clone, Args, Debug)]
pub(crate) struct DerivedRebuildOpts {
    /// 再構成対象
    #[arg(long = "target", value_name = "TARGET")]
    target: DerivedRebuildTarget,
}

impl DerivedRebuildOpts {
    pub(crate) fn target(&self) -> DerivedRebuildTarget {
        self.target
    }
}

impl ShowOptions for DerivedRebuildOpts {
    fn show_options(&self) {
        println!("derived rebuild command options");
        println!("   target: {:?}", self.target());
    }
}

impl Validate for DerivedRebuildOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

impl ApplyConfig for DerivedRebuildOpts {
    fn apply_config(&mut self, _config: &Config) {}
}
