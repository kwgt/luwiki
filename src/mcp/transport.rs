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
mod tests;
