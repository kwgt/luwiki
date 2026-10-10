/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページソース取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{StatusCode, header};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::types::{BearerScope, PageId};
use crate::fts;
use crate::http_server::app_state::AppState;
use crate::markdown_source::front_matter::{
    ExtractFrontMatterError,
    FrontMatterError,
    FrontMatterValidationError,
};
use crate::rest_api::AuthContext;
use crate::rest_api::{
    build_etag,
    if_none_match_matches,
    require_request_scope,
    CACHE_CONTROL_NO_STORE,
    CACHE_CONTROL_REVALIDATE_PRIVATE,
};
/// ロック認証ヘッダの名称

#[derive(Deserialize)]
struct GetSourceQuery {
    rev: Option<String>,
}

#[derive(Deserialize)]
struct PutSourceQuery {
    amend: Option<String>,
}

///
/// front matter エラーを REST API 向けレスポンスへ変換する
///
/// # 引数
/// * `err` - front matter エラー
/// * `source` - 検証対象ソース
///
/// # 戻り値
/// 変換済みレスポンスを返す。
///
fn resp_front_matter_error(
    err: &FrontMatterError,
    source: &str,
) -> HttpResponse {
    match err {
        FrontMatterError::Extract(ExtractFrontMatterError::ClosingDelimiterNotFound) => {
            let line = source.lines().count().max(1);
            let body = json!({
                "error": "front matter invalid",
                "kind": "front_matter",
                "detail": {
                    "type": "syntax",
                    "line": line,
                    "message": "front matter closing delimiter not found",
                }
            });

            HttpResponse::BadRequest()
                .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
                .content_type("application/json")
                .body(body.to_string())
        }
        FrontMatterError::Validate(FrontMatterValidationError::Syntax {
            message,
            line,
            column,
        }) => {
            let mut detail = json!({
                "type": "syntax",
                "message": message,
            });
            if let Some(line) = line {
                detail["line"] = json!(*line);
            }
            if let Some(column) = column {
                detail["column"] = json!(*column);
            }

            let body = json!({
                "error": "front matter invalid",
                "kind": "front_matter",
                "detail": detail,
            });

            HttpResponse::BadRequest()
                .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
                .content_type("application/json")
                .body(body.to_string())
        }
        FrontMatterError::Validate(FrontMatterValidationError::Validation {
            property_path,
            message,
        }) => {
            let body = json!({
                "error": "front matter invalid",
                "kind": "front_matter",
                "detail": {
                    "type": "validation",
                    "property_path": property_path,
                    "message": message,
                }
            });

            HttpResponse::BadRequest()
                .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
                .content_type("application/json")
                .body(body.to_string())
        }
    }
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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<GetSourceQuery>::from_query(
        req.query_string(),
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
    let page = match state.db().read_authorized_page(
        &page_id, revision, true, &policy,
    ) {
        Ok(page) => {
            crate::rest_api::audit::remember_page(&req, &page);
            page
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
    if page.index.is_draft() {
        return Ok(resp_error_json(
            StatusCode::NOT_FOUND, "draft has no source yet"));
    }
    let page_source = match page.source {
        Some(source) => source,
        None => return Ok(resp_error_json(
            StatusCode::NOT_FOUND, "page source not found")),
    };

    /*
     * FTSの更新
     */
    let body = page_source.source();

    if let Some(instance_id) = page_source.instance_id() {
        let etag = build_etag(instance_id.to_string());
        if if_none_match_matches(&req, &etag) {
            return Ok(HttpResponse::NotModified()
                .insert_header((
                    header::CACHE_CONTROL,
                    CACHE_CONTROL_REVALIDATE_PRIVATE,
                ))
                .insert_header((header::ETAG, etag))
                .finish());
        }

        return Ok(HttpResponse::Ok()
            .content_type("text/markdown")
            .insert_header((
                header::CACHE_CONTROL,
                CACHE_CONTROL_REVALIDATE_PRIVATE,
            ))
            .insert_header((header::ETAG, etag))
            .body(body));
    }

    Ok(HttpResponse::Ok()
        .content_type("text/markdown")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(body))
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
/// 認証ユーザ取得、状態取得、
/// ロック検証、ページ更新の順。
///
pub async fn put(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
    body: web::Bytes,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Update) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<PutSourceQuery>::from_query(
        req.query_string(),
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
    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.amend = Some(amend);
    });


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
    let auth_user = match req.extensions().get::<AuthContext>() {
        Some(context) => context.user_id().to_string(),
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
    let source_for_error = source.clone();
    let token = crate::rest_api::auth::mutation_lock_input(&req);
    let access = crate::database::MutationAccess {
        policy: crate::auth::PageAccessPolicy::new(auth.path_prefixes()),
        lock: crate::database::MutationLock::Token {
            user_name: &auth_user,
            token: token
                .as_ref()
                .map(|value| value.as_ref())
                .map_err(|err| *err),
            validate_unlocked: false,
            invalid_token_reason: "lock token invalid",
        },
        allow_draft: true, allow_deleted: false, cleanup_draft: true,
        expected_version: None,
    };
    let update_result = state.db().put_page_authorized(
        &page_id, &auth_user, source, amend, &access,
    );
    let _committed = match update_result {
        Ok(result) => result.outcome,
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::mutation_access_error(
                &req, &err, "page update failed",
            ) { return Ok(resp); }
            if let Some(front_matter_err) =
                err.downcast_ref::<FrontMatterError>()
            {
                return Ok(resp_front_matter_error(
                    front_matter_err,
                    &source_for_error,
                ));
            }
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
    };

    req.extensions_mut().insert(_committed);

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

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent().finish())
}
