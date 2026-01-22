/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::sync::Mutex;

use reqwest::blocking::Client;
use serde_json::Value;

use common::*;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.lock().expect("test mutex failed")
}

#[test]
fn page_lock_lifecycle_works() {
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/lock", "lock source");

    let lock_url = format!("{}/{}/lock", base_url, page_id);

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
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/lock2", "lock source");

    let lock_url = format!("{}/{}/lock", base_url, page_id);

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
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(
        &client,
        &base_url,
        "/lock-invalid",
        "lock source",
    );

    let lock_url = format!("{}/{}/lock", base_url, page_id);

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
    let _guard = lock_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(
        &client,
        &base_url,
        "/lock-missing",
        "lock source",
    );

    let lock_url = format!("{}/{}/lock", base_url, page_id);

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

///
/// ロックレスポンスのヘッダからトークンを抽出する。
///
/// # 引数
/// * `response` - レスポンス
///
/// # 戻り値
/// トークン文字列(存在しない場合はNone)
///
fn parse_lock_header(response: &reqwest::blocking::Response) -> Option<String> {
    let raw = response.headers().get("X-Page-Lock")?.to_str().ok()?;
    for part in raw.split_whitespace() {
        if let Some(value) = part.strip_prefix("token=") {
            return Some(value.to_string());
        }
    }
    None
}

///
/// テスト用ページを作成する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `base_url` - APIベースURL
/// * `path` - ページパス
/// * `body` - ページ本文
///
/// # 戻り値
/// 作成したページID
///
fn create_page(
    client: &Client,
    base_url: &str,
    path: &str,
    body: &str,
) -> String {
    /*
     * ドラフト作成
     */
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
