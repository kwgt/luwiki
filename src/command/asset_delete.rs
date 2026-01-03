/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset delete"の実装
//!

use anyhow::{anyhow, Result};

use crate::cmd_args::{AssetDeleteOpts, Options};
use crate::database::types::{AssetId, PageId};
use crate::database::{DatabaseManager, DbError};
use crate::rest_api::{validate_asset_file_name, validate_page_path};
use super::CommandContext;

///
/// "asset delete"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetDeleteCommandContext {
    manager: DatabaseManager,
    target: String,
    hard_delete: bool,
    purge: bool,
}

impl AssetDeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &AssetDeleteOpts) -> Result<Self> {
        if sub_opts.is_purge() && sub_opts.is_hard_delete() {
            return Err(anyhow!(
                "purge option cannot be used with hard-delete",
            ));
        }
        Ok(Self {
            manager: opts.open_database()?,
            target: sub_opts.target(),
            hard_delete: sub_opts.is_hard_delete(),
            purge: sub_opts.is_purge(),
        })
    }

    fn resolve_page_id(&self, target: &str) -> Result<PageId> {
        if let Ok(page_id) = PageId::from_string(target) {
            if self.manager.get_page_index_by_id(&page_id)?.is_some() {
                return Ok(page_id);
            }
            return Err(anyhow!(DbError::PageNotFound));
        }

        if let Err(message) = validate_page_path(target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        self.manager
            .get_page_id_by_path(target)?
            .ok_or_else(|| anyhow!(DbError::PageNotFound))
    }

    fn resolve_target(&self) -> Result<AssetDeleteTarget> {
        if let Ok(asset_id) = AssetId::from_string(&self.target) {
            if self.manager.get_asset_info_by_id(&asset_id)?.is_some() {
                return Ok(AssetDeleteTarget::Asset(asset_id));
            }
        }

        if let Ok(page_id) = PageId::from_string(&self.target) {
            if self.manager.get_page_index_by_id(&page_id)?.is_some() {
                return Ok(AssetDeleteTarget::Page(page_id));
            }
        }

        if let Ok((page_path, file_name)) = parse_asset_path(&self.target) {
            if let Err(message) = validate_page_path(&page_path) {
                return Err(anyhow!("invalid page path: {}", message));
            }
            if let Err(message) = validate_asset_file_name(&file_name) {
                return Err(anyhow!("invalid file name: {}", message));
            }

            if let Some(page_id) = self.manager.get_page_id_by_path(&page_path)? {
                if let Some(asset_id) =
                    self.manager.get_asset_id_by_page_file(&page_id, &file_name)?
                {
                    return Ok(AssetDeleteTarget::Asset(asset_id));
                }
            }
        }

        let page_id = self.resolve_page_id(&self.target)?;
        Ok(AssetDeleteTarget::Page(page_id))
    }

    fn delete_page_assets(&self, page_id: &crate::database::types::PageId) -> Result<()> {
        let assets = self.manager.list_page_assets(page_id)?;
        if assets.is_empty() {
            return Err(anyhow!(DbError::AssetNotFound));
        }

        if !self.hard_delete && assets.iter().any(|asset| asset.deleted()) {
            return Err(anyhow!(DbError::AssetDeleted));
        }

        for asset in assets {
            if self.hard_delete {
                self.manager.delete_asset_hard(&asset.id())?;
            } else {
                self.manager.delete_asset(&asset.id())?;
            }
        }

        Ok(())
    }

    fn purge_page_assets(&self, page_id: &crate::database::types::PageId) -> Result<()> {
        let assets = self.manager.list_page_assets(page_id)?;
        let mut deleted_assets = Vec::new();
        for asset in assets {
            if asset.deleted() {
                deleted_assets.push(asset.id());
            }
        }

        if deleted_assets.is_empty() {
            return Ok(());
        }

        for asset_id in deleted_assets {
            self.manager.delete_asset_hard(&asset_id)?;
        }

        Ok(())
    }
}

// CommandContextの実装
impl CommandContext for AssetDeleteCommandContext {
    fn exec(&self) -> Result<()> {
        if self.purge {
            let page_id = self.resolve_page_id(&self.target)
                .map_err(|_| anyhow!("purge option requires page id or page path"))?;
            return self.purge_page_assets(&page_id);
        }

        match self.resolve_target()? {
            AssetDeleteTarget::Asset(asset_id) => {
                if self.hard_delete {
                    self.manager.delete_asset_hard(&asset_id)?;
                } else {
                    self.manager.delete_asset(&asset_id)?;
                }
            }
            AssetDeleteTarget::Page(page_id) => {
                self.delete_page_assets(&page_id)?;
            }
        }

        Ok(())
    }
}

enum AssetDeleteTarget {
    Asset(AssetId),
    Page(crate::database::types::PageId),
}

///
/// アセットパスを分割する
///
/// # 引数
/// * `path` - アセットパス
///
/// # 戻り値
/// ページパスとファイル名を返す。
///
fn parse_asset_path(path: &str) -> Result<(String, String)> {
    let trimmed = path.trim_end_matches('/');
    let pos = trimmed.rfind('/')
        .ok_or_else(|| anyhow!("asset path must contain page path"))?;
    if pos == 0 {
        let file_name = &trimmed[1..];
        if file_name.is_empty() {
            return Err(anyhow!("file name is empty"));
        }
        return Ok(("/".to_string(), file_name.to_string()));
    }

    let (page_path, file_name) = trimmed.split_at(pos);
    let file_name = &file_name[1..];
    if page_path.is_empty() || file_name.is_empty() {
        return Err(anyhow!("asset path is invalid"));
    }

    Ok((page_path.to_string(), file_name.to_string()))
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &AssetDeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(AssetDeleteCommandContext::new(opts, sub_opts)?))
}
