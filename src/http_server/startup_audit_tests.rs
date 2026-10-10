/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 共通監査基盤の起動だけを隔離fixtureで検証する
//!

use std::fs;
use std::net::TcpListener;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use actix_web::web;
use chrono::Utc;
use reqwest::Client;
use serde_json::json;
use tempfile::tempdir;

use super::{AuditLogConfig, create_server, initialize_audit_sink};
use crate::audit::model::AuditRecord;
use crate::audit::rotation::active_log_path;
use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::database::types::{BearerScopeSet, PathPrefixSet};
use crate::fts::FtsIndexConfig;
use crate::http_server::app_state::AppState;
use crate::mcp::McpEndpoint;
use crate::mcp::session_manager::ManagedSessionManager;

///
/// 既存設定だけで起動fixtureの監査設定を作る
///
/// # 引数
/// * `path` - 隔離出力先
///
/// # 戻り値
/// 正値の既存設定。
///
fn config(path: &std::path::Path) -> AuditLogConfig {
    AuditLogConfig::new(path.to_path_buf(), chrono::Duration::hours(1), 1024)
}

///
/// MCP有効/無効の両方でRESTを記録し同ArcへMCPも記録する
///
/// # 概要
/// 製品と同じinitializer/state/serverで隔離HTTPを実行する。
///
/// # 注記
/// 稼働中にfixtureが明示flushし停止flushの証拠とは扱わない。
///
#[actix_web::test]
async fn audit_startup_mcp_modes_share_prepared_sink_and_jsonl() {
    for enabled in [false, true] {
        /*
         * MCP条件なしのinitializerを両modeで使う
         */
        let dir = tempdir().unwrap();
        let output = dir.path().join("audit");
        let sink = initialize_audit_sink(Some(&config(&output))).unwrap();
        assert!(output.is_dir());
        assert!(active_log_path(&output).is_file());
        let db = DatabaseManager::open(
            dir.path().join("db"), dir.path().join("assets"),
        ).unwrap();
        db.add_user("user", "pass", None).unwrap();
        db.ensure_default_root("user").unwrap();
        let (token, _) = db.create_bearer_token(
            "user", BearerScopeSet::all(), PathPrefixSet::new(),
            chrono::Duration::hours(1), None,
        ).unwrap();
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            db, FrontendConfig::default(),
            FtsIndexConfig::new(dir.path().join("fts")),
            None, "fixture".into(), None, 1024, Some(sink.clone()),
        ))));
        assert!(Arc::ptr_eq(&sink,
            &state.read().unwrap().audit_sink().unwrap()));

        /*
         * 実endpointの有無だけを切り替えて既存serverを作る
         */
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let endpoint =
            enabled.then(|| McpEndpoint::new("/mcp", "fixture".into()));
        let manager = enabled.then(ManagedSessionManager::new);
        let server = create_server(
            "127.0.0.1".into(), address.port(), state,
            1024, false, std::path::PathBuf::new(), false, None,
            endpoint, manager,
        ).unwrap();
        let handle = server.handle();
        let task = actix_web::rt::spawn(server);
        let client = Client::builder().timeout(Duration::from_secs(10))
            .build().unwrap();
        let base = format!("http://{address}");
        let response = client.get(format!("{base}/api/hello"))
            .basic_auth("user", Some("pass")).send().await.unwrap();
        assert_eq!(response.status(), 200);

        /*
         * MCP有効時は実handshakeとget_pageを同じstateで実行する
         */
        if enabled {
            let body = json!({
                "jsonrpc":"2.0", "id":1, "method":"initialize",
                "params":{"protocolVersion":"2025-03-26","capabilities":{},
                    "clientInfo":{"name":"fixture","version":"1"}}
            });
            let response = client.post(format!("{base}/mcp"))
                .bearer_auth(token.expose())
                .header("Accept", "application/json, text/event-stream")
                .header("Content-Type", "application/json")
                .body(body.to_string()).send().await.unwrap();
            assert_eq!(response.status(), 200);
            let session = response.headers().get("mcp-session-id")
                .unwrap().to_str().unwrap().to_string();
            response.text().await.unwrap();
            let response = client.post(format!("{base}/mcp"))
                .bearer_auth(token.expose())
                .header("Accept", "application/json, text/event-stream")
                .header("mcp-session-id", &session)
                .header("Content-Type", "application/json")
                .body(json!({"jsonrpc":"2.0",
                    "method":"notifications/initialized","params":{}})
                    .to_string())
                .send().await.unwrap();
            assert_eq!(response.status(), 202);
            let response = client.post(format!("{base}/mcp"))
                .bearer_auth(token.expose())
                .header("Accept", "application/json, text/event-stream")
                .header("mcp-session-id", &session)
                .header("Content-Type", "application/json")
                .body(json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                    "params":{"name":"get_page","arguments":{"path":"/"}}})
                    .to_string())
                .send().await.unwrap();
            assert_eq!(response.status(), 200);
            response.text().await.unwrap();
        } else {
            assert_eq!(client.get(format!("{base}/mcp")).send().await.unwrap()
                .status(), 404);
        }

        /*
         * 稼働中の明示fixture flushで同じJSONLへの混在を確認する
         */
        sink.write().unwrap().flush().unwrap();
        let rows: Vec<AuditRecord> =
            fs::read_to_string(active_log_path(&output)).unwrap().lines()
                .map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(rows.len(), if enabled { 2 } else { 1 });
        assert_eq!(rows[0].operation.as_str(), "GET /api/hello");
        assert_eq!(rows[0].token_id, None);
        if enabled {
            assert_eq!(rows[1].operation.as_str(), "get");
            assert!(rows[1].token_id.is_some());
        }
        fs::write(
            format!("/tmp/ms4-5-1-mode-{enabled}-records.json"),
            serde_json::to_string_pretty(&rows).unwrap(),
        ).unwrap();

        /*
         * fixture serverだけを掃除する。製品終了flushは未接続
         */
        handle.stop(true).await;
        task.await.unwrap().unwrap();
    }
}

///
/// 新規出力先の準備と既存active appendを維持する
///
/// # 概要
/// initializerのprepareが既存active内容をtruncateしないことを検証する。
///
/// # 注記
/// 空dirと既存sentinelを用いて起動前後のbyte列を比較する。
///
#[test]
fn audit_startup_prepares_dir_and_preserves_existing_active_bytes() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("new/audit");
    let writer = crate::audit::writer::AuditWriter::new(
        crate::audit::writer::AuditWriterConfig {
            output_dir: output.clone(),
            rotation_policy: crate::audit::rotation::AuditRotationPolicy::new(
                1024,
            ),
        },
    );
    assert!(!output.exists());
    drop(writer);
    let sink = initialize_audit_sink(Some(&config(&output))).unwrap();
    assert!(output.is_dir());
    assert_eq!(fs::read(active_log_path(&output)).unwrap(), b"");
    drop(sink);
    fs::write(active_log_path(&output), b"existing active bytes\n").unwrap();
    let sink = initialize_audit_sink(Some(&config(&output))).unwrap();
    assert_eq!(fs::read(active_log_path(&output)).unwrap(),
        b"existing active bytes\n");
    drop(sink);
}

///
/// 起動時保持は期限超過だけを削除しactive/規則外を温存する
///
/// # 概要
/// 既存保持plan/executeを再利用し周期taskを追加しない。
///
/// # 注記
/// old/new rotatedとactive/unmanagedの4種類を比較する。
///
#[test]
fn audit_startup_retention_keeps_active_and_unmanaged_files() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("audit");
    fs::create_dir_all(&output).unwrap();
    let old = output.join("audit-20000101T000000Z-000001.jsonl");
    let fresh = output.join(format!(
        "audit-{}-000002.jsonl", Utc::now().format("%Y%m%dT%H%M%SZ"),
    ));
    let unmanaged = output.join("private_unmanaged_name");
    fs::write(&old, b"expired").unwrap();
    fs::write(&fresh, b"fresh").unwrap();
    fs::write(&unmanaged, b"unmanaged").unwrap();
    fs::write(active_log_path(&output), b"active").unwrap();
    let _sink = initialize_audit_sink(Some(&config(&output))).unwrap();
    assert!(!old.exists());
    assert!(fresh.exists());
    assert_eq!(fs::read(&unmanaged).unwrap(), b"unmanaged");
    assert_eq!(fs::read(active_log_path(&output)).unwrap(), b"active");
}

///
/// 設定不足とdir/open障害を固定診断の起動Errとして返す
///
/// # 概要
/// 成功起動後のrequest障害へ先送りせず起動時に拒否する。
///
/// # 注記
/// config None/既存正値制約/file位置/active directoryを比較する。
///
#[test]
fn audit_startup_config_and_output_failures_are_propagated_without_paths() {
    assert_eq!(initialize_audit_sink(None).unwrap_err().to_string(),
        "audit config missing");
    let dir = tempdir().unwrap();
    for (retention, size) in [
        (chrono::Duration::zero(), 1024),
        (chrono::Duration::hours(1), 0),
    ] {
        let invalid = AuditLogConfig::new(dir.path().into(), retention, size);
        let error = initialize_audit_sink(Some(&invalid)).unwrap_err();
        assert_eq!(error.to_string(),
            "audit config invalid");
    }
    let blocked = dir.path().join("private_output_path");
    fs::write(&blocked, b"file").unwrap();
    let error = initialize_audit_sink(Some(&config(&blocked))).unwrap_err();
    assert_eq!(error.to_string(), "audit output preparation failed");
    assert!(!format!("{error:#}").contains("private_output_path"));
    let output = dir.path().join("audit");
    fs::create_dir_all(active_log_path(&output)).unwrap();
    assert_eq!(initialize_audit_sink(Some(&config(&output)))
        .unwrap_err().to_string(), "audit output preparation failed");
}


///
/// run入口もMCP有無によらず設定欠損を起動Resultへ伝播する
///
/// # 概要
/// initializerのErrをserver bindより前にそのまま返す。
///
/// # 注記
/// 既存run関数をmode別の隔離DBで呼び、サーバを稼働させない。
///
#[test]
fn audit_startup_run_missing_config_fails_for_both_mcp_modes() {
    for enabled in [false, true] {
        /*
         * 実runへ欠損設定を渡し、初期化失敗の伝播を確認する
         */
        let dir = tempdir().unwrap();
        let db = DatabaseManager::open(
            dir.path().join("db"), dir.path().join("assets"),
        ).unwrap();
        let result = super::run(
            "127.0.0.1".into(), 0, db, FrontendConfig::default(),
            FtsIndexConfig::new(dir.path().join("fts")),
            None, "fixture".into(), None, 1024, false,
            std::path::PathBuf::new(), false, None, None,
            enabled.then(|| McpEndpoint::new("/mcp", "fixture".into())),
            None, None,
        );
        assert_eq!(result.unwrap_err().to_string(), "audit config missing");
    }
}
