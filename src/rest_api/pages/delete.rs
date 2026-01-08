/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ削除APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::database::DbError;
use crate::database::types::{LockToken, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthUser;
use super::super::resp_error_json;

/// ロック認証ヘッダの名称
const LOCK_AUTH_HEADER: &str = "X-Lock-Authentication";

#[derive(Deserialize)]
struct DeleteQuery {
    recursive: Option<bool>,
}

///
/// DELETE /api/pages/{page_id} の実体
///
/// # 概要
/// ページを削除する。
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
     * クエリ取得
     */
    let query = match web::Query::<DeleteQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: recursive",
            ));
        }
    };

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

    if page_index.is_draft() && query.recursive.unwrap_or(false) {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "draft page cannot be deleted recursively",
        ));
    }

    /*
     * 再帰削除指定がある場合の処理
     * ※処理は個々で完結する
     */
    let recursive = query.recursive.unwrap_or(false);

    if recursive {
        if let Err(err) = state.db().delete_pages_recursive_by_id(
            &page_id,
            false,
        ) {
            if let Some(DbError::PageNotFound) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "page not found",
                ));
            }
            if let Some(DbError::RootPageProtected) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "root page is protected",
                ));
            }
            if let Some(DbError::PageLocked) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::LOCKED,
                    "page locked",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page delete failed",
            ));
        }

        return Ok(HttpResponse::NoContent().finish());
    }

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

    /*
     * ページ削除
     */
    let token = match parse_lock_token_optional(&req) {
        Ok(token) => token,
        Err(resp) => return Ok(resp),
    };

    match state.db().delete_page_by_id_with_lock_token(
        &page_id,
        &user_id,
        token.as_ref(),
    ) {
        Ok(()) => {}
        Err(err) => {
            if let Some(DbError::PageNotFound) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "page not found",
                ));
            }
            if let Some(DbError::RootPageProtected) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "root page is protected",
                ));
            }
            if let Some(DbError::PageLocked) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::LOCKED,
                    "page locked",
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
                "page delete failed",
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
/// # 引数
/// * `raw` - 解析対象の文字列
///
/// # 戻り値
/// 解析に成功した場合は`Ok(PageId)`を返す。
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
/// # 概要
/// リクエストヘッダからロック解除トークンを抽出する。
///
/// # 引数
/// * `req` - HTTPリクエスト
///
/// # 戻り値
/// 解析に成功した場合は`Ok(LockToken)`を返す。
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
/// ロック解除トークンの解析（任意）
///
/// # 概要
/// ヘッダが存在する場合のみロック解除トークンを解析する。
///
/// # 引数
/// * `req` - HTTPリクエスト
///
/// # 戻り値
/// トークンが指定されていない場合は`Ok(None)`を返す。
///
fn parse_lock_token_optional(
    req: &HttpRequest,
) -> Result<Option<LockToken>, HttpResponse> {
    if !req.headers().contains_key(LOCK_AUTH_HEADER) {
        return Ok(None);
    }

    parse_lock_token(req).map(Some)
}
