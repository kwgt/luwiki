/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::DateTime;
use serde_json::Value;

use common::*;

#[test]
/// GET: 最新メタ情報の取得を確認する。
fn get_page_meta_returns_latest_info() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/meta", "meta body");

    let url = format!(
        "http://127.0.0.1:{}/api/pages/{}/meta",
        port,
        page_id
    );
    let client = build_client();
    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("Content-Type")
            .expect("missing content-type")
            .to_str()
            .expect("content-type to_str failed"),
        "application/json"
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

    let body = response.text().expect("read body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse meta response failed");

    assert_eq!(value["page_info"]["path"]["kind"], "current");
    assert_eq!(value["page_info"]["path"]["value"], "/meta");
    assert_eq!(value["page_info"]["revision_scope"]["latest"], 1);
    assert_eq!(value["page_info"]["revision_scope"]["oldest"], 1);
    assert_eq!(value["page_info"]["deleted"], false);
    assert_eq!(value["page_info"]["locked"], false);

    let rename_revisions = value["page_info"]["rename_revisions"]
        .as_array()
        .expect("rename_revisions missing");
    assert!(rename_revisions.iter().any(|rev| rev.as_u64() == Some(1)));

    assert_eq!(value["revision_info"]["revision"], 1);
    assert_eq!(value["revision_info"]["username"], TEST_USERNAME);

    let timestamp = value["revision_info"]["timestamp"]
        .as_str()
        .expect("timestamp missing");
    DateTime::parse_from_rfc3339(timestamp)
        .expect("timestamp parse failed");

    assert!(value["revision_info"].get("rename_info").is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: rev 指定でリビジョンが切り替わることを確認する。
fn get_page_meta_respects_revision() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/meta-rev", "rev1");

    let source_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/source",
        port,
        page_id
    );
    let client = build_client();
    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("rev2".to_string())
        .send()
        .expect("put source failed");
    assert_eq!(response.status().as_u16(), 204);

    let meta_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/meta",
        port,
        page_id
    );

    let response = client
        .get(&meta_url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta rev=1 failed");
    assert_eq!(response.status().as_u16(), 200);
    let value: Value = serde_json::from_str(
        &response.text().expect("read meta body failed")
    ).expect("parse meta rev=1 failed");
    assert_eq!(value["revision_info"]["revision"], 1);

    let response = client
        .get(&meta_url)
        .query(&[("rev", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta rev=2 failed");
    assert_eq!(response.status().as_u16(), 200);
    let value: Value = serde_json::from_str(
        &response.text().expect("read meta body failed")
    ).expect("parse meta rev=2 failed");
    assert_eq!(value["revision_info"]["revision"], 2);

    let response = client
        .get(&meta_url)
        .query(&[("rev", "3")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta rev=3 failed");
    assert_eq!(response.status().as_u16(), 404);

    let response = client
        .get(&meta_url)
        .query(&[("rev", "abc")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta rev=abc failed");
    assert_eq!(response.status().as_u16(), 400);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: 認証必須と不正ID時の挙動を確認する。
fn get_page_meta_requires_auth_and_valid_id() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let url = format!(
        "http://127.0.0.1:{}/api/pages/not-a-ulid/meta",
        port
    );
    let client = build_client();

    let response = client
        .get(&url)
        .send()
        .expect("get meta without auth failed");
    assert_eq!(response.status().as_u16(), 401);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta invalid id failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: ロック状態が反映されることを確認する。
fn get_page_meta_reflects_lock_state() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let base_url = format!("http://127.0.0.1:{}/api/pages", port);
    let page_id = create_page(&base_url, "/meta-lock", "meta body");

    let lock_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/lock",
        port,
        page_id
    );
    let meta_url = format!(
        "http://127.0.0.1:{}/api/pages/{}/meta",
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
        .get(&meta_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta failed");
    assert_eq!(response.status().as_u16(), 200);

    let value: Value = serde_json::from_str(
        &response.text().expect("read meta body failed")
    ).expect("parse meta failed");
    assert_eq!(value["page_info"]["locked"], true);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
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
