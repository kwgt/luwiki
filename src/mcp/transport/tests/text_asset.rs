/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 直接入力アセットのMCP公開境界を検証する
//!

use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;

///
/// session付きtool呼出しのHTTP応答本文を返す
///
/// # 引数
/// * `context` - テストサーバ
/// * `session` - session ID
/// * `body` - JSON-RPC本文
///
/// # 戻り値
/// HTTPステータスと応答本文。
///
async fn post(context: &TestServerContext, session: &str, body: &str)
    -> (u16, String)
{
    let response = context.post_json(body)
        .header("mcp-session-id", session).send().await.unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

///
/// 本文送信と早期413応答の競合を避けてHTTP上限超過を確認する。
///
/// # 引数
/// * `context` - テストサーバ
/// * `session` - session ID
/// * `length` - 宣言する本文長(byte)
///
/// # 戻り値
/// 応答のHTTPステータス。
///
async fn post_oversized_headers(
    context: &TestServerContext, session: &str, length: usize,
) -> u16 {
    /* 接続先と認証付きリクエストヘッダを準備する。 */
    let url = reqwest::Url::parse(&context.base_url).unwrap();
    let host = url.host_str().unwrap();
    let port = url.port_or_known_default().unwrap();
    let mut stream = tokio::net::TcpStream::connect((host, port))
        .await.unwrap();
    let request = format!(concat!(
        "POST {} HTTP/1.1\r\nHost: {}:{}\r\n",
        "Authorization: {}\r\n",
        "Content-Type: application/json\r\nAccept: {}\r\n",
        "Mcp-Session-Id: {}\r\nContent-Length: {}\r\n",
        "Connection: close\r\n\r\n",
    ), url.path(), host, port, context.bearer_token,
        ACCEPT_BOTH, session, length);

    /* 本文は送らず、Content-Length検証による応答を読む。 */
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    tokio::time::timeout(Duration::from_secs(10),
        reader.read_line(&mut status)).await.unwrap().unwrap();
    status.split_whitespace().nth(1).unwrap().parse().unwrap()
}

///
/// 公開呼出しの排他・null・不正入力と本文秘匿を確認する
/// # 概要
/// 公開呼出しの排他・null・不正入力と本文秘匿を確認する
///
///
/// # 注記
/// 型不正はJSON-RPC、意味不正はtoolエラー。保存と監査も照合する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn text_asset_public_input_and_audit() {
    let context = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/mcp"]),
    ).await;
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;

    /*
     * 意味不正は固定toolエラーにする
     */
    for fields in [json!({}), json!({"text":"PRIVATE_TEXT_SENTINEL",
        "data_base64":"PRIVATE_BASE64_SENTINEL"})]
    {
        let mut args = json!({"path":"/mcp/page", "file_name":"bad.txt",
            "mime_type":"text/plain"});
        args.as_object_mut().unwrap().extend(
            fields.as_object().unwrap().clone(),
        );
        let (status, text) = post(&context, &session,
            &build_tool_call_body("upload_page_asset", args)).await;
        assert_eq!(status, 200);
        assert!(!text.contains("PRIVATE_"));
        let (result, payload) = parse_tool_result_payload(&text);
        assert_eq!(result["result"]["isError"], true);
        assert_eq!(payload["code"], "invalid_input");
    }

    /*
     * nullと型不正はhandlerへ到達しない
     */
    for field in ["text", "data_base64"] {
        for value in [json!(null), json!(["PRIVATE_TEXT_SENTINEL"]),
            json!({"private":"PRIVATE_TEXT_SENTINEL"}), json!(1), json!(false)]
        {
            let mut args = json!({"path":"/mcp/page", "file_name":"bad.txt",
                "mime_type":"text/plain"});
            args[field] = value;
            let (_, text) = post(&context, &session,
                &build_tool_call_body("upload_page_asset", args)).await;
            assert!(!text.contains("PRIVATE_TEXT_SENTINEL"));
            assert_eq!(parse_sse_payload(&text)["error"]["code"], -32602);
        }
    }
    let state = context.state.read().unwrap();
    let page = state.db().get_page_id_by_path("/mcp/page").unwrap().unwrap();
    assert!(state.db().get_asset_id_by_page_file(&page, "bad.txt")
        .unwrap().is_none());
    drop(state);
    assert!(!context.read_audit_text().contains("PRIVATE_"));
    context.shutdown().await;
}

///
/// raw上限・JSON escapeとHTTP body上限を通常MCP経路で確認する
/// # 概要
/// raw上限・JSON escapeとHTTP body上限を通常MCP経路で確認する
///
///
/// # 注記
/// 大容量入力は逐次処理し、保存bytesと64MiB超過の413を確認する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn text_asset_http_capacity_boundaries() {
    let mut context = spawn_test_server_with_auth(
        SessionManagerConfig::default(),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/mcp"]),
    ).await;
    context.client = Client::builder().timeout(Duration::from_secs(60))
        .build().unwrap();
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;
    let max = 10 * 1024 * 1024;

    /*
     * 10MiB text・Base64・全量escapeの到達と保存を確認する
     */
    for (name, mode) in [("large.txt", 0), ("binary.bin", 1),
        ("escaped.txt", 2)]
    {
        let raw = vec![if mode == 0 { b'A' } else { 0 }; max];
        let mut args = json!({"path":"/mcp/page", "file_name":name,
            "mime_type":"text/plain"});
        if mode == 1 {
            args["data_base64"] = json!(
                base64::engine::general_purpose::STANDARD.encode(&raw)
            );
        } else { args["text"] = json!(""); }
        let mut body = build_tool_call_body("upload_page_asset", args);
        if mode != 1 {
            let encoded = if mode == 0 { "A".repeat(max) }
                else { "\\u0000".repeat(max) };
            body = body.replace("\"text\":\"\"",
                &format!("\"text\":\"{}\"", encoded));
        }
        let (status, text) = post(&context, &session, &body).await;
        assert_eq!(status, 200);
        let (result, payload) = parse_tool_result_payload(&text);
        assert_eq!(result["result"]["isError"], false);
        assert_eq!(payload["asset"]["size"], max);
        let state = context.state.read().unwrap();
        let page = state.db().get_page_id_by_path("/mcp/page")
            .unwrap().unwrap();
        let id = state.db().get_asset_id_by_page_file(&page, name)
            .unwrap().unwrap();
        assert_eq!(state.db().read_asset_data(&id).unwrap(), raw);
    }

    /*
     * HTTP上限一致は通常処理、1byte超過は413
     */
    let mut body = "{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"tools/list\"}"
        .to_string();
    body.extend(std::iter::repeat_n(' ', 64 * 1024 * 1024 - body.len()));
    let (status, _) = post(&context, &session, &body).await;
    assert_eq!(status, 200);
    let status = post_oversized_headers(
        &context, &session, body.len() + 1,
    ).await;
    assert_eq!(status, 413);
    context.shutdown().await;
}

///
/// 配布スキル掲載JSONをそのまま実行し、取得bytesと監査を照合する
/// # 概要
/// 配布スキル掲載JSONをそのまま実行し、取得bytesと監査を照合する
///
///
/// # 注記
/// 合成サンプル用ページをfixtureにだけ作成する。
///
/// # 戻り値
/// なし。
///
#[actix_web::test]
async fn skill_setup_text_asset_example_round_trips() {
    let context = spawn_test_server_with_audit(
        BearerScopeSet::from_iter([BearerScope::Read, BearerScope::Update]),
        PathPrefixSet::from_iter(["/"]),
    ).await;
    context.state.read().unwrap().db().create_page(
        "/example", "alice", "# example".into(),
    ).unwrap();
    let session = context.initialize_session().await;
    context.send_initialized_notification(&session).await;
    let doc = include_str!("../../../../assets/mcp/agent-skill.md");
    let example = doc.split("Text asset upload example").nth(1).unwrap()
        .split("```json\n").nth(1).unwrap().split("\n```").next().unwrap();
    let args: Value = serde_json::from_str(example).unwrap();
    let expected = args["params"]["arguments"]["text"].as_str().unwrap();
    let (_, response) = post(&context, &session, example).await;
    let (result, payload) = parse_tool_result_payload(&response);
    assert_eq!(result["result"]["isError"], false);
    assert_eq!(payload["asset"]["mime_type"], "text/markdown; charset=utf-8");
    let (_, response) = post(&context, &session, &build_tool_call_body(
        "get_page_asset_data",
        json!({"path":"/example", "file_name":"sample.md"}),
    )).await;
    let (_, payload) = parse_tool_result_payload(&response);
    let bytes = base64::engine::general_purpose::STANDARD.decode(
        payload["data_base64"].as_str().unwrap(),
    ).unwrap();
    assert_eq!(bytes, expected.as_bytes());
    let audit = context.read_audit_text();
    assert!(!audit.contains("# Example"));
    assert!(!audit.contains(payload["data_base64"].as_str().unwrap()));
    context.shutdown().await;
}
