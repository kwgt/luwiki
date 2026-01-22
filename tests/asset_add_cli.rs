/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use reqwest::blocking::Client;
use serde_json::Value;

use common::*;

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
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let page_id = create_page(&client, &api_url, "/asset-add", "body");

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
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let assets = list_page_assets(&client, &api_url, &page_id);
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
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let page_id = create_page(
        &client,
        &api_url,
        "/asset-add-config",
        "body",
    );

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
    let (api_url, client) = wait_for_server_with_scheme(
        port,
        server.stderr_path(),
    );
    let assets = list_page_assets(&client, &api_url, &page_id);
    assert_eq!(assets.len(), 1);

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用ページを作成する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `api_url` - APIベースURL
/// * `path` - ページパス
/// * `body` - ページ本文
///
/// # 戻り値
/// 作成したページID
///
fn create_page(
    client: &Client,
    api_url: &str,
    path: &str,
    body: &str,
) -> String {
    /*
     * ドラフト作成
     */
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

///
/// ページに紐づくアセット一覧を取得する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `api_url` - APIベースURL
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// アセット一覧
///
fn list_page_assets(
    client: &Client,
    api_url: &str,
    page_id: &str,
) -> Vec<Value> {
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

///
/// asset add を実行して成功を確認する
///
/// # 引数
/// * `db_path` - DBパス
/// * `assets_dir` - アセットディレクトリ
/// * `config_path` - 設定ファイルパス
/// * `user_name` - ユーザ名
/// * `file_path` - アセットファイルパス
/// * `target` - 対象ページID
///
/// # 戻り値
/// なし
///
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
        .arg(fts_index_path(db_path));

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
