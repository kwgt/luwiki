/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod cmd_args;
pub(crate) mod command;
pub(crate) mod database;
pub(crate) mod fts;
pub(crate) mod http_server;
pub(crate) mod rest_api;

use std::sync::Arc;

use anyhow::Result;
use cmd_args::Options;

///
/// プログラムのエントリポイント
///
fn main() {
    /*
     * コマンドラインオプションのパース
     */
    let opts = match cmd_args::parse() {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("error: {}", err);
            std::process::exit(1);
        }
    };

    /*
     * 実行関数の実行
     */
    if let Err(err) = run(opts) {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

///
/// プログラムの実行関数
///
/// # 引数
/// * `opts` - オプション情報をパックしたオブジェクト
///
/// # 戻り値
/// 処理に失敗した場合はエラー情報を`Err()`でラップして返す。
///
fn run(opts: Arc<Options>) -> Result<()> {
    opts.build_context()?.exec()?;
    Ok(())
}
