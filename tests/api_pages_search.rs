/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use serde_json::Value;

use common::{
    build_client, prepare_test_dirs, reserve_port, run_add_user,
    unique_suffix, wait_for_server, ServerGuard, TEST_PASSWORD,
    TEST_USERNAME,
};

#[test]
/// GET: 検索対象省略時に本文検索されることを確認する。
///
/// # 注記
/// - 本文にのみ含まれるトークンで検索する。
fn search_defaults_to_body_target() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * ページ作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let token = format!("body-token-{}", unique_suffix());
    let body = format!("body {}", token);
    let page_id = create_page(&api_url, "/search-body", &body);

    /*
     * 検索と検証
     */
    let results = search_pages(&api_url, &token, None, None, None);
    assert!(contains_page(&results, &page_id));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: code/combination 指定でコードブロック検索できることを確認する。
///
/// # 注記
/// - コードブロックのみ含むトークンで検索する。
/// - body,code の複合指定も確認する。
fn search_target_code_and_combination() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * ページ作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let token = format!("code-token-{}", unique_suffix());
    let body = format!("body only\n\n```\n{}\n```", token);
    let page_id = create_page(&api_url, "/search-code", &body);

    /*
     * 検索と検証
     */
    let results = search_pages(
        &api_url,
        &token,
        Some("code"),
        None,
        None,
    );
    assert!(contains_page(&results, &page_id));

    let results = search_pages(
        &api_url,
        &token,
        Some("body"),
        None,
        None,
    );
    assert!(!contains_page(&results, &page_id));

    let results = search_pages(
        &api_url,
        &token,
        Some("body,code"),
        None,
        None,
    );
    assert!(contains_page(&results, &page_id));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: headings,body 指定時に重複結果が1件になることを確認する。
///
/// # 注記
/// - 見出しと本文の両方に同一トークンを含める。
fn search_target_combination_deduplicates_results() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * ページ作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let token = format!("combo-token-{}", unique_suffix());
    let body = format!("# {}\n\n本文 {}", token, token);
    let page_id = create_page(&api_url, "/search-combo", &body);

    /*
     * 検索と検証
     */
    let results = search_pages(
        &api_url,
        &token,
        Some("headings,body"),
        None,
        None,
    );
    assert_eq!(results.len(), 1);
    assert!(contains_page(&results, &page_id));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: expr が不正な場合に 400 が返ることを確認する。
///
/// # 注記
/// - 空文字と未指定の両方で確認する。
fn search_rejects_invalid_expr() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * 検索と検証
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    assert_search_error(
        &api_url,
        Some(""),
        None,
        None,
        None,
        "invalid query parameter: expr",
    );
    assert_search_error(
        &api_url,
        None,
        None,
        None,
        None,
        "invalid query parameter: expr",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: target が不正な場合に 400 が返ることを確認する。
///
/// # 注記
/// - 未知のターゲット指定と空指定を確認する。
fn search_rejects_invalid_target() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * 検索と検証
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    assert_search_error(
        &api_url,
        Some("token"),
        Some("body,unknown"),
        None,
        None,
        "invalid query parameter: target",
    );
    assert_search_error(
        &api_url,
        Some("token"),
        Some(""),
        None,
        None,
        "invalid query parameter: target",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: with_deleted/all_revision が不正な場合に 400 が返ることを確認する。
///
/// # 注記
/// - それぞれ別の値で検証する。
fn search_rejects_invalid_flags() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * 検索と検証
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    assert_search_error(
        &api_url,
        Some("token"),
        None,
        Some("maybe"),
        None,
        "invalid query parameter: with_deleted",
    );
    assert_search_error(
        &api_url,
        Some("token"),
        None,
        None,
        Some("1"),
        "invalid query parameter: all_revision",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: all_revision/with_deleted の挙動を確認する。
///
/// # 注記
/// - 旧リビジョン専用の検索結果を確認する。
/// - 削除済みページの検索可否を確認する。
fn search_with_deleted_and_all_revision_behavior() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    /*
     * ページ作成とリビジョン更新
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let token = format!("rev-token-{}", unique_suffix());
    let body = format!("body {}", token);
    let page_id = create_page(&api_url, "/search-rev", &body);

    update_page_source(&api_url, &page_id, "updated body");

    /*
     * all_revision の検証
     */
    let results = search_pages(&api_url, &token, None, None, None);
    assert!(!contains_page(&results, &page_id));

    let results = search_pages(
        &api_url,
        &token,
        None,
        None,
        Some(true),
    );
    let item = find_page(&results, &page_id)
        .expect("missing search result");
    assert_eq!(item["revision"].as_u64(), Some(1));

    /*
     * with_deleted の検証
     */
    delete_page(&api_url, &page_id);

    let results = search_pages(
        &api_url,
        &token,
        None,
        Some(false),
        Some(true),
    );
    assert!(!contains_page(&results, &page_id));

    let results = search_pages(
        &api_url,
        &token,
        None,
        Some(true),
        Some(true),
    );
    let item = find_page(&results, &page_id)
        .expect("missing deleted result");
    assert_eq!(item["deleted"].as_bool(), Some(true));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 検索結果に指定ページが含まれるか判定する
///
/// # 引数
/// * `results` - 検索結果
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// 含まれる場合は`true`
///
fn contains_page(results: &[Value], page_id: &str) -> bool {
    results.iter().any(|item| {
        item["page_id"].as_str() == Some(page_id)
    })
}

///
/// 検索結果から指定ページの情報を取得する
///
/// # 引数
/// * `results` - 検索結果
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// 検索結果を返す。見つからない場合は`None`
///
fn find_page<'a>(results: &'a [Value], page_id: &str) -> Option<&'a Value> {
    results.iter().find(|item| {
        item["page_id"].as_str() == Some(page_id)
    })
}

///
/// 検索APIを呼び出して結果配列を返す
///
/// # 引数
/// * `api_url` - APIベースURL
/// * `expr` - 検索式
/// * `target` - 対象指定
/// * `with_deleted` - 削除済み対象フラグ
/// * `all_revision` - 全リビジョン対象フラグ
///
/// # 戻り値
/// 検索結果配列
///
fn search_pages(
    api_url: &str,
    expr: &str,
    target: Option<&str>,
    with_deleted: Option<bool>,
    all_revision: Option<bool>,
) -> Vec<Value> {
    /*
     * クエリーパラメータの構築
     */
    let mut params: Vec<(String, String)> = Vec::new();
    params.push(("expr".to_string(), expr.to_string()));
    if let Some(target) = target {
        params.push(("target".to_string(), target.to_string()));
    }
    if let Some(value) = with_deleted {
        params.push(("with_deleted".to_string(), value.to_string()));
    }
    if let Some(value) = all_revision {
        params.push(("all_revision".to_string(), value.to_string()));
    }

    /*
     * リクエスト実行
     */
    let client = build_client();
    let response = client
        .get(&format!("{}/pages/search", api_url))
        .query(&params)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("search request failed");

    assert_eq!(response.status().as_u16(), 200);

    let body = response.text().expect("read search body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse search response failed");
    value
        .as_array()
        .expect("search response must be array")
        .clone()
}

///
/// エラー応答を期待する検索リクエストを実行する
///
/// # 引数
/// * `api_url` - APIベースURL
/// * `expr` - 検索式
/// * `target` - 対象指定
/// * `with_deleted` - 削除済み対象指定
/// * `all_revision` - 全リビジョン対象指定
/// * `expected_reason` - 期待する理由メッセージ
///
/// # 戻り値
/// なし
///
fn assert_search_error(
    api_url: &str,
    expr: Option<&str>,
    target: Option<&str>,
    with_deleted: Option<&str>,
    all_revision: Option<&str>,
    expected_reason: &str,
) {
    /*
     * クエリーパラメータの構築
     */
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(expr) = expr {
        params.push(("expr".to_string(), expr.to_string()));
    }
    if let Some(target) = target {
        params.push(("target".to_string(), target.to_string()));
    }
    if let Some(value) = with_deleted {
        params.push(("with_deleted".to_string(), value.to_string()));
    }
    if let Some(value) = all_revision {
        params.push(("all_revision".to_string(), value.to_string()));
    }

    /*
     * リクエスト実行と検証
     */
    let client = build_client();
    let response = client
        .get(&format!("{}/pages/search", api_url))
        .query(&params)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("search request failed");

    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(read_error_reason(response), expected_reason);
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

///
/// 更新APIでページソースを更新する
///
/// # 引数
/// * `api_url` - APIベースURL
/// * `page_id` - 対象ページID
/// * `body` - 更新後の本文
///
/// # 戻り値
/// なし
///
fn update_page_source(api_url: &str, page_id: &str, body: &str) {
    let client = build_client();
    let response = client
        .put(&format!("{}/pages/{}/source", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(body.to_string())
        .send()
        .expect("update page failed");

    assert_eq!(response.status().as_u16(), 204);
}

///
/// 削除APIでページを削除する
///
/// # 引数
/// * `api_url` - APIベースURL
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// なし
///
fn delete_page(api_url: &str, page_id: &str) {
    let client = build_client();
    let response = client
        .delete(&format!("{}/pages/{}", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page failed");

    assert_eq!(response.status().as_u16(), 204);
}

///
/// テスト用ディレクトリを準備する
///
/// # 戻り値
/// (ベースディレクトリ, DBパス, アセットディレクトリ)
///
/// ページを作成してIDを返す
/// 
/// # 引数
/// * `api_url` - APIベースURL
/// * `path` - ページパス
/// * `body` - 初期本文
///
/// # 戻り値
/// ページID
///
fn create_page(api_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let client = build_client();
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
