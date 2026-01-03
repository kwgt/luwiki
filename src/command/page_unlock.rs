/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page unlock"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{Options, PageUnlockOpts};
use crate::database::types::PageId;
use crate::database::{DatabaseManager, DbError};
use crate::rest_api::validate_page_path;
use super::CommandContext;

///
/// "page unlock"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageUnlockCommandContext {
    manager: DatabaseManager,
    target: String,
}

impl PageUnlockCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageUnlockOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            target: sub_opts.target(),
        })
    }
}

// CommandContextの実装
impl CommandContext for PageUnlockCommandContext {
    fn exec(&self) -> Result<()> {
        let (_page_id, index) = if let Ok(page_id) = PageId::from_string(&self.target) {
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

        if index.deleted() {
            return Err(anyhow!("page deleted"));
        }

        if !self.manager.delete_page_lock_by_id(&_page_id)? {
            return Err(anyhow!(DbError::LockNotFound));
        }

        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageUnlockOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageUnlockCommandContext::new(opts, sub_opts)?))
}
