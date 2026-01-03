/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset add"の実装
//!

use std::fs;

use anyhow::{anyhow, Result};
use mime_guess::MimeGuess;

use crate::cmd_args::{AssetAddOpts, Options};
use crate::database::types::PageId;
use crate::database::{DatabaseManager, DbError};
use crate::rest_api::{validate_asset_file_name, validate_page_path};
use super::CommandContext;

/// アセットの最大サイズ(10MiB)
const MAX_ASSET_SIZE: u64 = 10 * 1024 * 1024;

///
/// "asset add"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetAddCommandContext {
    manager: DatabaseManager,
    user_name: String,
    mime_type: Option<String>,
    file_path: std::path::PathBuf,
    target: String,
}

impl AssetAddCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &AssetAddOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            user_name: sub_opts.user_name(),
            mime_type: sub_opts.mime_type(),
            file_path: sub_opts.file_path(),
            target: sub_opts.target(),
        })
    }
}

// CommandContextの実装
impl CommandContext for AssetAddCommandContext {
    fn exec(&self) -> Result<()> {
        let metadata = fs::metadata(&self.file_path)?;
        if metadata.len() > MAX_ASSET_SIZE {
            return Err(anyhow!("asset size exceeds limit"));
        }

        let file_name = self.file_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("file name is invalid"))?;
        if let Err(message) = validate_asset_file_name(file_name) {
            return Err(anyhow!("invalid file name: {}", message));
        }

        let page_id = if let Ok(page_id) = PageId::from_string(&self.target) {
            page_id
        } else {
            if let Err(message) = validate_page_path(&self.target) {
                return Err(anyhow!("invalid page path: {}", message));
            }
            self.manager
                .get_page_id_by_path(&self.target)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?
        };

        let mime = if let Some(value) = &self.mime_type {
            value.clone()
        } else {
            MimeGuess::from_path(&self.file_path)
                .first_or_octet_stream()
                .essence_str()
                .to_string()
        };

        let data = fs::read(&self.file_path)?;
        let asset_id = self.manager.create_asset(
            &page_id,
            file_name,
            &mime,
            &self.user_name,
            &data,
        )?;

        println!("{}", asset_id.to_string());
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &AssetAddOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(AssetAddCommandContext::new(opts, sub_opts)?))
}
