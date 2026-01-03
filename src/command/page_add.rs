/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page add"の実装
//!

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use pulldown_cmark::Parser;

use crate::cmd_args::{Options, PageAddOpts};
use crate::database::DatabaseManager;
use super::CommandContext;

///
/// "page add"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageAddCommandContext {
    manager: DatabaseManager,
    user_name: String,
    file_path: PathBuf,
    page_path: String,
}

impl PageAddCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageAddOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            user_name: sub_opts.user_name(),
            file_path: sub_opts.file_path(),
            page_path: sub_opts.page_path(),
        })
    }
}

// CommandContextの実装
impl CommandContext for PageAddCommandContext {
    fn exec(&self) -> Result<()> {
        let source = fs::read_to_string(&self.file_path)?;
        let parser = Parser::new(&source);
        for _ in parser {
        }

        let page_id = self.manager.create_page(
            &self.page_path,
            &self.user_name,
            source,
        )?;
        println!("{}", page_id.to_string());
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageAddOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageAddCommandContext::new(opts, sub_opts)?))
}
