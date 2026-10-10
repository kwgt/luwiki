/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 保持周期・同期MCP barrier・終了失敗を隔離fixtureで検証する
//!

use std::fs;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::Utc;
use tempfile::tempdir;
use tokio::sync::watch;

use super::*;
use crate::audit::model::{AuditOperation, AuditRecord, AuditResult};
use crate::database::types::UserId;

///
/// 共通sinkを既存設定で準備する
///
/// # 引数
/// * path - 隔離出力先
///
/// # 戻り値
/// 共通sink。
///
fn sink(path: &std::path::Path) -> Arc<RwLock<AuditSink>> {
    initialize_audit_sink(Some(&AuditLogConfig::new(
        path.into(), ChronoDuration::hours(1), 1024 * 1024,
    ))).unwrap()
}

///
/// 期限超過fileを置く
///
/// # 引数
/// * path - 隔離出力先
///
/// # 戻り値
/// 期限超過file。
///
fn old_log(path: &std::path::Path) -> PathBuf {
    let file = path.join("audit-20000101T000000Z-000001.jsonl");
    fs::write(&file, b"expired").unwrap();
    file
}

///
/// 保持dir障害後も次周期へ進み停止通知でjoinできる
///
/// # 概要
/// 固定60秒の製品入口と内部短周期fixtureを区別する。
///
/// # 注記
/// 失敗dirを復旧して削除継続を確認し、終了後は処理が増えない。
///
#[actix_web::test]
async fn lifecycle_retention_recovers_and_stops_without_append_flush() {
    /*
     * 製品周期は60秒、fixtureだけ短い同じloopへ接続する
     */
    assert_eq!(AUDIT_MAINTENANCE_SECS, 60);
    let dir = tempdir().unwrap();
    let output = dir.path().join("audit");
    let shared = sink(&output);
    fs::remove_dir_all(&output).unwrap();
    let (stop, receiver) = watch::channel(false);
    let task = tokio::spawn(audit_retention_loop(
        shared.clone(), AuditRetentionPolicy::new(ChronoDuration::hours(1)),
        receiver, Duration::from_millis(20),
    ));
    time::sleep(Duration::from_millis(50)).await;
    assert!(!task.is_finished());

    /*
     * directory復旧後の次周期がexpiredだけを処理する
     */
    fs::create_dir_all(&output).unwrap();
    let old = old_log(&output);
    fs::write(output.join("unmanaged"), b"kept").unwrap();
    time::timeout(Duration::from_secs(2), async {
        while old.exists() { time::sleep(Duration::from_millis(10)).await; }
    }).await.unwrap();
    assert!(output.join("unmanaged").exists());

    /*
     * 完了join後に周期処理が新fileへ到達しない
     */
    stop.send(true).unwrap();
    time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    let after = old_log(&output);
    time::sleep(Duration::from_millis(50)).await;
    assert!(after.exists());
}

///
/// 個別削除失敗/lock poisonを業務応答の外で継続処理する
///
/// # 概要
/// 既存plan/executeへportableなfilesystem障害を注入する。
///
/// # 注記
/// plan生成後のfile→directory差替えで一件失敗しても次候補を削除する。
///
#[test]
fn lifecycle_retention_individual_failure_and_poison_are_nonfatal() {
    /*
     * plan生成と実削除の間に候補をdirectoryへ替える
     */
    let dir = tempdir().unwrap();
    let output = dir.path().join("audit");
    let shared = sink(&output);
    let bad = old_log(&output);
    let good = output.join("audit-20000101T000000Z-000002.jsonl");
    fs::write(&good, b"expired").unwrap();
    let plan = build_retention_plan(&output,
        &AuditRetentionPolicy::new(ChronoDuration::hours(1)), Utc::now(),
    ).unwrap();
    fs::remove_file(&bad).unwrap();
    fs::create_dir(&bad).unwrap();
    let result = execute_retention_plan(plan);
    assert_eq!(result.failed_files, vec![bad]);
    assert_eq!(result.deleted_files, vec![good]);

    /*
     * poisoned audit lockは固定診断でfalseを返しpanicしない
     */
    let clone = shared.clone();
    let _ = std::thread::spawn(move || {
        let _guard = clone.write().unwrap();
        panic!("fixture audit lock poison");
    }).join();
    assert!(!audit_maintenance_tick(&shared,
        &AuditRetentionPolicy::new(ChronoDuration::hours(1))));
}

///
/// 元server errorと元writer flush errorを同時に保持する
///
/// # 概要
/// 固定Displayと原因保持を区別しどちらも終了Resultで観測する。
///
/// # 注記
/// server Errでもflushを一度試みnative io errorのdowncastを維持する。
///
#[test]
fn lifecycle_shutdown_preserves_server_and_native_flush_errors() {
    /*
     * writer最終flush障害を注入する
     */
    let dir = tempdir().unwrap();
    let shared = sink(dir.path());
    shared.write().unwrap().inject_shutdown_failure(None, false, true);
    let error = complete_shutdown(
        Err(anyhow!("private server cause")), None, Ok(()), &shared,
    ).unwrap_err();
    let failure = error.downcast_ref::<ShutdownFailure>().unwrap();
    assert_eq!(failure.server.as_ref().unwrap().to_string(),
        "private server cause");
    let audit = failure.audit.as_ref().unwrap()
        .downcast_ref::<crate::audit::sink::AuditShutdownFailure>().unwrap();
    let native = audit.flush.as_ref().unwrap()
        .downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(native.to_string(), "injected audit writer flush failure");
    assert_eq!(shared.read().unwrap().shutdown_attempts().1, 1);
    assert!(!format!("{error:#}").contains("private server cause"));
    assert!(!format!("{error:#}").contains("injected"));

    /*
     * sink lock障害も正常終了として扱わない
     */
    let shared = sink(&dir.path().join("poison"));
    let clone = shared.clone();
    let _ = std::thread::spawn(move || {
        let _guard = clone.write().unwrap();
        panic!("fixture audit lock poison");
    }).join();
    assert!(complete_shutdown(Ok(()), None, Ok(()), &shared).is_err());
}

///
/// 起動callback/bind失敗でもtaskを終了し最終flushへ到達する
///
/// # 概要
/// 自前runtimeを持つrunを専用threadで呼び停止hangを検出する。
///
/// # 注記
/// callback Errとbind Errは実server起動前提の異なる失敗として扱う。
///
#[test]
fn lifecycle_startup_callback_and_bind_failures_finish_cleanup() {
    for bind_failure in [false, true] {
        /*
         * 専用threadのrun結果を時間上限付きで受け取る
         */
        let dir = tempdir().unwrap();
        let db = DatabaseManager::open(
            dir.path().join("db"), dir.path().join("assets"),
        ).unwrap();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        if !bind_failure { drop(listener); }
        let output = dir.path().join("audit");
        let fts = FtsIndexConfig::new(dir.path().join("fts"));
        let config = AuditLogConfig::new(output.clone(),
            ChronoDuration::hours(1), 1024);
        let (done, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result = run("127.0.0.1".into(), port, db,
                FrontendConfig::default(), fts, None, "fixture".into(),
                None, 1024, false, PathBuf::new(), false, None,
                Some(config), None, None, Some(Arc::new(|| {
                    Err(anyhow!("fixture startup callback failure"))
                })));
            done.send(result).unwrap();
        });
        let result = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        let error = result.unwrap_err();
        let failure = error.downcast_ref::<ShutdownFailure>().unwrap();
        assert_eq!(failure.server.is_some(), bind_failure);
        assert_eq!(failure.startup.is_some(), !bind_failure);
        assert!(failure.audit.is_none());
        assert!(failure.background.is_none());
        thread.join().unwrap();
        assert!(crate::audit::rotation::active_log_path(&output).is_file());
    }
}

///
/// MCP同期業務/record終了を待ちqueued処理をflush後に実行しない
///
/// # 概要
/// 実with_state_read入口へ同期barrierを使いlock順序を検証する。
///
/// # 注記
/// state.writeはinflight recordを待ち、終了flag後のclosureは呼ばれない。
///
#[test]
fn lifecycle_mcp_state_barrier_prevents_records_after_final_flush() {
    /*
     * 実MCP入口と共通state/sinkを準備する
     */
    let dir = tempdir().unwrap();
    let shared = sink(&dir.path().join("audit"));
    let db = DatabaseManager::open(
        dir.path().join("db"), dir.path().join("assets"),
    ).unwrap();
    let state = Arc::new(RwLock::new(AppState::new(
        db, FrontendConfig::default(),
        FtsIndexConfig::new(dir.path().join("fts")),
        None, "fixture".into(), None, 1024, Some(shared.clone()),
    )));
    let server =
        Arc::new(crate::mcp::server::LuwikiMcpServer::new(state.clone()));
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let active = server.clone();
    let worker = std::thread::spawn(move || {
        active.with_state_read(|state| {
            entered.send(()).unwrap();
            waiting.recv().unwrap();
            state.audit_sink().unwrap().write().unwrap().record(
                AuditRecord::new(
                AuditOperation::Get, UserId::new(), None, None, None,
                AuditResult::Success, Utc::now(), None, None,
            )).unwrap();
            Ok(())
        }).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();

    /*
     * write barrierは同期MCP処理とそのrecordの終了まで待つ
     */
    let (closed, is_closed) = std::sync::mpsc::channel();
    let closing = state.clone();
    let gate = std::thread::spawn(move || {
        closing.write().unwrap().begin_shutdown();
        closed.send(()).unwrap();
    });
    assert!(is_closed.recv_timeout(Duration::from_millis(50)).is_err());
    release.send(()).unwrap();
    worker.join().unwrap();
    gate.join().unwrap();
    is_closed.recv_timeout(Duration::from_secs(2)).unwrap();
    complete_shutdown(Ok(()), None, Ok(()), &shared).unwrap();
    let path = crate::audit::rotation::active_log_path(
        &dir.path().join("audit"));
    let before = fs::read(&path).unwrap();

    /*
     * queued入口は業務もrecordも呼ばず、flush後のbyte列が変わらない
     */
    assert!(server.with_state_read::<(), _>(|_| {
        panic!("queued MCP must not execute after shutdown");
    }).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}
