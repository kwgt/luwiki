/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 状態取得・変更待ちのHTTP・取消・監査を検証する
//!

use super::*;
use crate::mcp::handler::page_state::poll_count_for_test;

///
/// 一意なidの認証済みtool要求を作る
///
/// # 引数
/// * `name` - tool名
/// * `args` - 公開入力
///
/// # 戻り値
/// JSON-RPC本文。
///
fn body(name: &str, args: Value) -> Value {
    static ID: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(20000);
    json!({"jsonrpc":"2.0", "id":ID.fetch_add(1,
        std::sync::atomic::Ordering::Relaxed), "method":"tools/call",
        "params":{"name":name,"arguments":args}})
}

///
/// tool応答を認証付きHTTPで取得する
///
/// # 引数
/// * `ctx` - 隔離サーバ
/// * `session` - 初期化済みsession
/// * `name` - tool名
/// * `args` - 入力
///
/// # 戻り値
/// tool resultとpayload。
///
async fn call(ctx: &TestServerContext, session: &str,
    name: &str, args: Value) -> (Value, Value)
{
    let response = ctx.post_json(&body(name, args).to_string())
        .header("mcp-session-id", session).send().await.unwrap();
    assert_eq!(response.status(), 200);
    parse_tool_result_payload(&response.text().await.unwrap())
}

///
/// 初回の状態snapshot完了を条件に同期する
///
/// # 引数
/// * `ctx` - 隔離サーバ
/// * `before` - 要求前の回数
///
/// # 戻り値
/// なし。条件を満たさない場合は試験失敗。
///
async fn snapshot_completed(ctx: &TestServerContext, before: usize) {
    let limit = tokio::time::Instant::now() + Duration::from_secs(3);
    while poll_count_for_test(&ctx.bearer_token_id) <= before {
        assert!(tokio::time::Instant::now() < limit, "snapshot not reached");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

///
/// 状態取得・正常timeout・amend・初回消失を確認する
///
/// # 注記
/// 各応答後に確定した変更を入れ、取得／再呼出し間の差を検証する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_timeout_and_gap_changes() {
    let ctx = spawn_test_server_with_auth(SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (r, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/./page"})).await;
    assert_eq!(r["result"]["isError"], false);
    assert_eq!(first["path"], "/mcp/page");
    assert!(first.get("content").is_none() && first.get("page_id").is_none());
    let token = first["state_token"].as_str().unwrap();
    let began = std::time::Instant::now();
    let (r, timed) = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":token, "timeout_seconds":1,
    })).await;
    assert_eq!(r["result"]["isError"], false);
    assert_eq!(timed["status"], "timeout");
    assert_eq!(timed["change"], Value::Null);
    assert_eq!(timed["state"]["state_token"], token);
    assert!(began.elapsed() >= Duration::from_secs(1));

    /* 同じrevisionで本文が同じでもamendを検知する。 */
    {
        let state = ctx.state.read().unwrap();
        let id = state.db().get_page_id_by_path("/mcp/page").unwrap().unwrap();
        let source = state.db().get_page_source(&id, 1).unwrap().unwrap();
        state.db().put_page(&id, "alice", source.source(), true).unwrap();
    }
    let (_, changed) = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":token,
    })).await;
    assert_eq!(changed["change"], "updated");
    assert_eq!(changed["state"]["revision"], first["revision"]);
    {
        let state = ctx.state.read().unwrap();
        state.db().rename_page("/mcp/page", "/mcp/moved").unwrap();
    }
    let (_, gone) = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":token,
    })).await;
    assert_eq!(gone["status"], "changed");
    assert_eq!(gone["change"], "unavailable");
    assert_eq!(gone["state"], Value::Null);
    ctx.shutdown().await;
}

///
/// 待機中のpublic保存と権限変更を初回snapshot後に実行する
///
/// # 注記
/// snapshotカウンタで開始を同期し、長時間read lock保持がないことを確認。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_wait_allows_save_and_rechecks_revocation() {
    let ctx = spawn_test_server_with_auth(SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let wait = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
    }));
    let save = async {
        snapshot_completed(&ctx, before).await;
        call(&ctx, &session, "update_page", json!({
            "path":"/mcp/page", "content":"# saved during wait",
        })).await
    };
    let (waited, saved) = tokio::join!(wait, save);
    assert_eq!(saved.0["result"]["isError"], false);
    assert_eq!(waited.1["change"], "updated");
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let wait = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":waited.1["state"]["state_token"],
    }));
    let revoke = async {
        snapshot_completed(&ctx, before).await;
        ctx.state.read().unwrap().db()
            .revoke_bearer_token_by_id(&ctx.bearer_token_id).unwrap();
    };
    let (denied, _) = tokio::join!(wait, revoke);
    assert_eq!(denied.0["result"]["isError"], true);
    assert_eq!(denied.1["code"], "forbidden");
    ctx.shutdown().await;
}

///
/// 入力境界・scope・prefixとReadOnlyをHTTP経由で確認する
///
/// # 注記
/// 数値のnull／小数も正常timeoutへ変換せず拒否する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_input_boundaries_and_read_only() {
    let ctx = spawn_test_server_with_auth_and_attributes(
        SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"]),
        UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
    ).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    for value in [json!(0), json!(31), json!(-1), json!(1.5), Value::Null] {
        let (r, payload) = call(&ctx, &session, "wait_page_change", json!({
            "path":"/mcp/page", "expected_state":first["state_token"],
            "timeout_seconds":value,
        })).await;
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(payload["code"], "invalid_input");
    }
    for path in ["relative", "/mcp//page", "/mcp/page/", "/mcp/a b"] {
        let (_, payload) = call(&ctx, &session, "get_page_state",
            json!({"path":path})).await;
        assert_eq!(payload["code"], "invalid_input");
    }
    let (_, payload) = call(&ctx, &session, "get_page_state",
        json!({"path":"/private"})).await;
    assert_eq!(payload["code"], "forbidden");
    let (_, payload) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/missing"})).await;
    assert_eq!(payload["code"], "not_found");
    let (_, payload) = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":"v1:invalid",
    })).await;
    assert_eq!(payload["code"], "invalid_input");
    ctx.shutdown().await;
}

///
/// 一要求一件の監査と比較tokenの秘匿を確認する
///
/// # 注記
/// 内部pollが複数回でもtimeout監査は一件だけになる。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_audit_once_and_redacts_token() {
    let ctx = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
        "timeout_seconds":1,
    })).await;
    let records = ctx.read_audit_values();
    assert_eq!(records.iter().filter(|r| r["operation"] == "get_page_state")
        .count(), 1);
    let waits: Vec<_> = records.iter().filter(|r| {
        r["operation"] == "wait_page_change"
    }).collect();
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0]["result"], "success");
    assert!(waits[0]["summary"].as_str().unwrap().contains("timeout"));
    let text = ctx.read_audit_text();
    assert!(!text.contains(first["state_token"].as_str().unwrap()));
    assert!(!text.contains("# page"));
    ctx.shutdown().await;
}

///
/// MCP cancelの終了と監査の二重投入防止を確認する
///
/// # 注記
/// 初回snapshotの完了後に対象idへcancel通知を送り、終了監査を待つ。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_cancel_records_once() {
    let ctx = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let request = body("wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
    }));
    let waiting = async {
        let response = ctx.post_json(&request.to_string())
            .header("mcp-session-id", &session).send().await.unwrap();
        response.text().await
    };
    let cancel = async {
        snapshot_completed(&ctx, before).await;
        let notification = json!({"jsonrpc":"2.0",
            "method":"notifications/cancelled", "params":{
                "requestId":request["id"],"reason":"test"}});
        let response = ctx.post_json(&notification.to_string())
            .header("mcp-session-id", &session).send().await.unwrap();
        assert_eq!(response.status(), 202);
    };
    let _ = tokio::join!(waiting, cancel);
    let limit = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let records = ctx.read_audit_values();
        let waits: Vec<_> = records.iter().filter(|r| {
            r["operation"] == "wait_page_change"
        }).collect();
        if !waits.is_empty() {
            assert_eq!(waits.len(), 1);
            assert_eq!(waits[0]["result"], "operation_denied");
            assert!(waits[0]["summary"].as_str().unwrap()
                .contains("cancelled"));
            break;
        }
        assert!(tokio::time::Instant::now() < limit, "cancel audit missing");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(ctx.state.read().unwrap().page_state_request_count(), 0);
    ctx.shutdown().await;
}

///
/// 終了barrier後の待機と新schema・annotationsを検証する
///
/// # 注記
/// 期限のschemaと取消可能な待機の停止を実handlerで確認する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_shutdown_and_tool_schema() {
    let ctx = spawn_test_server_with_auth(SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let response = ctx.post_json(&json!({"jsonrpc":"2.0","id":80000,
        "method":"tools/list"}).to_string())
        .header("mcp-session-id", &session).send().await.unwrap();
    let tools = parse_sse_payload(&response.text().await.unwrap());
    let tools = tools["result"]["tools"].as_array().unwrap();
    let tool = tools.iter().find(|t| t["name"] == "wait_page_change").unwrap();
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    let timeout = &tool["inputSchema"]["properties"]["timeout_seconds"];
    assert_eq!(timeout["type"], "integer");
    assert_eq!(timeout["minimum"], 1);
    assert_eq!(timeout["maximum"], 30);
    assert_eq!(timeout["default"], 30);
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let waiting = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
    }));
    let stop = async {
        snapshot_completed(&ctx, before).await;
        ctx.state.write().unwrap().begin_shutdown();
    };
    let (response, _) = tokio::join!(waiting, stop);
    assert_eq!(response.0["result"]["isError"], true);
    assert_eq!(response.1["code"], "internal_error");
    assert_eq!(ctx.state.read().unwrap().page_state_request_count(), 0);
    assert!(ctx.state.read().unwrap().begin_page_state_request().is_none());
    ctx.shutdown().await;
}

///
/// session closeとHTTP切断後も要求・監査が終了する
///
/// # 注記
/// HTTP切断はcancel伝播を前提にせず短い期限による終了も許可する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_session_close_and_disconnect_release_requests() {
    let ctx = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let request = body("wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
        "timeout_seconds":1,
    }));
    /* 初回snapshot到達後にHTTP応答bodyを破棄する。 */
    let response = ctx.post_json(&request.to_string())
        .header("mcp-session-id", &session).send().await.unwrap();
    snapshot_completed(&ctx, before).await;
    drop(response);
    let limit = tokio::time::Instant::now() + Duration::from_secs(3);
    while ctx.state.read().unwrap().page_state_request_count() != 0 {
        assert!(tokio::time::Instant::now() < limit);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    /* session自体の終了後もguardが残存しないことを確認する。 */
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let request = body("wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
        "timeout_seconds":1,
    }));
    let response = ctx.post_json(&request.to_string())
        .header("mcp-session-id", &session).send().await.unwrap();
    snapshot_completed(&ctx, before).await;
    let closed = ctx.delete_session().header("mcp-session-id", &session)
        .send().await.unwrap();
    assert_eq!(closed.status(), 204);
    drop(response);
    let limit = tokio::time::Instant::now() + Duration::from_secs(3);
    while ctx.state.read().unwrap().page_state_request_count() != 0 {
        assert!(tokio::time::Instant::now() < limit);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(ctx.read_audit_values().iter().filter(|r| {
        r["operation"] == "wait_page_change"
    }).count(), 2);
    ctx.shutdown().await;
}

///
/// LRU eviction後も待機leaseが終了することを確認する
///
/// # 注記
/// 1session上限で2つ目を初期化し、最初の要求と監査の終了を待つ。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_lru_releases_wait_request() {
    let ctx = spawn_test_server_with_auth(SessionManagerConfig::new(
        Duration::from_secs(60), Duration::from_secs(60), 1,
    ), BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let request = body("wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
        "timeout_seconds":1,
    }));
    let response = ctx.post_json(&request.to_string())
        .header("mcp-session-id", &session).send().await.unwrap();
    snapshot_completed(&ctx, before).await;
    let second = ctx.initialize_session().await;
    assert_ne!(second, session);
    drop(response);
    let limit = tokio::time::Instant::now() + Duration::from_secs(3);
    while ctx.state.read().unwrap().page_state_request_count() != 0 {
        assert!(tokio::time::Instant::now() < limit);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    ctx.shutdown().await;
}

///
/// 期限後に完了する最終snapshotでも観測した変更を優先する
///
/// # 注記
/// 初回snapshot後にstate write guardで次の取得を止め、期限後に保存して解放する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_final_snapshot_prefers_change_after_deadline() {
    let ctx = spawn_test_server_with_auth(SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    let (_, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    let before = poll_count_for_test(&ctx.bearer_token_id);
    let waiting = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
        "timeout_seconds":2,
    }));
    let update = async {
        snapshot_completed(&ctx, before).await;
        let state = ctx.state.write().unwrap();
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let id = state.db().get_page_id_by_path("/mcp/page").unwrap().unwrap();
        state.db().put_page(&id, "alice", "# late snapshot".into(), false)
            .unwrap();
        drop(state);
    };
    let (response, _) = tokio::join!(waiting, update);
    assert_eq!(response.1["status"], "changed");
    assert_eq!(response.1["change"], "updated");
    ctx.shutdown().await;
}

///
/// 状態toolの監査書込失敗が正常結果を変更しない
///
/// # 注記
/// 監査ディレクトリに相当する位置をfileで塞ぎ、実際のsink失敗を発生させる。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn page_state_audit_failure_does_not_change_result() {
    let ctx = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = ctx.initialize_session().await;
    ctx.send_initialized_notification(&session).await;
    std::fs::write(&ctx.audit_dir, b"fixture blocks audit directory").unwrap();
    let (result, first) = call(&ctx, &session, "get_page_state",
        json!({"path":"/mcp/page"})).await;
    assert_eq!(result["result"]["isError"], false);
    let (result, waited) = call(&ctx, &session, "wait_page_change", json!({
        "path":"/mcp/page", "expected_state":first["state_token"],
        "timeout_seconds":1,
    })).await;
    assert_eq!(result["result"]["isError"], false);
    assert_eq!(waited["status"], "timeout");
    assert!(ctx.audit_dir.is_file());
    ctx.shutdown().await;
}
