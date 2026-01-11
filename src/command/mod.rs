/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンドの処理を提供するモジュール
//!

pub(crate) mod common;
pub(crate) mod asset_add;
pub(crate) mod asset_delete;
pub(crate) mod asset_list;
pub(crate) mod asset_move_to;
pub(crate) mod asset_undelete;
pub(crate) mod commands;
pub(crate) mod fts_merge;
pub(crate) mod fts_rebuild;
pub(crate) mod fts_search;
pub(crate) mod help_all;
pub(crate) mod lock_delete;
pub(crate) mod lock_list;
pub(crate) mod page_add;
pub(crate) mod page_delete;
pub(crate) mod page_list;
pub(crate) mod page_move_to;
pub(crate) mod page_undelete;
pub(crate) mod page_unlock;
pub(crate) mod run;
pub(crate) mod user_add;
pub(crate) mod user_delete;
pub(crate) mod user_edit;
pub(crate) mod user_list;

use anyhow::Result;

///
/// コマンドコンテキスト集約するトレイト
///
pub(crate) trait CommandContext {
    ///
    /// サブコマンドの実行
    ///
    fn exec(&self) -> Result<()>;
}
