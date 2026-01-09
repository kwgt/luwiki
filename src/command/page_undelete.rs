/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page undelete"の実装
//!

use anyhow::Result;

use crate::cmd_args::{Options, PageUndeleteOpts};
use crate::database::types::PageId;
use crate::database::DatabaseManager;
use super::CommandContext;

///
/// "page undelete"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageUndeleteCommandContext {
    manager: DatabaseManager,
    page_id: PageId,
    restore_to: String,
    recursive: bool,
    with_assets: bool,
}

impl PageUndeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageUndeleteOpts) -> Result<Self> {
        let page_id = PageId::from_string(&sub_opts.target())?;
        Ok(Self {
            manager: opts.open_database()?,
            page_id,
            restore_to: sub_opts.restore_to(),
            recursive: sub_opts.is_recursive(),
            with_assets: !sub_opts.is_without_assets(),
        })
    }
}

// CommandContextの実装
impl CommandContext for PageUndeleteCommandContext {
    fn exec(&self) -> Result<()> {
        if self.recursive {
            self.manager.undelete_pages_recursive_by_id(
                &self.page_id,
                &self.restore_to,
                self.with_assets,
            )
        } else {
            self.manager.undelete_page_by_id(
                &self.page_id,
                &self.restore_to,
                self.with_assets,
            )
        }
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageUndeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageUndeleteCommandContext::new(opts, sub_opts)?))
}
