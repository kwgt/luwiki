/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2026 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 公開MCPの保存・並行実行・全文索引失敗を検証する。
//!

use super::*;

///
/// 認証済みtool呼出しのpayloadを取得する。
///
/// # 概要
/// 並行要求へ一意なidを付け、HTTP応答をtool resultとして解析する。
///
/// # 引数
/// * `context` - 隔離サーバ
/// * `session` - 初期化済みsession
/// * `name` - tool名
/// * `args` - 引数
///
/// # 戻り値
/// tool resultとpayload。
///
async fn call(context: &TestServerContext, session: &str,
    name: &str, args: Value) -> (Value, Value)
{
    /* 並行要求の応答が同じJSON-RPC idへ結合されないようにする。 */
    static NEXT_ID: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(100);
    let mut body: Value = serde_json::from_str(
        &build_tool_call_body(name, args),
    ).unwrap();
    body["id"] = json!(NEXT_ID.fetch_add(1,
        std::sync::atomic::Ordering::Relaxed));
    let response = context.post_json(&body.to_string())
        .header("mcp-session-id", session).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    /* 応答本文だけをtool resultとして解析する。 */
    let text = response.text().await.unwrap_or_else(|error| {
        panic!("read {name} response failed: {error}")
    });
    parse_tool_result_payload(&text)
}

///
/// 指定語の検索payloadを取得する。
///
/// # 引数
/// * `context` - 隔離サーバ
/// * `session` - 初期化済みsession
/// * `query` - 検索式
///
/// # 戻り値
/// search_pagesのpayload。
///
async fn search(context: &TestServerContext, session: &str,
    query: &str) -> Value
{
    let (result, payload) = call(context, session, "search_pages",
        json!({"query":query,"target":["body"]})).await;
    assert_eq!(result["result"]["isError"], false);
    payload
}

///
/// 公開MCP保存後の即時検索と並行追記・再帰移動を確認する。
///
/// # 概要
/// 共有FTS排他と正本/FTS同期を通常handshake経由で確認する。
///
/// # 注記
/// create/update/edit/並行append/親子renameの各応答後に検索する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn fts_sync_public_mutations_and_concurrent_appends() {
    /* 初期化し、作成直後に検索する。 */
    let context = spawn_test_server_with_auth(SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;
    let (result, _) = call(&context, &session, "create_page",
        json!({"path":"/mcp/fts","content":"# FTS\n\ncreatedterm\n"}))
        .await;
    assert_eq!(result["result"]["isError"], false);
    assert_eq!(search(&context, &session, "createdterm").await
        ["items"].as_array().unwrap().len(), 1);

    /* 全文更新/部分編集でも旧語を最新検索から除外する。 */
    call(&context, &session, "update_page",
        json!({"path":"/mcp/fts","content":"# FTS\n\nupdatedterm\n"}))
        .await;
    assert!(search(&context, &session, "createdterm").await
        ["items"].as_array().unwrap().is_empty());
    let (_, current) = call(&context, &session, "get_page",
        json!({"path":"/mcp/fts"})).await;
    let (result, _) = call(&context, &session, "edit_page", json!({
        "path":"/mcp/fts","revision":current["revision"],
        "instance_id":current["instance_id"],"operation":[{
            "type":"replace_text","old_text":"updatedterm",
            "new_text":"editedterm"
        }]
    })).await;
    assert_eq!(result["result"]["isError"], false);
    assert_eq!(search(&context, &session, "editedterm").await
        ["items"].as_array().unwrap().len(), 1);

    /* 並行要求でも両方の追記が残り、writer競合で失敗しない。 */
    let (first, second) = tokio::join!(
        call(&context, &session, "append_page", json!({
            "path":"/mcp/fts","content":"\nfirstparallel\n"})),
        call(&context, &session, "append_page", json!({
            "path":"/mcp/fts","content":"\nsecondparallel\n"})),
    );
    assert_eq!(first.0["result"]["isError"], false);
    assert_eq!(second.0["result"]["isError"], false);
    for query in ["firstparallel", "secondparallel"] {
        assert_eq!(search(&context, &session, query).await
            ["items"].as_array().unwrap().len(), 1);
    }

    /* 子を含む移動の後も新path/latest revisionを検索できる。 */
    call(&context, &session, "create_page", json!({
        "path":"/mcp/fts/child","content":"# Child\n\nchildpublicterm\n"
    })).await;
    let (result, _) = call(&context, &session, "rename_page", json!({
        "path":"/mcp/fts","rename_to":"/mcp/moved"
    })).await;
    assert_eq!(result["result"]["isError"], false);
    let hits = search(&context, &session, "childpublicterm").await;
    assert_eq!(hits["items"][0]["path"], "/mcp/moved/child");
    let (_, current) = call(&context, &session, "get_page",
        json!({"path":"/mcp/moved/child"})).await;
    assert_eq!(hits["items"][0]["revision"], current["revision"]);
    context.shutdown().await;
}

///
/// 索引失敗を固定エラーで返し、保存済み状態と監査秘匿を確認する。
///
/// # 概要
/// 保存後FTS失敗を未保存/成功として誤記録しないことを確認する。
///
/// # 注記
/// 未作成indexを通常ファイルで塞ぎ、create応答・raw再取得・監査を照合する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn fts_sync_public_failure_keeps_saved_source_and_safe_audit() {
    /* 保存後にだけ失敗する索引を準備する。 */
    let context = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/mcp"])).await;
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;
    let index = context.state.read().unwrap().fts_config()
        .index_path().to_path_buf();
    assert!(index.is_dir());
    std::fs::remove_dir(&index).unwrap();
    std::fs::write(&index, b"blocker").unwrap();

    /* 応答は保存後失敗を伝え、本文/内部pathを含めない。 */
    let (result, payload) = call(&context, &session, "create_page", json!({
        "path":"/mcp/saved","content":"PRIVATE_FTS_BODY"
    })).await;
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(payload["code"], "internal_error");
    assert_eq!(payload["message"],
        "page was saved but full-text index update failed");
    let (_, saved) = call(&context, &session, "get_page",
        json!({"path":"/mcp/saved"})).await;
    assert_eq!(saved["content"], "PRIVATE_FTS_BODY");
    assert_eq!(saved["revision"], 1);

    /* 監査は内部エラーを記録し、本文を含めない。 */
    let records = context.read_audit_values();
    assert_eq!(records[0]["operation"], "create");
    assert_eq!(records[0]["result"], "internal_error");
    assert!(!context.read_audit_text().contains("PRIVATE_FTS_BODY"));
    context.shutdown().await;
}
