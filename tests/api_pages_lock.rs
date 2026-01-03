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
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde_json::Value;

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

#[test]
fn page_lock_lifecycle_works() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/lock", "lock source");

    let lock_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/lock",
        port,
        page_id
    );
    let client = build_client();

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);
    let token = parse_lock_header(&response)
        .expect("missing lock token");

    let response = client
        .get(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock get failed");
    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("lock get body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("lock get parse failed");
    assert_eq!(value["username"], TEST_USERNAME);
    assert!(value["expire"].as_str().is_some());

    let response = client
        .put(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("X-Lock-Authentication", format!("token={}", token))
        .send()
        .expect("lock put failed");
    assert_eq!(response.status().as_u16(), 204);
    let new_token = parse_lock_header(&response)
        .expect("missing lock token");
    assert_ne!(token, new_token);

    let response = client
        .delete(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("X-Lock-Authentication", format!("token={}", new_token))
        .send()
        .expect("lock delete failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock get after delete failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn page_lock_conflict_and_auth_checks() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/lock2", "lock source");

    let lock_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/lock",
        port,
        page_id
    );
    let client = build_client();

    let response = client
        .get(&lock_url)
        .send()
        .expect("lock get without auth failed");
    assert_eq!(response.status().as_u16(), 401);

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post conflict failed");
    assert_eq!(response.status().as_u16(), 409);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn page_lock_rejects_invalid_token_on_update_and_delete() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/lock-invalid", "lock source");

    let lock_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/lock",
        port,
        page_id
    );
    let client = build_client();

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .put(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("X-Lock-Authentication", "token=invalid")
        .send()
        .expect("lock put invalid token failed");
    assert_eq!(response.status().as_u16(), 403);

    let response = client
        .delete(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("X-Lock-Authentication", "token=invalid")
        .send()
        .expect("lock delete invalid token failed");
    assert_eq!(response.status().as_u16(), 403);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn page_lock_requires_token_on_update_and_delete() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/lock-missing", "lock source");

    let lock_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/lock",
        port,
        page_id
    );
    let client = build_client();

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .put(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock put missing token failed");
    assert_eq!(response.status().as_u16(), 403);

    let response = client
        .delete(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock delete missing token failed");
    assert_eq!(response.status().as_u16(), 403);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn parse_lock_header(response: &reqwest::blocking::Response) -> Option<String> {
    let raw = response.headers().get("X-Page-Lock")?.to_str().ok()?;
    for part in raw.split_whitespace() {
        if let Some(value) = part.strip_prefix("token=") {
            return Some(value.to_string());
        }
    }
    None
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

fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .expect("client build failed")
}

fn create_page(base_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let client = build_client();
    let pages_url = if base_url.ends_with("/pages") {
        base_url.to_string()
    } else {
        format!("{}/pages", base_url)
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
