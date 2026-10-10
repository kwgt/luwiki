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

use super::super::resp_error_json;
use crate::database::DbError;
use crate::database::types::{BearerScope, PageId};
use crate::fts;
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

/// ロック認証ヘッダの名称

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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Delete) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    /*
     * クエリ取得
     */
    let query = match web::Query::<DeleteQuery>::from_query(
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
    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.recursive = Some(query.recursive.unwrap_or(false));
    });


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
    let recursive = query.recursive.unwrap_or(false);
    let auth_user = auth.user_id().to_string();
    let token = crate::rest_api::auth::mutation_lock_input(&req);
    let lock = if recursive { crate::database::MutationLock::Unlocked } else {
        crate::database::MutationLock::Token {
            user_name: &auth_user,
            token: token
                .as_ref()
                .map(|value| value.as_ref())
                .map_err(|err| *err),
            validate_unlocked: true,
            invalid_token_reason: "lock forbidden",
        }
    };
    let access = crate::database::MutationAccess {
        policy: crate::auth::PageAccessPolicy::new(auth.path_prefixes()), lock,
        allow_draft: !recursive, allow_deleted: false,
        cleanup_draft: false, expected_version: None,
    };
    let outcome = match state.db().delete_page_authorized(
        &page_id, recursive, &auth_user, &access,
    ) {
        Ok(result) => result.outcome,
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::mutation_access_error(
                &req,
                &err,
                "page delete failed",
            ) {
                return Ok(resp);
            }
            if let Some(DbError::RootPageProtected) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "root page is protected",
                ));
            }
            if let Some(DbError::DraftPage) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(StatusCode::BAD_REQUEST,
                    "draft page cannot be deleted recursively"));
            }
            return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                "page delete failed"));
        }
    };
    req.extensions_mut().insert(outcome.clone());

    /*
     * DB commit後は確定IDだけを既存FTS後処理へ渡す
     */
    if let Err(err) = fts::update_pages_index(
        state.fts_config(), state.db(), &outcome.affected_page_ids, true,
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
        Err(_) => Err(resp_error_json(StatusCode::NOT_FOUND, "page not found")),
    }
}
