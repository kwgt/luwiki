/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"fts rebuild"の実装
//!

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::cmd_args::Options;
use crate::database::DatabaseManager;
use crate::fts::{extract_markdown_sections, FtsDocument, FtsIndexConfig};
use super::CommandContext;

///
/// "fts rebuild"コマンド実行コンテキスト
///
struct FtsRebuildCommandContext {
    manager: DatabaseManager,
    index_path: PathBuf,
}

impl FtsRebuildCommandContext {
    ///
    /// コンテキストの生成
    ///
    /// # 引数
    /// * `opts` - コマンドラインオプション
    ///
    /// # 戻り値
    /// 生成したコンテキスト
    ///
    fn new(opts: &Options) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            index_path: opts.fts_index_path(),
        })
    }
}

impl CommandContext for FtsRebuildCommandContext {
    ///
    /// コマンドの実行
    ///
    /// # 概要
    /// 全ページの本文から索引文書を生成し、インデックスを再構築する。
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn exec(&self) -> Result<()> {
        /*
         * インデックス情報の収集
         */
        let mut index_map = HashMap::new();
        for entry in self.manager.list_page_index_entries()? {
            let index = entry.index();
            if index.is_draft() {
                continue;
            }
            index_map.insert(entry.page_id(), (index.deleted(), index.latest()));
        }

        /*
         * 文書の構築
         */
        let mut docs = Vec::new();
        for entry in self.manager.list_page_source_entries()? {
            let (deleted, latest) = match index_map.get(&entry.page_id()) {
                Some(value) => *value,
                None => continue,
            };
            let source = entry.source().source();
            let sections = extract_markdown_sections(&source);
            let is_latest = entry.revision() == latest;
            docs.push(FtsDocument::new(
                entry.page_id(),
                entry.revision(),
                deleted,
                is_latest,
                sections.headings,
                sections.body,
                sections.code,
            ));
        }

        /*
         * 再構築の実行
         */
        let config = FtsIndexConfig::new(self.index_path.clone());
        crate::fts::rebuild_index(&config, &docs)?;
        println!("indexed: {}", docs.len());
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
/// # 引数
/// * `opts` - コマンドラインオプション
///
/// # 戻り値
/// 生成したコマンドコンテキスト
///
pub(crate) fn build_context(
    opts: &Options,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(FtsRebuildCommandContext::new(opts)?))
}
