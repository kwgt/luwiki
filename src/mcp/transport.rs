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
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{create_endpoint, create_scope};
    use crate::cmd_args::FrontendConfig;
    use crate::database::DatabaseManager;
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        PathPrefixSet,
        TokenId,
        UserAttribute,
        UserAttributeSet,
    };
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

        /// 共有状態
        state: web::Data<Arc<RwLock<AppState>>>,

        /// Bearerトークン文字列
        bearer_token: String,

        /// BearerトークンID
        bearer_token_id: TokenId,

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
            self.initialize_with_result().await.0
        }

        ///
        /// 初期化からsession IDと
        /// JSON-RPC result取得までを実行する
        ///
        /// # 戻り値
        /// 発行されたsession IDとinitialize応答JSONを返す。
        ///
        async fn initialize_with_result(&self) -> (String, Value) {
            let response = self
                .post_json(INIT_REQUEST_BODY)
                .send()
                .await
                .expect("send initialize request failed");
            assert_eq!(response.status(), 200);
            let session_id = response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .expect("mcp-session-id header missing")
                .to_string();
            let body_text = response
                .text()
                .await
                .expect("read initialize response failed");

            (session_id, parse_sse_payload(&body_text))
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

        ///
        /// 指定 path の latest revision と instance_id を取得する
        ///
        /// # 戻り値
        /// `(latest_revision, instance_id)` を返す。
        ///
        fn latest_revision_and_instance_id(&self, path: &str) -> (u64, String) {
            let state = self.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path(path)
                .expect("resolve page id failed")
                .expect("page id missing");
            let resolved = state
                .db()
                .get_current_page_state_by_path(path)
                .expect("resolve current page failed")
                .expect("current page missing");
            let latest_revision = resolved
                .latest_revision()
                .expect("latest revision missing");
            let latest_source = state
                .db()
                .get_page_source(&page_id, latest_revision)
                .expect("get latest source failed")
                .expect("latest source missing");

            (
                latest_revision,
                latest_source
                    .instance_id()
                    .expect("instance_id missing")
                    .to_string(),
            )
        }

        ///
        /// 指定 path の本文を直接更新する
        ///
        /// # 引数
        /// * `path` - 対象 path
        /// * `user` - 更新ユーザ
        /// * `content` - 更新本文
        ///
        /// # 戻り値
        /// なし
        ///
        fn put_page(&self, path: &str, user: &str, content: &str) {
            let state = self.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path(path)
                .expect("resolve page id failed")
                .expect("page id missing");
            state
                .db()
                .put_page(&page_id, user, content.to_string(), false)
                .expect("put page failed");
        }

        ///
        /// テスト用Bearerトークンを失効する
        ///
        /// # 戻り値
        /// なし
        ///
        fn revoke_bearer_token(&self) {
            let state = self.state.read().expect("lock app state failed");
            state
                .db()
                .revoke_bearer_token_by_id(&self.bearer_token_id)
                .expect("revoke bearer token failed");
        }

        ///
        /// 指定 path にページロックを設定する
        ///
        /// # 引数
        /// * `path` - 対象 path
        /// * `user` - ロック所有者
        ///
        /// # 戻り値
        /// なし
        ///
        fn acquire_page_lock(&self, path: &str, user: &str) {
            let state = self.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path(path)
                .expect("resolve page id failed")
                .expect("page id missing");
            state
                .db()
                .acquire_page_lock(&page_id, user)
                .expect("acquire page lock failed");
        }
    }

    ///
    /// transport 統合テスト用サーバを起動する
    ///
    /// # 戻り値
    /// 起動済みサーバ情報を返す。
    ///
    async fn spawn_test_server() -> TestServerContext {
        spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
        )
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
        spawn_test_server_with_auth(
            session_config,
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await
    }

    ///
    /// 指定認可条件で transport 統合テスト用サーバを起動する
    ///
    /// # 引数
    /// * `session_config` - session 管理設定
    /// * `scopes` - Bearer scope 集合
    /// * `path_prefixes` - path prefix 制約
    ///
    /// # 戻り値
    /// 起動済みサーバ情報を返す。
    ///
    async fn spawn_test_server_with_auth(
        session_config: SessionManagerConfig,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
    ) -> TestServerContext {
        spawn_test_server_with_auth_and_attributes(
            session_config,
            scopes,
            path_prefixes,
            UserAttributeSet::new(),
        )
        .await
    }

    ///
    /// 指定認可条件・ユーザ属性で
    /// テスト用サーバを起動する
    ///
    /// # 引数
    /// * `session_config` - session管理設定
    /// * `scopes` - Bearer scope集合
    /// * `path_prefixes` - path prefix制約
    /// * `attributes` - ユーザ属性集合
    ///
    /// # 戻り値
    /// 起動済みサーバ情報を返す。
    ///
    async fn spawn_test_server_with_auth_and_attributes(
        session_config: SessionManagerConfig,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
        attributes: UserAttributeSet,
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
            .add_user_with_attributes(
                "alice",
                Some("password123"),
                None,
                attributes,
            )
            .expect("add user failed");
        manager
            .create_page("/mcp/page", "alice", "# page\nbody".to_string())
            .expect("create page failed");
        let (token, token_info) = manager
            .create_bearer_token(
                "alice",
                scopes,
                path_prefixes,
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
            None,
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
        let server_state = state.clone();
        let server = HttpServer::new(move || {
            App::new().service(create_scope(
                create_endpoint(),
                server_state.clone(),
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
            state,
            bearer_token: format!("Bearer {}", token.expose()),
            bearer_token_id: token_info.token_id(),
            handle,
        }
    }

    fn build_tool_call_body(name: &str, arguments: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        })
        .to_string()
    }

    fn parse_sse_payload(body_text: &str) -> Value {
        let payload = body_text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("tool response data line missing");
        serde_json::from_str(payload).expect("tool response must contain JSON")
    }

    fn parse_tool_result_payload(body_text: &str) -> (Value, Value) {
        let body_json = parse_sse_payload(body_text);
        let content_text = body_json["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text missing");
        let payload_json: Value = serde_json::from_str(content_text)
            .expect("tool result payload must contain JSON");
        (body_json, payload_json)
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
        let capabilities = &body_json["result"]["capabilities"];
        assert!(capabilities.is_object());
        assert!(capabilities["tools"].is_object());
        assert!(capabilities["prompts"].is_object());
        assert!(capabilities["prompts"]["listChanged"].is_null());
        assert!(capabilities["resources"].is_null());
        assert!(body_json["result"]["serverInfo"].is_object());
        assert_eq!(body_json["result"]["serverInfo"]["name"], "luwiki");
        assert_eq!(
            body_json["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION"),
        );

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
    /// 標準handshake後にprompts/listとprompts/getを
    /// Streamable HTTP経由で利用できることを確認する。
    ///
    /// # 注記
    /// promptページを作成してinitializeとinitializedを完了し、
    /// 同じsessionから一覧・取得を順に呼び出す。
    ///
    #[actix_web::test]
    async fn prompts_are_available_after_standard_handshake() {
        let context = spawn_test_server().await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/prompts/transport",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: transport-prompt\n",
                        "  description: transport description\n",
                        "  system: System {{@target}}\n",
                        "  arguments:\n",
                        "    - name: target\n",
                        "      description: target value\n",
                        "      required: true\n",
                        "---\n",
                        "Body {{@target}}",
                    )
                    .to_string(),
                )
                .expect("create prompt failed");
        }
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        /*
         * prompts/listの標準応答を確認する
         */
        let list_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/list",
            "params": {},
        })
        .to_string();
        let list_response = context
            .post_json(&list_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send prompts/list failed");
        assert_eq!(list_response.status(), 200);
        let list_text = list_response
            .text()
            .await
            .expect("read prompts/list response failed");
        let list_json = parse_sse_payload(&list_text);
        let prompts = list_json["result"]["prompts"]
            .as_array()
            .expect("prompts result missing");
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], "transport-prompt");
        assert_eq!(
            prompts[0]["description"],
            "transport description",
        );
        assert_eq!(prompts[0]["arguments"][0]["name"], "target");
        assert_eq!(prompts[0]["arguments"][0]["required"], true);

        /*
         * prompts/getの標準応答を確認する
         */
        let get_body = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/get",
            "params": {
                "name": "transport-prompt",
                "arguments": {
                    "target": "transport-value",
                },
            },
        })
        .to_string();
        let get_response = context
            .post_json(&get_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send prompts/get failed");
        assert_eq!(get_response.status(), 200);
        let get_text = get_response
            .text()
            .await
            .expect("read prompts/get response failed");
        let get_json = parse_sse_payload(&get_text);
        assert_eq!(
            get_json["result"]["description"],
            "transport description",
        );
        let messages = get_json["result"]["messages"]
            .as_array()
            .expect("prompt messages missing");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"]["type"], "text");
        assert_eq!(
            messages[0]["content"]["text"],
            concat!(
                "System transport-value\n\n",
                "Body transport-value",
            ),
        );

        /*
         * sessionを明示的に終了する
         */
        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// 標準handshake後にresources/listとresources/readを
    /// Streamable HTTP経由で利用できることを確認する。
    ///
    /// # 注記
    /// resourceページを作成してinitializeとinitializedを完了し、
    /// 同じsessionから一覧・読込を順に呼び出す。
    ///
    #[actix_web::test]
    async fn resources_are_available_after_standard_handshake() {
        let context = spawn_test_server().await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/resources/transport",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: resource\n",
                        "  resource_id: docs/transport\n",
                        "  name: transport-resource\n",
                        "  description: transport resource description\n",
                        "  mime_type: text/plain\n",
                        "---\n",
                        "# Transport Resource\n",
                        "\n",
                        "resource body",
                    )
                    .to_string(),
                )
                .expect("create resource failed");
            state
                .db()
                .rebuild_resource_candidates()
                .expect("rebuild resources failed");
        }
        let (session_id, initialize_json) =
            context.initialize_with_result().await;
        assert!(
            initialize_json["result"]["capabilities"]["resources"]
                .is_object(),
        );
        context.send_initialized_notification(&session_id).await;

        /*
         * resources/listの標準応答を確認する
         */
        let list_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "resources/list",
            "params": {},
        })
        .to_string();
        let list_response = context
            .post_json(&list_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send resources/list failed");
        assert_eq!(list_response.status(), 200);
        let list_text = list_response
            .text()
            .await
            .expect("read resources/list response failed");
        let list_json = parse_sse_payload(&list_text);
        let resources = list_json["result"]["resources"]
            .as_array()
            .expect("resources result missing");
        let uris = resources
            .iter()
            .map(|resource| {
                resource["uri"]
                    .as_str()
                    .expect("resource uri missing")
            })
            .collect::<Vec<_>>();
        assert!(uris.contains(
            &"luwiki://local.luwiki/builtin/front-matter-spec"
        ));
        assert!(uris.contains(
            &"luwiki://local.luwiki/builtin/mcp-prompt-spec"
        ));
        assert!(uris.contains(
            &"luwiki://local.luwiki/page/docs/transport"
        ));

        /*
         * resources/readの標準応答を確認する
         */
        let read_body = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/read",
            "params": {
                "uri": "luwiki://local.luwiki/page/docs/transport",
            },
        })
        .to_string();
        let read_response = context
            .post_json(&read_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send resources/read failed");
        assert_eq!(read_response.status(), 200);
        let read_text = read_response
            .text()
            .await
            .expect("read resources/read response failed");
        let read_json = parse_sse_payload(&read_text);
        let contents = read_json["result"]["contents"]
            .as_array()
            .expect("resource contents missing");
        assert_eq!(contents.len(), 1);
        assert_eq!(
            contents[0]["uri"],
            "luwiki://local.luwiki/page/docs/transport",
        );
        assert_eq!(contents[0]["mimeType"], "text/plain");
        assert_eq!(
            contents[0]["text"],
            "# Transport Resource\n\nresource body",
        );
        assert!(!contents[0]["text"]
            .as_str()
            .expect("resource text missing")
            .contains("primitive: resource"));

        /*
         * sessionを明示的に終了する
         */
        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// resourcesがtransport境界でも認可とroutingを
    /// 維持することを確認する。
    ///
    /// # 注記
    /// 同一session上でtools、prompts、resourcesを交互に呼び出し、
    /// resourcesのscope不足、path prefix秘匿、ReadOnly許可を検証する。
    ///
    #[actix_web::test]
    async fn resources_transport_honors_authorization_and_session_routing() {
        /*
         * tools、prompts、resourcesが共存するsessionを準備する
         */
        let context = spawn_test_server().await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/prompts/resource-routing",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: resource-routing-prompt\n",
                        "  description: resource routing description\n",
                        "---\n",
                        "prompt body",
                    )
                    .to_string(),
                )
                .expect("create prompt failed");
            state
                .db()
                .create_page(
                    "/resources/routing",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: resource\n",
                        "  resource_id: docs/routing\n",
                        "  name: routing-resource\n",
                        "  description: routing resource description\n",
                        "  mime_type: text/plain\n",
                        "---\n",
                        "routing body",
                    )
                    .to_string(),
                )
                .expect("create resource failed");
            state
                .db()
                .rebuild_resource_candidates()
                .expect("rebuild resources failed");
        }
        let (session_id, initialize_json) =
            context.initialize_with_result().await;
        assert!(
            initialize_json["result"]["capabilities"]["tools"]
                .is_object(),
        );
        assert!(
            initialize_json["result"]["capabilities"]["prompts"]
                .is_object(),
        );
        assert!(
            initialize_json["result"]["capabilities"]["resources"]
                .is_object(),
        );
        context.send_initialized_notification(&session_id).await;

        /*
         * 同一sessionでtools、prompts、resourcesのroutingを確認する
         */
        for request in [
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "prompts/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "resources/list",
                "params": {},
            }),
        ] {
            let response = context
                .post_json(&request.to_string())
                .header("mcp-session-id", &session_id)
                .send()
                .await
                .expect("send list request failed");
            assert_eq!(response.status(), 200);
            let body_text = response
                .text()
                .await
                .expect("read list response failed");
            let body_json = parse_sse_payload(&body_text);
            assert!(body_json["result"].is_object());
            assert!(body_json["error"].is_null());
        }

        let read_body = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "resources/read",
            "params": {
                "uri": "luwiki://local.luwiki/page/docs/routing",
            },
        })
        .to_string();
        let read_response = context
            .post_json(&read_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send resources/read failed");
        assert_eq!(read_response.status(), 200);
        let read_text = read_response
            .text()
            .await
            .expect("read resources/read failed");
        let read_json = parse_sse_payload(&read_text);
        assert_eq!(
            read_json["result"]["contents"][0]["text"],
            "routing body",
        );

        /*
         * resourcesはread scope不足をtransport越しにforbiddenへ変換する
         */
        let append_only_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let append_only_session =
            append_only_context.initialize_session().await;
        append_only_context
            .send_initialized_notification(&append_only_session)
            .await;
        for request in [
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "resources/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "resources/read",
                "params": {
                    "uri": "luwiki://local.luwiki/builtin/front-matter-spec",
                },
            }),
        ] {
            let response = append_only_context
                .post_json(&request.to_string())
                .header("mcp-session-id", &append_only_session)
                .send()
                .await
                .expect("send forbidden resource request failed");
            assert_eq!(response.status(), 200);
            let body_text = response
                .text()
                .await
                .expect("read forbidden resource response failed");
            let body_json = parse_sse_payload(&body_text);
            assert_eq!(body_json["error"]["code"], -32600);
            assert_eq!(
                body_json["error"]["message"],
                "operation is not allowed",
            );
            assert_eq!(
                body_json["error"]["data"]["code"],
                "forbidden",
            );
        }

        /*
         * path prefix範囲外のページ由来resourceはnot_foundへ秘匿する
         */
        let prefix_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/allowed"]),
        )
        .await;
        {
            let state = prefix_context
                .state
                .read()
                .expect("lock prefix app state failed");
            state
                .db()
                .create_page(
                    "/private/resource",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: resource\n",
                        "  resource_id: docs/private\n",
                        "  name: private-resource\n",
                        "  description: private resource description\n",
                        "---\n",
                        "private body",
                    )
                    .to_string(),
                )
                .expect("create private resource failed");
            state
                .db()
                .rebuild_resource_candidates()
                .expect("rebuild private resources failed");
        }
        let prefix_session = prefix_context.initialize_session().await;
        prefix_context
            .send_initialized_notification(&prefix_session)
            .await;
        let prefix_body = json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "resources/read",
            "params": {
                "uri": "luwiki://local.luwiki/page/docs/private",
            },
        })
        .to_string();
        let prefix_response = prefix_context
            .post_json(&prefix_body)
            .header("mcp-session-id", &prefix_session)
            .send()
            .await
            .expect("send prefix denied resource request failed");
        assert_eq!(prefix_response.status(), 200);
        let prefix_text = prefix_response
            .text()
            .await
            .expect("read prefix denied resource response failed");
        let prefix_json = parse_sse_payload(&prefix_text);
        assert_eq!(prefix_json["error"]["code"], -32602);
        assert_eq!(
            prefix_json["error"]["message"],
            "resource not found",
        );
        assert_eq!(
            prefix_json["error"]["data"]["code"],
            "not_found",
        );
        let serialized_prefix = prefix_json.to_string();
        assert!(!serialized_prefix.contains("/private/resource"));
        assert!(!serialized_prefix.contains("private-resource"));

        /*
         * ReadOnly属性ユーザでもread scopeならresourcesを利用できる
         */
        let read_only_context = spawn_test_server_with_auth_and_attributes(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
        )
        .await;
        {
            let state = read_only_context
                .state
                .read()
                .expect("lock read only app state failed");
            state
                .db()
                .create_page(
                    "/resources/read-only",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: resource\n",
                        "  resource_id: docs/read-only\n",
                        "  name: read-only-resource\n",
                        "  description: read only resource description\n",
                        "---\n",
                        "read only body",
                    )
                    .to_string(),
                )
                .expect("create read only resource failed");
            state
                .db()
                .rebuild_resource_candidates()
                .expect("rebuild read only resources failed");
        }
        let read_only_session =
            read_only_context.initialize_session().await;
        read_only_context
            .send_initialized_notification(&read_only_session)
            .await;
        for request in [
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "resources/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "resources/read",
                "params": {
                    "uri": "luwiki://local.luwiki/page/docs/read-only",
                },
            }),
        ] {
            let response = read_only_context
                .post_json(&request.to_string())
                .header("mcp-session-id", &read_only_session)
                .send()
                .await
                .expect("send read only resource request failed");
            assert_eq!(response.status(), 200);
            let body_text = response
                .text()
                .await
                .expect("read read only resource response failed");
            let body_json = parse_sse_payload(&body_text);
            assert!(body_json["result"].is_object());
            assert!(body_json["error"].is_null());
        }

        context.shutdown().await;
        append_only_context.shutdown().await;
        prefix_context.shutdown().await;
        read_only_context.shutdown().await;
    }

    ///
    /// prompts要求がBearer欠落、不正、失効時に
    /// MCP handlerへ到達しないことを確認する。
    ///
    /// # 注記
    /// handshake済みsessionに対するprompts/listを
    /// transport認証境界で拒否する。
    ///
    #[actix_web::test]
    async fn prompts_reject_missing_invalid_and_revoked_bearers() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let list_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/list",
            "params": {},
        })
        .to_string();

        /*
         * Bearer欠落をtransportで拒否する
         */
        let missing_response = context
            .client
            .post(&context.base_url)
            .header(header::CONTENT_TYPE.as_str(), "application/json")
            .header(header::ACCEPT.as_str(), ACCEPT_BOTH)
            .header("mcp-session-id", &session_id)
            .body(list_body.clone())
            .send()
            .await
            .expect("send request without bearer failed");
        assert_eq!(missing_response.status(), 401);
        let missing_text = missing_response
            .text()
            .await
            .expect("read missing bearer response failed");
        let missing_json: Value = serde_json::from_str(&missing_text)
            .expect("decode missing bearer response failed");
        assert_eq!(
            missing_json["reason"],
            "missing bearer token for MCP",
        );
        assert!(missing_json["result"].is_null());
        assert!(missing_json["error"].is_null());

        /*
         * 不正Bearerをtransportで拒否する
         */
        let invalid_response = context
            .client
            .post(&context.base_url)
            .header(
                header::AUTHORIZATION.as_str(),
                "Bearer invalid-token",
            )
            .header(header::CONTENT_TYPE.as_str(), "application/json")
            .header(header::ACCEPT.as_str(), ACCEPT_BOTH)
            .header("mcp-session-id", &session_id)
            .body(list_body.clone())
            .send()
            .await
            .expect("send request with invalid bearer failed");
        assert_eq!(invalid_response.status(), 401);
        let invalid_text = invalid_response
            .text()
            .await
            .expect("read invalid bearer response failed");
        let invalid_json: Value = serde_json::from_str(&invalid_text)
            .expect("decode invalid bearer response failed");
        assert_eq!(invalid_json["reason"], "unauthorized");
        assert!(invalid_json["result"].is_null());
        assert!(invalid_json["error"].is_null());

        /*
         * 失効Bearerをtransportで拒否する
         */
        context.revoke_bearer_token();
        let revoked_response = context
            .post_json(&list_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send request with revoked bearer failed");
        assert_eq!(revoked_response.status(), 401);
        let revoked_text = revoked_response
            .text()
            .await
            .expect("read revoked bearer response failed");
        let revoked_json: Value = serde_json::from_str(&revoked_text)
            .expect("decode revoked bearer response failed");
        assert_eq!(revoked_json["reason"], "unauthorized");
        assert!(revoked_json["result"].is_null());
        assert!(revoked_json["error"].is_null());

        context.shutdown().await;
    }

    ///
    /// read scope不足のBearerがprompts/listと
    /// prompts/getで固定認可エラーになることを確認する。
    ///
    /// # 注記
    /// append scopeだけで標準handshakeを完了し、
    /// 両prompts methodを同じsessionから呼び出す。
    ///
    #[actix_web::test]
    async fn prompts_require_read_scope_after_handshake() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let requests = [
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "prompts/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "prompts/get",
                "params": {
                    "name": "secret-prompt",
                },
            }),
        ];

        /*
         * 両methodの固定forbidden応答を確認する
         */
        for request in requests {
            let response = context
                .post_json(&request.to_string())
                .header("mcp-session-id", &session_id)
                .send()
                .await
                .expect("send prompts request failed");
            assert_eq!(response.status(), 200);
            let body_text = response
                .text()
                .await
                .expect("read prompts error response failed");
            let body_json = parse_sse_payload(&body_text);
            assert_eq!(body_json["error"]["code"], -32600);
            assert_eq!(
                body_json["error"]["message"],
                "operation is not allowed",
            );
            assert_eq!(
                body_json["error"]["data"]["code"],
                "forbidden",
            );
            let serialized = body_json.to_string();
            assert!(!serialized.contains("/mcp/page"));
            assert!(!serialized.contains("# page"));
        }

        /*
         * sessionを明示的に終了する
         */
        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// promptsがpage用path prefixを無視し、
    /// path基準toolが従来制約を維持することを確認する。
    ///
    /// # 注記
    /// `/allowed`だけを許可したread tokenから、`/private`の
    /// promptと`/mcp/page`のget_pageを同じsessionで呼び出す。
    ///
    #[actix_web::test]
    async fn prompts_ignore_page_path_prefix_while_tools_enforce_it() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/allowed"]),
        )
        .await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/private/prompts/prefix",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: prefix-prompt\n",
                        "  description: prefix description\n",
                        "---\n",
                        "prefix body",
                    )
                    .to_string(),
                )
                .expect("create prompt failed");
        }
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        /*
         * prefix範囲外promptの一覧・取得を確認する
         */
        let list_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/list",
            "params": {},
        })
        .to_string();
        let list_response = context
            .post_json(&list_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send prompts/list failed");
        let list_text = list_response
            .text()
            .await
            .expect("read prompts/list failed");
        let list_json = parse_sse_payload(&list_text);
        assert_eq!(
            list_json["result"]["prompts"][0]["name"],
            "prefix-prompt",
        );
        assert!(!list_json.to_string().contains("/private/prompts"));

        let get_body = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/get",
            "params": {
                "name": "prefix-prompt",
            },
        })
        .to_string();
        let get_response = context
            .post_json(&get_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send prompts/get failed");
        let get_text = get_response
            .text()
            .await
            .expect("read prompts/get failed");
        let get_json = parse_sse_payload(&get_text);
        assert_eq!(
            get_json["result"]["messages"][0]["content"]["text"],
            "prefix body",
        );
        assert!(!get_json.to_string().contains("/private/prompts"));

        /*
         * path基準get_pageのprefix拒否を確認する
         */
        let tool_response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get_page failed");
        let tool_text = tool_response
            .text()
            .await
            .expect("read get_page failed");
        let (_, tool_payload) = parse_tool_result_payload(&tool_text);
        assert_eq!(tool_payload["code"], "forbidden");
        assert_eq!(
            tool_payload["message"],
            "path prefix denied: /mcp/page",
        );

        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// ReadOnly属性ユーザがread scopeでpromptsを
    /// 利用できることを確認する。
    ///
    /// # 注記
    /// ReadOnly属性付きaliceへread tokenを発行し、
    /// 標準handshake後に一覧・取得を呼び出す。
    ///
    #[actix_web::test]
    async fn read_only_user_can_use_prompts_with_read_scope() {
        let context = spawn_test_server_with_auth_and_attributes(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
        )
        .await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/prompts/read-only",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: read-only-prompt\n",
                        "  description: read only description\n",
                        "---\n",
                        "read only body",
                    )
                    .to_string(),
                )
                .expect("create prompt failed");
        }
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let requests = [
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "prompts/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "prompts/get",
                "params": {
                    "name": "read-only-prompt",
                },
            }),
        ];

        /*
         * ReadOnly属性で両prompts methodが
         * 成功することを確認する
         */
        for request in requests {
            let response = context
                .post_json(&request.to_string())
                .header("mcp-session-id", &session_id)
                .send()
                .await
                .expect("send prompts request failed");
            assert_eq!(response.status(), 200);
            let body_text = response
                .text()
                .await
                .expect("read prompts response failed");
            let body_json = parse_sse_payload(&body_text);
            assert!(body_json["result"].is_object());
            assert!(body_json["error"].is_null());
        }

        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// prompt更新・削除・再構成後もlistChangedを
    /// capabilityへ追加しないことを確認する。
    ///
    /// # 注記
    /// 各mutation後に新しいinitialize応答を取得し、
    /// prompts capabilityの通知非対応を検証する。
    ///
    #[actix_web::test]
    async fn prompt_mutations_do_not_enable_list_changed() {
        let context = spawn_test_server().await;
        let page_id = {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/prompts/mutation",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: mutation-prompt\n",
                        "  description: mutation description\n",
                        "---\n",
                        "mutation body",
                    )
                    .to_string(),
                )
                .expect("create prompt failed")
        };

        /*
         * 保存後同期後のcapabilityを確認する
         */
        let (created_session, created_info) =
            context.initialize_with_result().await;
        assert!(
            created_info["result"]["capabilities"]["prompts"]
                ["listChanged"]
                .is_null(),
        );
        let response = context
            .delete_session()
            .header("mcp-session-id", created_session)
            .send()
            .await
            .expect("delete created session failed");
        assert_eq!(response.status(), 204);

        /*
         * soft delete・hard delete後のcapabilityを確認する
         */
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .delete_page_by_id(&page_id)
                .expect("soft delete prompt failed");
        }
        let (deleted_session, deleted_info) =
            context.initialize_with_result().await;
        assert!(
            deleted_info["result"]["capabilities"]["prompts"]
                ["listChanged"]
                .is_null(),
        );
        let response = context
            .delete_session()
            .header("mcp-session-id", deleted_session)
            .send()
            .await
            .expect("delete soft-delete session failed");
        assert_eq!(response.status(), 204);
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .delete_page_by_id_hard(&page_id)
                .expect("hard delete prompt failed");
        }
        let (hard_session, hard_info) =
            context.initialize_with_result().await;
        assert!(
            hard_info["result"]["capabilities"]["prompts"]
                ["listChanged"]
                .is_null(),
        );
        let response = context
            .delete_session()
            .header("mcp-session-id", hard_session)
            .send()
            .await
            .expect("delete hard-delete session failed");
        assert_eq!(response.status(), 204);

        /*
         * 再構成後のcapabilityを確認する
         */
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .rebuild_prompt_candidates()
                .expect("rebuild prompts failed");
        }
        let (rebuilt_session, rebuilt_info) =
            context.initialize_with_result().await;
        assert!(
            rebuilt_info["result"]["capabilities"]["prompts"]
                ["listChanged"]
                .is_null(),
        );
        let response = context
            .delete_session()
            .header("mcp-session-id", rebuilt_session)
            .send()
            .await
            .expect("delete rebuilt session failed");
        assert_eq!(response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// prompts追加後もtoolsとsession routingが
    /// 同じhandshake上で共存することを確認する。
    ///
    /// # 注記
    /// tools/list、prompts/list、get_page、prompts/getを
    /// 同じsessionから交互に呼び出す。
    ///
    #[actix_web::test]
    async fn prompts_preserve_tools_and_session_routing() {
        let context = spawn_test_server().await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            state
                .db()
                .create_page(
                    "/prompts/coexist",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: coexist-prompt\n",
                        "  description: coexist description\n",
                        "---\n",
                        "coexist body",
                    )
                    .to_string(),
                )
                .expect("create prompt failed");
        }
        let (session_id, initialize_json) =
            context.initialize_with_result().await;
        assert!(
            initialize_json["result"]["capabilities"]["tools"]
                .is_object(),
        );
        assert!(
            initialize_json["result"]["capabilities"]["prompts"]
                .is_object(),
        );
        context.send_initialized_notification(&session_id).await;

        /*
         * tools/listとprompts/listのroutingを確認する
         */
        let list_requests = [
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "prompts/list",
                "params": {},
            }),
        ];
        for request in list_requests {
            let response = context
                .post_json(&request.to_string())
                .header("mcp-session-id", &session_id)
                .send()
                .await
                .expect("send list request failed");
            let body_text = response
                .text()
                .await
                .expect("read list response failed");
            let body_json = parse_sse_payload(&body_text);
            assert!(body_json["result"].is_object());
        }

        /*
         * get_pageとprompts/getのroutingを確認する
         */
        let tool_response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get_page failed");
        let tool_text = tool_response
            .text()
            .await
            .expect("read get_page failed");
        let (_, tool_payload) = parse_tool_result_payload(&tool_text);
        assert_eq!(tool_payload["path"], "/mcp/page");
        assert_eq!(tool_payload["content"], "# page\nbody");

        let prompt_body = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "prompts/get",
            "params": {
                "name": "coexist-prompt",
            },
        })
        .to_string();
        let prompt_response = context
            .post_json(&prompt_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send prompts/get failed");
        let prompt_text = prompt_response
            .text()
            .await
            .expect("read prompts/get failed");
        let prompt_json = parse_sse_payload(&prompt_text);
        assert_eq!(
            prompt_json["result"]["messages"][0]["content"]["text"],
            "coexist body",
        );

        let delete_response = context
            .delete_session()
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send session delete failed");
        assert_eq!(delete_response.status(), 204);
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
        assert!(payload_json["instance_id"].is_string());
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
    /// `tools/call(get_page_toc)` が `instance_id` を含む既存応答形を返すことを確認する。
    ///
    #[actix_web::test]
    async fn get_page_toc_tool_call_returns_instance_id() {
        let context = spawn_test_server().await;
        context.put_page(
            "/mcp/page",
            "alice",
            "# page\nintro\n\n## Child\n\nchild body\n",
        );
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "get_page_toc",
            json!({
                "path": "/mcp/page"
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get_page_toc tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read get_page_toc tool call response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 2);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["sections"][0]["title"], "page");
        assert_eq!(payload_json["sections"][1]["title"], "Child");

        context.shutdown().await;
    }

    ///
    /// `tools/call(create_page)` が `instance_id` を含む既存応答形を返すことを確認する。
    ///
    #[actix_web::test]
    async fn create_page_tool_call_returns_instance_id() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Create]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "create_page",
            json!({
                "path": "/mcp/created",
                "content": "# created\nbody"
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send create_page tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read create_page tool call response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/created");
        assert_eq!(payload_json["revision"], 1);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["summary"], "page created");

        context.shutdown().await;
    }

    ///
    /// `tools/call(update_page)` が全文置換として動作し続けることを確認する。
    ///
    #[actix_web::test]
    async fn update_page_tool_call_preserves_full_replace_behavior() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "update_page",
            json!({
                "path": "/mcp/page",
                "content": "# replaced\nnew body"
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send update_page tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read update_page tool call response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 2);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["summary"], "page updated");

        let source = {
            let state = context.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path("/mcp/page")
                .expect("resolve page id failed")
                .expect("page id missing");
            state
                .db()
                .get_page_source(&page_id, 2)
                .expect("get updated source failed")
                .expect("updated source missing")
        };
        assert_eq!(source.source(), "# replaced\nnew body");

        context.shutdown().await;
    }

    ///
    /// `tools/call(edit_page)` 成功時の公開応答形を確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_returns_edit_response() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        let request_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send edit_page tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read edit_page tool call response failed");
        let (body_json, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(body_json["jsonrpc"], "2.0");
        assert_eq!(body_json["id"], 2);
        assert_eq!(body_json["result"]["content"][0]["type"], "text");
        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 2);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["summary"], "page edited");

        context.shutdown().await;
    }

    ///
    /// `tools/call(append_page)` 成功時に `instance_id` を返すことを確認する。
    ///
    #[actix_web::test]
    async fn append_page_tool_call_returns_instance_id() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "append_page",
            json!({
                "path": "/mcp/page",
                "content": "\nnext"
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send append_page tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read append_page tool call response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 1);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["summary"], "page appended (amended)");
        assert_eq!(payload_json["amended"], true);

        context.shutdown().await;
    }

    ///
    /// `tools/call(append_page)` が末尾追記責務を維持することを確認する。
    ///
    #[actix_web::test]
    async fn append_page_tool_call_preserves_append_behavior() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        context.put_page("/mcp/page", "alice", "# page\nbody v2");
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "append_page",
            json!({
                "path": "/mcp/page",
                "content": "\nnext"
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send append_page regression tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read append_page regression response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 2);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["summary"], "page appended (amended)");
        assert_eq!(payload_json["amended"], true);

        let source = {
            let state = context.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path("/mcp/page")
                .expect("resolve page id failed")
                .expect("page id missing");
            state
                .db()
                .get_page_source(&page_id, 2)
                .expect("get appended source failed")
                .expect("appended source missing")
        };
        assert_eq!(source.source(), "# page\nbody v2\nnext");

        context.shutdown().await;
    }

    ///
    /// `tools/call(rename_page)` 成功時に `instance_id` を返すことを確認する。
    ///
    #[actix_web::test]
    async fn rename_page_tool_call_returns_instance_id() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "rename_page",
            json!({
                "path": "/mcp/page",
                "rename_to": "/mcp/page-renamed"
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send rename_page tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read rename_page tool call response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/page-renamed");
        assert_eq!(payload_json["revision"], 2);
        assert!(payload_json["instance_id"].is_string());
        assert_eq!(payload_json["summary"], "page renamed from /mcp/page");

        context.shutdown().await;
    }

    ///
    /// `tools/call(get_page_section)` が既存 selector 解決と section 抽出を維持することを確認する。
    ///
    #[actix_web::test]
    async fn get_page_section_tool_call_preserves_section_resolution() {
        let context = spawn_test_server().await;
        context.put_page(
            "/mcp/page",
            "alice",
            "# page\nintro\n\n## Child\n\nchild body\n\n# next\nother",
        );
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = build_tool_call_body(
            "get_page_section",
            json!({
                "path": "/mcp/page",
                "section": { "by": "title", "value": "Child" }
            }),
        );

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get_page_section tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read get_page_section tool call response failed");
        let (_, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(payload_json["path"], "/mcp/page");
        assert_eq!(payload_json["revision"], 2);
        assert_eq!(payload_json["section"]["title"], "Child");
        assert_eq!(payload_json["section"]["level"], 2);
        assert_eq!(payload_json["content"], "child body\n\n");

        context.shutdown().await;
    }

    ///
    /// `edit_page` の内容整合性エラーが公開コードへ写像されることを確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_maps_consistency_errors() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        context.put_page("/mcp/page", "alice", "# page\nbody v2");

        let stale_revision_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let stale_revision_response = context
            .post_json(&stale_revision_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send stale revision request failed");
        let stale_revision_text = stale_revision_response
            .text()
            .await
            .expect("read stale revision response failed");
        let (_, stale_revision_payload) =
            parse_tool_result_payload(&stale_revision_text);
        assert_eq!(stale_revision_payload["code"], "not_latest_revision");
        assert_eq!(
            stale_revision_payload["message"],
            "revision is not latest"
        );

        let (latest_revision, _) =
            context.latest_revision_and_instance_id("/mcp/page");
        let mismatched_instance_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": latest_revision,
                "instance_id": "instance-mismatch",
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let mismatched_instance_response = context
            .post_json(&mismatched_instance_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send mismatched instance request failed");
        let mismatched_instance_text = mismatched_instance_response
            .text()
            .await
            .expect("read mismatched instance response failed");
        let (_, mismatched_instance_payload) =
            parse_tool_result_payload(&mismatched_instance_text);
        assert_eq!(
            mismatched_instance_payload["code"],
            "instance_id_not_match"
        );
        assert_eq!(
            mismatched_instance_payload["message"],
            "instance_id does not match latest content"
        );

        context.shutdown().await;
    }

    ///
    /// `edit_page` の `invalid_input` / `conflict` / `not_found` 写像を確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_maps_common_error_categories() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");

        let invalid_input_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id.clone(),
                "operation": {
                    "type": "replace_text",
                    "old_text": "",
                    "new_text": "updated"
                }
            }),
        );
        let invalid_input_response = context
            .post_json(&invalid_input_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send invalid input request failed");
        let invalid_input_text = invalid_input_response
            .text()
            .await
            .expect("read invalid input response failed");
        let (_, invalid_input_payload) =
            parse_tool_result_payload(&invalid_input_text);
        assert_eq!(invalid_input_payload["code"], "invalid_input");

        context.acquire_page_lock("/mcp/page", "alice");
        let conflict_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id.clone(),
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let conflict_response = context
            .post_json(&conflict_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send conflict request failed");
        let conflict_text = conflict_response
            .text()
            .await
            .expect("read conflict response failed");
        let (_, conflict_payload) = parse_tool_result_payload(&conflict_text);
        assert_eq!(conflict_payload["code"], "conflict");
        assert_eq!(conflict_payload["message"], "page is locked");

        let missing_page_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let missing_page_session = missing_page_context.initialize_session().await;
        missing_page_context
            .send_initialized_notification(&missing_page_session)
            .await;
        let (missing_page_revision, missing_page_instance_id) =
            missing_page_context.latest_revision_and_instance_id("/mcp/page");
        let not_found_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": missing_page_revision,
                "instance_id": missing_page_instance_id,
                "operation": {
                    "type": "delete_section",
                    "section": { "by": "id", "value": "s-999" }
                }
            }),
        );
        let not_found_response = missing_page_context
            .post_json(&not_found_body)
            .header("mcp-session-id", &missing_page_session)
            .send()
            .await
            .expect("send not found request failed");
        let not_found_text = not_found_response
            .text()
            .await
            .expect("read not found response failed");
        let (_, not_found_payload) = parse_tool_result_payload(&not_found_text);
        assert_eq!(not_found_payload["code"], "not_found");

        context.shutdown().await;
        missing_page_context.shutdown().await;
    }

    ///
    /// `edit_page` の認可と path prefix 制約を確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_honors_authorization_rules() {
        let update_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let update_session = update_context.initialize_session().await;
        update_context
            .send_initialized_notification(&update_session)
            .await;
        let (update_revision, update_instance_id) =
            update_context.latest_revision_and_instance_id("/mcp/page");
        let update_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": update_revision,
                "instance_id": update_instance_id,
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let update_response = update_context
            .post_json(&update_body)
            .header("mcp-session-id", &update_session)
            .send()
            .await
            .expect("send update scope request failed");
        let update_text = update_response
            .text()
            .await
            .expect("read update scope response failed");
        let (_, update_payload) = parse_tool_result_payload(&update_text);
        assert_eq!(update_payload["summary"], "page edited");

        let write_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let write_session = write_context.initialize_session().await;
        write_context
            .send_initialized_notification(&write_session)
            .await;
        let (write_revision, write_instance_id) =
            write_context.latest_revision_and_instance_id("/mcp/page");
        let write_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": write_revision,
                "instance_id": write_instance_id,
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let write_response = write_context
            .post_json(&write_body)
            .header("mcp-session-id", &write_session)
            .send()
            .await
            .expect("send write scope request failed");
        let write_text = write_response
            .text()
            .await
            .expect("read write scope response failed");
        let (_, write_payload) = parse_tool_result_payload(&write_text);
        assert_eq!(write_payload["summary"], "page edited");

        let append_only_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let append_only_session = append_only_context.initialize_session().await;
        append_only_context
            .send_initialized_notification(&append_only_session)
            .await;
        let (append_revision, append_instance_id) =
            append_only_context.latest_revision_and_instance_id("/mcp/page");
        let append_only_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": append_revision,
                "instance_id": append_instance_id,
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let append_only_response = append_only_context
            .post_json(&append_only_body)
            .header("mcp-session-id", &append_only_session)
            .send()
            .await
            .expect("send append only request failed");
        let append_only_text = append_only_response
            .text()
            .await
            .expect("read append only response failed");
        let (_, append_only_payload) =
            parse_tool_result_payload(&append_only_text);
        assert_eq!(append_only_payload["code"], "forbidden");
        assert_eq!(
            append_only_payload["message"],
            "required scope denied: update"
        );

        let prefix_denied_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/private"]),
        )
        .await;
        let prefix_denied_session = prefix_denied_context.initialize_session().await;
        prefix_denied_context
            .send_initialized_notification(&prefix_denied_session)
            .await;
        let (prefix_revision, prefix_instance_id) =
            prefix_denied_context.latest_revision_and_instance_id("/mcp/page");
        let prefix_denied_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": prefix_revision,
                "instance_id": prefix_instance_id,
                "operation": {
                    "type": "replace_text",
                    "old_text": "body",
                    "new_text": "updated"
                }
            }),
        );
        let prefix_denied_response = prefix_denied_context
            .post_json(&prefix_denied_body)
            .header("mcp-session-id", &prefix_denied_session)
            .send()
            .await
            .expect("send prefix denied request failed");
        let prefix_denied_text = prefix_denied_response
            .text()
            .await
            .expect("read prefix denied response failed");
        let (_, prefix_denied_payload) =
            parse_tool_result_payload(&prefix_denied_text);
        assert_eq!(prefix_denied_payload["code"], "forbidden");
        assert_eq!(
            prefix_denied_payload["message"],
            "path prefix denied: /mcp/page"
        );

        update_context.shutdown().await;
        write_context.shutdown().await;
        append_only_context.shutdown().await;
        prefix_denied_context.shutdown().await;
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
