/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! authorityの通常MCP取得・raw取得境界を検証する
//!

use super::*;

///
/// session付きJSON-RPCを送り標準応答を返す
///
/// # 引数
/// * `context` - 起動済みサーバ
/// * `session` - session ID
/// * `body` - JSON-RPC要求
///
/// # 戻り値
/// SSEから取得したJSON-RPC応答。
///
async fn call(
    context: &TestServerContext, session: &str, body: Value,
) -> Value {
    let response = context.post_json(&body.to_string())
        .header("mcp-session-id", session).send().await.unwrap();
    assert_eq!(response.status(), 200);
    parse_sse_payload(&response.text().await.unwrap())
}

///
/// 独自authorityを通常resource/promptで展開し、rawとmetadataを維持する
///
/// # 概要
/// 再起動後相当の独自authorityで標準handshakeから取得を確認する。
///
/// # 注記
/// prompt要求引数と固定組み込み文書は新helperで再処理しない。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn authority_macro_standard_mcp_boundaries() {
    let context = spawn_test_server_with_options(
        SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/"]), UserAttributeSet::new(),
        true, "docs.example.test",
    ).await;
    let raw = concat!("---\nmcp:\n  primitive: resource\n",
        "  resource_path: /test/authority\n  name: reference\n",
        "  description: '{{mcp:authority}}'\n---\n",
        "`luwiki://{{mcp:authority}}/pages/manual`\n");
    {
        let state = context.state.read().unwrap();
        state.db().create_page("/resource", "alice", raw.into()).unwrap();
        state.db().create_page("/prompt", "alice", concat!(
            "---\nmcp:\n  primitive: prompt\n  name: authority\n",
            "  description: example\n",
            "  system: 'System {{mcp:authority}}'\n",
            "  arguments:\n    - name: value\n",
            "      description: value\n      required: true\n",
            "---\n{{mcp:authority}} {{@value}}\n",
        ).into()).unwrap();
    }
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;

    /*
     * resourceとpromptは元sourceだけを展開する
     */
    let resource = call(&context, &session, json!({"jsonrpc":"2.0",
        "id":3, "method":"resources/read", "params":{
            "uri":"luwiki://docs.example.test/test/authority"}})).await;
    assert_eq!(resource["result"]["contents"][0]["text"],
        "`luwiki://docs.example.test/pages/manual`\n");
    let prompt = call(&context, &session, json!({"jsonrpc":"2.0",
        "id":4, "method":"prompts/get", "params":{
            "name":"authority", "arguments":{"value":"{{mcp:authority}}"}
        }})).await;
    assert_eq!(prompt["result"]["messages"][0]["content"]["text"],
        "System docs.example.test\n\ndocs.example.test {{mcp:authority}}\n");
    let list = call(&context, &session, json!({"jsonrpc":"2.0", "id":5,
        "method":"resources/list"})).await;
    assert!(list["result"]["resources"].as_array().unwrap().iter()
        .any(|item| item["description"] == "{{mcp:authority}}"));

    /*
     * rawページ取得と固定組み込み文書はmarkerを保持する
     */
    let body: Value = serde_json::from_str(&build_tool_call_body("get_page",
        json!({"path":"/resource"}))).unwrap();
    let page = call(&context, &session, body).await;
    let payload: Value = serde_json::from_str(
        page["result"]["content"][0]["text"].as_str().unwrap(),
    ).unwrap();
    assert_eq!(payload["content"], raw);
    let builtin = call(&context, &session, json!({"jsonrpc":"2.0", "id":6,
        "method":"resources/read", "params":{
            "uri":"luwiki://docs.example.test/builtin/mcp-prompt-spec"
        }})).await;
    assert_eq!(
        builtin["result"]["contents"][0]["text"],
        include_str!("../../../../assets/mcp/mcp-prompt-spec.md"),
    );
    context.shutdown().await;
}

///
/// 標準リソースと代替ツールの本文・認可・公開形式を比較する。
///
/// # 注記
/// 狭いページprefixとReadOnlyで取得し、独立したappend scopeを拒否する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn agent_instructions_match_standard_resource_and_enforce_scope() {
    /*
     * 狭いprefixとReadOnlyでも固定本文を取得する
     */
    let context = spawn_test_server_with_options(
        SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/only-this-page"]),
        UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
        true,
        "docs.example.test",
    ).await;
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;
    let uri = "luwiki://docs.example.test/builtin/agent-skill";
    let request = json!({"jsonrpc":"2.0", "id":2, "method":"resources/read",
        "params":{"uri":uri}});
    let resource = call(&context, &session, request).await;
    let standard = &resource["result"]["contents"][0];
    assert_eq!(standard["uri"], uri);
    assert_eq!(standard["mimeType"], "text/markdown");
    assert_eq!(
        standard["text"],
        include_str!("../../../../assets/mcp/agent-skill.md"),
    );

    /*
     * 一覧・引数省略・空オブジェクト・未定義引数を確認する
     */
    let listed = call(&context, &session, json!({
        "jsonrpc":"2.0", "id":3, "method":"tools/list",
    })).await;
    let entry = listed["result"]["tools"].as_array().unwrap().iter()
        .find(|t| t["name"] == "get_agent_instructions").unwrap();
    assert_eq!(entry["annotations"]["readOnlyHint"], true);
    assert_eq!(entry["inputSchema"]["additionalProperties"], false);
    for params in [
        json!({"name":"get_agent_instructions"}),
        json!({"name":"get_agent_instructions", "arguments":{}}),
    ] {
        let tool = call(&context, &session, json!({
            "jsonrpc":"2.0", "id":4, "method":"tools/call", "params":params,
        })).await;
        let result = &tool["result"];
        assert_eq!(result["isError"], false);
        assert!(result.get("structuredContent").is_none());
        let payload: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap(),
        ).unwrap();
        assert_eq!(payload["uri"], standard["uri"]);
        assert_eq!(payload["mime_type"], standard["mimeType"]);
        assert_eq!(payload["content"], standard["text"]);
    }
    let invalid = call(&context, &session, json!({
        "jsonrpc":"2.0", "id":5, "method":"tools/call",
        "params":{"name":"get_agent_instructions", "arguments":{"path":"/"}},
    })).await;
    assert_eq!(invalid["error"]["code"], -32602);
    let audit = context.read_audit_values();
    assert_eq!(audit.iter().filter(|record|
        record["operation"] == "get_agent_instructions"
    ).count(), 2);
    assert!(!context.read_audit_text().contains("Wait for answers"));
    context.shutdown().await;

    /*
     * 独立したappend scopeには本文を返さない
     */
    let denied = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Append]),
        PathPrefixSet::from_iter(["/"]),
    ).await;
    let session = denied.initialize_session().await;
    denied.send_initialized_notification(&session).await;
    let standard_denied = call(&denied, &session, json!({
        "jsonrpc":"2.0", "id":6, "method":"resources/read",
        "params":{"uri":"luwiki://local.luwiki/builtin/agent-skill"},
    })).await;
    assert_eq!(standard_denied["error"]["data"]["code"], "forbidden");
    let tool_denied = call(&denied, &session, json!({
        "jsonrpc":"2.0", "id":7, "method":"tools/call",
        "params":{"name":"get_agent_instructions"},
    })).await;
    assert_eq!(tool_denied["result"]["isError"], true);
    let payload: Value = serde_json::from_str(
        tool_denied["result"]["content"][0]["text"].as_str().unwrap(),
    ).unwrap();
    assert_eq!(payload["code"], "forbidden");
    assert!(denied.read_audit_values().iter().any(|record|
        record["operation"] == "get_agent_instructions"
    ));
    denied.shutdown().await;
}
