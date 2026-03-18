/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export / import CLI の結合テスト
//!

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use reqwest::blocking::Client;
use serde_json::Value;

use common::*;

#[test]
///
/// backup export/import の正常系を確認する。
///
/// # 注記
/// 1) ページとアセットを持つ元DBを作成する
/// 2) CLI で backup export を実行する
/// 3) 新規DBへ CLI で backup import を実行する
/// 4) page list / asset list で復元結果を確認する
///
fn backup_export_import_cli_round_trip_works() {
    /*
     * 元データを準備する
     */
    let (src_base_dir, src_db_path, src_assets_dir) = prepare_test_dirs();
    let src_port = reserve_port();

    run_add_user(&src_db_path, &src_assets_dir);
    let src_server = ServerGuard::start(src_port, &src_db_path, &src_assets_dir);
    let (src_api_url, client) =
        wait_for_server_with_scheme(src_port, src_server.stderr_path());
    let page_id = create_page(
        &client,
        &format!("{}/pages", src_api_url),
        "/backup/source",
        "# backup source\n",
    );
    upload_asset_by_page_id(
        &client,
        &src_api_url,
        &page_id,
        "note.txt",
        "text/plain",
        b"backup-asset",
    );
    drop(src_server);

    /*
     * backup export / import を実行する
     */
    let archive_path = src_base_dir.join("backup.zip");
    let export_output = run_export(
        &src_db_path,
        &src_assets_dir,
        None,
        &archive_path,
    );
    assert!(export_output.contains("export completed: type=backup"));

    let (dst_base_dir, dst_db_path, dst_assets_dir) = prepare_test_dirs();
    let import_output = run_import(
        &dst_db_path,
        &dst_assets_dir,
        None,
        &archive_path,
    );
    assert!(import_output.contains("import completed: type=backup"));

    /*
     * import 結果を確認する
     */
    let page_list = run_page_list(&dst_db_path, &dst_assets_dir);
    assert!(page_list.contains("/backup/source"));

    let asset_list = run_asset_list(&dst_db_path, &dst_assets_dir);
    assert!(asset_list.contains("note.txt"));

    fs::remove_dir_all(src_base_dir).expect("source cleanup failed");
    fs::remove_dir_all(dst_base_dir).expect("destination cleanup failed");
}

#[test]
///
/// migrate export/import の再配置と削除連動を確認する。
///
/// # 注記
/// 1) リネーム済みページ、ドラフト、ロックを持つ元DBを作成する
/// 2) CLI で migrate export を実行する
/// 3) 元DBから通常ページ、ドラフト、ロックが消えることを確認する
/// 4) 新規DBへ CLI で migrate import を実行する
/// 5) 再配置先パスと removed_by_migrate を確認する
///
fn migrate_export_import_cli_relocates_and_cleans_source_tree() {
    /*
     * 元データを準備する
     */
    let (src_base_dir, src_db_path, src_assets_dir) = prepare_test_dirs();
    let src_port = reserve_port();

    run_add_user(&src_db_path, &src_assets_dir);
    let src_server = ServerGuard::start(src_port, &src_db_path, &src_assets_dir);
    let (src_api_url, client) =
        wait_for_server_with_scheme(src_port, src_server.stderr_path());
    let pages_url = format!("{}/pages", src_api_url);

    let page_id = create_page(
        &client,
        &pages_url,
        "/tree/source",
        "# migrate source\n",
    );
    rename_page(&client, &pages_url, &page_id, "/tree/renamed");
    create_draft_page(&client, &pages_url, "/tree/draft");
    lock_page(&client, &pages_url, &page_id);
    create_page(
        &client,
        &pages_url,
        "/other/page",
        "# other page\n",
    );
    drop(src_server);

    /*
     * migrate export 後の削除連動を確認する
     */
    let archive_path = src_base_dir.join("migrate.zip");
    let export_output = run_export(
        &src_db_path,
        &src_assets_dir,
        Some("/tree"),
        &archive_path,
    );
    assert!(export_output.contains("export completed: type=migrate"));

    let src_page_list = run_page_list(&src_db_path, &src_assets_dir);
    assert!(!src_page_list.contains("/tree/renamed"));
    assert!(!src_page_list.contains("/tree/draft"));
    assert!(src_page_list.contains("/other/page"));

    let src_lock_list = run_lock_list(&src_db_path, &src_assets_dir);
    assert!(!src_lock_list.contains("/tree/renamed"));

    /*
     * migrate import の結果を確認する
     */
    let (dst_base_dir, dst_db_path, dst_assets_dir) = prepare_test_dirs();
    let import_output = run_import(
        &dst_db_path,
        &dst_assets_dir,
        Some("/dest"),
        &archive_path,
    );
    assert!(import_output.contains("import completed: type=migrate"));

    let dst_page_list = run_page_list(&dst_db_path, &dst_assets_dir);
    assert!(dst_page_list.contains("/dest/renamed"));
    assert!(!dst_page_list.contains("/tree/renamed"));
    assert!(!dst_page_list.contains("/other/page"));

    let dst_port = reserve_port();
    let dst_server = ServerGuard::start(dst_port, &dst_db_path, &dst_assets_dir);
    let (dst_api_url, dst_client) =
        wait_for_server_with_scheme(dst_port, dst_server.stderr_path());
    let imported_page_id = find_page_id_by_path(
        &dst_client,
        &dst_api_url,
        "/dest",
        "/dest/renamed",
    );
    let meta = get_page_meta(&dst_client, &dst_api_url, &imported_page_id);

    assert_eq!(
        meta["revision_info"]["rename_info"]["kind"],
        "removed_by_migrate"
    );
    let rename_revisions = meta["page_info"]["rename_revisions"]
        .as_array()
        .expect("rename_revisions missing");
    assert!(rename_revisions.is_empty());

    drop(dst_server);
    fs::remove_dir_all(src_base_dir).expect("source cleanup failed");
    fs::remove_dir_all(dst_base_dir).expect("destination cleanup failed");
}

///
/// ページを作成する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `path` - 作成するページパス
/// * `body` - 保存する本文
///
/// # 戻り値
/// 作成されたページIDを返す。
///
fn create_page(
    client: &Client,
    pages_url: &str,
    path: &str,
    body: &str,
) -> String {
    /*
     * ドラフトを作成する
     */
    let response = client
        .post(pages_url)
        .query(&[("path", path)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("create draft failed");
    assert_eq!(response.status().as_u16(), 201);

    /*
     * ロック情報とページIDを取得する
     */
    let lock_token = parse_lock_token(&response);
    let body_text = response.text().expect("read create page body failed");
    let value: Value =
        serde_json::from_str(&body_text).expect("parse create page body failed");
    let page_id = value["id"]
        .as_str()
        .expect("missing page id")
        .to_string();

    /*
     * 本文を保存して通常ページ化する
     */
    let response = client
        .put(&format!("{}/{}/source", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", lock_token))
        .body(body.to_string())
        .send()
        .expect("update page source failed");
    assert_eq!(response.status().as_u16(), 204);

    page_id
}

///
/// ドラフトページを作成する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `path` - 作成するドラフトパス
///
/// # 戻り値
/// 作成されたドラフトのページIDを返す。
///
fn create_draft_page(client: &Client, pages_url: &str, path: &str) -> String {
    let response = client
        .post(pages_url)
        .query(&[("path", path)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("create draft failed");
    assert_eq!(response.status().as_u16(), 201);

    let body_text = response.text().expect("read draft body failed");
    let value: Value =
        serde_json::from_str(&body_text).expect("parse draft body failed");
    value["id"]
        .as_str()
        .expect("missing draft id")
        .to_string()
}

///
/// ページをリネームする。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `page_id` - 対象ページID
/// * `rename_to` - リネーム先パス
///
/// # 戻り値
/// なし
///
fn rename_page(
    client: &Client,
    pages_url: &str,
    page_id: &str,
    rename_to: &str,
) {
    let response = client
        .post(&format!("{}/{}/path", pages_url, page_id))
        .query(&[("rename_to", rename_to)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("rename page failed");
    assert_eq!(response.status().as_u16(), 204);
}

///
/// ページロックを取得する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `pages_url` - `/pages` エンドポイントURL
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// なし
///
fn lock_page(client: &Client, pages_url: &str, page_id: &str) {
    let response = client
        .post(&format!("{}/{}/lock", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock page failed");
    assert_eq!(response.status().as_u16(), 204);
}

///
/// アセットをページに追加する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_url` - API ベースURL
/// * `page_id` - 対象ページID
/// * `file_name` - ファイル名
/// * `mime` - MIME type
/// * `data` - 送信内容
///
/// # 戻り値
/// 作成されたアセットIDを返す。
///
fn upload_asset_by_page_id(
    client: &Client,
    api_url: &str,
    page_id: &str,
    file_name: &str,
    mime: &str,
    data: &[u8],
) -> String {
    let response = client
        .post(&format!(
            "{}/pages/{}/assets/{}",
            api_url, page_id, file_name
        ))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", mime)
        .body(data.to_vec())
        .send()
        .expect("upload asset failed");
    assert_eq!(response.status().as_u16(), 201);

    let body_text = response.text().expect("read asset body failed");
    let value: Value =
        serde_json::from_str(&body_text).expect("parse asset body failed");
    value["id"]
        .as_str()
        .expect("missing asset id")
        .to_string()
}

///
/// ページロックヘッダからトークンを抽出する。
///
/// # 引数
/// * `response` - 対象レスポンス
///
/// # 戻り値
/// ロックトークンを返す。
///
fn parse_lock_token(response: &reqwest::blocking::Response) -> String {
    let raw = response
        .headers()
        .get("X-Page-Lock")
        .expect("missing X-Page-Lock header")
        .to_str()
        .expect("lock header decode failed");

    raw.split_whitespace()
        .find_map(|part| part.strip_prefix("token="))
        .map(str::to_string)
        .expect("missing lock token")
}

///
/// page list コマンドを実行する。
///
/// # 引数
/// * `db_path` - DB パス
/// * `assets_dir` - アセットディレクトリ
///
/// # 戻り値
/// 標準出力文字列を返す。
///
fn run_page_list(db_path: &Path, assets_dir: &Path) -> String {
    run_cli_command(db_path, assets_dir, &["page", "list"])
}

///
/// asset list コマンドを実行する。
///
/// # 引数
/// * `db_path` - DB パス
/// * `assets_dir` - アセットディレクトリ
///
/// # 戻り値
/// 標準出力文字列を返す。
///
fn run_asset_list(db_path: &Path, assets_dir: &Path) -> String {
    run_cli_command(db_path, assets_dir, &["asset", "list"])
}

///
/// lock list コマンドを実行する。
///
/// # 引数
/// * `db_path` - DB パス
/// * `assets_dir` - アセットディレクトリ
///
/// # 戻り値
/// 標準出力文字列を返す。
///
fn run_lock_list(db_path: &Path, assets_dir: &Path) -> String {
    run_cli_command(db_path, assets_dir, &["lock", "list"])
}

///
/// export コマンドを実行する。
///
/// # 引数
/// * `db_path` - DB パス
/// * `assets_dir` - アセットディレクトリ
/// * `subtree` - migrate export 時のサブツリー
/// * `archive_path` - 出力ZIPパス
///
/// # 戻り値
/// 標準出力文字列を返す。
///
fn run_export(
    db_path: &Path,
    assets_dir: &Path,
    subtree: Option<&str>,
    archive_path: &Path,
) -> String {
    let mut args = vec!["export".to_string(), "-y".to_string()];
    if let Some(path) = subtree {
        args.push("--subtree".to_string());
        args.push(path.to_string());
    }
    args.push(archive_path.to_string_lossy().to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli_command(db_path, assets_dir, &refs)
}

///
/// import コマンドを実行する。
///
/// # 引数
/// * `db_path` - DB パス
/// * `assets_dir` - アセットディレクトリ
/// * `migrate_prefix` - migrate import 時の配置先
/// * `archive_path` - 入力ZIPパス
///
/// # 戻り値
/// 標準出力文字列を返す。
///
fn run_import(
    db_path: &Path,
    assets_dir: &Path,
    migrate_prefix: Option<&str>,
    archive_path: &Path,
) -> String {
    let mut args = vec!["import".to_string(), "-y".to_string()];
    if let Some(prefix) = migrate_prefix {
        args.push("--migrate".to_string());
        args.push(prefix.to_string());
    }
    args.push(archive_path.to_string_lossy().to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli_command(db_path, assets_dir, &refs)
}

///
/// 共通 CLI 実行処理。
///
/// # 引数
/// * `db_path` - DB パス
/// * `assets_dir` - アセットディレクトリ
/// * `args` - サブコマンド引数列
///
/// # 戻り値
/// 標準出力文字列を返す。
///
fn run_cli_command(
    db_path: &Path,
    assets_dir: &Path,
    args: &[&str],
) -> String {
    let exe = test_binary_path();
    let base_dir = db_path.parent().expect("db_path parent missing");
    let output = Command::new(exe)
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index_path(db_path))
        .args(args)
        .output()
        .expect("run cli command failed");

    if !output.status.success() {
        panic!(
            "command failed: {}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout).expect("stdout decode failed")
}

///
/// 指定パスのページIDを一覧APIから取得する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_url` - API ベースURL
/// * `prefix` - 一覧取得プレフィクス
/// * `page_path` - 探索対象パス
///
/// # 戻り値
/// 対応するページIDを返す。
///
fn find_page_id_by_path(
    client: &Client,
    api_url: &str,
    prefix: &str,
    page_path: &str,
) -> String {
    let response = client
        .get(&format!("{}/pages", api_url))
        .query(&[
            ("prefix", prefix),
            ("limit", "100"),
            ("with_deleted", "false"),
        ])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("list pages failed");
    assert_eq!(response.status().as_u16(), 200);

    let body_text = response.text().expect("read page list body failed");
    let value: Value =
        serde_json::from_str(&body_text).expect("parse page list body failed");
    let items = value["items"].as_array().expect("items missing");

    items
        .iter()
        .find(|item| item["path"].as_str() == Some(page_path))
        .and_then(|item| item["page_id"].as_str())
        .map(str::to_string)
        .expect("imported page not found")
}

///
/// ページメタ情報を取得する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_url` - API ベースURL
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// メタ情報JSONを返す。
///
fn get_page_meta(client: &Client, api_url: &str, page_id: &str) -> Value {
    let response = client
        .get(&format!("{}/pages/{}/meta", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page meta failed");
    assert_eq!(response.status().as_u16(), 200);

    let body_text = response.text().expect("read page meta body failed");
    serde_json::from_str(&body_text).expect("parse page meta body failed")
}

///
/// テスト用ディレクトリ群を準備する。
///
/// # 戻り値
/// (ベースディレクトリ, DB パス, アセットディレクトリ) を返す。
///
fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
    common::prepare_test_dirs()
}
