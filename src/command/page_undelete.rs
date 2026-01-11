/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page undelete"の実装
//!

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::cmd_args::{Options, PageUndeleteOpts};
use crate::database::DatabaseManager;
use crate::database::types::PageId;
use crate::fts::{self, FtsIndexConfig};
use super::CommandContext;

///
/// "page undelete"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageUndeleteCommandContext {
    manager: DatabaseManager,
    index_path: PathBuf,
    page_id: PageId,
    restore_to: String,
    recursive: bool,
    with_assets: bool,
}

impl PageUndeleteCommandContext {
    ///
    /// オブジェクトの生成
    ///
    /// # 引数
    /// * `opts` - 共通オプション
    /// * `sub_opts` - サブコマンドオプション
    ///
    /// # 戻り値
    /// 生成したコンテキスト
    ///
    fn new(opts: &Options, sub_opts: &PageUndeleteOpts) -> Result<Self> {
        let page_id = PageId::from_string(&sub_opts.target())?;
        Ok(Self {
            manager: opts.open_database()?,
            index_path: opts.fts_index_path(),
            page_id,
            restore_to: sub_opts.restore_to(),
            recursive: sub_opts.is_recursive(),
            with_assets: !sub_opts.is_without_assets(),
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
        fts::update_pages_index(
            &config,
            &self.manager,
            page_ids,
            false,
        )?;

        Ok(())
    }
}

// CommandContextの実装
impl CommandContext for PageUndeleteCommandContext {
    fn exec(&self) -> Result<()> {
        if self.recursive {
            /*
             * 再帰復帰の実行
             */
            self.manager.undelete_pages_recursive_by_id(
                &self.page_id,
                &self.restore_to,
                self.with_assets,
            )?;

            /*
             * 復帰済みページの収集
             */
            let base_path = self.manager
                .get_page_index_by_id(&self.page_id)?
                .ok_or_else(|| anyhow!("page not found"))?
                .path();
            let target_ids = fts::collect_page_ids_by_path_prefix(
                &self.manager,
                &base_path,
            )?;

            /*
             * インデックスの更新
             */
            self.update_fts_for_pages(&target_ids)?;

            Ok(())
        } else {
            /*
             * 単体復帰の実行
             */
            self.manager.undelete_page_by_id(
                &self.page_id,
                &self.restore_to,
                self.with_assets,
            )?;

            /*
             * インデックスの更新
             */
            self.update_fts_for_pages(&[self.page_id.clone()])?;

            Ok(())
        }
    }
}

///
/// コマンドコンテキストの生成
///
/// # 引数
/// * `opts` - 共通オプション
/// * `sub_opts` - サブコマンドオプション
///
/// # 戻り値
/// コマンドコンテキスト
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageUndeleteOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageUndeleteCommandContext::new(opts, sub_opts)?))
}
