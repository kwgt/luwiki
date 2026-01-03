/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page delete"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{Options, PageDeleteOpts};
use crate::database::types::PageId;
use crate::database::{DatabaseManager, DbError};
use crate::rest_api::validate_page_path;
use super::CommandContext;

///
/// "page delete"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageDeleteCommandContext {
    manager: DatabaseManager,
    target: String,
    hard_delete: bool,
    force: bool,
}

impl PageDeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageDeleteOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            target: sub_opts.target(),
            hard_delete: sub_opts.is_hard_delete(),
            force: sub_opts.is_force(),
        })
    }
}

// CommandContextの実装
impl CommandContext for PageDeleteCommandContext {
    fn exec(&self) -> Result<()> {
        let (page_id, index) = if let Ok(page_id) = PageId::from_string(&self.target) {
            self.manager
                .get_page_index_entry_by_id(&page_id)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))
                .map(|(_, index)| (page_id, index))?
        } else {
            if let Err(message) = validate_page_path(&self.target) {
                return Err(anyhow!("invalid page path: {}", message));
            }
            let page_id = self.manager
                .get_page_id_by_path(&self.target)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?;
            let (_, index) = self.manager
                .get_page_index_entry_by_id(&page_id)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?;
            (page_id, index)
        };

        if index.deleted() && !self.hard_delete {
            return Err(anyhow!("page already deleted"));
        }

        if !self.force {
            let lock_info = self.manager.get_page_lock_info(&page_id)?;
            if lock_info.is_some() {
                return Err(anyhow!(DbError::PageLocked));
            }
        }

        if self.hard_delete {
            self.manager.delete_page_by_id_hard(&page_id)?;
        } else {
            self.manager.delete_page_by_id(&page_id)?;
        }

        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageDeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageDeleteCommandContext::new(opts, sub_opts)?))
}
