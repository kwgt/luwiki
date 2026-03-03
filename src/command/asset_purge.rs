/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset purge"の実装
//!

use anyhow::{anyhow, Result};

use super::CommandContext;
use crate::cmd_args::{AssetPurgeOpts, Options};
use crate::database::types::PageId;
use crate::database::{DatabaseManager, DbError};
use crate::rest_api::validate_page_path;

///
/// "asset purge"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetPurgeCommandContext {
    manager: DatabaseManager,
    target: Option<String>,
}

impl AssetPurgeCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &AssetPurgeOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            target: sub_opts.target(),
        })
    }

    ///
    /// 指定文字列から対象ページIDを解決
    ///
    /// # 引数
    /// * `target` - ページIDまたはページパス
    ///
    /// # 戻り値
    /// 解決できたページIDを返す。
    ///
    fn resolve_page_id(&self, target: &str) -> Result<PageId> {
        /*
         * ページID指定の解決
         */
        if let Ok(page_id) = PageId::from_string(target) {
            if self.manager.get_page_index_by_id(&page_id)?.is_some() {
                return Ok(page_id);
            }
            return Err(anyhow!(DbError::PageNotFound));
        }

        /*
         * ページパス指定の検証と解決
         */
        if let Err(message) = validate_page_path(target) {
            return Err(anyhow!("invalid page path: {}", message));
        }

        self.manager
            .get_page_id_by_path(target)?
            .ok_or_else(|| anyhow!(DbError::PageNotFound))
    }

    ///
    /// 指定ページ配下の削除済みアセットをパージ
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// パージに成功した場合は`Ok(())`を返す。
    ///
    fn purge_page_assets(&self, page_id: &PageId) -> Result<()> {
        let assets = self.manager.list_page_assets(page_id)?;
        let mut deleted_assets = Vec::new();
        for asset in assets {
            if asset.deleted() {
                deleted_assets.push(asset.id());
            }
        }

        for asset_id in deleted_assets {
            self.manager.delete_asset_hard(&asset_id)?;
        }

        Ok(())
    }

    ///
    /// 全ページの削除済みアセットをパージ
    ///
    /// # 戻り値
    /// パージに成功した場合は`Ok(())`を返す。
    ///
    fn purge_all_assets(&self) -> Result<()> {
        let assets = self.manager.list_assets()?;
        for asset in assets {
            if asset.deleted() {
                self.manager.delete_asset_hard(&asset.id())?;
            }
        }

        Ok(())
    }
}

impl CommandContext for AssetPurgeCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// パージ処理に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        /*
         * 対象指定の有無に応じてパージ処理を振り分け
         */
        match &self.target {
            Some(target) => {
                let page_id = self.resolve_page_id(target)?;
                self.purge_page_assets(&page_id)
            }
            None => self.purge_all_assets(),
        }
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &AssetPurgeOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(AssetPurgeCommandContext::new(opts, sub_opts)?))
}
