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

use super::CommandContext;
use crate::cmd_args::{AssetAddOpts, Options};
use crate::database::types::PageId;
use crate::database::{DatabaseManager, DbError};
use crate::rest_api::{validate_asset_file_name, validate_page_path};

///
/// "asset add"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetAddCommandContext {
    manager: DatabaseManager,
    user_name: String,
    mime_type: Option<String>,
    file_path: std::path::PathBuf,
    target: String,
    asset_limit_size: u64,
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
            asset_limit_size: opts.asset_limit_size()?,
        })
    }
}

impl CommandContext for AssetAddCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// アセット追加に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        /*
         * 入力ファイルの検証
         */
        let metadata = fs::metadata(&self.file_path)?;
        if metadata.len() > self.asset_limit_size {
            return Err(anyhow!("asset size exceeds limit"));
        }

        let file_name = self
            .file_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("file name is invalid"))?;
        if let Err(message) = validate_asset_file_name(file_name) {
            return Err(anyhow!("invalid file name: {}", message));
        }

        /*
         * 追加先ページの解決
         */
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

        /*
         * MIMEタイプとデータの準備
         */
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

        /*
         * 実行結果の出力
         */
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
