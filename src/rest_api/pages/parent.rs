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
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::types::{BearerScope, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * ページID解析
     */
    let page_id_raw = path.into_inner();
    let page_id = match PageId::from_string(&page_id_raw) {
        Ok(page_id) => page_id,
        Err(_) => {
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
        }
    };

    /*
     * クエリ取得
     */
    let query = match web::Query::<ParentQuery>::from_query(
        req.query_string(),
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
    let (parent_id, parent_path, target) =
        match state.db().read_authorized_parent_target(
        &page_id, recursive, &policy,
    ) {
        Ok(parent) => parent,
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            if err.downcast_ref::<crate::database::ParentNotFound>().is_some() {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND, "parent not found"));
            }
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR, "parent lookup failed"));
        }
    };

    req.extensions_mut().insert(target);

    let body = json!({
        "id": parent_id.to_string(),
        "path": parent_path,
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(body.to_string()))
}
