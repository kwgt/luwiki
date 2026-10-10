/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページロック関連APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde_json::json;

use super::super::resp_error_json;
use crate::database::DbError;
use crate::database::types::{BearerScope, LockToken, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::{
    AuthContext,
    CACHE_CONTROL_NO_STORE,
    require_request_scope,
};

/// ロック情報ヘッダの名称
const PAGE_LOCK_HEADER: &str = "X-Page-Lock";
/// ロック認証ヘッダの名称
const LOCK_AUTH_HEADER: &str = "X-Lock-Authentication";

///
/// POST /api/pages/{page_id}/lock の実体
///
/// # 概要
/// ページロックの取得
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn post(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
    let auth = match crate::rest_api::auth::require_request_any_scope(
        &req, &[BearerScope::Create, BearerScope::Update],
    ) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * ページID解析
     */
    let page_id = match parse_page_id(path.into_inner()) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };

    /*
     * 認証ユーザ取得
     */
    let auth_user = match req.extensions().get::<AuthContext>() {
        Some(context) => context.user_id().to_string(),
        None => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth context missing",
            ));
        }
    };

    /*
     * 共有状態取得
     */
    let state = match state.read() {
        Ok(state) => state,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state lock failed",
            ));
        }
    };

    /*
     * ロック取得
     */
    let (lock_info, _target) = match state.db().acquire_page_lock_authorized(
        &page_id, &auth_user, &policy,
    ) {
        Ok(result) => {
            crate::rest_api::audit::remember_index(&req, &result.target);
            (result.value, result.target)
        },
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            if let Some(DbError::PageLocked) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(
                    StatusCode::CONFLICT,
                    "page already locked",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock acquire failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent()
        .insert_header((PAGE_LOCK_HEADER, build_lock_header(&lock_info)))
        .finish())
}

///
/// PUT /api/pages/{page_id}/lock の実体
///
/// # 概要
/// ページロックの延長
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn put(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
    let auth = match crate::rest_api::auth::require_request_any_scope(
        &req, &[BearerScope::Create, BearerScope::Update],
    ) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * ページID解析
     */
    let page_id = match parse_page_id(path.into_inner()) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };

    /*
     * ロック解除トークン解析
     */
    let token = parse_lock_token(&req);

    /*
     * 認証ユーザ取得
     */
    let auth_user = match req.extensions().get::<AuthContext>() {
        Some(context) => context.user_id().to_string(),
        None => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth context missing",
            ));
        }
    };

    /*
     * 共有状態取得
     */
    let state = match state.read() {
        Ok(state) => state,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state lock failed",
            ));
        }
    };

    /*
     * ロック延長
     */
    let (lock_info, _target) = match state
        .db()
        .renew_page_lock_authorized(
            &page_id, &auth_user, token.as_ref().map_err(|err| *err), &policy,
        )
    {
        Ok(result) => {
            crate::rest_api::audit::remember_index(&req, &result.target);
            (result.value, result.target)
        },
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            if let Some(DbError::LockNotFound) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "lock not found",
                ));
            }
            if let Some(DbError::LockForbidden) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::FORBIDDEN,
                    "lock forbidden",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock update failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent()
        .insert_header((PAGE_LOCK_HEADER, build_lock_header(&lock_info)))
        .finish())
}

///
/// GET /api/pages/{page_id}/lock の実体
///
/// # 概要
/// ページロック情報の取得
///
/// # 引数
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * ページID解析
     */
    let page_id = match parse_page_id(path.into_inner()) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };

    /*
     * 共有状態取得
     */
    let state = match state.read() {
        Ok(state) => state,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state lock failed",
            ));
        }
    };

    /*
     * ロック情報取得
     */
    let (_, lock, user_name) = match state.db().read_authorized_page_meta(
        &page_id, None, false, false, &policy,
    ) {
        Ok(result) => {
            crate::rest_api::audit::remember_page(&req, &result.0);
            result
        },
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR, "lock lookup failed"));
        }
    };
    let lock_info = match lock {
        Some(lock) => lock,
        None => return Ok(resp_error_json(
            StatusCode::NOT_FOUND, "lock not found")),
    };
    let user_name = match user_name {
        Some(name) => name,
        None => return Ok(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR, "user not found")),
    };

    /*
     * レスポンス生成
     */
    let body = json!({
        "expire": lock_info.expire().to_rfc3339(),
        "username": user_name,
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(body.to_string()))
}

///
/// DELETE /api/pages/{page_id}/lock の実体
///
/// # 概要
/// ページロックの解除
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn delete(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Update) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * ページID解析
     */
    let page_id = match parse_page_id(path.into_inner()) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };

    /*
     * ロック解除トークン解析
     */
    let token = parse_lock_token(&req);

    /*
     * 認証ユーザ取得
     */
    let auth_user = match req.extensions().get::<AuthContext>() {
        Some(context) => context.user_id().to_string(),
        None => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth context missing",
            ));
        }
    };

    /*
     * 共有状態取得
     */
    let state = match state.read() {
        Ok(state) => state,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state lock failed",
            ));
        }
    };

    /*
     * ロック解除
     */
    let _target = match state.db().release_page_lock_authorized(
        &page_id, &auth_user, token.as_ref().map_err(|err| *err), &policy,
    ) {
        Ok(result) => {
            crate::rest_api::audit::remember_index(&req, &result.target);
            result.target
        },
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            if let Some(DbError::LockNotFound) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "lock not found",
                ));
            }
            if let Some(DbError::LockForbidden) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::FORBIDDEN,
                    "lock forbidden",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock release failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent().finish())
}

///
/// ページIDの解析
///
fn parse_page_id(raw: String) -> Result<PageId, HttpResponse> {
    match PageId::from_string(&raw) {
        Ok(page_id) => Ok(page_id),
        Err(_) => Err(resp_error_json(StatusCode::NOT_FOUND, "page not found")),
    }
}

///
/// ロック解除トークンの解析
///
fn parse_lock_token(
    req: &HttpRequest,
) -> Result<LockToken, crate::database::LockTokenInputError> {
    /*
     * ヘッダ値の取得
     */
    let raw = match req.headers().get(LOCK_AUTH_HEADER) {
        Some(raw) => raw,
        None => {
            return Err(crate::database::LockTokenInputError {
                reason: "lock token required",
            });
        }
    };

    /*
     * トークン文字列の抽出
     */
    let raw = match raw.to_str() {
        Ok(raw) => raw.trim(),
        Err(_) => {
            return Err(crate::database::LockTokenInputError {
                reason: "lock token invalid",
            });
        }
    };

    let mut token_value = None;
    for part in raw.split_whitespace() {
        if let Some(value) = part.strip_prefix("token=") {
            token_value = Some(value);
            break;
        }
    }

    let token = match token_value {
        Some(value) => value,
        None => {
            return Err(crate::database::LockTokenInputError {
                reason: "lock token invalid",
            });
        }
    };

    /*
     * LockTokenへの変換
     */
    match LockToken::from_string(token) {
        Ok(token) => Ok(token),
        Err(_) => Err(crate::database::LockTokenInputError {
                reason: "lock token invalid",
            }),
    }
}

///
/// ロック情報ヘッダの生成
///
pub(crate) fn build_lock_header(
    lock_info: &crate::database::types::LockInfo,
) -> String {
    format!(
        "expire={} token={}",
        lock_info.expire().to_rfc3339(),
        lock_info.token(),
    )
}
