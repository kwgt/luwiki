/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

#[test]
///
/// page unlock がロック解除を行えることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIでページを作成しロックする
/// 3) page unlock を実行する
/// 4) APIでロックが解除されたことを確認する
fn page_unlock_cli_releases_lock() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/unlock", "body");
    lock_page(&api_url, &page_id);

    drop(server);

    run_page_unlock(&db_path, &assets_dir, &page_id);

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    wait_for_server(&hello_url);
    assert_lock_not_found(&api_url, &page_id);

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
/// ユーザ追加コマンドの実行
///
/// # 概要
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
/// テスト用サーバプロセス管理
struct ServerGuard {
    child: Child,
}

impl ServerGuard {
    ///
    /// サーバ起動
    fn start(port: u16, db_path: &Path, assets_dir: &Path) -> Self {
        let exe = test_binary_path();
        let child = Command::new(exe)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
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
        .timeout(Duration::from_millis(2000))
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
    let value: serde_json::Value = serde_json::from_str(&response_body)
        .expect("parse create page response failed");
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
/// ページのロック
fn lock_page(api_url: &str, page_id: &str) {
    let client = build_client();
    let response = client
        .post(&format!("{}/pages/{}/lock", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock page failed");

    assert_eq!(response.status().as_u16(), 204);
}

///
/// ロック解除ができたことの確認
fn assert_lock_not_found(api_url: &str, page_id: &str) {
    let client = build_client();
    let response = client
        .get(&format!("{}/pages/{}/lock", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page lock failed");

    assert_eq!(response.status().as_u16(), 404);
}

///
/// page unlock を実行する。
fn run_page_unlock(db_path: &Path, assets_dir: &Path, target: &str) {
    let exe = test_binary_path();
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("page")
        .arg("unlock")
        .arg(target)
        .output()
        .expect("page unlock failed");

    if !output.status.success() {
        panic!(
            "page unlock failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

///
/// テスト対象バイナリのパスを解決する。
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
