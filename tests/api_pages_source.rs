/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use reqwest::blocking::Client;
use serde_json::Value;

use common::*;

#[test]
/// GET: 最新ソースとヘッダが取得できることを確認する。
fn get_page_source_returns_latest_markdown() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/test", "source body");

    let url = format!("{}/{}/source", base_url, page_id);
    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("Content-Type")
            .expect("missing content-type")
            .to_str()
            .expect("content-type to_str failed"),
        "text/markdown"
    );
    assert_eq!(
        response
            .headers()
            .get("Cache-Control")
            .expect("missing cache-control")
            .to_str()
            .expect("cache-control to_str failed"),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        response
            .headers()
            .get("ETag")
            .expect("missing etag")
            .to_str()
            .expect("etag to_str failed"),
        format!("\"{}:1\"", page_id)
    );
    assert_eq!(response.text().expect("read body failed"), "source body");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: rev パラメータの妥当性検証を確認する。
fn get_page_source_with_rev_validates_revision() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/test", "source body");

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .get(&url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source rev=1 failed");
    assert_eq!(response.status().as_u16(), 200);

    let response = client
        .get(&url)
        .query(&[("rev", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source rev=2 failed");
    assert_eq!(response.status().as_u16(), 404);

    let response = client
        .get(&url)
        .query(&[("rev", "abc")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source rev=abc failed");
    assert_eq!(response.status().as_u16(), 400);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: 認証必須と不正ID時の挙動を確認する。
fn get_page_source_requires_auth_and_valid_id() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let url = format!("{}/pages/not-a-ulid/source", api_base_url);

    let response = client
        .get(&url)
        .send()
        .expect("get source without auth failed");
    assert_eq!(response.status().as_u16(), 401);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source invalid id failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: 通常更新で新リビジョンが作成されることを確認する。
fn put_page_source_creates_new_revision() {
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
        "/put-test",
        "original body",
    );

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("updated body".to_string())
        .send()
        .expect("put source failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get latest after put failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("ETag")
            .expect("missing etag")
            .to_str()
            .expect("etag to_str failed"),
        format!("\"{}:2\"", page_id)
    );
    assert_eq!(response.text().expect("read body failed"), "updated body");

    let response = client
        .get(&url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev=1 after put failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), "original body");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: amend=true で同一リビジョン更新になることを確認する。
fn put_page_source_amend_updates_latest_without_revision() {
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
        "/amend-test",
        "before amend",
    );

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .query(&[("amend", "true")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("after amend".to_string())
        .send()
        .expect("put amend failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get latest after amend failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("ETag")
            .expect("missing etag")
            .to_str()
            .expect("etag to_str failed"),
        format!("\"{}:1\"", page_id)
    );
    assert_eq!(response.text().expect("read body failed"), "after amend");

    let response = client
        .get(&url)
        .query(&[("rev", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev=2 after amend failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: ロック中はトークン必須で、成功時にロック解除されることを確認する。
fn put_page_source_requires_lock_token_when_locked() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/lock-put", "initial");

    let source_url = format!("{}/{}/source", base_url, page_id);
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
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("blocked".to_string())
        .send()
        .expect("put without lock token failed");
    assert_eq!(response.status().as_u16(), 423);

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", token))
        .body("allowed".to_string())
        .send()
        .expect("put with lock token failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock get after put failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: 不正クエリ/ヘッダ/ボディで 400 になることを確認する。
fn put_page_source_rejects_invalid_query_and_headers() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-invalid", "body");

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .query(&[("amend", "maybe")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("update".to_string())
        .send()
        .expect("put amend=maybe failed");
    assert_eq!(response.status().as_u16(), 400);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .body("update".to_string())
        .send()
        .expect("put without content-type failed");
    assert_eq!(response.status().as_u16(), 400);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "application/json")
        .body("update".to_string())
        .send()
        .expect("put invalid content-type failed");
    assert_eq!(response.status().as_u16(), 400);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(vec![0xff, 0xfe, 0xfd])
        .send()
        .expect("put invalid utf8 failed");
    assert_eq!(response.status().as_u16(), 400);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: 不正なページIDで 404 になることを確認する。
fn put_page_source_rejects_invalid_page_id() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let url = format!("{}/pages/not-a-ulid/source", api_base_url);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("update".to_string())
        .send()
        .expect("put invalid page id failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: ロック関連の 403/423 が理由付きで返ることを確認する。
fn put_page_source_lock_errors_include_reason() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/lock-reason", "body");

    let source_url = format!("{}/{}/source", base_url, page_id);
    let lock_url = format!("{}/{}/lock", base_url, page_id);

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("update".to_string())
        .send()
        .expect("put without token failed");
    assert_eq!(response.status().as_u16(), 423);
    assert_eq!(read_error_reason(response), "page locked");

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", "token=invalid")
        .body("update".to_string())
        .send()
        .expect("put invalid token failed");
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(read_error_reason(response), "lock token invalid");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
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
/// エラーレスポンスから理由メッセージを取得する
///
/// # 引数
/// * `response` - エラーレスポンス
///
/// # 戻り値
/// 理由メッセージ
///
fn read_error_reason(response: reqwest::blocking::Response) -> String {
    let body = response.text().expect("read error body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse error body failed");
    value["reason"]
        .as_str()
        .expect("missing reason")
        .to_string()
}
