/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 削除済みページ候補取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::types::BearerScope;
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

#[derive(Deserialize)]
struct DeletedQuery {
    path: String,
}

///
/// GET /api/pages/deleted?path={page_path} の実体
///
/// # 概要
/// 削除済みページ候補の一覧を取得する
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<DeletedQuery>::from_query(
        req.query_string(),
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };

    let mut query = query.into_inner();
    query.path = match super::normalize_page_path(&query.path) {
        Ok(path) => path,
        Err(message) => {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
        }
    };
    crate::rest_api::audit::remember_path(
        &req,
        crate::wiki_path::NormalizedWikiPath::from_stored(&query.path),
    );


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
     * 削除済みページ候補取得
     */
    let page_ids = match state.db().get_authorized_deleted_page_ids(
        &query.path, &policy,
    ) {
        Ok(page_ids) => page_ids,
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.count = Some(page_ids.len());
    });
    let body = json!(
        page_ids
            .into_iter()
            .map(|page_id| page_id.to_string())
            .collect::<Vec<String>>()
    );

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(body.to_string()))
}
