/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 結合テスト用の共通ヘルパー
//!

use std::fs;
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

///
/// 全文検索インデックスのパスを返す
///
/// # 引数
/// * `db_path` - DBパス
///
/// # 戻り値
/// インデックス格納パス
///
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
pub fn reserve_port() -> u16 {
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
pub struct ServerGuard {
    child: Child,
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
    pub fn start(port: u16, db_path: &Path, assets_dir: &Path) -> Self {
        /*
         * サーバ起動
         */
        let exe = test_binary_path();
        let base_dir = db_path
            .parent()
            .expect("db_path parent missing");
        let fts_index = fts_index_path(db_path);
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
/// サーバの起動完了を待機する
///
/// # 引数
/// * `url` - ヘルスチェックURL
///
/// # 戻り値
/// なし
///
#[allow(dead_code)]
pub fn wait_for_server(url: &str) {
    /*
     * 起動確認
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
