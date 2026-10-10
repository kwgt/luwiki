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

use crate::auth::{
    AuthContext, AuthUser, ScopeAuthorizationError,
    authenticate_bearer_token, authorize_any_scope,
};
use crate::database::types::{
    BearerScope,
    BearerScopeSet,
    BearerTokenPlaintext,
    PathPrefixSet,
};
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
// 非request呼出しとの互換wrapperを維持する。
#[allow(dead_code)]
pub(crate) fn require_scope(
    auth: &AuthContext,
    required: BearerScope,
) -> Result<(), HttpResponse> {
    require_any_scope(auth, &[required])
}

///
/// OR要求scopeを共有基盤で判定する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `required` - 当該操作を許可するscope候補
///
/// # 戻り値
/// 許可、または403応答を返す。
///
// 非request呼出しとの互換wrapperを維持する。
#[allow(dead_code)]
pub(crate) fn require_any_scope(
    auth: &AuthContext,
    required: &[BearerScope],
) -> Result<(), HttpResponse> {
    authorize_any_scope(auth, required)
        .map_err(|_| AuthErrorResponse::forbidden().error_response())
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
    require_request_any_scope(req, &[required])
}

///
/// リクエストのOR要求scopeを判定する
///
/// # 引数
/// * `req` - リクエスト
/// * `required` - 許可scope候補
///
/// # 戻り値
/// 認証文脈、またはHTTP拒否応答を返す。
///
pub(crate) fn require_request_any_scope(
    req: &HttpRequest,
    required: &[BearerScope],
) -> Result<AuthContext, HttpResponse> {
    /*
     * 不変認証文脈を取得しcoreの固定拒否理由を保持する
     */
    let auth = auth_context_from_request(req)?;
    authorize_any_scope(&auth, required).map_err(|error| {
        let result = match error.downcast_ref::<ScopeAuthorizationError>() {
            Some(ScopeAuthorizationError::ScopeDenied) =>
                crate::audit::model::AuditResult::ScopeDenied,
            Some(ScopeAuthorizationError::ReadOnlyDenied) =>
                crate::audit::model::AuditResult::ReadOnlyDenied,
            None => crate::audit::model::AuditResult::OperationDenied,
        };
        super::audit::update_metadata(req, |metadata| {
            metadata.result = Some(result);
        });
        AuthErrorResponse::forbidden().error_response()
    })?;
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

    let user_info = match state.db().get_user_info_by_name(&username) {
        Ok(Some(user_info)) => user_info,
        Ok(None) => {
            return Err((ErrorInternalServerError("user not found"), req));
        }
        Err(_) => {
            return Err((ErrorInternalServerError("auth failed"), req));
        }
    };

    req.extensions_mut().insert(AuthContext::new_with_actor(
        AuthUser::new(username),
        BearerScopeSet::all(),
        PathPrefixSet::new(),
        user_info.attributes(),
        None,
        None,
        user_info.id(),
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
    if res.response().extensions()
        .get::<super::audit::RestNextErrorResponse>().is_some()
    {
        return Ok(res);
    }

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

///
/// DB対象解決・パス認可の型付きエラーを公開応答へ変換する
///
/// # 引数
/// * `err` - DB操作エラー
///
/// # 戻り値
/// 共通の認可/状態エラーの場合はHTTP応答を返す。
///
pub(crate) fn page_access_error(err: &anyhow::Error) -> Option<HttpResponse> {
    use crate::database::{DbError, LockTokenInputError, PathPrefixDenied};
    use super::resp_error_json;
    if let Some(denied) = err.downcast_ref::<PathPrefixDenied>() {
        let (status, reason) = if denied.parent_candidate {
            (StatusCode::NOT_FOUND, "parent not found")
        } else { (StatusCode::FORBIDDEN, "forbidden") };
        return Some(resp_error_json(status, reason));
    }
    if let Some(input) = err.downcast_ref::<LockTokenInputError>() {
        return Some(resp_error_json(StatusCode::FORBIDDEN, input.reason));
    }
    if err.downcast_ref::<crate::database::PageSourceNotFound>().is_some() {
        return Some(resp_error_json(StatusCode::NOT_FOUND,
            "page source not found"));
    }
    match err.downcast_ref::<DbError>() {
        Some(DbError::PageNotFound) => Some(resp_error_json(
            StatusCode::NOT_FOUND, "page not found")),
        Some(DbError::PageDeleted) => Some(resp_error_json(
            StatusCode::GONE, "page deleted")),
        _ => None,
    }
}

///
/// 要求内に型付きパス拒否を保持して既存応答へ変換する
///
/// # 引数
/// * `req` - 認証済み要求
/// * `err` - DB認可エラー
///
/// # 戻り値
/// 既存のHTTP認可・状態応答。
///
/// # 概要
/// 最終statusへ変換する前に型付き拒否を要求内へ残す。
///
pub(crate) fn page_access_error_for_request(
    req: &HttpRequest,
    err: &anyhow::Error,
) -> Option<HttpResponse> {
    use crate::database::PathPrefixDenied;
    super::audit::remember_error(req, err);
    /*
     * 同txn対象と拒否分類を既存HTTP変換より前に保持する
     */
    if let Some(denied) = err.downcast_ref::<PathPrefixDenied>()
    {
        super::audit::update_metadata(req, |metadata| {
            metadata.result =
                Some(crate::audit::model::AuditResult::PathPrefixDenied);
            if denied.parent_candidate {
                metadata.denied_side =
                    Some(super::audit::RestDeniedSide::Parent);
            }
        });
    }
    page_access_error(err)
}

///
/// 任意lockヘッダの入力をHTTP非依存エラーとして遅延する
///
/// # 引数
/// * `req` - HTTPリクエスト
///
/// # 戻り値
/// tokenの有無、または認可後に判定する入力不正。
///
pub(crate) fn mutation_lock_input(
    req: &HttpRequest,
) -> Result<Option<crate::database::types::LockToken>,
    crate::database::LockTokenInputError> {
    let invalid = || crate::database::LockTokenInputError {
        reason: "lock token invalid",
    };
    let Some(raw) = req.headers().get("X-Lock-Authentication") else {
        return Ok(None);
    };
    let raw = raw.to_str().map_err(|_| invalid())?;
    let token = raw.split_whitespace()
        .find_map(|part| part.strip_prefix("token="))
        .ok_or_else(invalid)?;
    crate::database::types::LockToken::from_string(token)
        .map(Some).map_err(|_| invalid())
}

///
/// write認可・既存lock・commit後失敗をHTTP応答へ変換する
///
/// # 引数
/// * `err` - 操作エラー
/// * `failure_reason` - 既存操作の500理由
///
/// # 戻り値
/// 既知の境界エラーなら公開応答を返す。
///
/// # 概要
/// commit後の派生失敗は元PageNotFound等から404へ誤変換しない。
///
pub(crate) fn mutation_access_error(
    req: &HttpRequest,
    err: &anyhow::Error,
    failure_reason: &'static str,
) -> Option<HttpResponse> {
    use crate::database::{DbError, MutationOutcome, PathPrefixDenied};
    use super::resp_error_json;
    if let Some(outcome) = err.downcast_ref::<MutationOutcome>() {
        req.extensions_mut().insert(outcome.clone());
    }
    if let Some(denied) = err.downcast_ref::<PathPrefixDenied>() {
        req.extensions_mut().insert(denied.clone());
    }
    if err.downcast_ref::<MutationOutcome>()
        .is_some_and(|outcome| outcome.committed
            && outcome.post_processing_failure)
    {
        return Some(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR, failure_reason));
    }
    if let Some(response) = page_access_error_for_request(req, err) {
        return Some(response);
    }
    match err.downcast_ref::<DbError>() {
        Some(DbError::PageLocked) => Some(resp_error_json(
            StatusCode::LOCKED, "page locked")),
        Some(DbError::LockForbidden) => Some(resp_error_json(
            StatusCode::FORBIDDEN, "lock forbidden")),
        _ => None,
    }
}

///
/// asset固有の不在/削除を既存応答へ変換する
///
/// # 引数
/// * `req` - リクエスト
/// * `err` - 認可/assetエラー
/// * `failure_reason` - 既存500理由
///
/// # 戻り値
/// 既知エラーのHTTP応答。
///
pub(crate) fn asset_access_error(
    req: &HttpRequest, err: &anyhow::Error, failure_reason: &'static str,
) -> Option<HttpResponse> {
    if let Some(target) =
        err.downcast_ref::<crate::database::AssetAccessTarget>()
    {
        req.extensions_mut().insert(target.clone());
    }
    if let Some(resp) = mutation_access_error(req, err, failure_reason) {
        return Some(resp);
    }
    if err.downcast_ref::<crate::database::AssetBodyReadFailure>().is_some() {
        return Some(super::resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
            "asset read failed"));
    }
    match err.downcast_ref::<crate::database::DbError>() {
        Some(crate::database::DbError::AssetNotFound) => Some(
            super::resp_error_json(StatusCode::NOT_FOUND, "asset not found"),
        ),
        Some(crate::database::DbError::AssetDeleted) => {
            Some(super::resp_error_json(StatusCode::GONE, "asset deleted"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod audit_actor_tests {
    use actix_web::test::TestRequest;

    use super::*;

    ///
    /// Basic実認証のIDを要求途中の主体変更から独立させる
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 認証後の表示名更新・削除・同名再作成とtoken nullを確認する。
    ///
    #[actix_web::test]
    async fn audit_actor_basic_survives_user_replacement() {
        /*
         * 既存隔離fixtureのBasic認証入口を実際に通す
         */
        let (base, state) = crate::rest_api::authorization_tests::fixture();
        let id = state.db().get_user_info_by_name("user")
            .unwrap().unwrap().id();
        let state = web::Data::new(Arc::new(RwLock::new(state)));
        let req = TestRequest::default()
            .app_data(state.clone())
            .insert_header(("Authorization", "Basic dXNlcjpwYXNz"))
            .to_srv_request();
        let req = validate_basic_auth(req).await.unwrap();
        let context = req.extensions().get::<AuthContext>().unwrap().clone();
        assert_eq!(context.actor_id(), Some(&id));
        assert_eq!(context.token_id(), None);
        assert_eq!(context.user_id(), "user");

        /*
         * 要求中の表示名更新・ユーザ差替えでも認証済みIDを保持する
         */
        {
            let guard = state.read().unwrap();
            guard.db().update_user("user", Some("new display"), None).unwrap();
            assert_eq!(context.actor_id(), Some(&id));
            guard.db().delete_user("user").unwrap();
            let removed = guard.db().get_user_info_by_name("user").unwrap();
            assert!(removed.is_none());
            assert_eq!(context.actor_id(), Some(&id));
            guard.db().add_user("user", "newpass", None).unwrap();
            let new_id = guard.db().get_user_info_by_name("user")
                .unwrap().unwrap().id();
            assert_ne!(new_id, id);
            assert_eq!(context.actor_id(), Some(&id));
        }
        drop(req);
        drop(state);
        std::fs::remove_dir_all(base).unwrap();
    }
}
