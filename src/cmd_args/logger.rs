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
use flexi_logger::filter::{LogLineFilter, LogLineWriter};
use log::Record;

use super::Options;

/// ログファイル1本あたりの最大サイズ(バイト)
const MAX_LOG_SIZE: u64 = 2 * 1024 * 1024;

/// 保管するログファイルの最大数
const MAX_LOG_FILES: usize = 10;

///
/// 依存MCPクレートの本文ログを製品出力から除外するfilter
///
struct McpLogFilter;

impl LogLineFilter for McpLogFilter {
    ///
    /// 本文をformatせずtargetだけで出力可否を判定する
    ///
    /// # 引数
    /// * `now` - 出力時刻
    /// * `record` - 対象レコード
    /// * `writer` - 製品出力先
    ///
    /// # 戻り値
    /// 許可したレコードの出力結果。
    ///
    fn write(
        &self,
        now: &mut DeferredNow,
        record: &Record,
        writer: &dyn LogLineWriter,
    ) -> std::io::Result<()> {
        /*
         * 依存クレートの本文ログを除き、他の出力を委譲する
         */
        let target = record.target();
        if ["rmcp", "rmcp_actix_web"].iter().any(|name| {
            target == *name || target.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with("::"))
        }) {
            return Ok(());
        }
        writer.write(now, record)
    }
}

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
        .filter(Box::new(McpLogFilter))
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
    P: AsRef<Path>,
{
    /*
     * 出力先ファイルの存在を保証
     */
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !path.exists() {
        // 指定されたパスに何もなければファイルを作成
        File::create(&path)?;
    }

    /*
     * 単一ファイル出力のロガーを初期化
     */
    Logger::try_with_env_or_str(level)?
        .filter(Box::new(McpLogFilter))
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
fn init_for_directory<S, P>(level: S, path: P, tee: bool) -> Result<()>
where
    S: AsRef<str>,
    P: AsRef<Path>,
{
    /*
     * 出力先ディレクトリの存在を保証
     */
    let path = path.as_ref();

    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    let path = std::fs::canonicalize(path)?;
    let path = FileSpec::try_from(path.join("log"))?.suffix("txt");

    /*
     * ローテーション付きロガーを初期化
     */
    Logger::try_with_env_or_str(level)?
        .filter(Box::new(McpLogFilter))
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

#[cfg(test)]
mod tests {
    use super::*;

    ///
    /// 製品loggerを隔離プロセスで初期化して秘匿を検証する
    /// # 概要
    /// 製品loggerを隔離プロセスで初期化して秘匿を検証する
    ///
    ///
    /// # 注記
    /// stdout/file/rotationとteeをtrace設定で確認する。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[test]
    fn mcp_logger_outputs_hide_dependency_payloads() {
        /*
         * 出力先ごとに新しいプロセスで製品loggerを初期化する
         */
        for mode in ["stdout", "file", "file-tee", "directory", "dir-tee"] {
            let dir = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(
                std::env::current_exe().unwrap(),
            ).args(["--exact", "cmd_args::logger::tests::mcp_logger_probe",
                "--nocapture"])
                .env("LUWIKI_LOG_PROBE_MODE", mode)
                .env("LUWIKI_LOG_PROBE_DIR", dir.path())
                .env("RUST_LOG", "trace")
                .output().unwrap();
            assert!(output.status.success(), "logger probe failed");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("PRIVATE_MCP_SENTINEL"));
            assert!(!String::from_utf8_lossy(&output.stderr)
                .contains("PRIVATE_MCP_SENTINEL"));
            if mode == "stdout" || mode.ends_with("tee") {
                assert!(stdout.contains("SAFE_LOG_MARKER"));
            }
            /*
             * ローテーション先を含め全fileとstdoutを照合する
             */
            let mut captured = stdout.into_owned();
            let mut count = 0;
            for entry in std::fs::read_dir(dir.path()).unwrap() {
                let path = entry.unwrap().path();
                if path.is_file() {
                    let content = std::fs::read_to_string(path).unwrap();
                    assert!(!content.contains("PRIVATE_MCP_SENTINEL"));
                    assert!(content.contains("SAFE_LOG_MARKER"));
                    captured.push_str(&content);
                    count += 1;
                }
            }
            for target in ["luwiki", "rmcp_extra", "rmcp_actix_web_extra"] {
                assert!(captured.contains(&format!("SAFE_LOG_MARKER {target}")));
            }
            if mode != "stdout" { assert!(count > 0); }
            if mode == "directory" || mode == "dir-tee" { assert!(count > 1); }
        }
    }

    ///
    /// 子プロセス専用の製品logger試験入口
    /// # 概要
    /// 子プロセス専用の製品logger試験入口
    ///
    ///
    /// # 注記
    /// 通常の単体実行では何も設定せず終了する。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[test]
    fn mcp_logger_probe() {
        /*
         * 子プロセス指定時だけglobal loggerを初期化する
         */
        let Ok(mode) = std::env::var("LUWIKI_LOG_PROBE_MODE") else { return; };
        let path = std::path::PathBuf::from(
            std::env::var("LUWIKI_LOG_PROBE_DIR").unwrap(),
        );
        match mode.as_str() {
            "stdout" => init_for_stdout("trace").unwrap(),
            "file" | "file-tee" => init_for_file(
                "trace", path.join("probe.log"), mode == "file-tee",
            ).unwrap(),
            _ => init_for_directory("trace", &path, mode == "dir-tee").unwrap(),
        }

        /*
         * crate境界と全レベル、およびtracing bridgeを確認する
         */
        for target in ["rmcp", "rmcp::service", "rmcp_actix_web",
            "rmcp_actix_web::transport"]
        {
            for level in [log::Level::Error, log::Level::Warn,
                log::Level::Info, log::Level::Debug, log::Level::Trace]
            {
                log::log!(target: target, level, "PRIVATE_MCP_SENTINEL");
            }
        }
        tracing::debug!(target: "rmcp::service",
            message = "PRIVATE_MCP_SENTINEL");
        for target in ["luwiki", "rmcp_extra", "rmcp_actix_web_extra"] {
            log::info!(target: target, "SAFE_LOG_MARKER {}", target);
        }
        if mode == "directory" || mode == "dir-tee" {
            let padding = "A".repeat(MAX_LOG_SIZE as usize);
            for _ in 0..3 {
                log::info!("SAFE_LOG_MARKER {}", padding);
            }
        }
        log::logger().flush();
    }
}
