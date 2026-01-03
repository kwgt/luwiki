/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページロック関連APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde_json::json;

use crate::database::types::{LockToken, PageId};
use crate::database::DbError;
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthUser;
use super::super::resp_error_json;

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
)
    -> actix_web::Result<HttpResponse>
{
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
    let auth_user = match req.extensions().get::<AuthUser>() {
        Some(user) => user.user_id().to_string(),
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
     * ページ情報取得
     */
    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    if page_index.deleted() {
        return Ok(resp_error_json(
            StatusCode::GONE,
            "page deleted",
        ));
    }

    /*
     * ロック取得
     */
    let lock_info = match state.db().acquire_page_lock(&page_id, &auth_user) {
        Ok(lock_info) => lock_info,
        Err(err) => {
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
)
    -> actix_web::Result<HttpResponse>
{
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
    let token = match parse_lock_token(&req) {
        Ok(token) => token,
        Err(resp) => return Ok(resp),
    };

    /*
     * 認証ユーザ取得
     */
    let auth_user = match req.extensions().get::<AuthUser>() {
        Some(user) => user.user_id().to_string(),
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
     * ページ情報取得
     */
    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    if page_index.deleted() {
        return Ok(resp_error_json(
            StatusCode::GONE,
            "page deleted",
        ));
    }

    /*
     * ロック延長
     */
    let lock_info = match state.db().renew_page_lock(&page_id, &auth_user, &token) {
        Ok(lock_info) => lock_info,
        Err(err) => {
            if let Some(DbError::LockNotFound) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "lock not found",
                ));
            }
            if let Some(DbError::LockForbidden) = err.downcast_ref::<DbError>() {
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
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
)
    -> actix_web::Result<HttpResponse>
{
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
     * ページ情報取得
     */
    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    if page_index.deleted() {
        return Ok(resp_error_json(
            StatusCode::GONE,
            "page deleted",
        ));
    }

    /*
     * ロック情報取得
     */
    let lock_info = match state.db().get_page_lock_info(&page_id) {
        Ok(Some(lock_info)) => lock_info,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "lock not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock lookup failed",
            ));
        }
    };

    let user_name = match state.db().get_user_name_by_id(&lock_info.user()) {
        Ok(Some(name)) => name,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user lookup failed",
            ));
        }
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
)
    -> actix_web::Result<HttpResponse>
{
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
    let token = match parse_lock_token(&req) {
        Ok(token) => token,
        Err(resp) => return Ok(resp),
    };

    /*
     * 認証ユーザ取得
     */
    let auth_user = match req.extensions().get::<AuthUser>() {
        Some(user) => user.user_id().to_string(),
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
     * ページ情報取得
     */
    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    if page_index.deleted() {
        return Ok(resp_error_json(
            StatusCode::GONE,
            "page deleted",
        ));
    }

    /*
     * ロック解除
     */
    match state.db().release_page_lock(&page_id, &auth_user, &token) {
        Ok(()) => {}
        Err(err) => {
            if let Some(DbError::LockNotFound) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "lock not found",
                ));
            }
            if let Some(DbError::LockForbidden) = err.downcast_ref::<DbError>() {
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
    }

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
        Err(_) => Err(resp_error_json(
            StatusCode::NOT_FOUND,
            "page not found",
        )),
    }
}

///
/// ロック解除トークンの解析
///
fn parse_lock_token(req: &HttpRequest) -> Result<LockToken, HttpResponse> {
    let raw = match req.headers().get(LOCK_AUTH_HEADER) {
        Some(raw) => raw,
        None => {
            return Err(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token required",
            ));
        }
    };

    let raw = match raw.to_str() {
        Ok(raw) => raw.trim(),
        Err(_) => {
            return Err(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
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
            return Err(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
        }
    };

    match LockToken::from_string(token) {
        Ok(token) => Ok(token),
        Err(_) => Err(resp_error_json(
            StatusCode::FORBIDDEN,
            "lock token invalid",
        )),
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
