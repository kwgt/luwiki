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
use serde_json::Value;

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

#[test]
///
/// asset add がアセットを登録できることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIでページを作成する
/// 3) asset add を --user 指定で実行する
/// 4) APIでアセット一覧を取得し登録を確認する
fn asset_add_cli_creates_asset() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/asset-add", "body");

    drop(server);

    let file_path = base_dir.join("asset.bin");
    fs::write(&file_path, b"asset").expect("write asset failed");

    run_asset_add(
        &db_path,
        &assets_dir,
        None,
        Some(TEST_USERNAME),
        &file_path,
        &page_id,
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    wait_for_server(&hello_url);
    let assets = list_page_assets(&api_url, &page_id);
    assert_eq!(assets.len(), 1);
    assert_eq!(
        assets[0]["file_name"].as_str().expect("file_name missing"),
        "asset.bin"
    );

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// asset.add.default_user が --user 未指定時に利用されることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) config.toml に asset.add.default_user を設定する
/// 3) APIでページを作成する
/// 4) --user 未指定で asset add を実行する
/// 5) APIでアセット一覧を取得し登録を確認する
fn asset_add_cli_uses_default_user_from_config() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let config_path = base_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[asset.add]\ndefault_user = \"{}\"\n",
            TEST_USERNAME
        ),
    ).expect("write config failed");

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/asset-add-config", "body");

    drop(server);

    let file_path = base_dir.join("asset.bin");
    fs::write(&file_path, b"asset").expect("write asset failed");

    run_asset_add(
        &db_path,
        &assets_dir,
        Some(&config_path),
        None,
        &file_path,
        &page_id,
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    wait_for_server(&hello_url);
    let assets = list_page_assets(&api_url, &page_id);
    assert_eq!(assets.len(), 1);

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

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

fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("bind failed");
    listener
        .local_addr()
        .expect("local_addr failed")
        .port()
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
        use std::io::Write;
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
        .timeout(Duration::from_millis(2000))
        .build()
        .expect("client build failed")
}

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

fn run_asset_add(
    db_path: &Path,
    assets_dir: &Path,
    config_path: Option<&Path>,
    user_name: Option<&str>,
    file_path: &Path,
    target: &str,
) {
    let exe = test_binary_path();
    let mut command = Command::new(exe);
    command
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir);

    if let Some(config_path) = config_path {
        command.arg("--config-path").arg(config_path);
    }

    command.arg("asset").arg("add");

    if let Some(user_name) = user_name {
        command.arg("--user").arg(user_name);
    }

    let output = command
        .arg(file_path)
        .arg(target)
        .output()
        .expect("asset add failed");

    if !output.status.success() {
        panic!(
            "asset add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
