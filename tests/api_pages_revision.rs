/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::thread;
use std::time::Duration;
use std::sync::Mutex;

use reqwest::blocking::Client;
use serde_json::Value;
use luwiki::page_source_exists_for_test;

use common::*;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn revision_test() -> std::sync::MutexGuard<'static, ()> {
    match TEST_MUTEX.lock() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

fn build_insecure_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(7000))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client build failed")
}

fn wait_for_server_with_scheme(port: u16) -> (String, Client) {
    let client = build_insecure_client();
    let mut last_error = String::new();
    let mut saw_invalid_http = false;

    for _ in 0..100 {
        let url = format!("http://127.0.0.1:{}/api/hello", port);
        let response = client
            .get(&url)
            .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
            .send();
        if let Ok(resp) = response {
            if resp.status().as_u16() == 200 {
                return (format!("http://127.0.0.1:{}/api/pages", port), client);
            }
            last_error = format!("status {}", resp.status().as_u16());
        } else if let Err(err) = response {
            let message = err.to_string();
            if message.contains("invalid HTTP version") {
                saw_invalid_http = true;
            }
            last_error = format!("request failed: {}", message);
        }
        thread::sleep(Duration::from_millis(100));
    }

    if saw_invalid_http {
        for _ in 0..100 {
            let url = format!("https://127.0.0.1:{}/api/hello", port);
            let response = client
                .get(&url)
                .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
                .send();
            if let Ok(resp) = response {
                if resp.status().as_u16() == 200 {
                    return (format!("https://127.0.0.1:{}/api/pages", port), client);
                }
                last_error = format!("status {}", resp.status().as_u16());
            } else if let Err(err) = response {
                last_error = format!("request failed: {}", err);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    panic!("server did not start: {}", last_error);
}

#[test]
fn post_revision_rollbacks_source_only() {
    let _guard = revision_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let (base_url, client) = wait_for_server_with_scheme(port);
    let page_id = create_page(&client, &base_url, "/rev-rollback", "rev1");

    update_page_source(&client, &base_url, &page_id, "rev2");
    update_page_source(&client, &base_url, &page_id, "rev3");

    let revision_url = format!("{}/{}/revision", base_url, page_id);
    let response = client
        .post(&revision_url)
        .query(&[("rollback_to", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("rollback request failed");
    assert_eq!(response.status().as_u16(), 204);

    let meta_url = format!("{}/{}/meta", base_url, page_id);
    let response = client
        .get(&meta_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta after rollback failed");
    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read meta body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse meta body failed");
    assert_eq!(value["page_info"]["revision_scope"]["latest"], 2);
    assert_eq!(value["page_info"]["revision_scope"]["oldest"], 1);

    let source_url = format!("{}/{}/source", base_url, page_id);
    let response = client
        .get(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get latest source failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read source failed"), "rev2");

    let response = client
        .get(&source_url)
        .query(&[("rev", "3")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev3 source failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn post_revision_compacts_and_removes_sources() {
    let _guard = revision_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let (base_url, client) = wait_for_server_with_scheme(port);
    let page_id = create_page(&client, &base_url, "/rev-compact", "rev1");

    update_page_source(&client, &base_url, &page_id, "rev2");
    update_page_source(&client, &base_url, &page_id, "rev3");

    let revision_url = format!("{}/{}/revision", base_url, page_id);
    let response = client
        .post(&revision_url)
        .query(&[("keep_from", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("compaction request failed");
    assert_eq!(response.status().as_u16(), 204);

    let meta_url = format!("{}/{}/meta", base_url, page_id);
    let response = client
        .get(&meta_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get meta after compaction failed");
    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read meta body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse meta body failed");
    assert_eq!(value["page_info"]["revision_scope"]["latest"], 3);
    assert_eq!(value["page_info"]["revision_scope"]["oldest"], 2);

    let source_url = format!("{}/{}/source", base_url, page_id);
    let response = client
        .get(&source_url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev1 source failed");
    assert_eq!(response.status().as_u16(), 404);

    drop(server);
    thread::sleep(Duration::from_millis(200));
    assert!(!page_source_exists_for_test(&db_path, &assets_dir, &page_id, 1)
        .expect("page_source_exists_for_test failed"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn post_revision_validates_query_parameters() {
    let _guard = revision_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let (base_url, client) = wait_for_server_with_scheme(port);
    let page_id = create_page(&client, &base_url, "/rev-invalid", "rev1");

    let revision_url = format!("{}/{}/revision", base_url, page_id);
    let response = client
        .post(&revision_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("revision without query failed");
    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(
        read_error_reason(response),
        "invalid query parameter: rollback_to or keep_from"
    );

    let response = client
        .post(&revision_url)
        .query(&[("rollback_to", "2"), ("keep_from", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("revision with both params failed");
    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(
        read_error_reason(response),
        "invalid query parameter: rollback_to or keep_from"
    );

    let response = client
        .post(&revision_url)
        .query(&[("rollback_to", "abc")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("revision with invalid rollback_to failed");
    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(
        read_error_reason(response),
        "invalid query parameter: rollback_to"
    );

    let response = client
        .post(&revision_url)
        .query(&[("keep_from", "abc")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("revision with invalid keep_from failed");
    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(
        read_error_reason(response),
        "invalid query parameter: keep_from"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn post_revision_rejects_locked_page() {
    let _guard = revision_test();
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let (base_url, client) = wait_for_server_with_scheme(port);
    let page_id = create_page(&client, &base_url, "/rev-lock", "rev1");

    let lock_url = format!("{}/{}/lock", base_url, page_id);
    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);

    let revision_url = format!("{}/{}/revision", base_url, page_id);
    let response = client
        .post(&revision_url)
        .query(&[("rollback_to", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("rollback on locked page failed");
    assert_eq!(response.status().as_u16(), 423);
    assert_eq!(read_error_reason(response), "page locked");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn create_page(client: &Client, base_url: &str, path: &str, body: &str) -> String {
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

fn update_page_source(client: &Client, base_url: &str, page_id: &str, body: &str) {
    let pages_url = if base_url.ends_with("/pages") {
        base_url.to_string()
    } else {
        format!("{}/pages", base_url)
    };
    let response = client
        .put(&format!("{}/{}/source", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(body.to_string())
        .send()
        .expect("update page failed");
    assert_eq!(response.status().as_u16(), 204);
}

fn read_error_reason(response: reqwest::blocking::Response) -> String {
    let body = response.text().expect("read error body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse error body failed");
    value["reason"]
        .as_str()
        .expect("missing reason")
        .to_string()
}
