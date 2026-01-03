/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ロガーの初期化処理をまとめたモジュール
//!

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Result};
use flexi_logger::{
    Cleanup, Criterion, DeferredNow, Duplicate, FileSpec, Logger, Naming,
    WriteMode
};
use log::Record;

use super::Options;

/// ログファイル1本あたりの最大サイズ(バイト)
const MAX_LOG_SIZE: u64 = 2 * 1024 * 1024;

/// 保管するログファイルの最大数
const MAX_LOG_FILES: usize = 10;

///
/// ロガーの初期化
///
/// # 引数
/// * `opts` - 設定情報をパックしたオブジェクト
///
/// # 注記
/// ログの出力方法は、出力先の指定に則り以下のように振り分ける
///
///  - 未設定の場合 -> 標準出力へ
///  - 存在しないパスの場合 -> ファイル作成を試み指定のパスへ出力
///  - ファイルのパスの場合 -> 指定のパスへ単一ファイルへ出力
///  - ディレクトリのパスの場合 -> 指定のパスへローテーション処理付きで出力
///
pub(super) fn init(opts: &Options) -> Result<()> {
    let level = opts.log_level();
    let path = opts.log_output();
    let tee = opts.log_tee();

    /*
     * オプションの設定状況に応じてロガーを初期化
     */
    if path == Path::new("-") {
        init_for_stdout(level)?;

    } else if path.exists() {
        if path.is_file() {
            init_for_file(level, &path, tee)?;

        } else if path.is_dir() {
            init_for_directory(level, &path, tee)?;

        } else {
            return Err(anyhow!("invalid log output path"));
        }

    } else if path.extension().is_some() {
        init_for_file(level, &path, tee)?;

    } else {
        init_for_directory(level, &path, tee)?;
    }

    /*
     * 終了
     */
    Ok(())
}

///
/// ログエントリのフォーマット関数
///
/// # 引数
/// * `writer` - 出力先のフォーマッター
/// * `now` - ログが出力時のタイムスタンプ
/// * `record` - ログレコードをパックしたオブジェクト
///
/// # 戻り値
/// フォーマッタへの書き込みに失敗した場合はエラー情報を `Err()`でパックして返
/// す。
///
fn format(writer: &mut dyn Write, now: &mut DeferredNow, record: &Record)
    -> std::io::Result<()>
{
    write!(
        writer,
        "[{} {:5}] - {} ({})",
        now.format("%Y-%m-%d %H:%M:%S"),
        record.level(),
        record.args(),
        source_info(&record),
    )
}

///
/// ソースコード情報の文字列化
///
/// # 引数
/// *`record` - ログレコードをパックしたオブジェクト
///
/// # 戻り値
/// ファイル名を行番号を表す文字列
///
/// # 注記
/// レコード情報からソースコード情報が得られない場合は、不明を表す文字列を返す。
///
fn source_info(record: &Record) -> String {
    /*
     * ファイル名の取得(ベースネームのみ)
     */
    let file = if let Some(path) = record.file() {
        if let Some(name) = Path::new(path).file_name() {
            name.to_string_lossy().to_string()
        } else {
            "?????".to_string()
        }
    } else {
        "?????".to_string()
    };

    /*
     * 行番号の取得
     */
    let line = if let Some(line) = record.line() {
        line.to_string()
    } else {
        "???".to_string()
    };

    /*
     * 戻り値の返却
     */
    format!("{}:{}", file, line)
}

///
/// 標準出力へ出力する場合の初期化処理
///
fn init_for_stdout<S>(level: S) -> Result<()>
where
    S: AsRef<str>
{
    Logger::try_with_env_or_str(level)?
        .log_to_stdout()
        .format(format)
        .write_mode(WriteMode::Direct)
        .start()?;

    Ok(())
}

///
/// ファイルへ出力する場合の初期化処理
///
/// # 注記
/// 出力先のファイルが存在しない場合はファイルの作成を試みる。
///
fn init_for_file<S, P>(level: S, path: P, tee: bool) -> Result<()>
where
    S: AsRef<str>,
    P: AsRef<Path>
{
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !path.exists() {
        // 指定されたパスに何もなければファイルを作成
        File::create(&path)?;
    }

    Logger::try_with_env_or_str(level)?
        .log_to_file(FileSpec::try_from(std::fs::canonicalize(path)?)?)
        .format(format)
        .duplicate_to_stdout(if tee {
            Duplicate::All
        } else {
            Duplicate::None
        })
        .append()
        .write_mode(WriteMode::Direct)
        .start()?;

    Ok(())
}

///
/// ログローテーション付きでディレクトリへ出力する場合の初期化処理
///
/// # 注記
/// ログローテションはログの量が2Mバイトを超えた場合に行う。また、ログファイル
/// は10本までを保存する。
///
fn init_for_directory<S, P>(level:S, path: P, tee: bool) -> Result<()>
where
    S: AsRef<str>,
    P: AsRef<Path>
{
    let path = path.as_ref();

    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    let path = std::fs::canonicalize(path)?;
    let path = FileSpec::try_from(path.join("log"))?.suffix("txt");

    Logger::try_with_env_or_str(level)?
        .log_to_file(path)
        .format(format)
        .duplicate_to_stdout(if tee {
            Duplicate::All
        } else {
            Duplicate::None
        })
        .rotate(
            Criterion::Size(MAX_LOG_SIZE),
            Naming::Numbers,
            Cleanup::KeepLogFiles(MAX_LOG_FILES),
        )
        .write_mode(WriteMode::Direct)
        .append()
        .start()?;

    Ok(())
}
