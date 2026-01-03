/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページソース取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::database::types::{LockToken, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthUser;
use super::super::resp_error_json;

/// キャッシュ指示ヘッダの固定値
const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// ロック認証ヘッダの名称
const LOCK_AUTH_HEADER: &str = "X-Lock-Authentication";

#[derive(Deserialize)]
struct GetSourceQuery {
    rev: Option<String>,
}

#[derive(Deserialize)]
struct PutSourceQuery {
    amend: Option<String>,
}

///
/// GET /api/pages/{page_id}/source の実体
///
/// # 概要
/// ページソースを取得する
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
/// # 注記
/// エラー時はJSON形式で返却する。
/// 処理の流れはクエリ検証、ID解析、状態取得、
/// ページ情報取得、ソース取得、レスポンス生成の順。
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<GetSourceQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rev",
            ));
        }
    };

    let revision = match query.rev.as_deref() {
        Some(raw) => match raw.parse::<u64>() {
            Ok(revision) => Some(revision),
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: rev",
                ));
            }
        },
        None => None,
    };

    /*
     * ページID解析
     */
    let page_id_raw = path.into_inner();
    let page_id = match PageId::from_string(&page_id_raw) {
        Ok(page_id) => page_id,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
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

    if page_index.is_draft() {
        return Ok(resp_error_json(
            StatusCode::NOT_FOUND,
            "draft has no source yet",
        ));
    }

    let revision = revision.unwrap_or(page_index.latest());

    /*
     * ページソース取得
     */
    let page_source = match state.db().get_page_source(&page_id, revision) {
        Ok(Some(source)) => source,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page source not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page source lookup failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    let etag = format!("\"{}:{}\"", page_id, revision);

    Ok(HttpResponse::Ok()
        .content_type("text/markdown")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE))
        .insert_header((header::ETAG, etag))
        .body(page_source.source()))
}

///
/// PUT /api/pages/{page_id}/source の実体
///
/// # 概要
/// ページソースを更新する
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
/// * `body` - ページソース
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
/// # 注記
/// エラー時はJSON形式で返却する。
/// 処理の流れはクエリ検証、Content-Type検証、ボディ解析、
/// 認証ユーザ取得、状態取得、ロック検証、ページ更新の順。
///
pub async fn put(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
    body: web::Bytes,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<PutSourceQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: amend",
            ));
        }
    };

    let amend = match query.amend.as_deref() {
        Some("true") => true,
        Some("false") => false,
        Some(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: amend",
            ));
        }
        None => false,
    };

    /*
     * Content-Typeヘッダ検証
     */
    let content_type = match req.headers().get(header::CONTENT_TYPE) {
        Some(value) => value,
        None => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "Content-Type is required",
            ));
        }
    };

    let content_type = match content_type.to_str() {
        Ok(value) => value,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "Content-Type is invalid",
            ));
        }
    };

    if !super::is_supported_markdown_content_type(content_type) {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "Content-Type is not supported",
        ));
    }

    /*
     * ボディ解析
     */
    let source = match std::str::from_utf8(&body) {
        Ok(source) => source.to_string(),
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "request body must be UTF-8",
            ));
        }
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
     * ページID解析
     */
    let page_id_raw = path.into_inner();
    let page_id = match PageId::from_string(&page_id_raw) {
        Ok(page_id) => page_id,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
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
     * ロック検証
     */
    let lock_info = match state.db().get_page_lock_info(&page_id) {
        Ok(info) => info,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock lookup failed",
            ));
        }
    };

    let mut lock_token = None;
    if let Some(lock_info) = lock_info {
        if !req.headers().contains_key(LOCK_AUTH_HEADER) {
            return Ok(resp_error_json(
                StatusCode::LOCKED,
                "page locked",
            ));
        }

        let token = match parse_lock_token(&req) {
            Ok(token) => token,
            Err(resp) => return Ok(resp),
        };

        if lock_info.token() != token {
            return Ok(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
        }

        let user_id = match state.db().get_user_id_by_name(&auth_user) {
            Ok(Some(user_id)) => user_id,
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

        if lock_info.user() != user_id {
            return Ok(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock forbidden",
            ));
        }

        lock_token = Some(token);
    }

    /*
     * ページ更新
     */
    let update_result = state.db().put_page(
        &page_id,
        &auth_user,
        source,
        amend,
    );
    match update_result {
        Ok(()) => {}
        Err(err) => {
            if let Some(crate::database::DbError::AmendForbidden) =
                err.downcast_ref::<crate::database::DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::FORBIDDEN,
                    "amend forbidden",
                ));
            }
            if let Some(crate::database::DbError::PageNotFound) =
                err.downcast_ref::<crate::database::DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "page not found",
                ));
            }
            if let Some(crate::database::DbError::UserNotFound) =
                err.downcast_ref::<crate::database::DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user not found",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page update failed",
            ));
        }
    }

    /*
     * ロック解除
     */
    if let Some(token) = lock_token {
        if let Err(err) = state.db().release_page_lock(
            &page_id,
            &auth_user,
            &token,
        ) {
            if let Some(crate::database::DbError::LockNotFound) =
                err.downcast_ref::<crate::database::DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "lock not found",
                ));
            }
            if let Some(crate::database::DbError::LockForbidden) =
                err.downcast_ref::<crate::database::DbError>()
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
    }

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent().finish())
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
                "lock token invalid",
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
