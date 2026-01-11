/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page delete"の実装
//!

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::cmd_args::{Options, PageDeleteOpts};
use crate::database::{DatabaseManager, DbError};
use crate::database::types::PageId;
use crate::fts::{self, FtsIndexConfig};
use crate::rest_api::validate_page_path;
use super::CommandContext;

///
/// "page delete"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageDeleteCommandContext {
    manager: DatabaseManager,
    index_path: PathBuf,
    target: String,
    hard_delete: bool,
    recursive: bool,
    force: bool,
}

impl PageDeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageDeleteOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            index_path: opts.fts_index_path(),
            target: sub_opts.target(),
            hard_delete: sub_opts.is_hard_delete(),
            recursive: sub_opts.is_recursive(),
            force: sub_opts.is_force(),
        })
    }

    ///
    /// FTSインデックスを更新する
    ///
    /// # 引数
    /// * `page_ids` - 更新対象のページID一覧
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn update_fts_for_pages(&self, page_ids: &[PageId]) -> Result<()> {
        /*
         * インデックス設定の準備
         */
        let config = FtsIndexConfig::new(self.index_path.clone());

        /*
         * 対象ページの更新
         */
        if self.hard_delete {
            fts::delete_pages_index(&config, page_ids)?;
        } else {
            fts::update_pages_index(
                &config,
                &self.manager,
                page_ids,
                true,
            )?;
        }

        Ok(())
    }
}

// CommandContextの実装
impl CommandContext for PageDeleteCommandContext {
    fn exec(&self) -> Result<()> {
        /*
         * 削除対象の解決
         */
        let (page_id, index) = if let Ok(page_id) =
            PageId::from_string(&self.target)
        {
            self.manager
                .get_page_index_entry_by_id(&page_id)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))
                .map(|(_, index)| (page_id, index))?
        } else {
            if let Err(message) = validate_page_path(&self.target) {
                return Err(anyhow!("invalid page path: {}", message));
            }
            let page_id = self.manager
                .get_page_id_by_path(&self.target)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?;
            let (_, index) = self.manager
                .get_page_index_entry_by_id(&page_id)?
                .ok_or_else(|| anyhow!(DbError::PageNotFound))?;
            (page_id, index)
        };

        /*
         * 削除条件の検証
         */
        if index.deleted() && !self.hard_delete {
            return Err(anyhow!("page already deleted"));
        }

        if self.recursive && index.is_draft() {
            return Err(anyhow!("draft page cannot be deleted recursively"));
        }

        /*
         * 再帰削除
         */
        if self.recursive {
            let deleted_ids = self.manager.delete_pages_recursive_by_id(
                &page_id,
                self.hard_delete,
            )?;

            /*
             * インデックスの更新
             */
            self.update_fts_for_pages(&deleted_ids)?;
            return Ok(());
        }

        /*
         * ロック検証
         */
        if !self.force {
            let lock_info = self.manager.get_page_lock_info(&page_id)?;
            if lock_info.is_some() {
                return Err(anyhow!(DbError::PageLocked));
            }
        }

        /*
         * ページ削除
         */
        if self.hard_delete {
            self.manager.delete_page_by_id_hard(&page_id)?;
        } else {
            self.manager.delete_page_by_id(&page_id)?;
        }

        /*
         * インデックスの更新
         */
        self.update_fts_for_pages(&[page_id])?;

        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageDeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageDeleteCommandContext::new(opts, sub_opts)?))
}
