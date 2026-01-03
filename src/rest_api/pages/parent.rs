/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ親情報取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use crate::database::DbError;
use crate::database::types::PageId;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

#[derive(Deserialize)]
struct ParentQuery {
    recursive: Option<bool>,
}

///
/// GET /api/pages/{page_id}/parent の実体
///
/// # 概要
/// ページの親情報を取得する
///
/// # 引数
/// * `req` - HTTPリクエスト
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
     * クエリ取得
     */
    let query = match web::Query::<ParentQuery>::from_query(
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
    let recursive = query.recursive.unwrap_or(false);

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

    if page_index.is_draft() {
        return Ok(resp_error_json(
            StatusCode::NOT_FOUND,
            "page not found",
        ));
    }

    let current_path = match page_index.current_path() {
        Some(path) => path.to_string(),
        None => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page path not found",
            ));
        }
    };

    let (parent_id, parent_path) = match resolve_parent(
        state.db(),
        current_path,
        recursive,
    ) {
        Ok(result) => result,
        Err(err) => {
            if let Some(DbError::PageNotFound) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "parent not found",
                ));
            }
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "parent lookup failed",
            ));
        }
    };

    let body = json!({
        "id": parent_id.to_string(),
        "path": parent_path,
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(body.to_string()))
}

fn resolve_parent(
    db: &crate::database::DatabaseManager,
    current_path: String,
    recursive: bool,
) -> anyhow::Result<(PageId, String)> {
    let mut path = parent_path(&current_path);
    if !recursive {
        let parent_id = db
            .get_page_id_by_path(&path)?
            .ok_or_else(|| anyhow::anyhow!(DbError::PageNotFound))?;
        return Ok((parent_id, path));
    }

    loop {
        if let Some(parent_id) = db.get_page_id_by_path(&path)? {
            return Ok((parent_id, path));
        }
        if path == "/" {
            break;
        }
        path = parent_path(&path);
    }

    Err(anyhow::anyhow!(DbError::PageNotFound))
}

fn parent_path(path: &str) -> String {
    if path == "/" {
        return "/".to_string();
    }

    let trimmed = path.trim_end_matches('/');
    if let Some(pos) = trimmed.rfind('/') {
        let parent = &trimmed[..pos];
        if parent.is_empty() {
            "/".to_string()
        } else {
            parent.to_string()
        }
    } else {
        "/".to_string()
    }
}
