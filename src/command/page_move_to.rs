/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page move_to"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{Options, PageMoveToOpts};
use crate::database::types::PageId;
use crate::database::{DatabaseManager, DbError};
use super::CommandContext;

///
/// "page move_to"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageMoveToCommandContext {
    manager: DatabaseManager,
    src_path: String,
    dst_path: String,
    force: bool,
}

impl PageMoveToCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageMoveToOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            src_path: sub_opts.src_path(),
            dst_path: sub_opts.dst_path(),
            force: sub_opts.is_force(),
        })
    }
}

// CommandContextの実装
impl CommandContext for PageMoveToCommandContext {
    fn exec(&self) -> Result<()> {
        let (src_path, index) = if let Ok(page_id) = PageId::from_string(&self.src_path) {
            self.manager
                .get_page_index_entry_by_id(&page_id)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?
        } else {
            let page_id = self.manager
                .get_page_id_by_path(&self.src_path)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?;
            self.manager
                .get_page_index_entry_by_id(&page_id)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?
        };

        if !self.force {
            let page_id = index.id();
            let lock_info = self.manager.get_page_lock_info(&page_id)?;
            if lock_info.is_some() {
                return Err(anyhow!(DbError::PageLocked));
            }
        }

        self.manager.rename_page(&src_path, &self.dst_path)?;
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageMoveToOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageMoveToCommandContext::new(opts, sub_opts)?))
}
