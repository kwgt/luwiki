/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP transport / endpoint の Actix adapter を定義するモジュール
//!

use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::ErrorInternalServerError;
use actix_web::http::{StatusCode, header};
use actix_web::middleware::{Next, from_fn};
use actix_web::{Error, HttpMessage, HttpResponse, web};
use rmcp_actix_web::transport::StreamableHttpService;
use serde_json::json;

use crate::auth::AuthContext;
use crate::http_server::app_state::AppState;
use crate::mcp::auth::{McpAuthError, McpAuthErrorKind, McpAuthGateway};
use crate::mcp::session_manager::ManagedSessionManager;
use crate::mcp::server::LuwikiMcpServer;

/// MCP endpoint の公開パス
const MCP_ENDPOINT_PATH: &str = "/mcp";

///
/// HTTPサーバ統合層へ渡すMCP endpoint情報
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct McpEndpoint {
    /// endpointの公開パス
    path: &'static str,
}

impl McpEndpoint {
    ///
    /// endpoint情報の生成
    ///
    /// # 引数
    /// * `path` - endpointの公開パス
    ///
    /// # 戻り値
    /// 生成したendpoint情報を返す。
    ///
    pub(crate) fn new(path: &'static str) -> Self {
        Self { path }
    }

    ///
    /// endpointの公開パスを取得
    ///
    /// # 戻り値
    /// endpointの公開パスを返す。
    ///
    pub(crate) fn path(self) -> &'static str {
        self.path
    }
}

///
/// MCP endpoint情報の生成
///
/// # 戻り値
/// HTTPサーバ統合層から参照するMCP endpoint情報を返す。
///
pub(crate) fn create_endpoint() -> McpEndpoint {
    McpEndpoint::new(MCP_ENDPOINT_PATH)
}

///
/// MCP endpoint 用の Actix scope を生成する
///
/// # 引数
/// * `endpoint` - 公開する endpoint 情報
/// * `state` - HTTPサーバ共有状態
///
/// # 戻り値
/// RMCP の Streamable HTTP transport を mount した scope を返す。
///
pub(crate) fn create_scope(
    endpoint: McpEndpoint,
    state: web::Data<Arc<RwLock<AppState>>>,
    session_manager: Arc<ManagedSessionManager>,
) -> impl actix_web::dev::HttpServiceFactory {
    let app_state = state.get_ref().clone();
    let auth_state = state.get_ref().clone();

    /*
     * RMCP Streamable HTTP transport を構築する
     */
    let http_service = StreamableHttpService::builder()
        .service_factory(Arc::new(move || {
            Ok(LuwikiMcpServer::new(app_state.clone()))
        }))
        .on_request_fn(|http_req, extensions| {
            if let Some(auth) = http_req.extensions().get::<AuthContext>() {
                extensions.insert(auth.clone());
            }
            if let Some(address) = http_req.extensions().get::<IpAddr>() {
                extensions.insert(*address);
            }
        })
        .session_manager(session_manager)
        .stateful_mode(true)
        .sse_keep_alive(Duration::from_secs(30))
        .build();

    web::scope(endpoint.path())
        .wrap(from_fn(move |req, next| {
            require_mcp_bearer(req, next, auth_state.clone())
        }))
        .service(http_service.scope())
}

///
/// MCP transport 境界で Bearer 認証を強制する
///
/// # 引数
/// * `req` - Actix サービスリクエスト
/// * `next` - 後続サービス
/// * `state` - HTTP サーバ共有状態
///
/// # 戻り値
/// 認証成功時は後続サービスの応答を返す。
/// 認証失敗時は transport レベルの HTTP エラー応答を返す。
///
async fn require_mcp_bearer<B>(
    mut req: ServiceRequest,
    next: Next<B>,
    state: Arc<RwLock<AppState>>,
) -> Result<ServiceResponse<EitherBody<B>>, Error>
where
    B: MessageBody + 'static,
{
    let authorization = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let auth_gateway = McpAuthGateway::new();

    /*
     * Bearer 認証を評価する
     */
    let auth_result = {
        let state = state
            .read()
            .map_err(|_| ErrorInternalServerError("mcp auth failed"))?;
        auth_gateway.authenticate(state.db(), authorization)
    };

    match auth_result {
        Ok(auth) => {
            /*
             * 認証済み文脈だけを内部へ渡し、
             * raw Authorization ヘッダは RMCP へ転送しない
             */
            let peer_address =
                req.connection_info().peer_addr().and_then(parse_peer_ip_addr);
            req.headers_mut().remove(header::AUTHORIZATION);
            if let Some(address) = peer_address {
                req.extensions_mut().insert(address);
            }
            req.extensions_mut().insert(auth);
            Ok(next.call(req).await?.map_into_left_body())
        }
        Err(error) => {
            let response = mcp_auth_error_response(&error);
            Ok(req.into_response(response).map_into_right_body())
        }
    }
}

///
/// peer address 文字列から IP アドレスを抽出する
///
/// # 引数
/// * `peer_addr` - Actix が認識した peer address
///
/// # 戻り値
/// IP アドレスが抽出できた場合はその値を返す。
///
fn parse_peer_ip_addr(peer_addr: &str) -> Option<IpAddr> {
    peer_addr
        .parse::<std::net::SocketAddr>()
        .map(|addr| addr.ip())
        .or_else(|_| peer_addr.parse::<IpAddr>())
        .ok()
}

///
/// MCP認証失敗を transport レベル HTTP 応答へ変換する
///
/// # 引数
/// * `error` - MCP認証失敗情報
///
/// # 戻り値
/// 認証失敗に対応する HTTP 応答を返す。
///
fn mcp_auth_error_response(error: &McpAuthError) -> HttpResponse {
    HttpResponse::build(map_mcp_auth_error_status(error.kind()))
        .content_type("application/json")
        .body(json!({ "reason": error.message() }).to_string())
}

///
/// MCP認証失敗種別を HTTP ステータスへ対応付ける
///
/// # 引数
/// * `kind` - MCP認証失敗種別
///
/// # 戻り値
/// 対応する HTTP ステータスを返す。
///
fn map_mcp_auth_error_status(kind: McpAuthErrorKind) -> StatusCode {
    match kind {
        McpAuthErrorKind::MissingBearer
        | McpAuthErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
        McpAuthErrorKind::UnsupportedScheme
        | McpAuthErrorKind::InvalidBearerFormat => StatusCode::BAD_REQUEST,
        McpAuthErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use actix_web::dev::ServerHandle;
    use actix_web::http::header;
    use actix_web::{App, HttpServer, web};
    use reqwest::Client;
    use serde_json::Value;
    use tempfile::tempdir;

    use super::{create_endpoint, create_scope};
    use crate::cmd_args::FrontendConfig;
    use crate::database::DatabaseManager;
    use crate::database::types::{BearerScope, BearerScopeSet, PathPrefixSet};
    use crate::fts::FtsIndexConfig;
    use crate::http_server::app_state::AppState;
    use crate::mcp::session_manager::{
        ManagedSessionManager,
        SessionManagerConfig,
    };

    const ACCEPT_BOTH: &str = "application/json, text/event-stream";
    const ACCEPT_SSE: &str = "text/event-stream";
    const INIT_REQUEST_BODY: &str = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",",
        "\"params\":{\"protocolVersion\":\"2025-03-26\",",
        "\"capabilities\":{},",
        "\"clientInfo\":{\"name\":\"test-client\",\"version\":\"1.0.0\"}}}",
    );
    const INITIALIZED_NOTIFICATION_BODY: &str = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",",
        "\"params\":{}}",
    );
    const GET_PAGE_TOOL_CALL_BODY: &str = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",",
        "\"params\":{\"name\":\"get_page\",",
        "\"arguments\":{\"path\":\"/mcp/page\"}}}",
    );

    ///
    /// transport 統合テスト用の起動済みサーバ情報
    ///
    struct TestServerContext {
        /// テスト対象のベースURL
        base_url: String,

        /// HTTPクライアント
        client: Client,

        /// Bearerトークン文字列
        bearer_token: String,

        /// サーバ停止用ハンドル
        handle: ServerHandle,
    }

    impl TestServerContext {
        ///
        /// Bearer付き POST request builder を生成する
        ///
        /// # 引数
        /// * `body` - JSON本文
        ///
        /// # 戻り値
        /// 初期化済みの request builder を返す。
        ///
        fn post_json(&self, body: &str) -> reqwest::RequestBuilder {
            self.client
                .post(&self.base_url)
                .header(header::AUTHORIZATION.as_str(), &self.bearer_token)
                .header(header::CONTENT_TYPE.as_str(), "application/json")
                .header(header::ACCEPT.as_str(), ACCEPT_BOTH)
                .body(body.to_string())
        }

        ///
        /// Bearer付き GET request builder を生成する
        ///
        /// # 戻り値
        /// 初期化済みの request builder を返す。
        ///
        fn get_stream(&self) -> reqwest::RequestBuilder {
            self.client
                .get(&self.base_url)
                .header(header::AUTHORIZATION.as_str(), &self.bearer_token)
                .header(header::ACCEPT.as_str(), ACCEPT_SSE)
        }

        ///
        /// Bearer付き DELETE request builder を生成する
        ///
        /// # 戻り値
        /// 初期化済みの request builder を返す。
        ///
        fn delete_session(&self) -> reqwest::RequestBuilder {
            self.client
                .delete(&self.base_url)
                .header(header::AUTHORIZATION.as_str(), &self.bearer_token)
        }

        ///
        /// 初期化から session ID 取得までを実行する
        ///
        /// # 戻り値
        /// 発行された session ID を返す。
        ///
        async fn initialize_session(&self) -> String {
            let response = self
                .post_json(INIT_REQUEST_BODY)
                .send()
                .await
                .expect("send initialize request failed");
            assert_eq!(response.status(), 200);
            response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .expect("mcp-session-id header missing")
                .to_string()
        }

        ///
        /// `notifications/initialized` を送信する
        ///
        /// # 引数
        /// * `session_id` - 対象 session ID
        ///
        /// # 戻り値
        /// なし
        ///
        async fn send_initialized_notification(&self, session_id: &str) {
            let response = self
                .post_json(INITIALIZED_NOTIFICATION_BODY)
                .header("mcp-session-id", session_id)
                .send()
                .await
                .expect("send initialized notification failed");
            assert_eq!(response.status(), 202);
        }

        ///
        /// テストサーバを停止する
        ///
        /// # 戻り値
        /// なし
        ///
        async fn shutdown(self) {
            /*
             * stop() の完了待ち自体が停止する場合があるため、
             * 停止要求だけ送って短く待機する
             */
            std::mem::drop(self.client);
            std::mem::drop(self.handle.stop(true));
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    ///
    /// transport 統合テスト用サーバを起動する
    ///
    /// # 戻り値
    /// 起動済みサーバ情報を返す。
    ///
    async fn spawn_test_server() -> TestServerContext {
        spawn_test_server_with_session_config(SessionManagerConfig::default())
            .await
    }

    ///
    /// 指定 session 設定で transport 統合テスト用サーバを起動する
    ///
    /// # 引数
    /// * `session_config` - session 管理設定
    ///
    /// # 戻り値
    /// 起動済みサーバ情報を返す。
    ///
    async fn spawn_test_server_with_session_config(
        session_config: SessionManagerConfig,
    ) -> TestServerContext {
        /*
         * テスト用 AppState を構築する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_dir = dir.path().join("assets");
        let index_dir = dir.path().join("fts");
        std::fs::create_dir_all(&asset_dir).expect("create assets dir failed");
        std::fs::create_dir_all(&index_dir).expect("create fts dir failed");

        let manager = DatabaseManager::open(&db_path, &asset_dir)
            .expect("open database failed");
        manager
            .add_user("alice", "password123", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/page", "alice", "# page\nbody".to_string())
            .expect("create page failed");
        let (token, _) = manager
            .create_bearer_token(
                "alice",
                BearerScopeSet::from_iter([BearerScope::Read]),
                PathPrefixSet::from_iter(["/"]),
                chrono::Duration::minutes(30),
                Some("transport test".to_string()),
            )
            .expect("create bearer token failed");
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_dir),
            None,
            "LUWIKI".to_string(),
            1024 * 1024,
            None,
        ))));

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("bind test listener failed");
        let address = listener
            .local_addr()
            .expect("get listener address failed");
        let session_manager =
            ManagedSessionManager::new_with_config(session_config);
        session_manager.start_background_sweep();
        let server = HttpServer::new(move || {
            App::new().service(create_scope(
                create_endpoint(),
                state.clone(),
                session_manager.clone(),
            ))
        })
        .listen(listener)
        .expect("listen test server failed")
        .run();
        let handle = server.handle();
        actix_web::rt::spawn(server);
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client failed");

        TestServerContext {
            base_url: format!("http://{}/mcp", address),
            client,
            bearer_token: format!("Bearer {}", token.expose()),
            handle,
        }
    }

    ///
    /// 標準 initialize request が成功し、
    /// JSON-RPC result と session ヘッダを返すことを確認する。
    ///
    /// # 戻り値
    /// テストに成功した場合は `Ok(())` を返す。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::initialize_request_returns_result`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn initialize_request_returns_result() {
        let context = spawn_test_server().await;

        /*
         * 標準 initialize request を送信する
         */
        let response = context
            .post_json(INIT_REQUEST_BODY)
            .send()
            .await
            .expect("send initialize request failed");

        /*
         * HTTP 応答と session ヘッダを確認する
         */
        assert!(
            response.status().is_success(),
            "unexpected status: {}",
            response.status(),
        );
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok());
        assert!(session_id.is_some(), "mcp-session-id header missing");
        let session_id = session_id.unwrap().to_string();

        /*
         * JSON-RPC result の主要項目を確認する
         */
        let body_text = response
            .text()
            .await
            .expect("read initialize response failed");
        assert!(
            body_text.starts_with("data: "),
            "initialize response must be SSE: {}",
            body_text,
        );
        let body_json: Value = serde_json::from_str(
            body_text
                .trim()
                .trim_start_matches("data: ")
                .trim(),
        )
        .expect("initialize response must contain JSON");
        assert_eq!(body_json["jsonrpc"], "2.0");
        assert_eq!(body_json["id"], 1);
        assert_eq!(
            body_json["result"]["protocolVersion"],
            "2025-03-26",
        );
        assert!(body_json["result"]["capabilities"].is_object());
        assert!(body_json["result"]["serverInfo"].is_object());
        assert_eq!(body_json["result"]["serverInfo"]["name"], "luwiki");
        assert_eq!(body_json["result"]["serverInfo"]["version"], "0.9.15");

        /*
         * セッションを明示的に閉じて background task を終了させる
         */
        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete request failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// Authorization なしの initialize request が
    /// 401 Unauthorized になることを確認する。
    ///
    #[actix_web::test]
    async fn initialize_request_without_bearer_returns_401() {
        let context = spawn_test_server().await;

        /*
         * Authorization なしで initialize を送信する
         */
        let response = context
            .client
            .post(&context.base_url)
            .header(header::CONTENT_TYPE.as_str(), "application/json")
            .header(header::ACCEPT.as_str(), ACCEPT_BOTH)
            .body(INIT_REQUEST_BODY)
            .send()
            .await
            .expect("send initialize request failed");

        /*
         * 認証失敗が 401 で返ることを確認する
         */
        assert_eq!(response.status(), 401);
        let body_text = response
            .text()
            .await
            .expect("read unauthorized response failed");
        let body_json: Value = serde_json::from_str(&body_text)
            .expect("decode unauthorized response failed");
        assert_eq!(body_json["reason"], "missing bearer token for MCP");
        context.shutdown().await;
    }

    ///
    /// Bearer付き GET で `Accept: text/event-stream` と
    /// `Mcp-Session-Id` を満たせば SSE stream を開けることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::get_stream_request_returns_sse`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn get_stream_request_returns_sse() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        let response = context
            .get_stream()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get stream request failed");

        assert_eq!(response.status(), 200);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("text/event-stream"),
            "expected event-stream, got {content_type}",
        );

        drop(response);
        let delete_response = context
            .delete_session()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send session delete request failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// Bearer付き GET でも `Accept: text/event-stream` が無ければ
    /// backend 準拠で 406 になることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::get_stream_without_sse_accept_returns_406`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn get_stream_without_sse_accept_returns_406() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;

        let response = context
            .client
            .get(&context.base_url)
            .header(header::AUTHORIZATION.as_str(), &context.bearer_token)
            .header(header::ACCEPT.as_str(), "application/json")
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get stream without sse accept failed");

        assert_eq!(response.status(), 406);
        context.shutdown().await;
    }

    ///
    /// Bearer付き GET で session ID が無ければ
    /// backend 準拠で 401 になることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::get_stream_without_session_id_returns_401`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn get_stream_without_session_id_returns_401() {
        let context = spawn_test_server().await;

        let response = context
            .get_stream()
            .send()
            .await
            .expect("send get stream without session failed");

        assert_eq!(response.status(), 401);
        let body_text = response
            .text()
            .await
            .expect("read get without session response failed");
        assert_eq!(body_text, "Unauthorized: Session ID is required");

        context.shutdown().await;
    }

    ///
    /// Bearer付き POST でも `Accept` が両 MIME を含まなければ
    /// backend 準拠で 406 になることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::post_initialize_without_dual_accept_returns_406`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn post_initialize_without_dual_accept_returns_406() {
        let context = spawn_test_server().await;

        let response = context
            .client
            .post(&context.base_url)
            .header(header::AUTHORIZATION.as_str(), &context.bearer_token)
            .header(header::CONTENT_TYPE.as_str(), "application/json")
            .header(header::ACCEPT.as_str(), "application/json")
            .body(INIT_REQUEST_BODY)
            .send()
            .await
            .expect("send initialize without dual accept failed");

        assert_eq!(response.status(), 406);
        context.shutdown().await;
    }

    ///
    /// Bearer付き POST でも `Content-Type: application/json` でなければ
    /// backend 準拠で 415 になることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::post_initialize_without_json_content_type_returns_415`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn post_initialize_without_json_content_type_returns_415() {
        let context = spawn_test_server().await;

        let response = context
            .client
            .post(&context.base_url)
            .header(header::AUTHORIZATION.as_str(), &context.bearer_token)
            .header(header::CONTENT_TYPE.as_str(), "text/plain")
            .header(header::ACCEPT.as_str(), ACCEPT_BOTH)
            .body(INIT_REQUEST_BODY)
            .send()
            .await
            .expect("send initialize without json content type failed");

        assert_eq!(response.status(), 415);
        context.shutdown().await;
    }

    ///
    /// Bearer付き DELETE で session ID が無ければ
    /// backend 準拠で 401 になることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::transport::tests::delete_without_session_id_returns_401`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn delete_without_session_id_returns_401() {
        let context = spawn_test_server().await;

        let response = context
            .delete_session()
            .send()
            .await
            .expect("send delete without session failed");

        assert_eq!(response.status(), 401);
        let body_text = response
            .text()
            .await
            .expect("read delete without session response failed");
        assert_eq!(body_text, "Unauthorized: Session ID is required");

        context.shutdown().await;
    }

    ///
    /// Bearer 付き `tools/call(get_page)` が既存 read 系処理へ接続され、
    /// ページ本文を返すことを確認する。
    ///
    /// # 注記
    /// `cargo test --bin luwiki mcp::transport::tests::get_page_tool_call_returns_page`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn get_page_tool_call_returns_page() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        let response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get_page tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read get_page tool call response failed");
        assert!(
            body_text.contains("\ndata: ")
                || body_text.starts_with("data: "),
            "tool response must be SSE: {}",
            body_text,
        );

        let payload = body_text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("tool response data line missing");
        let body_json: Value = serde_json::from_str(payload)
            .expect("tool response must contain JSON");
        assert_eq!(body_json["jsonrpc"], "2.0");
        assert_eq!(body_json["id"], 2);
        assert_eq!(body_json["result"]["content"][0]["type"], "text");

        let content_text = body_json["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text missing");
        let payload_json: Value = serde_json::from_str(content_text)
            .expect("tool result payload must contain JSON");
        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 1);
        assert_eq!(payload_json["content"], "# page\nbody");

        let delete_response = context
            .delete_session()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send session delete request failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// TTL 期限切れ後の GET が 401 になることを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::expired_session_get_returns_401`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn expired_session_get_returns_401() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new(
                Duration::from_millis(120),
                Duration::from_millis(30),
                8,
            ),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        tokio::time::sleep(Duration::from_millis(220)).await;

        let response = context
            .get_stream()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send expired get request failed");

        assert_eq!(response.status(), 401);
        context.shutdown().await;
    }

    ///
    /// TTL 期限切れ後の POST が 401 になることを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::expired_session_post_returns_401`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn expired_session_post_returns_401() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new(
                Duration::from_millis(120),
                Duration::from_millis(30),
                8,
            ),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        tokio::time::sleep(Duration::from_millis(220)).await;

        let response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send expired post request failed");

        assert_eq!(response.status(), 401);
        context.shutdown().await;
    }

    ///
    /// TTL 期限切れ後の DELETE が idempotent に 204 になることを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::expired_session_delete_returns_204`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn expired_session_delete_returns_204() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new(
                Duration::from_millis(120),
                Duration::from_millis(30),
                8,
            ),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        tokio::time::sleep(Duration::from_millis(220)).await;

        let response = context
            .delete_session()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send expired delete request failed");

        assert_eq!(response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// session 上限超過時に最古 session が eviction されることを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::session_limit_evicts_oldest_session`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn session_limit_evicts_oldest_session() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new(
                Duration::from_secs(300),
                Duration::from_secs(300),
                1,
            ),
        )
        .await;
        let first_session_id = context.initialize_session().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second_session_id = context.initialize_session().await;

        assert_ne!(first_session_id, second_session_id);

        let first_response = context
            .get_stream()
            .header("mcp-session-id", &first_session_id)
            .send()
            .await
            .expect("send first session get request failed");
        assert_eq!(first_response.status(), 401);

        let second_delete_response = context
            .delete_session()
            .header("mcp-session-id", &second_session_id)
            .send()
            .await
            .expect("send second session delete request failed");
        assert_eq!(second_delete_response.status(), 204);
        context.shutdown().await;
    }
}
