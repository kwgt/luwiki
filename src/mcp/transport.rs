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
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct McpEndpoint {
    /// endpointの公開パス
    path: &'static str,

    /// resource URI authority
    resource_authority: String,
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
    pub(crate) fn new(
        path: &'static str,
        resource_authority: String,
    ) -> Self {
        Self {
            path,
            resource_authority,
        }
    }

    ///
    /// endpointの公開パスを取得
    ///
    /// # 戻り値
    /// endpointの公開パスを返す。
    ///
    pub(crate) fn path(&self) -> &'static str {
        self.path
    }

    ///
    /// resource URI authorityを取得
    ///
    /// # 戻り値
    /// resource URI authorityを返す。
    ///
    pub(crate) fn resource_authority(&self) -> &str {
        &self.resource_authority
    }
}

///
/// MCP endpoint情報の生成
///
/// # 戻り値
/// HTTPサーバ統合層から参照するMCP endpoint情報を返す。
///
pub(crate) fn create_endpoint(resource_authority: String) -> McpEndpoint {
    McpEndpoint::new(MCP_ENDPOINT_PATH, resource_authority)
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
    let resource_authority = endpoint.resource_authority().to_string();

    /*
     * RMCP Streamable HTTP transport を構築する
     */
    let http_service = StreamableHttpService::builder()
        .service_factory(Arc::new(move || {
            Ok(LuwikiMcpServer::with_resource_authority(
                app_state.clone(),
                resource_authority.clone(),
            ))
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
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use actix_web::dev::ServerHandle;
    use actix_web::http::header;
    use actix_web::{App, HttpServer, web};
    use reqwest::Client;
    use serde_json::{Value, json};
    use tempfile::{TempDir, tempdir};

    use super::{create_endpoint, create_scope};
    use crate::audit::AuditSink;
    use crate::audit::buffer::AppendAuditBuffer;
    use crate::audit::rotation::{AuditRotationPolicy, active_log_path};
    use crate::audit::writer::{AuditWriter, AuditWriterConfig};
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
    use crate::mcp::service::DEFAULT_RESOURCE_AUTHORITY;

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

        /// テスト資材を保持する一時ディレクトリ
        _temp_dir: TempDir,

        /// 監査ログ出力ディレクトリ
        audit_dir: PathBuf,
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

        ///
        /// 監査ログをflushしてJSON value一覧を返す
        ///
        /// # 戻り値
        /// 永続化済み監査レコード一覧を返す。
        ///
        fn read_audit_values(&self) -> Vec<Value> {
            self.read_audit_text()
                .lines()
                .map(|line| {
                    serde_json::from_str(line)
                        .expect("decode audit record failed")
                })
                .collect()
        }

        ///
        /// 監査ログをflushしてJSONL本文を返す
        ///
        /// # 戻り値
        /// 永続化済み監査ログ本文を返す。
        ///
        fn read_audit_text(&self) -> String {
            let audit_sink = self
                .state
                .read()
                .expect("lock app state failed")
                .audit_sink()
                .expect("audit sink missing");
            audit_sink
                .write()
                .expect("lock audit sink failed")
                .flush()
                .expect("flush audit sink failed");
            std::fs::read_to_string(
                active_log_path(&self.audit_dir),
            )
            .expect("read audit log failed")
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
        spawn_test_server_with_options(
            session_config,
            scopes,
            path_prefixes,
            attributes,
            false,
        )
        .await
    }

    ///
    /// 監査を有効にしたtransport統合テスト用サーバを起動する
    ///
    async fn spawn_test_server_with_audit(
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
    ) -> TestServerContext {
        spawn_test_server_with_options(
            SessionManagerConfig::default(),
            scopes,
            path_prefixes,
            UserAttributeSet::new(),
            true,
        )
        .await
    }

    ///
    /// 指定条件でtransport統合テスト用サーバを起動する
    ///
    async fn spawn_test_server_with_options(
        session_config: SessionManagerConfig,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
        attributes: UserAttributeSet,
        enable_audit: bool,
    ) -> TestServerContext {
        /*
         * テスト用 AppState を構築する
         */
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_dir = dir.path().join("assets");
        let audit_dir = dir.path().join("audit");
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
        let audit_sink = enable_audit.then(|| {
            Arc::new(RwLock::new(AuditSink::new(
                AppendAuditBuffer::new(),
                AuditWriter::new(AuditWriterConfig {
                    output_dir: audit_dir.clone(),
                    rotation_policy: AuditRotationPolicy::new(1024 * 1024),
                }),
            )))
        });
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_dir),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            audit_sink,
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
                create_endpoint(
                    DEFAULT_RESOURCE_AUTHORITY.to_string(),
                ),
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
            _temp_dir: dir,
            audit_dir,
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

    async fn call_asset_tool(
        context: &TestServerContext,
        session_id: &str,
        name: &str,
        arguments: Value,
    ) -> Value {
        let request_body = build_tool_call_body(name, arguments);
        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send asset tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read asset tool response failed");
        let (body_json, payload_json) =
            parse_tool_result_payload(&body_text);
        assert_eq!(body_json["result"]["isError"], false);

        payload_json
    }

    async fn assert_asset_tool_call_error(
        context: &TestServerContext,
        session_id: &str,
        name: &str,
        arguments: Value,
        expected_code: &str,
        expected_message: &str,
    ) -> Value {
        let request_body = build_tool_call_body(name, arguments);
        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send asset tool error call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read asset tool error response failed");
        let (body_json, payload_json) =
            parse_tool_result_payload(&body_text);
        assert_eq!(body_json["result"]["isError"], true);
        assert_eq!(payload_json["code"], expected_code);
        assert_eq!(payload_json["message"], expected_message);

        payload_json
    }

    async fn assert_edit_tool_call_error(
        context: &TestServerContext,
        session_id: &str,
        arguments: Value,
        expected_code: &str,
        expected_message: Option<&str>,
    ) -> Value {
        let request_body = build_tool_call_body("edit_page", arguments);
        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", session_id)
            .send()
            .await
            .expect("send edit_page error tool call failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read edit_page error tool call response failed");
        let (body_json, payload_json) = parse_tool_result_payload(&body_text);

        assert_eq!(body_json["jsonrpc"], "2.0");
        assert_eq!(body_json["id"], 2);
        assert_eq!(body_json["result"]["isError"], true);
        assert_eq!(body_json["result"]["content"][0]["type"], "text");
        assert_eq!(payload_json["code"], expected_code);
        if let Some(expected_message) = expected_message {
            assert_eq!(payload_json["message"], expected_message);
        }

        payload_json
    }

    fn get_latest_page_source(context: &TestServerContext, path: &str) -> String {
        let state = context.state.read().expect("lock app state failed");
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

        state
            .db()
            .get_page_source(&page_id, latest_revision)
            .expect("get latest source failed")
            .expect("latest source missing")
            .source()
            .to_string()
    }

    fn json_value_contains(value: &Value, needle: &str) -> bool {
        match value {
            Value::String(value) => value.contains(needle),
            Value::Array(values) => values
                .iter()
                .any(|value| json_value_contains(value, needle)),
            Value::Object(values) => values
                .values()
                .any(|value| json_value_contains(value, needle)),
            _ => false,
        }
    }

    fn json_value_has_required_fields(value: &Value, fields: &[&str]) -> bool {
        match value {
            Value::Array(values) => {
                let required_matches = values.iter().all(|value| {
                    value
                        .as_str()
                        .is_some_and(|required| fields.contains(&required))
                }) && values.len() == fields.len();
                required_matches
                    || values
                        .iter()
                        .any(|value| json_value_has_required_fields(value, fields))
            }
            Value::Object(values) => values
                .values()
                .any(|value| json_value_has_required_fields(value, fields)),
            _ => false,
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
        let instructions = body_json["result"]["instructions"]
            .as_str()
            .expect("initialize instructions missing");
        assert!(instructions.contains(
            "Use Bearer authentication on every HTTP request."
        ));
        assert!(instructions.contains(
            "For edit_page.replace_section, do not include the target heading line in content"
        ));
        assert!(instructions.contains("use title to rename the section"));
        assert!(instructions.contains(
            "use include_subsection: true when replacing subsections too"
        ));

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
    /// `tools/list` 経由で公開される `edit_page.replace_section`
    /// schema と説明文を確認する。
    ///
    #[actix_web::test]
    async fn tools_list_exposes_replace_section_extension_schema() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {},
        })
        .to_string();

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send tools/list failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read tools/list response failed");
        let body_json = parse_sse_payload(&body_text);
        let tools = body_json["result"]["tools"]
            .as_array()
            .expect("tools result missing");
        let edit_page_tool = tools
            .iter()
            .find(|tool| tool["name"] == "edit_page")
            .expect("edit_page tool missing");
        let schema = &edit_page_tool["inputSchema"];
        let schema_text = serde_json::to_string(schema)
            .expect("edit_page schema must serialize");

        assert!(schema_text.contains("replace_section"));
        assert!(schema_text.contains("title"));
        assert!(schema_text.contains("include_subsection"));
        let legacy_name = ["include", "sub", "section"].join("_");
        assert!(!schema_text.contains(&legacy_name));
        assert!(json_value_has_required_fields(
            schema,
            &["type", "section", "content"],
        ));
        assert!(json_value_contains(
            schema,
            "対象見出し行は content に含めず、見出し変更には title を使用する。",
        ));
        assert!(json_value_contains(
            schema,
            "置換後本文。対象セクション自身の Markdown 見出し行は含めない。",
        ));
        assert!(json_value_contains(
            schema,
            "true の場合は対象セクション配下のサブセクションを含めて置換する。",
        ));

        context.shutdown().await;
    }

    ///
    /// `tools/list` 経由で公開される参照系アセット tool の
    /// schema と説明文を確認する。
    ///
    #[actix_web::test]
    async fn tools_list_exposes_reference_asset_tool_schemas() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {},
        })
        .to_string();

        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send tools/list failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read tools/list response failed");
        let body_json = parse_sse_payload(&body_text);
        let tools = body_json["result"]["tools"]
            .as_array()
            .expect("tools result missing");

        let list_tool = tools
            .iter()
            .find(|tool| tool["name"] == "list_page_assets")
            .expect("list_page_assets tool missing");
        assert!(json_value_has_required_fields(
            &list_tool["inputSchema"],
            &["path"],
        ));
        let list_schema_text = serde_json::to_string(&list_tool["inputSchema"])
            .expect("list_page_assets schema must serialize");
        assert!(!list_schema_text.contains("asset_id"));
        assert!(!list_schema_text.contains("file_name"));
        assert!(!list_schema_text.contains("data_base64"));
        assert!(json_value_contains(
            list_tool,
            "入力 path で指定されたページに紐付く通常アセットのメタ情報一覧を返す。",
        ));

        for name in [
            "resolve_page_asset",
            "get_page_asset_metadata",
            "get_page_asset_data",
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("asset tool missing");
            assert!(json_value_has_required_fields(
                &tool["inputSchema"],
                &["path", "file_name"],
            ));
            let schema_text = serde_json::to_string(&tool["inputSchema"])
                .expect("asset schema must serialize");
            assert!(!schema_text.contains("asset_id"));
            assert!(!schema_text.contains("data_base64"));
            assert!(!schema_text.contains("if-none-match"));
            assert!(!schema_text.contains("if_modified_since"));
            assert!(!schema_text.contains("etag"));
        }

        context.shutdown().await;
    }

    ///
    /// `tools/list` 経由で公開される更新系アセット tool の
    /// schemaを確認する。
    ///
    /// # 注記
    /// 標準handshake後にtools/listを呼び出し、公開名、必須入力、
    /// 初期版対象外入力が公開されないことを確認する。
    ///
    #[actix_web::test]
    async fn tools_list_exposes_update_asset_tool_schemas() {
        let context = spawn_test_server().await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {},
        })
        .to_string();

        /*
         * tools/list応答の取得
         */
        let response = context
            .post_json(&request_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send tools/list failed");

        assert_eq!(response.status(), 200);
        let body_text = response
            .text()
            .await
            .expect("read tools/list response failed");
        let body_json = parse_sse_payload(&body_text);
        let tools = body_json["result"]["tools"]
            .as_array()
            .expect("tools result missing");

        /*
         * upload_page_asset schemaの確認
         */
        let upload_tool = tools
            .iter()
            .find(|tool| tool["name"] == "upload_page_asset")
            .expect("upload_page_asset tool missing");
        let upload_schema = &upload_tool["inputSchema"];
        assert!(json_value_has_required_fields(
            upload_schema,
            &["path", "file_name", "mime_type", "data_base64"],
        ));
        let upload_properties = upload_schema["properties"]
            .as_object()
            .expect("upload_page_asset properties missing");
        assert_eq!(upload_properties.len(), 4);
        for field in ["path", "file_name", "mime_type", "data_base64"] {
            assert!(upload_properties.contains_key(field));
        }

        /*
         * delete_page_asset schemaの確認
         */
        let delete_tool = tools
            .iter()
            .find(|tool| tool["name"] == "delete_page_asset")
            .expect("delete_page_asset tool missing");
        let delete_schema = &delete_tool["inputSchema"];
        assert!(json_value_has_required_fields(
            delete_schema,
            &["path", "file_name"],
        ));
        let delete_properties = delete_schema["properties"]
            .as_object()
            .expect("delete_page_asset properties missing");
        assert_eq!(delete_properties.len(), 2);
        for field in ["path", "file_name"] {
            assert!(delete_properties.contains_key(field));
        }

        /*
         * 初期版対象外入力が両schemaに存在しないことを確認
         */
        for schema in [upload_schema, delete_schema] {
            let schema_text = serde_json::to_string(schema)
                .expect("update asset schema must serialize");
            for field in [
                "asset_id",
                "local_path",
                "local_file_path",
                "file_path",
                "lock_token",
                "overwrite",
            ] {
                assert!(!schema_text.contains(field));
            }
        }

        context.shutdown().await;
    }

    ///
    /// handshake後の更新系アセットtool成功と永続状態を確認する。
    ///
    /// # 注記
    /// 同一sessionでuploadとdeleteを実行し、tool result、DB状態、
    /// 保存ファイル、監査JSONLを横断確認する。
    ///
    #[actix_web::test]
    async fn update_asset_tools_persist_state_and_audit_after_handshake() {
        let context = spawn_test_server_with_audit(
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/mcp"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        /*
         * uploadのtool resultを確認する
         */
        let upload_body = build_tool_call_body(
            "upload_page_asset",
            json!({
                "path": "/mcp/./page",
                "file_name": "image.bin",
                "mime_type": "application/octet-stream",
                "data_base64": "aW1hZ2U=",
            }),
        );
        let upload_response = context
            .post_json(&upload_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send upload_page_asset failed");
        assert_eq!(upload_response.status(), 200);
        let upload_text = upload_response
            .text()
            .await
            .expect("read upload_page_asset response failed");
        let (upload_result, upload_payload) =
            parse_tool_result_payload(&upload_text);
        assert_eq!(upload_result["result"]["isError"], false);
        assert_eq!(upload_payload["asset"]["path"], "/mcp/page");
        assert_eq!(upload_payload["asset"]["file_name"], "image.bin");
        assert_eq!(
            upload_payload["asset"]["mime_type"],
            "application/octet-stream",
        );
        assert_eq!(upload_payload["asset"]["size"], 5);
        assert_eq!(upload_payload["summary"], "asset uploaded");
        let public_asset_id = upload_payload["asset"]["asset_id"]
            .as_str()
            .expect("uploaded asset id missing")
            .to_string();

        /*
         * upload後のDB状態と保存ファイルを確認する
         */
        let asset_id = {
            let state = context.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path("/mcp/page")
                .expect("resolve page id failed")
                .expect("page id missing");
            let asset_id = state
                .db()
                .get_asset_id_by_page_file(&page_id, "image.bin")
                .expect("lookup uploaded asset failed")
                .expect("uploaded asset missing");
            let asset_info = state
                .db()
                .get_asset_info_by_id(&asset_id)
                .expect("get uploaded asset info failed")
                .expect("uploaded asset info missing");
            let data = state
                .db()
                .read_asset_data(&asset_id)
                .expect("read uploaded asset failed");

            assert_eq!(asset_id.to_string(), public_asset_id);
            assert_eq!(asset_info.file_name(), "image.bin");
            assert_eq!(asset_info.mime(), "application/octet-stream");
            assert_eq!(asset_info.size(), 5);
            assert!(!asset_info.deleted());
            assert_eq!(data, b"image");
            assert!(state.db().asset_file_path(&asset_id).is_file());

            asset_id
        };

        /*
         * deleteのtool resultと論理削除状態を確認する
         */
        let delete_body = build_tool_call_body(
            "delete_page_asset",
            json!({
                "path": "/mcp/./page",
                "file_name": "image.bin",
            }),
        );
        let delete_response = context
            .post_json(&delete_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send delete_page_asset failed");
        assert_eq!(delete_response.status(), 200);
        let delete_text = delete_response
            .text()
            .await
            .expect("read delete_page_asset response failed");
        let (delete_result, delete_payload) =
            parse_tool_result_payload(&delete_text);
        assert_eq!(delete_result["result"]["isError"], false);
        assert_eq!(delete_payload["asset_id"], public_asset_id);
        assert_eq!(delete_payload["path"], "/mcp/page");
        assert_eq!(delete_payload["file_name"], "image.bin");
        assert_eq!(delete_payload["summary"], "asset deleted");

        {
            let state = context.state.read().expect("lock app state failed");
            let page_id = state
                .db()
                .get_page_id_by_path("/mcp/page")
                .expect("resolve page id failed")
                .expect("page id missing");
            let asset_info = state
                .db()
                .get_asset_info_by_id(&asset_id)
                .expect("get deleted asset info failed")
                .expect("deleted asset info missing");
            let lookup_id = state
                .db()
                .get_asset_id_by_page_file(&page_id, "image.bin")
                .expect("lookup deleted asset failed");

            assert!(asset_info.deleted());
            assert!(lookup_id.is_none());
            assert!(state.db().asset_file_path(&asset_id).is_file());
        }

        /*
         * HTTP入口から監査JSONLまでの接続を確認する
         */
        let expected_user_id = {
            let state = context.state.read().expect("lock app state failed");
            state
                .db()
                .get_user_id_by_name("alice")
                .expect("resolve audit user id failed")
                .expect("audit user id missing")
                .to_string()
        };
        let expected_token_id = context.bearer_token_id.to_string();
        let records = context.read_audit_values();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["operation"], "upload_page_asset");
        assert_eq!(records[0]["result"], "success");
        assert_eq!(records[0]["target_path"], "/mcp/page");
        assert!(records[0]["revision"].is_null());
        assert_eq!(records[1]["operation"], "delete_page_asset");
        assert_eq!(records[1]["result"], "success");
        assert_eq!(records[1]["target_path"], "/mcp/page");
        assert!(records[1]["revision"].is_null());
        for record in records {
            assert_eq!(record["user_id"], expected_user_id);
            assert_eq!(record["token_id"], expected_token_id);
            assert_eq!(record["address"], "127.0.0.1");
        }

        context.shutdown().await;
    }

    ///
    /// 更新系アセットtoolのHTTP認可境界を確認する。
    ///
    #[actix_web::test]
    async fn update_asset_tools_enforce_authorization_over_http() {
        /*
         * required scope不足を確認する
         */
        let scope_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let scope_session = scope_context.initialize_session().await;
        scope_context
            .send_initialized_notification(&scope_session)
            .await;
        assert_asset_tool_call_error(
            &scope_context,
            &scope_session,
            "upload_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "scope.bin",
                "mime_type": "application/octet-stream",
                "data_base64": "c2NvcGU=",
            }),
            "forbidden",
            "required scope denied: update",
        )
        .await;

        /*
         * path prefix違反を確認する
         */
        let prefix_context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/private"]),
        )
        .await;
        let prefix_session = prefix_context.initialize_session().await;
        prefix_context
            .send_initialized_notification(&prefix_session)
            .await;
        assert_asset_tool_call_error(
            &prefix_context,
            &prefix_session,
            "delete_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "missing.bin",
            }),
            "forbidden",
            "path prefix denied: /mcp/page",
        )
        .await;

        /*
         * ReadOnly拒否を確認する
         */
        let read_only_context = spawn_test_server_with_auth_and_attributes(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
        )
        .await;
        let read_only_session =
            read_only_context.initialize_session().await;
        read_only_context
            .send_initialized_notification(&read_only_session)
            .await;
        assert_asset_tool_call_error(
            &read_only_context,
            &read_only_session,
            "upload_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "read-only.bin",
                "mime_type": "application/octet-stream",
                "data_base64": "cmVhZC1vbmx5",
            }),
            "forbidden",
            "read only denied: write operation is not allowed",
        )
        .await;

        scope_context.shutdown().await;
        prefix_context.shutdown().await;
        read_only_context.shutdown().await;
    }

    ///
    /// 不正session ID付きPOSTがtransport段階で拒否されることを
    /// 確認する。
    ///
    #[actix_web::test]
    async fn post_tool_call_with_invalid_session_returns_401() {
        let context = spawn_test_server().await;
        let response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", "invalid-session-id")
            .send()
            .await
            .expect("send invalid session request failed");

        assert_eq!(response.status(), 401);
        let body_text = response
            .text()
            .await
            .expect("read invalid session response failed");
        assert_eq!(body_text, "Unauthorized: Session not found");

        context.shutdown().await;
    }

    ///
    /// 更新系アセットtoolの代表失敗をMCP errorへ変換できることを
    /// 確認する。
    ///
    #[actix_web::test]
    async fn update_asset_tool_failures_map_to_mcp_errors() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        assert_asset_tool_call_error(
            &context,
            &session_id,
            "upload_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "invalid.bin",
                "mime_type": "application/octet-stream",
                "data_base64": "not-valid-base64",
            }),
            "invalid_input",
            "data_base64 is invalid",
        )
        .await;
        assert_asset_tool_call_error(
            &context,
            &session_id,
            "delete_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "missing.bin",
            }),
            "not_found",
            "asset not found",
        )
        .await;

        context.acquire_page_lock("/mcp/page", "alice");
        assert_asset_tool_call_error(
            &context,
            &session_id,
            "upload_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "locked.bin",
                "mime_type": "application/octet-stream",
                "data_base64": "bG9ja2Vk",
            }),
            "conflict",
            "page is locked",
        )
        .await;

        context.shutdown().await;
    }

    ///
    /// read scope拒否と不正base64応答が入力値を秘匿することを確認する。
    #[actix_web::test]
    async fn upload_page_asset_failures_redact_data_base64() {
        const READ_SENTINEL: &str = "secret-read-scope:not-base64";
        const UPDATE_SENTINEL: &str = "secret-update-scope:not-base64";

        /*
         * read scopeではdecodeより先に認可が失敗することを確認する
         */
        let read_context = spawn_test_server_with_audit(
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let read_session = read_context.initialize_session().await;
        read_context
            .send_initialized_notification(&read_session)
            .await;
        let read_body = build_tool_call_body(
            "upload_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "read-secret.bin",
                "mime_type": "application/octet-stream",
                "data_base64": READ_SENTINEL,
            }),
        );
        let read_response = read_context
            .post_json(&read_body)
            .header("mcp-session-id", &read_session)
            .send()
            .await
            .expect("send read scope upload failed");
        assert_eq!(read_response.status(), 200);
        let read_text = read_response
            .text()
            .await
            .expect("read scope upload response failed");
        assert!(!read_text.contains(READ_SENTINEL));
        let (read_result, read_payload) =
            parse_tool_result_payload(&read_text);
        assert_eq!(read_result["result"]["isError"], true);
        assert_eq!(read_payload["code"], "forbidden");
        assert_eq!(
            read_payload["message"],
            "required scope denied: update",
        );
        let read_audit = read_context.read_audit_text();
        assert!(!read_audit.contains(READ_SENTINEL));
        let read_record: Value = serde_json::from_str(
            read_audit.lines().next().expect("read audit record missing"),
        )
        .expect("decode read audit record failed");
        assert_eq!(read_record["operation"], "upload_page_asset");
        assert_eq!(read_record["result"], "scope_denied");

        /*
         * update scopeでは入力検証へ到達することを確認する
         */
        let update_context = spawn_test_server_with_audit(
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let update_session = update_context.initialize_session().await;
        update_context
            .send_initialized_notification(&update_session)
            .await;
        let update_body = build_tool_call_body(
            "upload_page_asset",
            json!({
                "path": "/mcp/page",
                "file_name": "update-secret.bin",
                "mime_type": "application/octet-stream",
                "data_base64": UPDATE_SENTINEL,
            }),
        );
        let update_response = update_context
            .post_json(&update_body)
            .header("mcp-session-id", &update_session)
            .send()
            .await
            .expect("send invalid base64 upload failed");
        assert_eq!(update_response.status(), 200);
        let update_text = update_response
            .text()
            .await
            .expect("read invalid base64 response failed");
        assert!(!update_text.contains(UPDATE_SENTINEL));
        let (update_result, update_payload) =
            parse_tool_result_payload(&update_text);
        assert_eq!(update_result["result"]["isError"], true);
        assert_eq!(update_payload["code"], "invalid_input");
        assert_eq!(update_payload["message"], "data_base64 is invalid");
        let update_audit = update_context.read_audit_text();
        assert!(!update_audit.contains(UPDATE_SENTINEL));
        let update_record: Value = serde_json::from_str(
            update_audit
                .lines()
                .next()
                .expect("update audit record missing"),
        )
        .expect("decode update audit record failed");
        assert_eq!(update_record["operation"], "upload_page_asset");
        assert_eq!(update_record["result"], "invalid_input");

        read_context.shutdown().await;
        update_context.shutdown().await;
    }

    ///
    /// transport失敗がrequest bodyと認証情報を露出しないことを
    /// 確認する。
    ///
    #[actix_web::test]
    async fn transport_failures_redact_request_and_authorization() {
        const REQUEST_SENTINEL: &str = "secret-transport-request-body";
        const AUTH_SENTINEL: &str = "Bearer secret-authorization-header";

        let context = spawn_test_server_with_audit(
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        /*
         * 監査ログの比較基準となる正常呼び出しを記録する
         */
        let baseline_response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send baseline get_page failed");
        assert_eq!(baseline_response.status(), 200);
        let baseline_text = baseline_response
            .text()
            .await
            .expect("read baseline get_page response failed");
        assert_eq!(
            parse_tool_result_payload(&baseline_text).1["path"],
            "/mcp/page",
        );
        assert_eq!(context.read_audit_values().len(), 1);

        /*
         * session失敗がrequest bodyを応答へ含めないことを確認する
         */
        let invalid_session_body = build_tool_call_body(
            "get_page",
            json!({
                "path": "/mcp/page",
                "request_marker": REQUEST_SENTINEL,
            }),
        );
        let invalid_session_response = context
            .post_json(&invalid_session_body)
            .header("mcp-session-id", "invalid-session-id")
            .send()
            .await
            .expect("send request with invalid session failed");
        assert_eq!(invalid_session_response.status(), 401);
        let invalid_session_text = invalid_session_response
            .text()
            .await
            .expect("read invalid session response failed");
        assert!(!invalid_session_text.contains(REQUEST_SENTINEL));
        assert!(!invalid_session_text.contains(&context.bearer_token));

        /*
         * 認証失敗がAuthorization値を応答へ含めないことを確認する
         */
        let invalid_auth_response = context
            .client
            .post(&context.base_url)
            .header(header::AUTHORIZATION.as_str(), AUTH_SENTINEL)
            .header(header::CONTENT_TYPE.as_str(), "application/json")
            .header(header::ACCEPT.as_str(), ACCEPT_BOTH)
            .body(GET_PAGE_TOOL_CALL_BODY)
            .send()
            .await
            .expect("send request with invalid bearer failed");
        assert_eq!(invalid_auth_response.status(), 401);
        let invalid_auth_text = invalid_auth_response
            .text()
            .await
            .expect("read invalid bearer response failed");
        assert!(!invalid_auth_text.contains(AUTH_SENTINEL));
        assert!(!invalid_auth_text.contains(REQUEST_SENTINEL));

        /*
         * JSON-RPC protocol errorがparamsを応答へ含めないことを確認する
         */
        let protocol_body = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "unknown/method",
            "params": {
                "request_marker": REQUEST_SENTINEL,
            },
        })
        .to_string();
        let protocol_response = context
            .post_json(&protocol_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send unknown method request failed");
        assert_eq!(protocol_response.status(), 200);
        let protocol_text = protocol_response
            .text()
            .await
            .expect("read protocol error response failed");
        assert!(!protocol_text.contains(REQUEST_SENTINEL));
        assert!(!protocol_text.contains(&context.bearer_token));
        let protocol_json = parse_sse_payload(&protocol_text);
        assert!(protocol_json["error"].is_object());

        /*
         * transport失敗が監査レコードを追加しないことを確認する
         */
        let audit_text = context.read_audit_text();
        assert_eq!(audit_text.lines().count(), 1);
        assert!(!audit_text.contains(REQUEST_SENTINEL));
        assert!(!audit_text.contains(AUTH_SENTINEL));
        assert!(!audit_text.contains(&context.bearer_token));

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
                        "  resource_path: /docs/transport\n",
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
            &"luwiki://local.luwiki/docs/transport"
        ));

        /*
         * resources/readの標準応答を確認する
         */
        let read_body = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/read",
            "params": {
                "uri": "luwiki://local.luwiki/docs/transport",
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
            "luwiki://local.luwiki/docs/transport",
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
    /// resourcesのscope不足、resource ACL秘匿、ReadOnly許可を検証する。
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
                        "  resource_path: /docs/routing\n",
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
                "uri": "luwiki://local.luwiki/docs/routing",
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
         * ページ由来resourceはpath prefix範囲外でも取得できる
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
                        "  resource_path: /docs/private\n",
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
                .create_page(
                    "/private/denied-resource",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: resource\n",
                        "  resource_path: /docs/denied\n",
                        "  name: denied-resource\n",
                        "  description: denied resource description\n",
                        "  resource_acl:\n",
                        "    default:\n",
                        "      read: deny\n",
                        "---\n",
                        "denied body",
                    )
                    .to_string(),
                )
                .expect("create denied resource failed");
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
                "uri": "luwiki://local.luwiki/docs/private",
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
        assert_eq!(
            prefix_json["result"]["contents"][0]["text"],
            "private body",
        );
        assert!(prefix_json["error"].is_null());

        /*
         * resource ACLで拒否されたページ由来resourceはnot_foundへ秘匿する
         */
        let denied_body = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "resources/read",
            "params": {
                "uri": "luwiki://local.luwiki/docs/denied",
            },
        })
        .to_string();
        let denied_response = prefix_context
            .post_json(&denied_body)
            .header("mcp-session-id", &prefix_session)
            .send()
            .await
            .expect("send ACL denied resource request failed");
        assert_eq!(denied_response.status(), 200);
        let denied_text = denied_response
            .text()
            .await
            .expect("read ACL denied resource response failed");
        let denied_json = parse_sse_payload(&denied_text);
        assert_eq!(denied_json["error"]["code"], -32602);
        assert_eq!(
            denied_json["error"]["message"],
            "resource not found",
        );
        assert_eq!(
            denied_json["error"]["data"]["code"],
            "not_found",
        );
        let serialized_denied = denied_json.to_string();
        assert!(!serialized_denied.contains("/private/denied-resource"));
        assert!(!serialized_denied.contains("denied-resource"));

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
                        "  resource_path: /docs/read-only\n",
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
                    "uri": "luwiki://local.luwiki/docs/read-only",
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
    /// tools、ページtool、参照系・更新系アセットtool、prompts、
    /// resourcesが同じMCP構成とsessionで共存することを確認する。
    ///
    #[actix_web::test]
    async fn reference_assets_coexist_with_tools_prompts_and_resources() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            let page_id = state
                .db()
                .create_page(
                    "/mcp/coexist-assets",
                    "alice",
                    "# assets".to_string(),
                )
                .expect("create asset page failed");
            state
                .db()
                .create_asset(
                    &page_id,
                    "coexist.bin",
                    "application/octet-stream",
                    "alice",
                    b"asset-body",
                )
                .expect("create asset failed");
            state
                .db()
                .create_page(
                    "/prompts/asset-coexist",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: prompt\n",
                        "  name: asset-coexist-prompt\n",
                        "  description: asset coexist prompt\n",
                        "---\n",
                        "prompt body",
                    )
                    .to_string(),
                )
                .expect("create prompt failed");
            state
                .db()
                .create_page(
                    "/resources/asset-coexist",
                    "alice",
                    concat!(
                        "---\n",
                        "mcp:\n",
                        "  primitive: resource\n",
                        "  resource_path: /asset/coexist\n",
                        "  name: asset-coexist-resource\n",
                        "  description: asset coexist resource\n",
                        "  mime_type: text/plain\n",
                        "---\n",
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
         * tools/listで既存ページtoolと参照系アセットtoolの共存を確認する
         */
        let tools_body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {},
        })
        .to_string();
        let tools_response = context
            .post_json(&tools_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send tools/list failed");
        assert_eq!(tools_response.status(), 200);
        let tools_text = tools_response
            .text()
            .await
            .expect("read tools/list response failed");
        let tools_json = parse_sse_payload(&tools_text);
        let tool_names = tools_json["result"]["tools"]
            .as_array()
            .expect("tools result missing")
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name missing"))
            .collect::<Vec<_>>();
        for tool_name in [
            "get_page",
            "get_page_toc",
            "list_pages",
            "search_pages",
            "get_page_section",
            "list_page_assets",
            "resolve_page_asset",
            "get_page_asset_metadata",
            "get_page_asset_data",
            "upload_page_asset",
            "delete_page_asset",
        ] {
            assert!(tool_names.contains(&tool_name));
        }

        /*
         * 同じsessionでページtool、アセットtool、prompts、resourcesを順に呼び出す
         */
        let get_page_response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send get_page failed");
        assert_eq!(get_page_response.status(), 200);
        let get_page_text = get_page_response
            .text()
            .await
            .expect("read get_page response failed");
        let (_, get_page_payload) = parse_tool_result_payload(&get_page_text);
        assert_eq!(get_page_payload["path"], "/mcp/page");

        let asset_payload = call_asset_tool(
            &context,
            &session_id,
            "list_page_assets",
            json!({
                "path": "/mcp/coexist-assets",
            }),
        )
        .await;
        assert_eq!(asset_payload["path"], "/mcp/coexist-assets");
        assert_eq!(
            asset_payload["items"][0]["file_name"],
            "coexist.bin",
        );

        for tool_name in [
            "resolve_page_asset",
            "get_page_asset_metadata",
            "get_page_asset_data",
        ] {
            let payload = call_asset_tool(
                &context,
                &session_id,
                tool_name,
                json!({
                    "path": "/mcp/coexist-assets",
                    "file_name": "coexist.bin",
                }),
            )
            .await;
            if tool_name == "resolve_page_asset" {
                assert_eq!(payload["file_name"], "coexist.bin");
            } else {
                assert_eq!(payload["asset"]["file_name"], "coexist.bin");
            }
            if tool_name == "get_page_asset_data" {
                assert_eq!(payload["data_base64"], "YXNzZXQtYm9keQ==");
            }
        }

        let upload_payload = call_asset_tool(
            &context,
            &session_id,
            "upload_page_asset",
            json!({
                "path": "/mcp/coexist-assets",
                "file_name": "uploaded.bin",
                "mime_type": "application/octet-stream",
                "data_base64": "dXBsb2FkZWQ=",
            }),
        )
        .await;
        assert_eq!(upload_payload["asset"]["file_name"], "uploaded.bin");
        assert_eq!(upload_payload["summary"], "asset uploaded");

        let delete_payload = call_asset_tool(
            &context,
            &session_id,
            "delete_page_asset",
            json!({
                "path": "/mcp/coexist-assets",
                "file_name": "uploaded.bin",
            }),
        )
        .await;
        assert_eq!(delete_payload["file_name"], "uploaded.bin");
        assert_eq!(delete_payload["summary"], "asset deleted");

        let prompts_body = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/list",
            "params": {},
        })
        .to_string();
        let prompts_response = context
            .post_json(&prompts_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send prompts/list failed");
        assert_eq!(prompts_response.status(), 200);
        let prompts_text = prompts_response
            .text()
            .await
            .expect("read prompts/list response failed");
        let prompts_json = parse_sse_payload(&prompts_text);
        let prompt_names = prompts_json["result"]["prompts"]
            .as_array()
            .expect("prompts result missing")
            .iter()
            .map(|prompt| prompt["name"].as_str().expect("prompt name missing"))
            .collect::<Vec<_>>();
        assert!(prompt_names.contains(&"asset-coexist-prompt"));

        let resources_body = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/list",
            "params": {},
        })
        .to_string();
        let resources_response = context
            .post_json(&resources_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send resources/list failed");
        assert_eq!(resources_response.status(), 200);
        let resources_text = resources_response
            .text()
            .await
            .expect("read resources/list response failed");
        let resources_json = parse_sse_payload(&resources_text);
        let resource_uris = resources_json["result"]["resources"]
            .as_array()
            .expect("resources result missing")
            .iter()
            .map(|resource| {
                resource["uri"]
                    .as_str()
                    .expect("resource uri missing")
            })
            .collect::<Vec<_>>();
        assert!(resource_uris.contains(
            &"luwiki://local.luwiki/asset/coexist",
        ));

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
    /// Streamable HTTPのJSON入力で共通パス規則を確認する
    ///
    /// # 概要
    /// 実sessionからdot解決、percent保持、raw禁止の公開応答を比較する。
    ///
    /// # 注記
    /// get_pageを呼び、許可pathのpayloadと不正pathのtool errorを確認。
    ///
    /// # 戻り値
    /// なし
    ///
    #[actix_web::test]
    async fn tools_call_path_input_preserves_json_and_raw_validation() {
        /*
         * JSONのpercent表記が指す保存ページを準備する
         */
        let context = spawn_test_server().await;
        {
            let state = context.state.read().unwrap();
            state.db().create_page(
                "/mcp/%2E%2E/page", "alice", "literal".to_string(),
            ).unwrap();
        }
        let session = context.initialize_session().await;
        context.send_initialized_notification(&session).await;

        /*
         * dotだけを解決しpercent文字列をURLとして解釈しない
         */
        for (input, expected) in [
            ("/mcp/./page", "/mcp/page"),
            ("/mcp/%2E%2E/page", "/mcp/%2E%2E/page"),
        ] {
            let payload = call_asset_tool(
                &context, &session, "get_page", json!({"path": input}),
            ).await;
            assert_eq!(payload["path"], expected);
        }

        /*
         * raw不正がdot縮約等で受け入れられないことを確認する
         */
        for (input, message) in [
            ("/mcp/page/", "path must not have a trailing slash"),
            ("/mcp//page", "path must not contain consecutive slashes"),
            ("/mcp/../../page", "path must not traverse above root"),
            ("/mcp/bad name", "path contains invalid character"),
        ] {
            assert_asset_tool_call_error(
                &context, &session, "get_page", json!({"path": input}),
                "invalid_input", message,
            ).await;
        }
        context.shutdown().await;
    }

    ///
    /// 参照系アセット4 tool の代表呼び出しが
    /// Streamable HTTP経由で成功することを確認する。
    ///
    #[actix_web::test]
    async fn reference_asset_tool_calls_return_expected_payloads() {
        let context = spawn_test_server().await;
        let asset_id = {
            let state = context
                .state
                .read()
                .expect("lock app state failed");
            let page_id = state
                .db()
                .create_page(
                    "/mcp/assets",
                    "alice",
                    "# assets\nbody".to_string(),
                )
                .expect("create asset page failed");
            state
                .db()
                .create_asset(
                    &page_id,
                    "asset.bin",
                    "application/octet-stream",
                    "alice",
                    b"secret-payload",
                )
                .expect("create asset failed")
        };
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        /*
         * list_page_assets の代表応答を確認する
         */
        let list_body = build_tool_call_body(
            "list_page_assets",
            json!({
                "path": "/mcp/./assets",
            }),
        );
        let list_response = context
            .post_json(&list_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send list_page_assets failed");
        assert_eq!(list_response.status(), 200);
        let list_text = list_response
            .text()
            .await
            .expect("read list_page_assets response failed");
        let (_, list_payload) = parse_tool_result_payload(&list_text);
        assert_eq!(list_payload["path"], "/mcp/assets");
        assert_eq!(list_payload["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            list_payload["items"][0]["asset_id"],
            asset_id.to_string(),
        );
        assert_eq!(list_payload["items"][0]["file_name"], "asset.bin");
        assert_eq!(
            list_payload["items"][0]["mime_type"],
            "application/octet-stream",
        );
        assert_eq!(list_payload["items"][0]["size"], 14);
        assert!(list_payload["items"][0]["timestamp"].is_string());
        assert_eq!(list_payload["items"][0]["username"], "alice");
        assert!(list_payload["items"][0]["instance_id"].is_string());
        assert!(list_payload.get("data_base64").is_none());

        /*
         * file_name指定3 tool の代表応答を確認する
         */
        for (tool_name, expects_data) in [
            ("resolve_page_asset", false),
            ("get_page_asset_metadata", false),
            ("get_page_asset_data", true),
        ] {
            let request_body = build_tool_call_body(
                tool_name,
                json!({
                    "path": "/mcp/./assets",
                    "file_name": "asset.bin",
                }),
            );
            let response = context
                .post_json(&request_body)
                .header("mcp-session-id", &session_id)
                .send()
                .await
                .expect("send asset tool call failed");
            assert_eq!(response.status(), 200);
            let body_text = response
                .text()
                .await
                .expect("read asset tool call response failed");
            let (body_json, payload_json) =
                parse_tool_result_payload(&body_text);
            assert_eq!(body_json["result"]["isError"], false);
            if tool_name == "resolve_page_asset" {
                assert_eq!(payload_json["asset_id"], asset_id.to_string());
                assert_eq!(payload_json["path"], "/mcp/assets");
                assert_eq!(payload_json["file_name"], "asset.bin");
            } else {
                assert_eq!(
                    payload_json["asset"]["asset_id"],
                    asset_id.to_string(),
                );
                assert_eq!(payload_json["asset"]["path"], "/mcp/assets");
                assert_eq!(payload_json["asset"]["file_name"], "asset.bin");
                assert_eq!(
                    payload_json["asset"]["mime_type"],
                    "application/octet-stream",
                );
                assert_eq!(payload_json["asset"]["size"], 14);
                assert_eq!(payload_json["asset"]["username"], "alice");
            }
            if expects_data {
                assert_eq!(
                    payload_json["data_base64"],
                    "c2VjcmV0LXBheWxvYWQ=",
                );
            } else {
                assert!(payload_json.get("data_base64").is_none());
            }
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
    /// `tools/call(edit_page)` 経由で `replace_section` 拡張の
    /// 代表正常系が保存結果へ反映されることを確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_applies_replace_section_extension_fields() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        context.put_page(
            "/mcp/page",
            "alice",
            concat!(
                "# page\nintro\n\n",
                "## Target\nold body\n\n",
                "### Child\nchild body\n\n",
                "## Next\nnext body\n",
            ),
        );
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        /*
         * title と include_subsection:false の組み合わせで、
         * 見出し変更と既存サブセクション保持を確認する
         */
        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        let rename_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Target",
                    "title": "Renamed",
                    "content": "new body",
                    "include_subsection": false
                }
            }),
        );
        let rename_response = context
            .post_json(&rename_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send replace_section rename request failed");
        assert_eq!(rename_response.status(), 200);
        let rename_text = rename_response
            .text()
            .await
            .expect("read replace_section rename response failed");
        let (_, rename_payload) = parse_tool_result_payload(&rename_text);
        assert_eq!(rename_payload["path"], "/mcp/page");
        assert!(rename_payload["instance_id"].is_string());
        assert_eq!(rename_payload["summary"], "page edited");
        assert_eq!(
            get_latest_page_source(&context, "/mcp/page"),
            concat!(
                "# page\nintro\n\n",
                "## Renamed\nnew body\n\n",
                "### Child\nchild body\n\n",
                "## Next\nnext body\n",
            ),
        );

        /*
         * include_subsection 未指定が false と同じく
         * 既存サブセクションを保持することを確認する
         */
        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        let default_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Renamed",
                    "content": "default body"
                }
            }),
        );
        let default_response = context
            .post_json(&default_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send replace_section default request failed");
        assert_eq!(default_response.status(), 200);
        let default_text = default_response
            .text()
            .await
            .expect("read replace_section default response failed");
        let (_, default_payload) = parse_tool_result_payload(&default_text);
        assert_eq!(default_payload["summary"], "page edited");
        assert_eq!(
            get_latest_page_source(&context, "/mcp/page"),
            concat!(
                "# page\nintro\n\n",
                "## Renamed\ndefault body\n\n",
                "### Child\nchild body\n\n",
                "## Next\nnext body\n",
            ),
        );

        /*
         * include_subsection:true で対象配下の
         * サブセクション込み置換を確認する
         */
        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        let include_body = build_tool_call_body(
            "edit_page",
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Renamed",
                    "content": "whole body\n\n### Replacement\nchild replaced",
                    "include_subsection": true
                }
            }),
        );
        let include_response = context
            .post_json(&include_body)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send replace_section include request failed");
        assert_eq!(include_response.status(), 200);
        let include_text = include_response
            .text()
            .await
            .expect("read replace_section include response failed");
        let (_, include_payload) = parse_tool_result_payload(&include_text);
        assert_eq!(include_payload["summary"], "page edited");
        assert_eq!(
            get_latest_page_source(&context, "/mcp/page"),
            concat!(
                "# page\nintro\n\n",
                "## Renamed\nwhole body\n\n",
                "### Replacement\nchild replaced\n\n",
                "## Next\nnext body\n",
            ),
        );

        context.shutdown().await;
    }

    ///
    /// `tools/call(edit_page)` 経由で `replace_section.content` の
    /// 見出し行制約違反が tool result 内の論理エラーになることを確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_rejects_invalid_replace_section_content() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        context.put_page(
            "/mcp/page",
            "alice",
            concat!(
                "# Root\nintro\n\n",
                "## Child\nchild body\n\n",
                "# Next\nnext body\n",
            ),
        );
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");

        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Root",
                    "content": "\n# Root\nupdated",
                    "include_subsection": true
                }
            }),
            "invalid_input",
            Some(
                "replace_section.content must not include the target heading line",
            ),
        )
        .await;

        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Root",
                    "title": "Renamed root",
                    "content": "\n# Renamed root\nupdated",
                    "include_subsection": true
                }
            }),
            "invalid_input",
            Some(
                "replace_section.content must not include the target heading line",
            ),
        )
        .await;

        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Root",
                    "content": "updated\n\n## Unexpected\nbody",
                    "include_subsection": false
                }
            }),
            "invalid_input",
            Some(
                "replace_section.content must not include Markdown heading lines",
            ),
        )
        .await;

        let (revision, instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": revision,
                "instance_id": instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Child",
                    "content": "updated\n\n## Peer\nbody",
                    "include_subsection": true
                }
            }),
            "invalid_input",
            Some(
                "replace_section.content must not include same or higher level headings",
            ),
        )
        .await;

        context.shutdown().await;
    }

    ///
    /// `replace_section.content` が不正でも、既存の整合性確認と
    /// selector 解決のエラー分類が優先されることを確認する。
    ///
    #[actix_web::test]
    async fn edit_page_tool_call_prioritizes_replace_section_existing_errors() {
        let context = spawn_test_server_with_auth(
            SessionManagerConfig::default(),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/"]),
        )
        .await;
        context.put_page("/mcp/page", "alice", "# Root\nbody\n");
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;
        let (stale_revision, stale_instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        context.put_page("/mcp/page", "alice", "# Root\nlatest\n");

        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": stale_revision,
                "instance_id": stale_instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Root",
                    "content": "\n# Root\nupdated",
                    "include_subsection": true
                }
            }),
            "not_latest_revision",
            Some("revision is not latest"),
        )
        .await;

        let (latest_revision, _) =
            context.latest_revision_and_instance_id("/mcp/page");
        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": latest_revision,
                "instance_id": "instance-mismatch",
                "operation": {
                    "type": "replace_section",
                    "section": "Root",
                    "content": "\n# Root\nupdated",
                    "include_subsection": true
                }
            }),
            "instance_id_not_match",
            Some("instance_id does not match latest content"),
        )
        .await;

        let (latest_revision, latest_instance_id) =
            context.latest_revision_and_instance_id("/mcp/page");
        assert_edit_tool_call_error(
            &context,
            &session_id,
            json!({
                "path": "/mcp/page",
                "revision": latest_revision,
                "instance_id": latest_instance_id,
                "operation": {
                    "type": "replace_section",
                    "section": "Missing",
                    "content": "\n# Root\nupdated",
                    "include_subsection": true
                }
            }),
            "not_found",
            Some("section not found: Missing"),
        )
        .await;

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
    /// active GET stream 中は idle TTL 相当時間を超えても
    /// session が維持されることを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::active_get_stream_keeps_session_alive`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn active_get_stream_keeps_session_alive() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_millis(120),
                Duration::from_millis(30),
                Duration::from_millis(20),
                8,
            ),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        let stream_response = context
            .get_stream()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send active get stream request failed");
        assert_eq!(stream_response.status(), 200);
        let content_type = stream_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("text/event-stream"),
            "expected event-stream, got {content_type}",
        );

        tokio::time::sleep(Duration::from_millis(220)).await;

        let post_response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send kept alive post request failed");
        assert_eq!(post_response.status(), 200);

        drop(stream_response);
        let delete_response = context
            .delete_session()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send kept alive session delete request failed");
        assert_eq!(delete_response.status(), 204);
        context.shutdown().await;
    }

    ///
    /// DELETE 後に active stream 由来の Heartbeat が
    /// session を復活させないことを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::delete_after_active_stream_does_not_restore_session`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn delete_after_active_stream_does_not_restore_session() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_millis(120),
                Duration::from_millis(30),
                Duration::from_millis(20),
                8,
            ),
        )
        .await;
        let session_id = context.initialize_session().await;
        context.send_initialized_notification(&session_id).await;

        let stream_response = context
            .get_stream()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send active get stream request failed");
        assert_eq!(stream_response.status(), 200);

        let delete_response = context
            .delete_session()
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send active session delete request failed");
        assert_eq!(delete_response.status(), 204);

        tokio::time::sleep(Duration::from_millis(70)).await;

        let post_response = context
            .post_json(GET_PAGE_TOOL_CALL_BODY)
            .header("mcp-session-id", &session_id)
            .send()
            .await
            .expect("send deleted session post request failed");
        assert_eq!(post_response.status(), 401);

        drop(stream_response);
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

    ///
    /// active GET stream 中でも session 上限制御による
    /// LRU eviction が維持されることを確認する。
    ///
    /// # 注記
    /// `cargo test
    /// mcp::transport::tests::active_stream_does_not_prevent_session_limit_eviction`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn active_stream_does_not_prevent_session_limit_eviction() {
        let context = spawn_test_server_with_session_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_millis(20),
                1,
            ),
        )
        .await;
        let first_session_id = context.initialize_session().await;
        context.send_initialized_notification(&first_session_id).await;

        let stream_response = context
            .get_stream()
            .header("mcp-session-id", &first_session_id)
            .send()
            .await
            .expect("send first active get stream request failed");
        assert_eq!(stream_response.status(), 200);

        let second_session_id = context.initialize_session().await;
        assert_ne!(first_session_id, second_session_id);

        tokio::time::sleep(Duration::from_millis(70)).await;

        let first_response = context
            .get_stream()
            .header("mcp-session-id", &first_session_id)
            .send()
            .await
            .expect("send evicted first session get request failed");
        assert_eq!(first_response.status(), 401);

        drop(stream_response);
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
