/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//!
//! ページパス取得・リネームAPIの実装をまとめた
//! モジュール
//!
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::DbError;
use crate::database::types::{BearerScope, PageId};
use crate::fts;
use crate::http_server::app_state::AppState;
use crate::rest_api::{CACHE_CONTROL_NO_STORE, require_request_scope};

#[derive(Deserialize)]
struct RenameQuery {
    rename_to: Option<String>,
    restore_to: Option<String>,
    recursive: Option<bool>,
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
    let page_index = match state.db().read_authorized_page(
        &page_id, None, false, &policy,
    ) {
        Ok(page) => {
            crate::rest_api::audit::remember_page(&req, &page);
            page.index
        },
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR, "page lookup failed"));
        }
    };

    if page_index.deleted() {
        return Ok(resp_error_json(StatusCode::GONE, "page deleted"));
    }

    /*
     * レスポンス生成
     */
    let body = json!({
        "path": page_index.path(),
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
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
) -> actix_web::Result<HttpResponse> {

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<RenameQuery>::from_query(
        req.query_string(),
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rename_to",
            ));
        }
    };

    // rename_toかrestore_toかの判断
    let (target_path, is_restore) =
        if let (Some(path), None) = (&query.rename_to, &query.restore_to) {
            (path.to_owned(), false)
        } else if let (None, Some(path)) = (
            &query.rename_to,
            &query.restore_to,
        ) {
            (path.to_owned(), true)
        } else {
            // `rename_to`と`restore_to`のどちらも
            // 指定されていないか、
            // 両方指定されている場合はエラー
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rename_to or restore_to",
            ));
        };

    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.action = Some(if is_restore {
            crate::rest_api::audit::RestAuditAction::Restore
        } else { crate::rest_api::audit::RestAuditAction::Rename });
    });

    let auth = match require_request_scope(&req,
        if is_restore { BearerScope::Create } else { BearerScope::Update },
    ) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    // 再帰指定されているか否かの取得
    let recursive = query.recursive.unwrap_or(false);

    let destination = match crate::wiki_path::DestinationPathInput::parse(
        &target_path,
    ) {
        Ok(destination) => destination,
        Err(_) => {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST,
                "invalid destination path"));
        }
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
     * ページ情報取得
     */
    let access = crate::database::MutationAccess {
        policy: crate::auth::PageAccessPolicy::new(auth.path_prefixes()),
        lock: crate::database::MutationLock::Unlocked,
        allow_draft: false, allow_deleted: is_restore,
        cleanup_draft: false, expected_version: None,
    };
    let result = state.db().move_page_authorized(
        &page_id, crate::database::PageDestination::Rest(&destination),
        is_restore, recursive, &access, true,
    );

    let (changed, outcome) = match result {
        Ok(result) => (result.value, result.outcome),
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::mutation_access_error(
                &req, &err, "page path update failed",
            ) { return Ok(resp); }
            if err.downcast_ref::<crate::database::PageNotDeleted>().is_some() {
                return Ok(resp_error_json(StatusCode::CONFLICT,
                    "page not deleted"));
            }
            if let Some(DbError::DraftPage) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(StatusCode::BAD_REQUEST,
                    "draft page cannot be renamed"));
            }
            log::error!("page rename failed: {:?}", err);

            if let Some(DbError::PageAlreadyExists) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::CONFLICT,
                    "page already exists",
                ));
            }
            if let Some(DbError::PageLocked) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(StatusCode::LOCKED, "page locked"));
            }
            if let Some(DbError::PageNotFound) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "page not found",
                ));
            }
            if let Some(DbError::InvalidMoveDestination) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid destination path",
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
    };
    req.extensions_mut().insert(outcome.clone());
    if !changed { return Ok(HttpResponse::NoContent().finish()); }

    /*
     * FTSの更新
     */
    if let Err(err) = fts::update_pages_index(
        state.fts_config(), state.db(), &outcome.affected_page_ids, false,
    ) {
        log::error!("fts update failed: {:?}", err);
        return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
            "fts update failed"));
    }

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent().finish())
}
