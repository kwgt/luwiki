/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"derived rebuild"の実装
//!

use anyhow::Result;

use super::CommandContext;
use crate::cmd_args::{DerivedRebuildOpts, DerivedRebuildTarget, Options};
use crate::database::DatabaseManager;

struct DerivedRebuildCommandContext {
    manager: DatabaseManager,
    target: DerivedRebuildTarget,
    template_root: Option<String>,
}

impl DerivedRebuildCommandContext {
    fn new(opts: &Options, sub_opts: &DerivedRebuildOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            target: sub_opts.target(),
            template_root: opts.template_root(),
        })
    }
}

impl CommandContext for DerivedRebuildCommandContext {
    fn exec(&self) -> Result<()> {
        match self.target {
            DerivedRebuildTarget::All => {
                let counts = self
                    .manager
                    .rebuild_all_derived_data(
                        self.template_root.as_deref(),
                    )?;
                println!(
                    "rebuilt template candidates: {}",
                    counts.templates(),
                );
                println!(
                    "rebuilt prompt candidates: {}",
                    counts.prompts(),
                );
                println!(
                    "rebuilt resource candidates: {}",
                    counts.resources(),
                );
            }
            DerivedRebuildTarget::Templates => {
                let count = self
                    .manager
                    .rebuild_template_candidates_with_legacy(
                        self.template_root.as_deref(),
                    )?;
                println!("rebuilt template candidates: {}", count);
            }
            DerivedRebuildTarget::Prompts => {
                let count =
                    self.manager.rebuild_prompt_candidates()?;
                println!("rebuilt prompt candidates: {}", count);
            }
            DerivedRebuildTarget::Resources => {
                let count =
                    self.manager.rebuild_resource_candidates()?;
                println!("rebuilt resource candidates: {}", count);
            }
        }

        Ok(())
    }
}

pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &DerivedRebuildOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(DerivedRebuildCommandContext::new(opts, sub_opts)?))
}
