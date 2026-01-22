/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 結合テスト用の共通ヘルパー
//!
#![allow(dead_code)]

use std::fs::{self, File};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;

/// テスト用ユーザ名
pub const TEST_USERNAME: &str = "test_user";
/// テスト用パスワード
pub const TEST_PASSWORD: &str = "password123";

/// サーバ起動待機のリトライ回数
const SERVER_START_RETRY_COUNT: usize = 300;
/// サーバ起動待機のリトライ間隔(ミリ秒)
const SERVER_START_RETRY_INTERVAL_MS: u64 = 100;

///
/// 全文検索インデックスのパスを返す
///
/// # 引数
/// * `db_path` - DBパス
///
/// # 戻り値
/// インデックス格納パス
///
#[allow(dead_code)]
pub fn fts_index_path(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .expect("db_path parent missing")
        .join("fts_index")
}

///
/// テスト用ディレクトリを準備する
///
/// # 戻り値
/// (ベースディレクトリ, DBパス, アセットディレクトリ)
///
#[allow(dead_code)]
pub fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
    /*
     * ベースディレクトリの生成
     */
    let base = Path::new("tests").join("tmp").join(unique_suffix());
    let db_dir = base.join("db");
    let assets_dir = base.join("assets");

    /*
     * ディレクトリ作成
     */
    fs::create_dir_all(&db_dir).expect("create db dir failed");
    fs::create_dir_all(&assets_dir).expect("create assets dir failed");

    /*
     * パスの組み立て
     */
    let db_path = db_dir.join("database.redb");
    (base, db_path, assets_dir)
}

///
/// 一意なサフィックス文字列を生成する
///
/// # 戻り値
/// サフィックス文字列
///
#[allow(dead_code)]
pub fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time failed")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{}", pid, now, seq)
}

///
/// ローカル空きポートを確保する
///
/// # 戻り値
/// ポート番号
///
#[allow(dead_code)]
pub fn reserve_port() -> u16 {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /*
     * プロセス毎の開始ポートを算出
     */
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let pid = std::process::id() as usize;
    let base = 20000usize + (pid % 1000) * 40;

    /*
     * 予約可能なポートを探索
     */
    for _ in 0..80 {
        let offset = COUNTER.fetch_add(1, Ordering::Relaxed) % 40;
        let port = (base + offset) as u16;
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            drop(listener);
            return port;
        }
    }

    /*
     * 最終手段としてOSに割り当てを委ねる
     */
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("bind failed");
    listener.local_addr().expect("addr failed").port()
}

///
/// テスト用ユーザを追加する
///
/// # 引数
/// * `db_path` - DBパス
/// * `assets_dir` - アセットディレクトリ
///
/// # 戻り値
/// なし
///
#[allow(dead_code)]
pub fn run_add_user(db_path: &Path, assets_dir: &Path) {
    /*
     * CLI起動
     */
    let exe = test_binary_path();
    let base_dir = db_path
        .parent()
        .expect("db_path parent missing");
    let fts_index = fts_index_path(db_path);
    let mut child = Command::new(exe)
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
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

    /*
     * パスワード入力
     */
    {
        let stdin = child.stdin.as_mut().expect("stdin missing");
        writeln!(stdin, "{}", TEST_PASSWORD)
            .expect("write password failed");
        writeln!(stdin, "{}", TEST_PASSWORD)
            .expect("write confirm failed");
    }

    /*
     * 実行結果の確認
     */
    let status = child.wait().expect("wait add_user failed");
    assert!(status.success());
}

///
/// APIサーバの起動を管理するガード
///
#[allow(dead_code)]
pub struct ServerGuard {
    child: Child,
    stderr_path: PathBuf,
}

impl ServerGuard {
    ///
    /// APIサーバを起動する
    ///
    /// # 引数
    /// * `port` - 待受ポート
    /// * `db_path` - DBパス
    /// * `assets_dir` - アセットディレクトリ
    ///
    /// # 戻り値
    /// ServerGuard
    ///
    #[allow(dead_code)]
    pub fn start(port: u16, db_path: &Path, assets_dir: &Path) -> Self {
        /*
         * サーバ起動
         */
        let exe = test_binary_path();
        let base_dir = db_path
            .parent()
            .expect("db_path parent missing");
        let fts_index = fts_index_path(db_path);
        /*
         * テスト用設定の準備
         */
        let config_dir = base_dir.join(env!("CARGO_PKG_NAME"));
        fs::create_dir_all(&config_dir)
            .expect("create config dir failed");
        let config_path = config_dir.join("config.toml");
        fs::write(
            &config_path,
            "[run]\nuse_tls = false\n",
        ).expect("write test config failed");
        let stdout_path = base_dir.join("server.stdout.log");
        let stdout = File::create(&stdout_path)
            .expect("create server stdout failed");
        let stderr_path = base_dir.join("server.stderr.log");
        let stderr = File::create(&stderr_path)
            .expect("create server stderr failed");
        let child = Command::new(exe)
            .env("XDG_CONFIG_HOME", base_dir)
            .env("XDG_DATA_HOME", base_dir)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
            .arg("--fts-index")
            .arg(fts_index)
            .arg("run")
            .arg(format!("127.0.0.1:{}", port))
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn server failed");

        Self { child, stderr_path }
    }

    ///
    /// サーバの標準エラーパスを取得する
    ///
    /// # 戻り値
    /// 標準エラーログのパス
    ///
    #[allow(dead_code)]
    pub fn stderr_path(&self) -> &Path {
        &self.stderr_path
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

///
/// サーバの起動完了を待機する
///
/// # 引数
/// * `url` - ヘルスチェックURL
/// * `stderr_path` - サーバ標準エラーログのパス
///
/// # 戻り値
/// なし
///
#[allow(dead_code)]
pub fn wait_for_server(url: &str, stderr_path: &Path) {
    /*
     * 起動確認
     */
    let client = build_client();
    let mut last_error: Option<String> = None;

    for _ in 0..SERVER_START_RETRY_COUNT {
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

        thread::sleep(Duration::from_millis(
            SERVER_START_RETRY_INTERVAL_MS
        ));
    }

    /*
     * 失敗時のログ出力
     */
    let stderr_log = fs::read_to_string(stderr_path)
        .unwrap_or_else(|_| "<stderr log not available>".to_string());
    let stdout_path = stderr_path
        .with_file_name("server.stdout.log");
    let stdout_log = fs::read_to_string(&stdout_path)
        .unwrap_or_else(|_| "<stdout log not available>".to_string());
    let last_error = last_error.unwrap_or_else(|| "unknown".to_string());
    panic!(
        "server did not start\nstdout:\n{}\nstderr:\n{}\nlast error: {}",
        stdout_log,
        stderr_log,
        last_error
    );
}

///
/// APIサーバの起動完了を待機し、HTTP/HTTPSのスキームを返す。
///
/// # 概要
/// HTTPで疎通を試し、TLSが有効な場合はHTTPSで疎通を確認する。
///
/// # 引数
/// * `port` - 待受ポート
/// * `stderr_path` - サーバ標準エラーログのパス
///
/// # 戻り値
/// (APIベースURL, HTTPクライアント)のタプルを返す。
///
pub fn wait_for_server_with_scheme(
    port: u16,
    stderr_path: &Path,
) -> (String, Client) {
    /*
     * 事前情報の準備
     */
    let http_base = format!("http://127.0.0.1:{}/api", port);
    let https_base = format!("https://127.0.0.1:{}/api", port);
    let http_url = format!("{}/hello", http_base);
    let https_url = format!("{}/hello", https_base);
    let http_client = build_client();
    let https_client = Client::builder()
        .timeout(Duration::from_millis(7000))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("https client build failed");
    let mut last_error: Option<String> = None;

    /*
     * 起動確認
     */
    for _ in 0..SERVER_START_RETRY_COUNT {
        let response = http_client
            .get(&http_url)
            .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
            .send();

        if let Ok(resp) = response {
            let status = resp.status().as_u16();
            if status == 200 {
                return (http_base, http_client);
            }
            last_error = Some(format!("http status {}", status));
        } else if let Err(err) = response {
            let message = err.to_string();
            if message.contains("invalid HTTP version") {
                let https_response = https_client
                    .get(&https_url)
                    .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
                    .send();
                if let Ok(resp) = https_response {
                    let status = resp.status().as_u16();
                    if status == 200 {
                        return (https_base, https_client);
                    }
                    last_error = Some(format!("https status {}", status));
                } else if let Err(err) = https_response {
                    last_error = Some(format!("https error: {}", err));
                }
            } else {
                last_error = Some(format!("http error: {}", err));
            }
        }

        thread::sleep(Duration::from_millis(
            SERVER_START_RETRY_INTERVAL_MS
        ));
    }

    /*
     * 失敗時のログ出力
     */
    let stderr_log = fs::read_to_string(stderr_path)
        .unwrap_or_else(|_| "<stderr log not available>".to_string());
    let stdout_path = stderr_path
        .with_file_name("server.stdout.log");
    let stdout_log = fs::read_to_string(&stdout_path)
        .unwrap_or_else(|_| "<stdout log not available>".to_string());
    let last_error = last_error.unwrap_or_else(|| "unknown".to_string());
    panic!(
        "server did not start\nstdout:\n{}\nstderr:\n{}\nlast error: {}",
        stdout_log,
        stderr_log,
        last_error
    );
}

///
/// HTTPクライアントを生成する
///
/// # 戻り値
/// HTTPクライアント
///
#[allow(dead_code)]
pub fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_millis(7000))
        .build()
        .expect("client build failed")
}

///
/// テスト実行バイナリを取得する
///
/// # 戻り値
/// 実行バイナリのパス
///
#[allow(dead_code)]
pub fn test_binary_path() -> PathBuf {
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
