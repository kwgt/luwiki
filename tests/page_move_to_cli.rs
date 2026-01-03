/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

#[test]
///
/// page move_to がページの移動を行えることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) page add でページを作成する
/// 3) page move_to で移動を実行する
/// 4) page list で移動後のパスを確認する
fn page_move_to_cli_renames_page() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let md_path = base_dir.join("page.md");
    fs::write(&md_path, "# move\n").expect("write markdown failed");

    let _ = run_page_add(
        &db_path,
        &assets_dir,
        TEST_USERNAME,
        &md_path,
        "/before",
    );

    run_page_move_to(
        &db_path,
        &assets_dir,
        "/before",
        "/after",
    );

    let list_output = run_page_list(&db_path, &assets_dir);
    assert!(list_output.contains("/after"));
    assert!(!list_output.contains("/before"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
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
    (base, db_path, assets_dir)
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
    let mut child = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
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
/// * `user_name` - 登録ユーザ名
/// * `file_path` - 取り込むMarkdownファイル
/// * `page_path` - ページパス
///
/// # 戻り値
/// 標準出力を返す。
fn run_page_add(
    db_path: &Path,
    assets_dir: &Path,
    user_name: &str,
    file_path: &Path,
    page_path: &str,
) -> String {
    let exe = test_binary_path();
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("page")
        .arg("add")
        .arg("--user")
        .arg(user_name)
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
/// page move_to を実行する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `src_path` - 移動元のページパスまたはページID
/// * `dst_path` - 移動先のページパス
fn run_page_move_to(
    db_path: &Path,
    assets_dir: &Path,
    src_path: &str,
    dst_path: &str,
) {
    let exe = test_binary_path();
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("page")
        .arg("move_to")
        .arg(src_path)
        .arg(dst_path)
        .output()
        .expect("page move_to failed");

    if !output.status.success() {
        panic!(
            "page move_to failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
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
