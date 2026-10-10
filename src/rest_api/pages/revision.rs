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
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;

use super::super::resp_error_json;
use crate::database::DbError;
use crate::database::types::{BearerScope, PageId};
use crate::fts;
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

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
) -> actix_web::Result<HttpResponse> {

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<RevisionQuery>::from_query(
        req.query_string(),
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rollback_to or keep_from",
            ));
        }
    };

    let (rollback_to, keep_from) = match (
        &query.rollback_to,
        &query.keep_from,
    ) {
        (Some(rollback), None) => (Some(rollback.as_str()), None),
        (None, Some(keep)) => (None, Some(keep.as_str())),
        _ => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rollback_to or keep_from",
            ));
        }
    };

    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.action = Some(if rollback_to.is_some() {
            crate::rest_api::audit::RestAuditAction::Rollback
        } else { crate::rest_api::audit::RestAuditAction::Compaction });
    });

    let auth = match require_request_scope(&req,
        if rollback_to.is_some() {
            BearerScope::Update
        } else {
            BearerScope::Delete
        },
    ) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
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
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
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
    let access = crate::database::MutationAccess {
        policy: crate::auth::PageAccessPolicy::new(auth.path_prefixes()),
        lock: crate::database::MutationLock::Unlocked,
        allow_draft: false, allow_deleted: false,
        cleanup_draft: false, expected_version: None,
    };
    let result = state.db().revise_page_authorized(
        &page_id, target_revision, rollback_to.is_some(), &access,
    );

    let outcome = match result {
        Ok(result) => result.outcome,
        Err(err) => {
        if let Some(resp) = crate::rest_api::auth::mutation_access_error(
            &req, &err, "page revision update failed",
        ) { return Ok(resp); }
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
    };
    crate::rest_api::audit::update_metadata(&req, |metadata| {
        // DB成功により既存validatorが同txnでこの指定境界を確認済み。
        metadata.revision = Some(target_revision);
    });
    req.extensions_mut().insert(outcome);

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
