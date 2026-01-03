/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset undelete"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{AssetUndeleteOpts, Options};
use crate::database::types::AssetId;
use crate::database::{DatabaseManager, DbError};
use super::CommandContext;

///
/// "asset undelete"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetUndeleteCommandContext {
    manager: DatabaseManager,
    asset_id: AssetId,
}

impl AssetUndeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &AssetUndeleteOpts) -> Result<Self> {
        let asset_id = AssetId::from_string(&sub_opts.target())?;
        Ok(Self {
            manager: opts.open_database()?,
            asset_id,
        })
    }
}

// CommandContextの実装
impl CommandContext for AssetUndeleteCommandContext {
    fn exec(&self) -> Result<()> {
        let asset_info = self.manager
            .get_asset_info_by_id(&self.asset_id)?
            .ok_or_else(|| anyhow!(DbError::AssetNotFound))?;
        if !asset_info.deleted() {
            return Err(anyhow!("asset not deleted"));
        }

        self.manager.undelete_asset(&self.asset_id)?;
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &AssetUndeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(AssetUndeleteCommandContext::new(opts, sub_opts)?))
}
