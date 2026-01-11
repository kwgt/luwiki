/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use common::*;

use std::fs;
use std::fs::File;
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


#[test]
///
/// POST: /api/assets でアセットを作成できることを確認する。
///
/// # 概要
/// アセットをアップロードし、レスポンスの内容を検証する。
///
/// # 戻り値
/// なし
///
fn post_assets_creates_asset() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir, config_path) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir, &config_path);
    let server = ServerGuard::start(port, &db_path, &assets_dir, &config_path);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url, server.stderr_path());

    /*
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/assets-post", "body");
    let page_path = get_page_path(&api_url, &page_id);

    let asset_id = upload_asset_by_path(
        &api_url,
        &page_path,
        "logo.png",
        "image/png",
        b"asset-data",
    );

    /*
     * レスポンスの検証
     */
    assert!(!page_id.is_empty());
    assert!(!asset_id.is_empty());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// DELETE: /api/assets/{asset_id} でアセットを削除できることを
/// 確認する。
///
/// # 概要
/// アセットを削除し、削除後の取得が410となることを確認する。
///
/// # 戻り値
/// なし
///
fn delete_assets_marks_deleted() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir, config_path) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir, &config_path);
    let server = ServerGuard::start(port, &db_path, &assets_dir, &config_path);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url, server.stderr_path());

    /*
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/assets-delete", "body");
    let page_path = get_page_path(&api_url, &page_id);

    let asset_id = upload_asset_by_path(
        &api_url,
        &page_path,
        "delete.bin",
        "application/octet-stream",
        b"delete-data",
    );

    /*
     * 削除の実行
     */
    let client = build_client();
    let delete_url = format!("{}/assets/{}", api_url, asset_id);
    let response = client
        .delete(&delete_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete asset failed");

    assert_eq!(response.status().as_u16(), 204);

    /*
     * 削除後取得の検証
     */
    let data_url = format!("{}/assets/{}/data", api_url, asset_id);
    let response = client
        .get(&data_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get asset data after delete failed");

    assert_eq!(response.status().as_u16(), 410);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// GET: /api/assets でアセット取得先へリダイレクトできることを
/// 確認する。
///
/// # 概要
/// リダイレクトとメタ/データ取得が成功することを確認する。
///
/// # 戻り値
/// なし
///
fn get_assets_returns_redirect_and_data() {
    /*
     * テスト環境の準備
     */
    let (base_dir, db_path, assets_dir, config_path) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir, &config_path);
    let server = ServerGuard::start(port, &db_path, &assets_dir, &config_path);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url, server.stderr_path());

    /*
     * ページとアセットの作成
     */
    let api_url = format!("http://127.0.0.1:{}/api", port);
    let page_id = create_page(&api_url, "/assets-get", "body");
    let page_path = get_page_path(&api_url, &page_id);

    let asset_id = upload_asset_by_path(
        &api_url,
        &page_path,
        "data.bin",
        "application/octet-stream",
        b"data-contents",
    );

    /*
     * リダイレクトの検証
     */
    let client = build_no_redirect_client();
    let redirect_url = format!(
        "{}/assets?path=/assets-get&file=data.bin",
        api_url
    );
    let response = client
        .get(&redirect_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get asset redirect failed");

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

    /*
     * データ取得の検証
     */
    let client = build_client();
    let response = client
        .get(&format!("{}/assets/{}/data", api_url, asset_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get asset data failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("Content-Type")
            .expect("missing content-type")
            .to_str()
            .expect("content-type to_str failed"),
        "application/octet-stream"
    );
    assert_eq!(
        response.bytes().expect("read data failed").as_ref(),
        b"data-contents"
    );

    /*
     * メタ情報取得の検証
     */
    let response = client
        .get(&format!("{}/assets/{}/meta", api_url, asset_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get asset meta failed");

    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read meta body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse meta body failed");
    assert_eq!(
        value["file_name"].as_str().expect("missing file_name"),
        "data.bin"
    );
    assert_eq!(
        value["mime_type"].as_str().expect("missing mime_type"),
        "application/octet-stream"
    );
    assert_eq!(
        value["username"].as_str().expect("missing username"),
        TEST_USERNAME
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
fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    /*
     * 一時ディレクトリの生成
     */
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let pid = std::process::id();
    let base_dir = std::env::temp_dir()
        .join("luwiki_tests_api_assets")
        .join(format!(
            "{}-{}-{}",
            pid,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time error")
                .as_micros(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));

    let db_path = base_dir.join("database.redb");
    let assets_dir = base_dir.join("assets");
    let config_path = base_dir.join("config.toml");

    fs::create_dir_all(&assets_dir).expect("create assets dir failed");
    fs::write(
        &config_path,
        "[global]\nuse_tls = false\n",
    )
    .expect("write config failed");

    (base_dir, db_path, assets_dir, config_path)
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
fn run_add_user(db_path: &Path, assets_dir: &Path, config_path: &Path) {
    /*
     * ユーザ追加コマンドの実行
     */
    let exe = test_binary_path();
    let base_dir = db_path
        .parent()
        .expect("db_path parent missing");
    let fts_index = fts_index_path(db_path);
    let mut child = Command::new(exe)
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--config-path")
        .arg(config_path)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(fts_index)
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
    stderr_path: PathBuf,
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
    fn start(
        port: u16,
        db_path: &Path,
        assets_dir: &Path,
        config_path: &Path,
    ) -> Self {
        let exe = test_binary_path();
        let base_dir = db_path
            .parent()
            .expect("db_path parent missing");
        let fts_index = fts_index_path(db_path);
        let stderr_path = base_dir.join("server_stderr.log");
        let stderr_file = File::create(&stderr_path)
            .expect("create server stderr log failed");
        let child = Command::new(exe)
            .env("XDG_CONFIG_HOME", base_dir)
            .env("XDG_DATA_HOME", base_dir)
            .arg("--config-path")
            .arg(config_path)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
            .arg("--fts-index")
            .arg(fts_index)
            .arg("run")
            .arg(format!("127.0.0.1:{}", port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .expect("spawn server failed");

        Self { child, stderr_path }
    }

    fn stderr_path(&self) -> &Path {
        &self.stderr_path
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
fn wait_for_server(url: &str, stderr_path: &Path) {
    /*
     * サーバ起動待ち
     */
    let client = build_client();
    let mut last_error: Option<String> = None;

    for _ in 0..50 {
        let response = client
            .get(url)
            .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
            .send();

        if let Ok(resp) = response {
            let status = resp.status().as_u16();
            if status == 200 {
                return;
            }
            last_error = Some(format!("status {}", status));
        } else if let Err(err) = response {
            last_error = Some(format!("request error: {}", err));
        }

        thread::sleep(Duration::from_millis(100));
    }

    let stderr_output = fs::read_to_string(stderr_path)
        .unwrap_or_else(|_| "(stderr read failed)".to_string());
    panic!(
        "server did not start: {} (last error: {})",
        stderr_output,
        last_error.unwrap_or_else(|| "unknown".to_string())
    );
}

///
/// リダイレクトを無効化したHTTPクライアントの生成
///
/// # 戻り値
/// クライアントを返す。
///
fn build_no_redirect_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(7000))
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
        .timeout(Duration::from_millis(7000))
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
/// ページパスの取得
///
/// # 概要
/// APIでページメタ情報を取得し、ページパスを返す。
///
/// # 引数
/// * `api_url` - APIのベースURL
/// * `page_id` - ページID
///
/// # 戻り値
/// ページパスを返す。
///
fn get_page_path(api_url: &str, page_id: &str) -> String {
    /*
     * ページメタ情報の取得
     */
    let client = build_client();
    let response = client
        .get(&format!("{}/pages/{}/meta", api_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get page meta failed");

    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read page meta failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse page meta failed");
    value["page_info"]["path"]["value"]
        .as_str()
        .expect("missing page path")
        .to_string()
}

///
/// アセットの作成
///
/// # 概要
/// APIでアセットを作成する。
///
/// # 引数
/// * `api_url` - APIのベースURL
/// * `page_path` - ページパス
/// * `file_name` - ファイル名
/// * `content_type` - MIME種別
/// * `data` - アセットデータ
///
/// # 戻り値
/// 作成されたアセットIDを返す。
///
fn upload_asset_by_path(
    api_url: &str,
    page_path: &str,
    file_name: &str,
    content_type: &str,
    data: &[u8],
) -> String {
    /*
     * アセット作成リクエスト
     */
    let client = build_client();
    let response = client
        .post(&format!("{}/assets", api_url))
        .query(&[("path", page_path), ("file", file_name)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", content_type)
        .body(data.to_vec())
        .send()
        .expect("create asset failed");

    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let response_body = response.text().expect("read create body failed");
    if status != 201 {
        panic!(
            "create asset failed: status={} body={}",
            status,
            response_body
        );
    }
    let location = headers
        .get("Location")
        .expect("missing location")
        .to_str()
        .expect("location to_str failed")
        .to_string();
    let etag = headers
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    let value: Value = serde_json::from_str(&response_body)
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
