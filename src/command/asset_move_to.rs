/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset move_to"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{AssetMoveToOpts, Options};
use crate::database::types::{AssetId, PageId};
use crate::database::{AssetMoveResult, DatabaseManager, DbError};
use crate::rest_api::validate_page_path;
use super::CommandContext;

///
/// "asset move_to"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetMoveToCommandContext {
    manager: DatabaseManager,
    asset_id: AssetId,
    dst_target: String,
    force: bool,
}

impl AssetMoveToCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &AssetMoveToOpts) -> Result<Self> {
        let asset_id = AssetId::from_string(&sub_opts.asset_id())?;
        Ok(Self {
            manager: opts.open_database()?,
            asset_id,
            dst_target: sub_opts.dst_target(),
            force: sub_opts.is_force(),
        })
    }

    fn resolve_dst_page_id(&self) -> Result<PageId> {
        if let Ok(page_id) = PageId::from_string(&self.dst_target) {
            return Ok(page_id);
        }

        if let Err(message) = validate_page_path(&self.dst_target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        self.manager
            .get_page_id_by_path(&self.dst_target)?
            .ok_or_else(|| anyhow!("destination page not found"))
    }
}

// CommandContextの実装
impl CommandContext for AssetMoveToCommandContext {
    fn exec(&self) -> Result<()> {
        let dst_page_id = self.resolve_dst_page_id()?;
        let dst_index = self.manager
            .get_page_index_by_id(&dst_page_id)?
            .ok_or_else(|| anyhow!("destination page not found"))?;
        let asset_info = self.manager
            .get_asset_info_by_id(&self.asset_id)?
            .ok_or_else(|| anyhow!(DbError::AssetNotFound))?;
        let file_name = asset_info.file_name();

        let conflict_asset = self.manager
            .get_asset_id_by_page_file(&dst_page_id, &file_name)?;
        let has_conflict = conflict_asset
            .as_ref()
            .map(|id| id != &self.asset_id)
            .unwrap_or(false);

        if !self.force {
            if dst_index.deleted() && has_conflict {
                return Err(anyhow!(
                    "destination page not found and asset already exists"
                ));
            }
            if dst_index.deleted() {
                return Err(anyhow!("destination page not found"));
            }
            if has_conflict {
                return Err(anyhow!("destination asset already exists"));
            }
        }

        match self.manager.move_asset(&self.asset_id, &dst_page_id, self.force)? {
            AssetMoveResult::Moved => Ok(()),
            AssetMoveResult::PageNotFound => {
                Err(anyhow!("destination page not found"))
            }
            AssetMoveResult::PageDeleted => {
                Err(anyhow!("destination page not found"))
            }
            AssetMoveResult::NameConflict => {
                Err(anyhow!("destination asset already exists"))
            }
        }
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &AssetMoveToOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(AssetMoveToCommandContext::new(opts, sub_opts)?))
}
