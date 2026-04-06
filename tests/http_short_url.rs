/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::Value;

use common::*;

#[test]
/// GET: 短縮URLが current path へ 302 リダイレクトすることを確認する。
///
/// # 注記
/// 1. 通常ページを作成する
/// 2. 短縮パス取得 API で `short_id` を取得する
/// 3. `/w/{short_id}` が `/wiki/{current_path}` へ 302 を返すことを確認する
///
fn short_url_redirects_to_current_wiki_path() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let pages_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &pages_url, "/short-http", "body");
    let short_id = fetch_short_path(&client, &pages_url, &page_id);

    /*
     * リダイレクト応答の確認
     */
    let response = get_short_url_response(&api_base_url, &short_id);
    assert_eq!(response.status().as_u16(), 302);
    assert_eq!(
        response
            .headers()
            .get("Location")
            .expect("missing location header")
            .to_str()
            .expect("location to_str failed"),
        "/wiki/short-http"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: rename / move 後も同じ短縮URLで新しい閲覧URLへ到達できることを確認する。
///
/// # 注記
/// 1. 通常ページを作成し短縮パスを取得する
/// 2. ページを別パスへ rename / move する
/// 3. 同じ `/w/{short_id}` が新しい `/wiki/{current_path}` へ 302 を返すことを確認する
///
fn short_url_redirects_to_renamed_page_path() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let pages_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &pages_url, "/short-http-move", "body");
    let short_id = fetch_short_path(&client, &pages_url, &page_id);

    /*
     * ページ移動
     */
    rename_page(&client, &pages_url, &page_id, "/moved/short-http");

    /*
     * リダイレクト先の確認
     */
    let response = get_short_url_response(&api_base_url, &short_id);
    assert_eq!(response.status().as_u16(), 302);
    assert_eq!(
        response
            .headers()
            .get("Location")
            .expect("missing location header")
            .to_str()
            .expect("location to_str failed"),
        "/wiki/moved/short-http"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: 不正 short_id と未存在 page_id 相当の short_id が 404 になることを確認する。
///
/// # 注記
/// 1. 不正文字列を `/w/...` へ渡して 404 を確認する
/// 2. 既存 short_id を変形して未存在 page_id 相当の short_id を作る
/// 3. 未存在側も 404 になることを確認する
///
fn short_url_returns_not_found_for_invalid_and_missing_targets() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let pages_url = format!("{}/pages", api_base_url);
    let page_id =
        create_page(&client, &pages_url, "/short-http-missing", "body");
    let existing_short_id = fetch_short_path(&client, &pages_url, &page_id);
    let missing_short_id = build_missing_short_id(&existing_short_id);

    /*
     * 不正 short_id の確認
     */
    let invalid_response = get_short_url_response(&api_base_url, "short");
    assert_eq!(invalid_response.status().as_u16(), 404);

    /*
     * 未存在 page_id 相当 short_id の確認
     */
    let missing_response = get_short_url_response(&api_base_url, &missing_short_id);
    assert_eq!(missing_response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: 削除済みページの短縮URLが 410 Gone になることを確認する。
///
/// # 注記
/// 1. 通常ページを作成して短縮パスを取得する
/// 2. ページをソフト削除する
/// 3. `/w/{short_id}` が 410 を返すことを確認する
///
fn short_url_returns_gone_for_deleted_page() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let pages_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &pages_url, "/short-http-deleted", "body");
    let short_id = fetch_short_path(&client, &pages_url, &page_id);

    /*
     * ソフト削除
     */
    delete_page(&client, &api_base_url, &page_id);

    /*
     * 410 応答の確認
     */
    let response = get_short_url_response(&api_base_url, &short_id);
    assert_eq!(response.status().as_u16(), 410);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: 短縮URLリダイレクト応答に no-store が付与されることを確認する。
///
/// # 注記
/// 1. 通常ページを作成する
/// 2. `/w/{short_id}` の応答ヘッダを確認する
/// 3. `Cache-Control: no-store` が付与されていることを確認する
///
fn short_url_redirect_response_disables_cache() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let pages_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &pages_url, "/short-http-cache", "body");
    let short_id = fetch_short_path(&client, &pages_url, &page_id);

    /*
     * キャッシュ禁止ヘッダの確認
     */
    let response = get_short_url_response(&api_base_url, &short_id);
    assert_eq!(response.status().as_u16(), 302);
    assert_eq!(
        response
            .headers()
            .get("Cache-Control")
            .expect("missing cache-control header")
            .to_str()
            .expect("cache-control to_str failed"),
        "no-store"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用ページを作成する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `path` - ページパス
/// * `body` - ページ本文
///
/// # 戻り値
/// 作成したページID
///
fn create_page(client: &Client, pages_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let response = client
        .post(pages_url)
        .query(&[("path", path)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("create page failed");
    assert_eq!(response.status().as_u16(), 201);

    /*
     * ロックトークンとページIDの取得
     */
    let lock_token = response
        .headers()
        .get("X-Page-Lock")
        .expect("missing lock header")
        .to_str()
        .expect("lock header to_str failed")
        .split_whitespace()
        .find_map(|part| part.strip_prefix("token="))
        .map(str::to_string)
        .expect("missing lock token");

    let response_body = response.text().expect("read response body failed");
    let value: Value =
        serde_json::from_str(&response_body).expect("parse create response failed");
    let page_id = value["id"].as_str().expect("missing page id").to_string();

    /*
     * ページ本文の登録
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
/// ページIDに対応する短縮パス断片を取得する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// 取得した `short_id`
///
fn fetch_short_path(client: &Client, pages_url: &str, page_id: &str) -> String {
    /*
     * API呼び出し
     */
    let response = client
        .get(&format!("{}/{}/short", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get short path failed");
    assert_eq!(response.status().as_u16(), 200);

    /*
     * ボディ解析
     */
    let body = response.text().expect("read short path body failed");
    let value: Value =
        serde_json::from_str(&body).expect("parse short path response failed");
    value["short_path"]
        .as_str()
        .expect("missing short_path")
        .to_string()
}

///
/// ページを rename / move する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `page_id` - 対象ページID
/// * `rename_to` - 変更後のパス
///
/// # 戻り値
/// なし
///
fn rename_page(client: &Client, pages_url: &str, page_id: &str, rename_to: &str) {
    /*
     * rename 実行
     */
    let response = client
        .post(&format!("{}/{}/path", pages_url, page_id))
        .query(&[("rename_to", rename_to)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("rename page failed");
    assert_eq!(response.status().as_u16(), 204);
}

///
/// ページをソフト削除する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `api_base_url` - APIベースURL
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// なし
///
fn delete_page(client: &Client, api_base_url: &str, page_id: &str) {
    /*
     * delete 実行
     */
    let response = client
        .delete(&format!("{}/pages/{}", api_base_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page failed");
    assert_eq!(response.status().as_u16(), 204);
}

///
/// 短縮URLのレスポンスをリダイレクト追従なしで取得する。
///
/// # 引数
/// * `api_base_url` - APIベースURL
/// * `short_id` - 短縮ID
///
/// # 戻り値
/// HTTPレスポンス
///
fn get_short_url_response(api_base_url: &str, short_id: &str) -> reqwest::blocking::Response {
    /*
     * クライアント準備
     */
    let base_url = api_base_url.trim_end_matches("/api");
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_millis(7000))
        .build()
        .expect("build no redirect client failed");

    /*
     * GET 実行
     */
    client
        .get(format!("{}/w/{}", base_url, short_id))
        .send()
        .expect("get short url failed")
}

///
/// 既存 short_id を未存在 page_id 相当の別 short_id へ変形する。
///
/// # 引数
/// * `short_id` - 元となる短縮ID
///
/// # 戻り値
/// 変形後の短縮ID
///
fn build_missing_short_id(short_id: &str) -> String {
    /*
     * 最終文字だけを別の base62 文字へ変更
     */
    let mut chars: Vec<char> = short_id.chars().collect();
    let last = chars.pop().expect("short id is empty");
    let replaced = if last == '0' { '1' } else { '0' };
    chars.push(replaced);
    chars.into_iter().collect()
}
