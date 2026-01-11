/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"fts search"の実装
//!

use std::path::PathBuf;

use anyhow::Result;

use crate::cmd_args::{FtsSearchOpts, Options};
use crate::database::DatabaseManager;
use crate::fts::{FtsIndexConfig, FtsSearchResult};
use super::CommandContext;

///
/// "fts search"コマンド実行コンテキスト
///
struct FtsSearchCommandContext {
    manager: DatabaseManager,
    index_path: PathBuf,
    target: crate::cmd_args::FtsSearchTarget,
    expression: String,
    with_deleted: bool,
    all_revision: bool,
}

impl FtsSearchCommandContext {
    ///
    /// コンテキストの生成
    ///
    /// # 引数
    /// * `opts` - コマンドラインオプション
    /// * `sub_opts` - サブコマンドオプション
    ///
    /// # 戻り値
    /// 生成したコンテキスト
    ///
    fn new(opts: &Options, sub_opts: &FtsSearchOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            index_path: opts.fts_index_path(),
            target: sub_opts.target(),
            expression: sub_opts.expression(),
            with_deleted: sub_opts.with_deleted(),
            all_revision: sub_opts.all_revision(),
        })
    }

    ///
    /// 検索結果表示用のパスを生成する
    ///
    /// # 引数
    /// * `result` - 検索結果
    ///
    /// # 戻り値
    /// 表示用のページパス
    ///
    fn display_path(&self, result: &FtsSearchResult) -> String {
        let page_id = result.page_id();
        let index = match self.manager.get_page_index_by_id(&page_id) {
            Ok(Some(index)) => index,
            _ => return "(unknown)".to_string(),
        };

        let path = index.path();
        if result.deleted() {
            format!("[{}]", path)
        } else {
            path
        }
    }
}

impl CommandContext for FtsSearchCommandContext {
    ///
    /// コマンドの実行
    ///
    /// # 概要
    /// 検索結果を取得し、スニペットと合わせて表示する。
    ///
    /// # 戻り値
    /// 処理に成功した場合は`Ok(())`
    ///
    fn exec(&self) -> Result<()> {
        /*
         * 検索の実行
         */
        let config = FtsIndexConfig::new(self.index_path.clone());
        let results = crate::fts::search_index(
            &config,
            self.target,
            &self.expression,
            self.with_deleted,
            self.all_revision,
        )?;

        /*
         * 検索結果の表示
         */
        for result in results {
            /*
             * 見出し情報の表示
             */
            let deleted_mark = if result.deleted() {
                " (削除済み)"
            } else {
                ""
            };
            let path = self.display_path(&result);
            println!(
                "- {} {} {:.3} {}{}",
                result.page_id(),
                result.revision(),
                result.score(),
                path,
                deleted_mark
            );

            /*
             * スニペットの表示
             */
            let snippet = normalize_snippet(&result.snippet());
            if snippet.is_empty() {
                println!("  ");
            } else {
                println!("  {}", snippet);
            }
        }

        Ok(())
    }
}

///
/// スニペット文字列の整形
///
/// # 引数
/// * `snippet` - スニペット文字列
///
/// # 戻り値
/// 整形後のスニペット
///
fn normalize_snippet(snippet: &str) -> String {
    snippet
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

///
/// コマンドコンテキストの生成
///
/// # 引数
/// * `opts` - コマンドラインオプション
/// * `sub_opts` - サブコマンドオプション
///
/// # 戻り値
/// 生成したコマンドコンテキスト
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &FtsSearchOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(FtsSearchCommandContext::new(opts, sub_opts)?))
}
