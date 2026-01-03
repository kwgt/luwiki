/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"lock delete"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{LockDeleteOpts, Options};
use crate::database::types::LockToken;
use crate::database::DatabaseManager;
use super::CommandContext;

///
/// "lock delete"サブコマンドのコンテキスト情報をパックした構造体
///
struct LockDeleteCommandContext {
    manager: DatabaseManager,
    lock_id: LockToken,
}

impl LockDeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &LockDeleteOpts) -> Result<Self> {
        let lock_id = LockToken::from_string(&sub_opts.lock_id())?;
        Ok(Self {
            manager: opts.open_database()?,
            lock_id,
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for LockDeleteCommandContext {
    fn exec(&self) -> Result<()> {
        if self.manager.delete_lock(&self.lock_id)? {
            Ok(())
        } else {
            Err(anyhow!("lock not found"))
        }
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &LockDeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(LockDeleteCommandContext::new(opts, sub_opts)?))
}
