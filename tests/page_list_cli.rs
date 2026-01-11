/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use common::*;

#[test]
fn page_list_cli_shows_created_pages() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);
    let base_url = format!("http://127.0.0.1:{}/api/pages", port);

    create_page(&base_url, "/a");
    create_page(&base_url, "/b");

    drop(server);

    let output = run_page_list(&db_path, &assets_dir, false);
    assert!(output.contains("/a"));
    assert!(output.contains("/b"));

    let output = run_page_list(&db_path, &assets_dir, true);
    assert!(output.contains("TIMESTAMP"));
    assert!(output.contains("USER"));
    assert!(output.contains("REV"));
    assert!(output.contains("PATH"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn create_page(base_url: &str, path: &str) {
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
        .body("test")
        .send()
        .expect("update page failed");

    assert_eq!(response.status().as_u16(), 204);
}

fn run_page_list(db_path: &Path, assets_dir: &Path, long_info: bool) -> String {
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
        .arg("list");

    if long_info {
        command.arg("--long-info");
    }

    let output = command
        .output()
        .expect("page list failed");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("stdout decode failed")
}
