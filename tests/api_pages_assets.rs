/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::Value;

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

#[test]
///
/// POST: /api/pages/{page_id}/assets/{file_name} を
/// 確認する。
///
/// # 概要
/// ページにアセットを追加できることを確認する。
///
/// # 戻り値
/// なし
///
fn post_page_assets_uploads_asset() {
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
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/page-assets-post", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &page_id,
        "page.bin",
        "application/octet-stream",
        b"page-asset",
    );

    /*
     * レスポンスの検証
     */
    assert!(!asset_id.is_empty());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// DELETE: /api/pages/{page_id} で付随アセットが削除されることを
/// 確認する。
///
/// # 概要
/// ページ削除後にアセット取得が410になることを確認する。
///
/// # 戻り値
/// なし
///
fn delete_page_marks_assets_deleted() {
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
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/page-assets-delete", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &page_id,
        "delete.bin",
        "application/octet-stream",
        b"delete-asset",
    );

    /*
     * ページ削除の実行
     */
    let client = build_client();
    let response = client
        .delete(&format!("{}/pages/{}", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page failed");

    assert_eq!(response.status().as_u16(), 204);

    /*
     * アセット削除の検証
     */
    let response = client
        .get(&format!("{}/assets/{}/data", api_url, asset_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get asset after delete page failed");

    assert_eq!(response.status().as_u16(), 410);

    let response = client
        .get(&format!("{}/pages/{}/assets", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page assets after delete failed");

    assert_eq!(response.status().as_u16(), 410);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// GET: /api/pages/{page_id}/assets の一覧取得を確認する。
///
/// # 概要
/// 削除済みアセットが一覧から除外されることを確認する。
///
/// # 戻り値
/// なし
///
fn get_page_assets_list_excludes_deleted() {
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
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/page-assets-list", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &page_id,
        "list.bin",
        "application/octet-stream",
        b"list-asset",
    );

    /*
     * アセット削除の実行
     */
    let client = build_client();
    let response = client
        .delete(&format!("{}/assets/{}", api_url, asset_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete asset failed");

    assert_eq!(response.status().as_u16(), 204);

    /*
     * 一覧取得の検証
     */
    let response = client
        .get(&format!("{}/pages/{}/assets", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page assets failed");

    assert_eq!(response.status().as_u16(), 200);

    let body = response.text().expect("read page assets body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse page assets body failed");
    let assets = value.as_array().expect("assets is not array");
    assert!(assets.is_empty());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// GET: /api/pages/{page_id}/assets/{file_name} のリダイレクトを
/// 確認する。
///
/// # 概要
/// アセット取得のリダイレクトレスポンスを検証する。
///
/// # 戻り値
/// なし
///
fn get_page_asset_redirects() {
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
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/page-assets-get", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &page_id,
        "get.bin",
        "application/octet-stream",
        b"get-asset",
    );

    /*
     * リダイレクトの検証
     */
    let redirect_url = format!(
        "{}/pages/{}/assets/get.bin",
        api_url,
        page_id
    );
    let client = build_no_redirect_client();
    let response = client
        .get(&redirect_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page asset redirect failed");

    assert_eq!(response.status().as_u16(), 302);
    assert_eq!(
        response
            .headers()
            .get("Location")
            .expect("missing location")
            .to_str()
            .expect("location to_str failed"),
        format!("/api/assets/{}/data", asset_id)
    );

    let body = response.text().expect("read redirect body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse redirect body failed");
    assert_eq!(
        value["id"].as_str().expect("missing id"),
        asset_id
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用一時ディレクトリの準備
///
/// # 概要
/// テスト用の作業ディレクトリを作成する。
///
/// # 戻り値
/// テスト用の作業ディレクトリとDB/アセットパスを返す。
///
fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
    /*
     * 一時ディレクトリの生成
     */
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let base_dir = std::env::temp_dir()
        .join("luwiki_tests_api_pages_assets")
        .join(format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time error")
                .as_micros(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));

    let db_path = base_dir.join("database.redb");
    let assets_dir = base_dir.join("assets");

    fs::create_dir_all(&assets_dir).expect("create assets dir failed");

    (base_dir, db_path, assets_dir)
}

///
/// テスト用ポートの確保
///
/// # 戻り値
/// 利用可能なポート番号を返す。
///
fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("bind failed");
    listener
        .local_addr()
        .expect("local_addr failed")
        .port()
}

///
/// ユーザ追加コマンドの実行
///
/// # 概要
/// テスト用ユーザを作成する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
///
/// # 戻り値
/// なし
///
fn run_add_user(db_path: &Path, assets_dir: &Path) {
    /*
     * ユーザ追加コマンドの実行
     */
    let exe = test_binary_path();
    let mut child = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("user")
        .arg("add")
        .arg(TEST_USERNAME)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn add_user failed");

    {
        let stdin = child.stdin.as_mut().expect("stdin missing");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write password failed");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write confirm failed");
    }

    let status = child.wait().expect("wait add_user failed");
    assert!(status.success());
}

///
/// テスト用サーバプロセス管理
///
struct ServerGuard {
    child: Child,
}

impl ServerGuard {
    ///
    /// サーバ起動
    ///
    /// # 引数
    /// * `port` - バインドするポート
    /// * `db_path` - DBファイルのパス
    /// * `assets_dir` - アセットディレクトリのパス
    ///
    /// # 戻り値
    /// 起動したサーバガードを返す。
    ///
    fn start(port: u16, db_path: &Path, assets_dir: &Path) -> Self {
        let exe = test_binary_path();
        let child = Command::new(exe)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
            .arg("run")
            .arg(format!("127.0.0.1:{}", port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn server failed");

        Self { child }
    }
}

impl Drop for ServerGuard {
    ///
    /// サーバ停止
    ///
    /// # 戻り値
    /// なし
    ///
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

///
/// サーバ起動待機
///
/// # 概要
/// ヘルスチェックの応答を待つ。
///
/// # 引数
/// * `url` - ヘルスチェックURL
///
/// # 戻り値
/// なし
///
fn wait_for_server(url: &str) {
    /*
     * サーバ起動待ち
     */
    let client = build_client();

    for _ in 0..50 {
        let response = client
            .get(url)
            .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
            .send();

        if let Ok(resp) = response {
            if resp.status().as_u16() == 200 {
                return;
            }
        }

        thread::sleep(Duration::from_millis(100));
    }

    panic!("server did not start");
}

///
/// リダイレクトを無効化したHTTPクライアントの生成
///
/// # 戻り値
/// クライアントを返す。
///
fn build_no_redirect_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(2000))
        .redirect(Policy::none())
        .build()
        .expect("client build failed")
}

///
/// HTTPクライアントの生成
///
/// # 戻り値
/// クライアントを返す。
///
fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(2000))
        .build()
        .expect("client build failed")
}

///
/// ページの作成
///
/// # 概要
/// APIでページを作成する。
///
/// # 引数
/// * `api_url` - APIのベースURL
/// * `path` - ページパス
/// * `body` - ページソース
///
/// # 戻り値
/// 作成されたページIDを返す。
///
fn create_page(api_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let client = build_client();
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
/// アセットの作成
///
/// # 概要
/// APIでアセットを作成する。
///
/// # 引数
/// * `api_url` - APIのベースURL
/// * `page_id` - ページID
/// * `file_name` - ファイル名
/// * `content_type` - MIME種別
/// * `data` - アセットデータ
///
/// # 戻り値
/// 作成されたアセットIDを返す。
///
fn upload_asset_by_page_id(
    api_url: &str,
    page_id: &str,
    file_name: &str,
    content_type: &str,
    data: &[u8],
) -> String {
    /*
     * アセット作成リクエスト
     */
    let client = build_client();
    let response = client
        .post(&format!(
            "{}/pages/{}/assets/{}",
            api_url,
            page_id,
            file_name
        ))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", content_type)
        .body(data.to_vec())
        .send()
        .expect("create asset failed");

    assert_eq!(response.status().as_u16(), 201);
    let location = response
        .headers()
        .get("Location")
        .expect("missing location")
        .to_str()
        .expect("location to_str failed")
        .to_string();
    let etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    let body = response.text().expect("read create body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse create asset response failed");
    let asset_id = value["id"]
        .as_str()
        .expect("missing asset id")
        .to_string();

    assert_eq!(etag, asset_id);
    assert_eq!(location, format!("/api/assets/{}/data", asset_id));

    asset_id
}

///
/// テスト対象バイナリのパス解決
///
/// # 概要
/// 実行ファイルの配置場所を解決する。
///
/// # 戻り値
/// 実行対象バイナリのパスを返す。
///
fn test_binary_path() -> PathBuf {
    if let Some(exe) = std::env::var_os("CARGO_BIN_EXE_luwiki") {
        return PathBuf::from(exe);
    }

    let mut path = std::env::current_exe().expect("current exe missing");
    path.pop(); // deps
    path.pop(); // debug
    path.push("luwiki");
    if cfg!(windows) {
        path.set_extension("exe");
    }

    if !path.exists() {
        panic!("luwiki binary not found: {}", path.display());
    }

    path
}
