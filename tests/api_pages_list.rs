/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use reqwest::blocking::Client;
use serde_json::Value;

use common::{
    prepare_test_dirs, reserve_port, run_add_user, unique_suffix,
    wait_for_server_with_scheme, TEST_PASSWORD, TEST_USERNAME, ServerGuard,
};

#[test]
/// GET: prefix配下の一覧取得で起点ページが除外されることを確認する。
fn list_pages_excludes_base_path() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    /*
     * ページ作成
     */
    let suffix = unique_suffix();
    let base_path = format!("/list-base-{}", suffix);
    create_page(&client, &api_url, &base_path, "base");
    create_page(&client, &api_url, &format!("{}/child", base_path), "child");
    create_page(
        &client,
        &api_url,
        &format!("{}/child/grand", base_path),
        "grand",
    );
    create_page(&client, &api_url, &format!("/other-{}", suffix), "other");

    /*
     * 一覧取得と検証
     */
    let (items, _, _) = list_pages(
        &client,
        &api_url,
        &base_path,
        None,
        None,
        Some(100),
        None,
    );
    let paths = extract_paths(&items);
    assert!(!paths.contains(&base_path));
    assert!(paths.contains(&format!("{}/child", base_path)));
    assert!(paths.contains(&format!("{}/child/grand", base_path)));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: with_deleted指定により削除済みページが含まれることを確認する。
fn list_pages_with_deleted_behavior() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    /*
     * ページ作成と削除
     */
    let suffix = unique_suffix();
    let base_path = format!("/list-delete-{}", suffix);
    let active_id = create_page(
        &client,
        &api_url,
        &format!("{}/active", base_path),
        "active",
    );
    let deleted_id = create_page(
        &client,
        &api_url,
        &format!("{}/deleted", base_path),
        "deleted",
    );
    delete_page(&client, &api_url, &deleted_id);

    /*
     * with_deletedなし
     */
    let (items, _, _) = list_pages(
        &client,
        &api_url,
        &base_path,
        None,
        None,
        Some(100),
        Some(false),
    );
    assert!(items.iter().any(|item| {
        item["page_id"].as_str() == Some(active_id.as_str())
            && item["deleted"].as_bool() == Some(false)
    }));
    assert!(!items.iter().any(|item| {
        item["page_id"].as_str() == Some(deleted_id.as_str())
    }));

    /*
     * with_deletedあり
     */
    let (items, _, _) = list_pages(
        &client,
        &api_url,
        &base_path,
        None,
        None,
        Some(100),
        Some(true),
    );
    assert!(items.iter().any(|item| {
        item["page_id"].as_str() == Some(deleted_id.as_str())
            && item["deleted"].as_bool() == Some(true)
    }));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: ページングでanchorが機能することを確認する。
fn list_pages_pagination_forward() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );

    /*
     * ページ作成
     */
    let suffix = unique_suffix();
    let base_path = format!("/list-page-{}", suffix);
    create_page(&client, &api_url, &format!("{}/a", base_path), "a");
    create_page(&client, &api_url, &format!("{}/b", base_path), "b");
    create_page(&client, &api_url, &format!("{}/c", base_path), "c");

    /*
     * 1ページ目
     */
    let (items, has_more, anchor) = list_pages(
        &client,
        &api_url,
        &base_path,
        None,
        None,
        Some(2),
        None,
    );
    assert_eq!(items.len(), 2);
    assert!(has_more);
    let anchor = anchor.expect("anchor missing");

    /*
     * 2ページ目
     */
    let (items, has_more, anchor) = list_pages(
        &client,
        &api_url,
        &base_path,
        Some(anchor.as_str()),
        None,
        Some(2),
        None,
    );
    assert_eq!(items.len(), 1);
    assert!(!has_more);
    assert!(anchor.is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn list_pages(
    client: &Client,
    api_url: &str,
    prefix: &str,
    forward: Option<&str>,
    rewind: Option<&str>,
    limit: Option<usize>,
    with_deleted: Option<bool>,
) -> (Vec<Value>, bool, Option<String>) {
    /*
     * クエリーパラメータの構築
     */
    let mut params: Vec<(String, String)> = Vec::new();
    params.push(("prefix".to_string(), prefix.to_string()));
    if let Some(value) = forward {
        params.push(("forward".to_string(), value.to_string()));
    }
    if let Some(value) = rewind {
        params.push(("rewind".to_string(), value.to_string()));
    }
    if let Some(value) = limit {
        params.push(("limit".to_string(), value.to_string()));
    }
    if let Some(value) = with_deleted {
        params.push(("with_deleted".to_string(), value.to_string()));
    }

    /*
     * リクエスト実行
     */
    let response = client
        .get(&format!("{}/pages", api_url))
        .query(&params)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("list pages request failed");

    assert_eq!(response.status().as_u16(), 200);

    let body = response.text().expect("read list body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse list response failed");
    let items = value["items"]
        .as_array()
        .expect("items missing")
        .clone();
    let has_more = value["has_more"].as_bool().unwrap_or(false);
    let anchor = value["anchor"].as_str().map(str::to_string);
    (items, has_more, anchor)
}

fn extract_paths(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item["path"].as_str().map(str::to_string))
        .collect()
}

fn create_page(
    client: &Client,
    api_url: &str,
    path: &str,
    body: &str,
) -> String {
    /*
     * ドラフト作成
     */
    let pages_url = format!("{}/pages", api_url);
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

    let response_body = response.text().expect("read body failed");
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

fn delete_page(client: &Client, api_url: &str, page_id: &str) {
    let response = client
        .delete(&format!("{}/pages/{}", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page failed");

    assert_eq!(response.status().as_u16(), 204);
}
