/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use common::*;

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde_json::Value;


#[test]
///
/// page undelete が付随アセットを復旧することを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIでページとアセットを作成する
/// 3) page delete を実行する
/// 4) page undelete を実行する
/// 5) APIでアセット一覧に復旧したアセットが含まれることを確認する
fn page_undelete_cli_restores_assets_by_default() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/undelete-assets", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &page_id,
        "data.bin",
        "application/octet-stream",
        b"asset-data",
    );
    assert!(!asset_id.is_empty());

    drop(server);

    run_page_delete(&db_path, &assets_dir, &page_id, false);
    run_page_undelete(
        &db_path,
        &assets_dir,
        false,
        &page_id,
        "/undelete-assets",
        false,
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    wait_for_server(&hello_url);

    let assets = list_page_assets(&api_url, &page_id);
    assert_eq!(assets.len(), 1);

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// page undelete --without-assets がアセットを復旧しないことを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIでページとアセットを作成する
/// 3) page delete を実行する
/// 4) page undelete --without-assets を実行する
/// 5) APIでアセット一覧が空であることを確認する
fn page_undelete_cli_without_assets_keeps_assets_deleted() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/undelete-no-assets", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &page_id,
        "data.bin",
        "application/octet-stream",
        b"asset-data",
    );
    assert!(!asset_id.is_empty());

    drop(server);

    run_page_delete(&db_path, &assets_dir, &page_id, false);
    run_page_undelete(
        &db_path,
        &assets_dir,
        true,
        &page_id,
        "/undelete-without-assets",
        false,
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    wait_for_server(&hello_url);

    let assets = list_page_assets(&api_url, &page_id);
    assert!(assets.is_empty());

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// page undelete -r が配下ページを復帰することを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIで親子ページを作成する
/// 3) page delete -r を実行する
/// 4) page undelete -r を実行する
/// 5) APIで親子ページのパスが復帰していることを確認する
fn page_undelete_cli_recursive_restores_children() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let parent_id = create_page(&api_url, "/undelete-recursive", "body");
    let child_id = create_page(
        &api_url,
        "/undelete-recursive/child",
        "body",
    );

    drop(server);

    run_page_delete(&db_path, &assets_dir, &parent_id, true);
    run_page_undelete(
        &db_path,
        &assets_dir,
        false,
        &parent_id,
        "/undelete-recursive-new",
        true,
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    wait_for_server(&hello_url);

    let parent_path = fetch_page_path(&api_url, &parent_id);
    let child_path = fetch_page_path(&api_url, &child_id);
    assert_eq!(parent_path, "/undelete-recursive-new");
    assert_eq!(child_path, "/undelete-recursive-new/child");
    drop(server);
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
/// テスト用ポートの確保
///
/// # 戻り値
/// 利用可能なポート番号を返す。
fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("bind failed");
    listener
        .local_addr()
        .expect("local_addr failed")
        .port()
}

///
/// テスト用ユーザを作成する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
fn run_add_user(db_path: &Path, assets_dir: &Path) {
    let exe = test_binary_path();
    let base_dir = db_path
        .parent()
        .expect("db_path parent missing");
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
/// テスト用サーバプロセス管理
struct ServerGuard {
    child: Child,
}

impl ServerGuard {
    ///
    /// サーバ起動
    fn start(port: u16, db_path: &Path, assets_dir: &Path) -> Self {
        let exe = test_binary_path();
        let base_dir = db_path
            .parent()
            .expect("db_path parent missing");
        let fts_index = fts_index_path(db_path);
        let child = Command::new(exe)
            .env("XDG_CONFIG_HOME", base_dir)
            .env("XDG_DATA_HOME", base_dir)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
            .arg("--fts-index")
            .arg(fts_index)
            .arg("run")
            .arg(format!("127.0.0.1:{}", port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn server failed");

        Self { child }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

///
/// サーバ起動待機
fn wait_for_server(url: &str) {
    let client = build_client();

    for _ in 0..50 {
        let response = client
            .get(url)
            .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
            .send();

        if let Ok(resp) = response {
            if resp.status().as_u16() == 200 {
                return;
            }
        }

        thread::sleep(Duration::from_millis(100));
    }

    panic!("server did not start");
}

///
/// HTTPクライアントの生成
fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(7000))
        .build()
        .expect("client build failed")
}

///
/// ページの作成
fn create_page(api_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let client = build_client();
    let pages_url = if api_url.ends_with("/pages") {
        api_url.to_string()
    } else {
        format!("{}/pages", api_url)
    };
    let response = client
        .post(&pages_url)
        .query(&[("path", path)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("create page failed");

    assert_eq!(response.status().as_u16(), 201);

    /*
     * ロックトークンの取得
     */
    let lock_header = response
        .headers()
        .get("X-Page-Lock")
        .expect("missing lock header")
        .to_str()
        .expect("lock header to_str failed");
    let lock_token = lock_header
        .split_whitespace()
        .find_map(|part| part.strip_prefix("token="))
        .map(str::to_string)
        .expect("missing lock token");

    let response_body = response.text().expect("read response body failed");
    let value: Value = serde_json::from_str(&response_body)
        .expect("parse response failed");
    let page_id = value["id"]
        .as_str()
        .expect("missing page id")
        .to_string();

    /*
     * ページソースの登録
     */
    let response = client
        .put(&format!("{}/{}/source", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", lock_token))
        .body(body.to_string())
        .send()
        .expect("update page failed");

    assert_eq!(response.status().as_u16(), 204);

    page_id
}

///
/// アセットの作成
fn upload_asset_by_page_id(
    api_url: &str,
    page_id: &str,
    file_name: &str,
    mime: &str,
    data: &[u8],
) -> String {
    let client = build_client();
    let response = client
        .post(&format!(
            "{}/pages/{}/assets/{}",
            api_url,
            page_id,
            file_name
        ))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", mime)
        .body(data.to_vec())
        .send()
        .expect("create asset failed");

    assert_eq!(response.status().as_u16(), 201);
    let body = response.text().expect("read response body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse response failed");
    value["id"]
        .as_str()
        .expect("missing asset id")
        .to_string()
}

///
/// アセット一覧の取得
fn list_page_assets(api_url: &str, page_id: &str) -> Vec<Value> {
    let client = build_client();
    let response = client
        .get(&format!("{}/pages/{}/assets", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page assets failed");

    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read page assets failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse page assets failed");
    value.as_array().expect("assets is not array").clone()
}

///
/// ページパスの取得
fn fetch_page_path(api_url: &str, page_id: &str) -> String {
    let client = build_client();
    let response = client
        .get(&format!("{}/pages/{}/meta", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page meta failed");

    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read page meta failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse page meta failed");
    value["page_info"]["path"]["value"]
        .as_str()
        .expect("path value missing")
        .to_string()
}

///
/// page delete を実行する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `page_id` - 対象ページID
/// * `recursive` - 再帰削除を行う場合はtrue
fn run_page_delete(db_path: &Path, assets_dir: &Path, page_id: &str, recursive: bool) {
    let exe = test_binary_path();
    let base_dir = db_path
        .parent()
        .expect("db_path parent missing");
    let mut command = Command::new(exe);
    command
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index_path(db_path))
        .arg("page")
        .arg("delete");
    if recursive {
        command.arg("--recursive");
    }
    let output = command
        .arg(page_id)
        .output()
        .expect("page delete failed");

    if !output.status.success() {
        panic!(
            "page delete failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

///
/// page undelete を実行する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `without_assets` - アセット復旧を行わない場合はtrue
/// * `page_id` - 対象ページID
fn run_page_undelete(
    db_path: &Path,
    assets_dir: &Path,
    without_assets: bool,
    page_id: &str,
    restore_to: &str,
    recursive: bool,
) {
    let exe = test_binary_path();
    let mut command = Command::new(exe);
    let base_dir = db_path
        .parent()
        .expect("db_path parent missing");
    command
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index_path(db_path))
        .arg("page")
        .arg("undelete");
    if without_assets {
        command.arg("--without-assets");
    }
    if recursive {
        command.arg("--recursive");
    }
    let output = command
        .arg(page_id)
        .arg(restore_to)
        .output()
        .expect("page undelete failed");

    if !output.status.success() {
        panic!(
            "page undelete failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
