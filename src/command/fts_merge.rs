/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"fts merge"の実装
//!

use std::path::PathBuf;

use anyhow::Result;

use crate::cmd_args::Options;
use crate::fts::FtsIndexConfig;
use super::CommandContext;

///
/// "fts merge"コマンド実行コンテキスト
///
struct FtsMergeCommandContext {
    index_path: PathBuf,
}

impl FtsMergeCommandContext {
    ///
    /// コンテキストの生成
    ///
    /// # 引数
    /// * `opts` - コマンドラインオプション
    ///
    /// # 戻り値
    /// 生成したコンテキスト
    ///
    fn new(opts: &Options) -> Self {
        Self {
            index_path: opts.fts_index_path(),
        }
    }
}

impl CommandContext for FtsMergeCommandContext {
    ///
    /// コマンドの実行
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn exec(&self) -> Result<()> {
        let config = FtsIndexConfig::new(self.index_path.clone());
        crate::fts::merge_index(&config)?;
        println!("merge completed");
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
    Ok(Box::new(FtsMergeCommandContext::new(opts)))
}
