/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde_json::Value;

const TEST_USERNAME: &str = "test_user";
const TEST_PASSWORD: &str = "password123";

#[test]
///
/// asset move_to がアセットの移動を行えることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIでページとアセットを作成する
/// 3) asset move_to を実行する
/// 4) asset list のパスが更新されていることを確認する
fn asset_move_to_cli_moves_asset() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let src_page_id = create_page(&api_url, "/asset-src", "body");
    let _ = create_page(&api_url, "/asset-dst", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &src_page_id,
        "asset.bin",
        "application/octet-stream",
        b"asset-data",
    );

    drop(server);

    run_asset_move_to(&db_path, &assets_dir, false, &asset_id, "/asset-dst");

    let output = run_asset_list(&db_path, &assets_dir);
    assert!(output.contains("/asset-dst/asset.bin"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
///
/// asset move_to が削除済みページと同名競合のエラーを出し分けることを確認する。
///
/// # 注記
/// 1) テスト用ユーザを作成する
/// 2) APIでページとアセットを作成する
/// 3) 移動先ページを削除する
/// 4) asset move_to を実行しエラー文言を確認する
fn asset_move_to_cli_reports_deleted_and_conflict() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let hello_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&hello_url);

    let api_url = format!("http://127.0.0.1:{}/api", port);
    let src_page_id = create_page(&api_url, "/asset-src2", "body");
    let dst_page_id = create_page(&api_url, "/asset-dst2", "body");
    let asset_id = upload_asset_by_page_id(
        &api_url,
        &src_page_id,
        "asset.bin",
        "application/octet-stream",
        b"asset-data",
    );
    let _ = upload_asset_by_page_id(
        &api_url,
        &dst_page_id,
        "asset.bin",
        "application/octet-stream",
        b"asset-data",
    );

    drop(server);

    run_page_delete(&db_path, &assets_dir, &dst_page_id);

    let stderr = run_asset_move_to_expect_fail(
        &db_path,
        &assets_dir,
        &asset_id,
        "/asset-dst2",
    );
    assert!(stderr.contains("destination page not found"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用の作業ディレクトリを作成する。
///
/// # 戻り値
/// ベースディレクトリ、DBパス、アセットディレクトリを返す。
fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::current_dir()
        .expect("cwd missing")
        .join("tests")
        .join("tmp")
        .join(unique_suffix());
    let db_dir = base.join("db");
    let assets_dir = base.join("assets");
    fs::create_dir_all(&db_dir).expect("create db dir failed");
    fs::create_dir_all(&assets_dir).expect("create assets dir failed");

    let db_path = db_dir.join("database.redb");
    (base, db_path, assets_dir)
}

///
/// 一意性のあるサフィックス文字列を生成する。
fn unique_suffix() -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time failed")
        .as_millis();
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{}-{}-{}", pid, now, count)
}

///
/// テスト用ポートの確保
///
/// # 戻り値
/// 利用可能なポート番号を返す。
fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("bind failed");
    listener
        .local_addr()
        .expect("local_addr failed")
        .port()
}

///
/// テスト用ユーザを作成する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
fn run_add_user(db_path: &Path, assets_dir: &Path) {
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
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin missing");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write password failed");
        writeln!(stdin, "{}", TEST_PASSWORD).expect("write confirm failed");
    }

    let status = child.wait().expect("wait add_user failed");
    assert!(status.success());
}

///
/// テスト用サーバプロセス管理
struct ServerGuard {
    child: Child,
}

impl ServerGuard {
    ///
    /// サーバ起動
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
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

///
/// サーバ起動待機
fn wait_for_server(url: &str) {
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
/// HTTPクライアントの生成
fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(2000))
        .build()
        .expect("client build failed")
}

///
/// ページの作成
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
        .body(body.to_string())
        .send()
        .expect("update page failed");

    assert_eq!(response.status().as_u16(), 204);

    page_id
}

///
/// アセットの作成
fn upload_asset_by_page_id(
    api_url: &str,
    page_id: &str,
    file_name: &str,
    mime: &str,
    data: &[u8],
) -> String {
    let client = build_client();
    let response = client
        .post(&format!(
            "{}/pages/{}/assets/{}",
            api_url,
            page_id,
            file_name
        ))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", mime)
        .body(data.to_vec())
        .send()
        .expect("create asset failed");

    assert_eq!(response.status().as_u16(), 201);
    let body = response.text().expect("read response body failed");
    let value: Value = serde_json::from_str(&body)
        .expect("parse response failed");
    value["id"]
        .as_str()
        .expect("missing asset id")
        .to_string()
}

///
/// asset move_to を実行する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `force` - 強制移動を行う場合はtrue
/// * `asset_id` - 対象アセットID
/// * `dst` - 移動先ページIDまたはパス
fn run_asset_move_to(
    db_path: &Path,
    assets_dir: &Path,
    force: bool,
    asset_id: &str,
    dst: &str,
) {
    let exe = test_binary_path();
    let mut command = Command::new(exe);
    command
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("asset")
        .arg("move_to");
    if force {
        command.arg("--force");
    }
    let output = command
        .arg(asset_id)
        .arg(dst)
        .output()
        .expect("asset move_to failed");

    if !output.status.success() {
        panic!(
            "asset move_to failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

///
/// asset move_to の失敗を確認する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `asset_id` - 対象アセットID
/// * `dst` - 移動先ページIDまたはパス
///
/// # 戻り値
/// 標準エラー出力を返す。
fn run_asset_move_to_expect_fail(
    db_path: &Path,
    assets_dir: &Path,
    asset_id: &str,
    dst: &str,
) -> String {
    let exe = test_binary_path();
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("asset")
        .arg("move_to")
        .arg(asset_id)
        .arg(dst)
        .output()
        .expect("asset move_to failed");

    assert!(!output.status.success());
    String::from_utf8(output.stderr).expect("stderr decode failed")
}

///
/// asset list を実行し標準出力を返す。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
///
/// # 戻り値
/// 標準出力を返す。
fn run_asset_list(db_path: &Path, assets_dir: &Path) -> String {
    let exe = test_binary_path();
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("asset")
        .arg("list")
        .arg("--long-info")
        .output()
        .expect("asset list failed");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("stdout decode failed")
}

///
/// page delete を実行する。
///
/// # 引数
/// * `db_path` - DBファイルのパス
/// * `assets_dir` - アセットディレクトリのパス
/// * `page_id` - 対象ページID
fn run_page_delete(db_path: &Path, assets_dir: &Path, page_id: &str) {
    let exe = test_binary_path();
    let output = Command::new(exe)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("page")
        .arg("delete")
        .arg(page_id)
        .output()
        .expect("page delete failed");

    if !output.status.success() {
        panic!(
            "page delete failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

///
/// テスト対象バイナリのパスを解決する。
///
/// # 戻り値
/// 実行対象バイナリのパスを返す。
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
