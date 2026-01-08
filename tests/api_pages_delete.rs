/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde_json::Value;

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    match TEST_MUTEX.lock() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

#[test]
///
/// DELETE: ロック解除トークンが無い場合に削除が拒否されることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) ページを作成してロックする
/// 3) トークン無しで削除を実行する
fn delete_page_rejects_missing_lock_token() {
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let base_url = resolve_pages_base_url(port);
    let page_id = create_page(&base_url, "/delete-lock-missing", "body");
    let lock_token = lock_page(&base_url, &page_id);

    let delete_url = format!("{}/{}", base_url, page_id);
    let client = client_for_base_url(&base_url);
    let response = client
        .delete(&delete_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page missing token failed");
    assert_eq!(response.status().as_u16(), 423);

    let response = client
        .delete(&delete_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("X-Lock-Authentication", "token=invalid")
        .send()
        .expect("delete page invalid token failed");
    assert_eq!(response.status().as_u16(), 403);

    let response = client
        .delete(&delete_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("X-Lock-Authentication", format!("token={}", lock_token))
        .send()
        .expect("delete page with token failed");
    assert_eq!(response.status().as_u16(), 204);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// DELETE: 再帰削除で配下にロックページがある場合に拒否されることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) 親子ページを作成し子ページをロックする
/// 3) recursive=true で削除を実行する
fn delete_page_recursive_rejects_locked_children() {
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let base_url = resolve_pages_base_url(port);
    let parent_id = create_page(&base_url, "/delete-recursive", "body");
    let child_id = create_page(
        &base_url,
        "/delete-recursive/child",
        "body",
    );
    let _ = lock_page(&base_url, &child_id);

    let delete_url = format!("{}/{}", base_url, parent_id);
    let client = client_for_base_url(&base_url);
    let response = client
        .delete(&delete_url)
        .query(&[("recursive", "true")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page recursive failed");
    assert_eq!(response.status().as_u16(), 423);

    let response = client
        .get(&format!("{}/{}/meta", base_url, parent_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta parent failed");
    assert_eq!(response.status().as_u16(), 200);
    let value: Value = serde_json::from_str(
        &response.text().expect("read meta parent failed")
    ).expect("parse meta parent failed");
    assert_eq!(value["page_info"]["deleted"], false);

    let response = client
        .get(&format!("{}/{}/meta", base_url, child_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta child failed");
    assert_eq!(response.status().as_u16(), 200);
    let value: Value = serde_json::from_str(
        &response.text().expect("read meta child failed")
    ).expect("parse meta child failed");
    assert_eq!(value["page_info"]["deleted"], false);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let base = Path::new("tests").join("tmp").join(unique_suffix());
    let db_dir = base.join("db");
    let assets_dir = base.join("assets");
    fs::create_dir_all(&db_dir).expect("create db dir failed");
    fs::create_dir_all(&assets_dir).expect("create assets dir failed");

    let db_path = db_dir.join("database.redb");
    (base, db_path, assets_dir)
}

fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time failed")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{}", pid, now, seq)
}

fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("bind failed");
    listener.local_addr().expect("addr failed").port()
}

fn run_add_user(db_path: &Path, assets_dir: &Path) {
    let exe = test_binary_path();
    let base_dir = db_path
        .parent()
        .expect("db_path parent missing");
    let mut child = Command::new(exe)
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
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
        let stdin = child.stdin.as_mut().expect("stdin missing");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write password failed");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write confirm failed");
    }

    let status = child.wait().expect("wait add_user failed");
    assert!(status.success());
}

struct ServerGuard {
    child: Child,
}

impl ServerGuard {
    fn start(port: u16, db_path: &Path, assets_dir: &Path) -> Self {
        let exe = test_binary_path();
        let base_dir = db_path
            .parent()
            .expect("db_path parent missing");
        let child = Command::new(exe)
            .env("XDG_CONFIG_HOME", base_dir)
            .env("XDG_DATA_HOME", base_dir)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
            .arg("--log-tee")
            .arg("run")
            .arg(format!("127.0.0.1:{}", port))
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
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

fn resolve_pages_base_url(port: u16) -> String {
    let https_url = format!("https://127.0.0.1:{}/api/hello", port);
    let http_url = format!("http://127.0.0.1:{}/api/hello", port);
    let https_pages = format!("https://127.0.0.1:{}/api/pages", port);
    let http_pages = format!("http://127.0.0.1:{}/api/pages", port);
    let client = build_client();
    let tls_client = build_tls_client();

    for _ in 0..50 {
        if server_ready(&tls_client, &https_url) {
            return https_pages;
        }
        if server_ready(&client, &http_url) {
            return http_pages;
        }

        thread::sleep(Duration::from_millis(100));
    }

    panic!("server did not start");
}

fn server_ready(client: &Client, url: &str) -> bool {
    let response = client
        .get(url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send();
    if let Ok(resp) = response {
        let status = resp.status().as_u16();
        return status == 200 || status == 401;
    }

    false
}

fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(7000))
        .build()
        .expect("client build failed")
}

fn build_tls_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(7000))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("tls client build failed")
}

fn client_for_base_url(base_url: &str) -> Client {
    if base_url.starts_with("https://") {
        build_tls_client()
    } else {
        build_client()
    }
}

fn create_page(base_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let client = client_for_base_url(base_url);
    let response = client
        .post(base_url)
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
        .expect("parse create page response failed");
    let page_id = value["id"]
        .as_str()
        .expect("missing page id")
        .to_string();

    /*
     * ページソースの登録
     */
    let response = client
        .put(&format!("{}/{}/source", base_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", lock_token))
        .body(body.to_string())
        .send()
        .expect("update page failed");

    assert_eq!(response.status().as_u16(), 204);

    page_id
}

fn lock_page(base_url: &str, page_id: &str) -> String {
    let client = client_for_base_url(base_url);
    let response = client
        .post(&format!("{}/{}/lock", base_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock page failed");

    assert_eq!(response.status().as_u16(), 204);

    let lock_header = response
        .headers()
        .get("X-Page-Lock")
        .expect("missing lock header")
        .to_str()
        .expect("lock header to_str failed");
    lock_header
        .split_whitespace()
        .find_map(|part| part.strip_prefix("token="))
        .map(str::to_string)
        .expect("missing lock token")
}

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
