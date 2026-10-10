/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 隔離実binaryのSIGTERM正常停止と最終JSONLを検証する
//!
#![cfg(unix)]

mod common;

use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde_json::{Value, json};

use common::{
    TEST_PASSWORD, TEST_USERNAME, prepare_test_dirs, reserve_port,
    run_add_user, run_create_token, test_binary_path,
};

///
/// 強制killとは区別して明示停止結果を返すfixture server
///
struct GracefulFixture {
    /// このfixtureだけが所有するchild
    child: Option<Child>,
}

impl GracefulFixture {
    ///
    /// 既存設定で隔離serverを起動する
    ///
    /// # 引数
    /// * port - fixture待受port
    /// * db - fixture DB
    /// * assets - fixture assets
    /// * audit - fixture監査出力
    /// * mcp - MCP公開有無
    ///
    /// # 戻り値
    /// fixture所有child。
    ///
    /// # 概要
    /// 実設定・stdout/stderrをfixture領域だけへ閉じ込める。
    ///
    fn start(port: u16, db: &Path, assets: &Path, audit: &Path, mcp: bool)
        -> Self
    {
        /*
         * 実運用configへfallbackせず既存CLI設定を使用する
         */
        let base = fs::canonicalize(db.parent().unwrap()).unwrap();
        let config_dir = base.join(env!("CARGO_PKG_NAME"));
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.toml"),
            "[run]\nuse_tls = false\n").unwrap();
        let child = Command::new(test_binary_path())
            .env("XDG_CONFIG_HOME", &base).env("XDG_DATA_HOME", &base)
            .arg("--config-path").arg(config_dir.join("config.toml"))
            .arg("--db-path").arg(db).arg("--assets-path").arg(assets)
            .arg("--fts-index").arg(base.join("fts"))
            .arg("--audit-log-dir").arg(audit)
            .arg("run").arg(format!("127.0.0.1:{port}"))
            .arg(format!("--mcp={mcp}"))
            .stdin(Stdio::null())
            .stdout(File::create(base.join("graceful.stdout.log")).unwrap())
            .stderr(File::create(base.join("graceful.stderr.log")).unwrap())
            .spawn().unwrap();
        Self { child: Some(child) }
    }

    ///
    /// fixture childへSIGTERMを送り期限内ExitStatusを待つ
    ///
    /// # 引数
    /// * timeout - 正常停止の上限
    ///
    /// # 戻り値
    /// 正常wait結果。timeout時は掃除後test失敗。
    ///
    /// # 概要
    /// kill/Dropをflush成功の証拠として扱わない。
    ///
    fn stop_gracefully_and_wait(&mut self, timeout: Duration) -> ExitStatus {
        /*
         * child PIDだけへ既存Actix Unix正常停止signalを送る
         */
        let child = self.child.as_mut().unwrap();
        assert!(Command::new("kill").arg("-TERM")
            .arg(child.id().to_string()).status().unwrap().success());
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                self.child.take();
                return status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                self.child.take();
                panic!("fixture graceful shutdown timeout; kill is cleanup");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for GracefulFixture {
    ///
    /// 異常終了したfixtureだけを強制掃除する
    ///
    /// # 戻り値
    /// なし。正常停止証拠として使用しない。
    ///
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

///
/// fixture serverの実hello成功を待つ
///
/// # 引数
/// * client - HTTP client
/// * base - fixture server URL
///
/// # 戻り値
/// なし。上限超過はtest失敗。
///
fn wait_ready(client: &Client, base: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let last_error = match client.get(format!("{base}/api/hello"))
            .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD)).send()
        {
            Ok(response) if response.status().is_success() => return,
            Ok(response) => response.status().to_string(),
            Err(error) => error.to_string(),
        };
        assert!(Instant::now() < deadline,
            "fixture server did not start: {last_error}");
        thread::sleep(Duration::from_millis(30));
    }
}

///
/// MCP RPC応答bodyまで消費し要求完了を確認する
///
/// # 引数
/// * client - HTTP client
/// * base - fixture server URL
/// * token - fixture Bearer平文
/// * session - session ID（initializeではNone）
/// * body - 固定RPC/fixture入力
///
/// # 戻り値
/// session headerと有限応答body。
///
fn rpc(
    client: &Client, base: &str, token: &str,
    session: Option<&str>, body: Value,
) -> (Option<String>, String) {
    let mut request = client.post(format!("{base}/mcp"))
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json").body(body.to_string());
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request.send().unwrap();
    assert!(response.status().is_success());
    let session = response.headers().get("mcp-session-id")
        .map(|value| value.to_str().unwrap().to_string());
    (session, response.text().unwrap())
}

///
/// MCP off/onの正常停止後にRESTと保留appendが残る
///
/// # 概要
/// 実binaryへSIGTERMを送り正常ExitStatus後にJSONLを読む。
///
/// # 注記
/// MCP append2件は稼働中にflushせず、停止後一集約だけを確認する。
///
#[test]
fn api_audit_sigterm_flushes_rest_and_pending_mcp_append_for_both_modes() {
    for mcp in [false, true] {
        /*
         * build済み絶対binaryと隔離config/DB/assets/FTS/auditを使う
         */
        let (base, db, assets) = prepare_test_dirs();
        run_add_user(&db, &assets);
        let token = run_create_token(&db, &assets, "write");
        let audit = base.join("audit");
        let port = reserve_port();
        let mut server =
            GracefulFixture::start(port, &db, &assets, &audit, mcp);
        let client = Client::builder().timeout(Duration::from_secs(10))
            .build().unwrap();
        let url = format!("http://127.0.0.1:{port}");
        wait_ready(&client, &url);

        /*
         * 成功REST要求を実行し、MCP用のsourceを既存手順で作る
         */
        if mcp {
            let response = client.post(format!(
                "{url}/api/pages?path=/shutdown/page"))
                .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
                .body("").send().unwrap();
            assert!(response.status().is_success());
            let lock = response.headers().get("X-Page-Lock").unwrap()
                .to_str().unwrap().to_string();
            let body: Value = serde_json::from_str(&response.text().unwrap())
                .unwrap();
            let id = body["id"].as_str().unwrap();
            let response = client.put(format!("{url}/api/pages/{id}/source"))
                .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
                .header("Content-Type", "text/markdown")
                .header("X-Lock-Authentication", lock)
                .body("initial source").send().unwrap();
            assert!(response.status().is_success());

            /*
             * handshake後の2appendをbody完了まで待ち保留のまま止める
             */
            let (session, _) = rpc(&client, &url, &token, None, json!({
                "jsonrpc":"2.0","id":1,"method":"initialize",
                "params":{"protocolVersion":"2025-03-26","capabilities":{},
                    "clientInfo":{"name":"fixture","version":"1"}}
            }));
            let session = session.unwrap();
            rpc(&client, &url, &token, Some(&session), json!({
                "jsonrpc":"2.0","method":"notifications/initialized",
                "params":{}
            }));
            for id in [2, 3] {
                let (_, body) = rpc(
                    &client, &url, &token, Some(&session), json!({
                    "jsonrpc":"2.0","id":id,"method":"tools/call",
                    "params":{"name":"append_page","arguments":{
                        "path":"/shutdown/page",
                        "content":"private append body"}}
                }));
                assert!(!body.contains("\"isError\":true"));
                assert!(!body.contains("\"error\":"));
            }
            let before = fs::read_to_string(audit.join("audit.current.jsonl"))
                .unwrap();
            assert!(!before.contains("\"operation\":\"append\""));
        }

        /*
         * SIGTERM→期限付き正常wait→確定JSONLの順序で検証する
         */
        let status = server.stop_gracefully_and_wait(Duration::from_secs(15));
        assert!(status.success(), "fixture shutdown must return success");
        let content = fs::read_to_string(audit.join("audit.current.jsonl"))
            .unwrap();
        let rows: Vec<Value> = content.lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        fs::write(format!("/tmp/ms4-5-2-4-graceful-{mcp}-exit.json"),
            json!({"mcp":mcp,"exit_code":status.code(),
                "success":status.success(),"records":rows.len()})
                .to_string()).unwrap();
        assert!(rows.iter().any(|row| row["operation"]=="GET /api/hello"));
        if mcp {
            let append = rows.iter().filter(|row| row["operation"]=="append")
                .collect::<Vec<_>>();
            assert_eq!(append.len(), 1);
            assert!(append[0]["summary"].as_str().unwrap().contains("2"));
        } else {
            assert!(!rows.iter().any(|row| row["operation"]=="append"));
        }
        assert!(!content.contains("private append body"));
        fs::write(format!("/tmp/ms4-5-2-4-graceful-{mcp}-records.json"),
            serde_json::to_string_pretty(&rows).unwrap()).unwrap();
        drop(client);
        fs::remove_dir_all(base).unwrap();
    }
}
