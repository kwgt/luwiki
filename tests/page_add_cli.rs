/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use common::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
///
/// page add がページを作成できることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) Markdownファイルを用意する
/// 3) page add を --user 指定で実行する
/// 4) page list で登録結果を確認する
fn page_add_cli_creates_page() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let md_path = base_dir.join("page.md");
    fs::write(&md_path, "# test\n").expect("write markdown failed");

    let output = run_page_add(
        &db_path,
        &assets_dir,
        None,
        Some(TEST_USERNAME),
        &md_path,
        "/page-add",
    );
    assert!(!output.trim().is_empty());

    let list_output = run_page_list(&db_path, &assets_dir);
    assert!(list_output.contains("/page-add"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// page.add.default_user が --user 未指定時に利用されることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) config.toml に page.add.default_user を設定する
/// 3) Markdownファイルを用意する
/// 4) --user 未指定で page add を実行する
/// 5) page list で登録結果を確認する
fn page_add_cli_uses_default_user_from_config() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let config_path = base_dir.join("config.toml");
    fs::write(
        &config_path,
        format!("[page.add]\ndefault_user = \"{}\"\n", TEST_USERNAME),
    )
    .expect("write config failed");

    let md_path = base_dir.join("config_page.md");
    fs::write(&md_path, "# config test\n").expect("write markdown failed");

    let output = run_page_add(
        &db_path,
        &assets_dir,
        Some(&config_path),
        None,
        &md_path,
        "/page-add-config",
    );
    assert!(!output.trim().is_empty());

    let list_output = run_page_list(&db_path, &assets_dir);
    assert!(list_output.contains("/page-add-config"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 実CLIでprimitive競合の元診断と失敗側の無作成を確認する
///
/// # 概要
/// 同じprompt名を二つのpageへ登録し、mainのouter表示を検証する。
///
/// # 注記
/// rollback CLIは存在しない。同種DbErrorの実CLI公開経路を照合する。
///
#[test]
fn page_add_cli_preserves_primitive_conflict_diagnostic() {
    /*
     * 絶対fixture pathと独自XDGで運用configへのfallbackを防ぐ
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);
    let source = base_dir.join("prompt.md");
    fs::write(&source, concat!(
        "---\nmcp:\n  primitive: prompt\n  name: cli_conflict\n",
        "  description: fixture\n---\nfixture body\n",
    )).unwrap();
    run_page_add(&db_path, &assets_dir, None, Some(TEST_USERNAME),
        &source, "/cli-owner");
    let base = db_path.parent().unwrap();
    let output = Command::new(test_binary_path())
        .env("XDG_CONFIG_HOME", base).env("XDG_DATA_HOME", base)
        .arg("--db-path").arg(&db_path)
        .arg("--assets-path").arg(&assets_dir)
        .arg("--fts-index").arg(fts_index_path(&db_path))
        .arg("page").arg("add").arg("--user").arg(TEST_USERNAME)
        .arg(&source).arg("/cli-rejected").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(concat!(
        "MCP primitive name already exists: ",
        "primitive=prompt name=cli_conflict",
    )), "CLI native primitive cause missing");
    let listing = run_page_list(&db_path, &assets_dir);
    assert!(listing.contains("/cli-owner"));
    assert!(!listing.contains("/cli-rejected"));
    let evidence = std::path::Path::new("tmp/ms5-evidence/group3");
    fs::create_dir_all(evidence).unwrap();
    fs::write(evidence.join("cli-diagnostic-result.json"), format!(
        concat!("{{\"exit_code\":{},",
            "\"native_primitive_diagnostic\":true,",
            "\"owner_retained\":true,\"rejected_page_absent\":true}}\n"),
        output.status.code().unwrap(),
    )).unwrap();
    fs::remove_dir_all(base_dir).unwrap();
}

///
/// テスト用の作業ディレクトリを作成する。
///
/// # 戻り値
/// ベースディレクトリ、DBパス、アセットディレクトリを返す。
fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::current_dir()
        .expect("cwd missing")
        .join("tests")
        .join("tmp")
        .join(unique_suffix());
    let db_dir = base.join("db");
    let assets_dir = base.join("assets");
    fs::create_dir_all(&db_dir).expect("create db dir failed");
    fs::create_dir_all(&assets_dir).expect("create assets dir failed");

    let db_path = db_dir.join("database.redb");
    common::absolute_test_paths(base, db_path, assets_dir)
}

///
/// 一意性のあるサフィックス文字列を生成する。
fn unique_suffix() -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time failed")
        .as_millis();
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{}-{}-{}", pid, now, count)
}

///
/// テスト用ユーザを作成する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
fn run_add_user(db_path: &Path, assets_dir: &Path) {
    let exe = test_binary_path();
    let base_dir = db_path.parent().expect("db_path parent missing");
    let fts_index = fts_index_path(db_path);
    let mut child = Command::new(exe)
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index)
        .arg("user")
        .arg("add")
        .arg(TEST_USERNAME)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn add_user failed");

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin missing");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write password failed");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write confirm failed");
    }

    let status = child.wait().expect("wait add_user failed");
    assert!(status.success());
}

///
/// page add を実行し標準出力を返す。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `config_path` - config.toml のパス
/// * `user_name` - 登録ユーザ名
/// * `file_path` - 取り込むMarkdownファイル
/// * `page_path` - ページパス
///
/// # 戻り値
/// 標準出力を返す。
fn run_page_add(
    db_path: &Path,
    assets_dir: &Path,
    config_path: Option<&Path>,
    user_name: Option<&str>,
    file_path: &Path,
    page_path: &str,
) -> String {
    let exe = test_binary_path();
    let mut command = Command::new(exe);
    let base_dir = db_path.parent().expect("db_path parent missing");
    command
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index_path(db_path));

    if let Some(config_path) = config_path {
        command.arg("--config-path").arg(config_path);
    }

    command.arg("page").arg("add");

    if let Some(user_name) = user_name {
        command.arg("--user").arg(user_name);
    }

    let output = command
        .arg(file_path)
        .arg(page_path)
        .output()
        .expect("page add failed");

    if !output.status.success() {
        panic!(
            "page add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).expect("stdout decode failed")
}

///
/// page list を実行し標準出力を返す。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
///
/// # 戻り値
/// 標準出力を返す。
fn run_page_list(db_path: &Path, assets_dir: &Path) -> String {
    let exe = test_binary_path();
    let base_dir = db_path.parent().expect("db_path parent missing");
    let output = Command::new(exe)
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index_path(db_path))
        .arg("page")
        .arg("list")
        .output()
        .expect("page list failed");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("stdout decode failed")
}

///
/// テスト対象バイナリのパスを解決する。
///
/// # 戻り値
/// 実行対象バイナリのパスを返す。
fn test_binary_path() -> PathBuf {
    if let Some(exe) = std::env::var_os("CARGO_BIN_EXE_luwiki") {
        return PathBuf::from(exe);
    }

    let mut path = std::env::current_exe().expect("current exe missing");
    path.pop(); // deps
    path.pop(); // debug
    path.push("luwiki");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    if !path.exists() {
        panic!("luwiki binary not found: {}", path.display());
    }

    path
}
