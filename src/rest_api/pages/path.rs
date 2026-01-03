/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページパス取得・リネームAPIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use crate::database::DbError;
use crate::database::types::PageId;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

/// キャッシュ指示ヘッダの固定値
const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";

#[derive(Deserialize)]
struct RenameQuery {
    rename_to: Option<String>,
    restore_to: Option<String>,
}

///
/// GET /api/pages/{page_id}/path の実体
///
/// # 概要
/// ページパスを取得する
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
     * レスポンス生成
     */
    let etag = format!("\"{}:{}\"", page_id, page_index.latest());
    let body = json!({
        "path": page_index.path(),
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE))
        .insert_header((header::ETAG, etag))
        .body(body.to_string()))
}

///
/// POST /api/pages/{page_id}/path?rename_to={page_path} の実体
///
/// # 概要
/// ページパスをリネームする
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
     * クエリ取得と検証
     */
    let query = match web::Query::<RenameQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rename_to",
            ));
        }
    };

    let has_rename = query.rename_to.as_ref().is_some();
    let has_restore = query.restore_to.as_ref().is_some();
    if has_rename == has_restore {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "invalid query parameter: rename_to or restore_to",
        ));
    }

    let (target_path, is_restore) = if let Some(rename_to) = query.rename_to.as_deref() {
        (rename_to.to_string(), false)
    } else {
        let restore_to = match query.restore_to.as_deref() {
            Some(restore_to) => restore_to.to_string(),
            None => {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: restore_to",
                ));
            }
        };
        (restore_to, true)
    };

    if let Err(message) = super::validate_page_path(&target_path) {
        return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
    }

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

    if page_index.deleted() && !is_restore {
        return Ok(resp_error_json(
            StatusCode::GONE,
            "page deleted",
        ));
    }

    if page_index.is_draft() {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "draft page cannot be renamed",
        ));
    }

    if !is_restore && page_index.path() == "/" {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "root page is protected",
        ));
    }

    if target_path == "/" {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "root page is protected",
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

    if lock_info.is_some() {
        return Ok(resp_error_json(
            StatusCode::LOCKED,
            "page locked",
        ));
    }

    if !is_restore && page_index.path() == target_path {
        return Ok(HttpResponse::NoContent().finish());
    }

    /*
     * パス更新実行
     */
    let result = if is_restore {
        if !page_index.deleted() {
            return Ok(resp_error_json(
                StatusCode::CONFLICT,
                "page not deleted",
            ));
        }
        state.db().undelete_page_by_id(&page_id, &target_path, true)
    } else {
        let current_path = match page_index.current_path() {
            Some(path) => path.to_string(),
            None => {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "page path not found",
                ));
            }
        };
        state.db().rename_page(current_path, target_path)
    };

    match result {
        Ok(()) => {}
        Err(err) => {
            if let Some(DbError::PageAlreadyExists) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::CONFLICT,
                    "page already exists",
                ));
            }
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

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page path update failed",
            ));
        }
    }

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent().finish())
}
