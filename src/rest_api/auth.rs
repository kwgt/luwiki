/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! REST API認証入口の実装を集約するモジュール
//!

use std::future::{Ready, ready};
use std::sync::{Arc, RwLock};
use std::{fmt, fmt::Display};

use actix_web::body::MessageBody;
use actix_web::dev::{Payload, ServiceRequest, ServiceResponse};
use actix_web::error::ErrorInternalServerError;
use actix_web::http::{StatusCode, header};
use actix_web::middleware::Next;
use actix_web::{Error, FromRequest, HttpMessage, HttpRequest, HttpResponse, ResponseError, web};
use actix_web_httpauth::extractors::basic::BasicAuth;
use actix_web_httpauth::headers::www_authenticate::basic::Basic;
use chrono::{DateTime, Local};
use log::warn;
use serde_json::json;

use crate::auth::{AuthContext, AuthUser, authenticate_bearer_token};
use crate::database::types::{BearerScope, BearerScopeSet, BearerTokenPlaintext, PathPrefixSet};
use crate::http_server::app_state::AppState;

use super::{BASIC_AUTH_REALM, CACHE_CONTROL_NO_STORE};

/// Bearer期限通知ヘッダ名
pub(crate) const X_BEARER_EXPIRE_HEADER: &str = "X-Bearer-Expire";

///
/// Bearer有効期限ヘッダ引き継ぎ情報
///
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct BearerExpireHeaderValue {
    expire_at: DateTime<Local>,
}

///
/// Authorizationヘッダ群
///
/// 共通認証入口で件数検証やscheme判定を行うため、
/// まずはヘッダ群をまとめて受け取る。
///
#[derive(Clone, Debug)]
pub(crate) struct AuthorizationHeaders {
    values: Vec<header::HeaderValue>,
}

///
/// Authorizationヘッダの解析結果
///
#[derive(Clone, Debug)]
enum ParsedAuthorization {
    Basic,
    Bearer(BearerTokenPlaintext),
}

///
/// 認証エラーレスポンス
///
#[derive(Debug)]
struct AuthErrorResponse {
    status: StatusCode,
    reason: &'static str,
}

#[allow(dead_code)]
impl BearerExpireHeaderValue {
    ///
    /// Bearer有効期限ヘッダ引き継ぎ情報の生成
    ///
    /// # 引数
    /// * `expire_at` - 更新後の有効期限
    ///
    /// # 戻り値
    /// 生成した引き継ぎ情報を返す。
    ///
    pub(crate) fn new(expire_at: DateTime<Local>) -> Self {
        Self { expire_at }
    }

    ///
    /// 更新後の有効期限へのアクセサ
    ///
    /// # 戻り値
    /// 更新後の有効期限を返す。
    ///
    pub(crate) fn expire_at(&self) -> DateTime<Local> {
        self.expire_at
    }
}

impl AuthorizationHeaders {
    ///
    /// Authorizationヘッダ件数の取得
    ///
    /// # 戻り値
    /// Authorizationヘッダ件数を返す。
    ///
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    ///
    /// 単一Authorizationヘッダの取得
    ///
    /// # 戻り値
    /// 先頭のAuthorizationヘッダ値を返す。
    ///
    fn first(&self) -> Option<&header::HeaderValue> {
        self.values.first()
    }
}

impl AuthErrorResponse {
    ///
    /// 400 Bad Request を生成する
    ///
    /// # 戻り値
    /// 認証系 400 応答を返す。
    ///
    fn bad_request() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            reason: "bad request",
        }
    }

    ///
    /// 401 Unauthorized を生成する
    ///
    /// # 戻り値
    /// 認証系 401 応答を返す。
    ///
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            reason: "unauthorized",
        }
    }

    ///
    /// 403 Forbidden を生成する
    ///
    /// # 戻り値
    /// 認可系 403 応答を返す。
    ///
    fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            reason: "forbidden",
        }
    }
}

///
/// 必要スコープを満たすかを判定する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `required` - 要求スコープ
///
/// # 戻り値
/// 必要スコープを満たす場合は `Ok(())` を返す。
/// 満たさない場合は 403 Forbidden 応答を返す。
///
pub(crate) fn require_scope(
    auth: &AuthContext,
    required: BearerScope,
) -> Result<(), HttpResponse> {
    if auth.scopes().allows(required) {
        return Ok(());
    }

    Err(AuthErrorResponse::forbidden().error_response())
}

///
/// リクエストから認証文脈を取得する
///
/// # 引数
/// * `req` - HTTPリクエスト
///
/// # 戻り値
/// 認証文脈を取得できた場合はその複製を返す。
/// 取得できない場合は 500 応答を返す。
///
pub(crate) fn auth_context_from_request(
    req: &HttpRequest,
) -> Result<AuthContext, HttpResponse> {
    req.extensions()
        .get::<AuthContext>()
        .cloned()
        .ok_or_else(|| {
            HttpResponse::InternalServerError()
                .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
                .content_type("application/json")
                .body(json!({ "reason": "auth context missing" }).to_string())
        })
}

///
/// リクエストが必要スコープを満たすことを検証する
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `required` - 要求スコープ
///
/// # 戻り値
/// 認証文脈の取得と必要スコープ判定に成功した場合は
/// 認証文脈の複製を返す。
///
pub(crate) fn require_request_scope(
    req: &HttpRequest,
    required: BearerScope,
) -> Result<AuthContext, HttpResponse> {
    let auth = auth_context_from_request(req)?;
    require_scope(&auth, required)?;
    Ok(auth)
}

impl Display for AuthErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason)
    }
}

impl ResponseError for AuthErrorResponse {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        let body = json!({
            "reason": self.reason,
        });

        let mut builder = HttpResponse::build(self.status);

        if self.status == StatusCode::UNAUTHORIZED {
            builder.insert_header((
                header::WWW_AUTHENTICATE,
                Basic::with_realm(BASIC_AUTH_REALM).to_string(),
            ));
        }

        builder
            .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
            .content_type("application/json")
            .body(body.to_string())
    }
}

impl FromRequest for AuthorizationHeaders {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &HttpRequest,
        _: &mut Payload,
    ) -> <Self as FromRequest>::Future {
        let values = req
            .headers()
            .get_all(header::AUTHORIZATION)
            .into_iter()
            .cloned()
            .collect();

        ready(Ok(Self { values }))
    }
}

///
/// 共通認証入口
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `headers` - Authorizationヘッダ群
///
/// # 戻り値
/// 認証に成功した場合はリクエストをそのまま返す。
///
pub(crate) async fn validate_authorization(
    req: ServiceRequest,
    headers: AuthorizationHeaders,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    /*
     * 共通認証入口の件数検証
     */
    if headers.len() == 0 {
        return Err((AuthErrorResponse::unauthorized().into(), req));
    }
    if headers.len() > 1 {
        return Err((AuthErrorResponse::bad_request().into(), req));
    }

    let authorization = match parse_authorization_header(&headers) {
        Ok(authorization) => authorization,
        Err(err) => return Err((err, req)),
    };

    match authorization {
        ParsedAuthorization::Basic => validate_basic_auth(req).await,
        ParsedAuthorization::Bearer(token) => {
            validate_bearer_auth(req, &token).await
        }
    }
}

///
/// Basic認証の検証
///
/// # 引数
/// * `req` - HTTPリクエスト
/// # 戻り値
/// 認証に成功した場合はリクエストをそのまま返す。
///
async fn validate_basic_auth(
    mut req: ServiceRequest,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    let credentials = match req.extract::<BasicAuth>().await {
        Ok(credentials) => credentials,
        Err(_) => return Err((AuthErrorResponse::bad_request().into(), req)),
    };

    let data = match req.app_data::<web::Data<Arc<RwLock<AppState>>>>() {
        Some(data) => data.clone(),
        None => return Err((ErrorInternalServerError("state not found"), req)),
    };

    let password = match credentials.password() {
        Some(password) => password.to_owned(),
        None => return Err((AuthErrorResponse::unauthorized().into(), req)),
    };

    let username = credentials.user_id().to_string();

    let state = match data.read() {
        Ok(state) => state,
        Err(_) => {
            return Err((ErrorInternalServerError("state lock failed"), req));
        }
    };
    let ok = match state.db().verify_user(&username, &password) {
        Ok(ok) => ok,
        Err(_) => return Err((ErrorInternalServerError("auth failed"), req)),
    };

    if !ok {
        return Err((AuthErrorResponse::unauthorized().into(), req));
    }

    req.extensions_mut().insert(AuthContext::new(
        AuthUser::new(username),
        BearerScopeSet::all(),
        PathPrefixSet::new(),
        None,
    ));

    Ok(req)
}

///
/// Bearer認証の検証入口
///
/// # 引数
/// * `req` - HTTPリクエスト
///
/// # 戻り値
/// 認証に成功した場合はリクエストをそのまま返す。
///
async fn validate_bearer_auth(
    req: ServiceRequest,
    token: &BearerTokenPlaintext,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    let data = match req.app_data::<web::Data<Arc<RwLock<AppState>>>>() {
        Some(data) => data.clone(),
        None => return Err((ErrorInternalServerError("state not found"), req)),
    };

    let state = match data.read() {
        Ok(state) => state,
        Err(_) => {
            return Err((ErrorInternalServerError("state lock failed"), req));
        }
    };
    let success =
        match authenticate_bearer_token(state.db(), token, Local::now()) {
            Ok(Ok(success)) => success,
            Ok(Err(reason)) => {
            if let Some(token_id) = reason.token_id() {
                warn!(
                    "bearer auth failed: status=401 reason={} token_id={}",
                    reason.as_str(),
                    token_id,
                );
            } else {
                warn!(
                    "bearer auth failed: status=401 reason={}",
                    reason.as_str(),
                );
            }
            return Err((AuthErrorResponse::unauthorized().into(), req));
        }
        Err(_) => return Err((ErrorInternalServerError("auth failed"), req)),
    };
    req.extensions_mut().insert(success.auth().clone());
    if let Some(expire_at) = success.updated_expire_at() {
        req.extensions_mut()
            .insert(BearerExpireHeaderValue::new(expire_at));
    }

    Ok(req)
}

///
/// Bearer期限通知ヘッダの付与
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `next` - 後続サービス
///
/// # 戻り値
/// Bearer期限通知ヘッダを必要時のみ付与したレスポンスを返す。
///
pub(crate) async fn append_bearer_expire_header<B>(
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<B>, Error>
where
    B: MessageBody + 'static,
{
    let mut res = next.call(req).await?;

    let expire_at = res
        .request()
        .extensions()
        .get::<BearerExpireHeaderValue>()
        .map(|value| value.expire_at());

    if let Some(expire_at) = expire_at {
        let header_name =
            header::HeaderName::try_from(X_BEARER_EXPIRE_HEADER)
                .map_err(|_| ErrorInternalServerError("auth failed"))?;
        res.response_mut().headers_mut().insert(
            header_name,
            header::HeaderValue::from_str(
                &expire_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            )
            .map_err(|_| ErrorInternalServerError("auth failed"))?,
        );
    }

    Ok(res)
}

///
/// Authorizationヘッダの解析
///
/// # 引数
/// * `headers` - Authorizationヘッダ群
///
/// # 戻り値
/// 解析に成功した場合はschemeと認証情報を返す。
///
fn parse_authorization_header(
    headers: &AuthorizationHeaders,
) -> Result<ParsedAuthorization, Error> {
    let value = headers
        .first()
        .ok_or_else(|| Error::from(AuthErrorResponse::unauthorized()))?;
    let value = value
        .to_str()
        .map_err(|_| Error::from(AuthErrorResponse::bad_request()))?;

    let mut parts = value.split_whitespace();
    let scheme = parts
        .next()
        .ok_or_else(|| Error::from(AuthErrorResponse::bad_request()))?;
    let credentials = parts
        .next()
        .ok_or_else(|| Error::from(AuthErrorResponse::bad_request()))?
        .to_string();

    if credentials.is_empty() || parts.next().is_some() {
        return Err(AuthErrorResponse::bad_request().into());
    }

    if scheme.eq_ignore_ascii_case("basic") {
        return Ok(ParsedAuthorization::Basic);
    }
    if scheme.eq_ignore_ascii_case("bearer") {
        return Ok(ParsedAuthorization::Bearer(BearerTokenPlaintext::new(
            credentials,
        )));
    }

    Err(AuthErrorResponse::bad_request().into())
}
