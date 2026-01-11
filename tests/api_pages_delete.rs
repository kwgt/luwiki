/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::Value;

use common::*;

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

#[test]
///
/// POST: 再帰復帰で配下ページも復帰できることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) 親子ページを作成する
/// 3) recursive=true で削除する
/// 4) restore_to と recursive=true で復帰する
fn restore_page_recursive_restores_children() {
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let base_url = resolve_pages_base_url(port);
    let parent_id = create_page(&base_url, "/restore-recursive", "body");
    let child_id = create_page(&base_url, "/restore-recursive/child", "body");

    let client = client_for_base_url(&base_url);
    let delete_url = format!("{}/{}", base_url, parent_id);
    let response = client
        .delete(&delete_url)
        .query(&[("recursive", "true")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page recursive failed");
    assert_eq!(response.status().as_u16(), 204);

    let restore_url = format!("{}/{}/path", base_url, parent_id);
    let response = client
        .post(&restore_url)
        .query(&[
            ("restore_to", "/restore-recursive-new"),
            ("recursive", "true"),
        ])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("restore page recursive failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&format!("{}/{}/meta", base_url, parent_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta parent failed");
    assert_eq!(response.status().as_u16(), 200);
    let value: Value = serde_json::from_str(
        &response.text().expect("read meta parent failed")
    ).expect("parse meta parent failed");
    assert_eq!(value["page_info"]["path"]["value"], "/restore-recursive-new");

    let response = client
        .get(&format!("{}/{}/meta", base_url, child_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta child failed");
    assert_eq!(response.status().as_u16(), 200);
    let value: Value = serde_json::from_str(
        &response.text().expect("read meta child failed")
    ).expect("parse meta child failed");
    assert_eq!(value["page_info"]["path"]["value"], "/restore-recursive-new/child");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
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
