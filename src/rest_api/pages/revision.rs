/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページリビジョン操作APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::database::DbError;
use crate::database::types::PageId;
use crate::fts;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

#[derive(Deserialize)]
struct RevisionQuery {
    rollback_to: Option<String>,
    keep_from: Option<String>,
}

///
/// POST /api/pages/{page_id}/revision の実体
///
/// # 概要
/// ページソースのロールバック／コンパクションを行う。
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
    let query = match web::Query::<RevisionQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rollback_to or keep_from",
            ));
        }
    };

    let (rollback_to, keep_from) = match (&query.rollback_to, &query.keep_from) {
        (Some(rollback), None) => (Some(rollback.as_str()), None),
        (None, Some(keep)) => (None, Some(keep.as_str())),
        _ => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rollback_to or keep_from",
            ));
        }
    };

    let target_revision = if let Some(raw) = rollback_to {
        match raw.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: rollback_to",
                ));
            }
        }
    } else if let Some(raw) = keep_from {
        match raw.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: keep_from",
                ));
            }
        }
    } else {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "invalid query parameter: rollback_to or keep_from",
        ));
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
     * ロールバック／コンパクション実行
     */
    let result = if rollback_to.is_some() {
        state.db().rollback_page_source_only(&page_id, target_revision)
    } else {
        state.db().compact_page_source(&page_id, target_revision)
    };

    if let Err(err) = result {
        if let Some(DbError::PageNotFound) = err.downcast_ref::<DbError>() {
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
        }
        if let Some(DbError::PageDeleted) = err.downcast_ref::<DbError>() {
            return Ok(resp_error_json(StatusCode::GONE, "page deleted"));
        }
        if let Some(DbError::InvalidRevision) = err.downcast_ref::<DbError>() {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rollback_to or keep_from",
            ));
        }
        if let Some(DbError::PageLocked) = err.downcast_ref::<DbError>() {
            return Ok(resp_error_json(StatusCode::LOCKED, "page locked"));
        }
        return Ok(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "revision update failed",
        ));
    }

    /*
     * FTSの更新
     */
    if let Err(err) = fts::reindex_page(
        state.fts_config(),
        state.db(),
        &page_id,
        false,
    ) {
        log::error!("fts update failed: {:?}", err);
        return Ok(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "fts update failed",
        ));
    }

    Ok(HttpResponse::NoContent().finish())
}
