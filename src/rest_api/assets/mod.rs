/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセット関連APIの実装をまとめたモジュール
//!

pub(crate) mod data;
pub(crate) mod delete;
pub(crate) mod meta;

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::resp_error_json;
use crate::database::DbError;
use crate::database::types::{BearerScope};
use crate::http_server::app_state::AppState;
use crate::rest_api::{
    AuthContext,
    CACHE_CONTROL_NO_STORE,
    require_request_scope,
};

/// ロック認証ヘッダの名称

#[derive(Deserialize)]
struct AssetQuery {
    path: Option<String>,
    file: Option<String>,
}

///
/// POST /api/assets?path={page_path}&file={file_name} の実体
///
/// # 概要
/// アセットをアップロードする。
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `body` - アセットデータ
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn post(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    body: web::Bytes,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Update) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<AssetQuery>::from_query(req.query_string()) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };

    let path = match query.path.as_deref() {
        Some(path) => path,
        None => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };
    let normalized = match crate::rest_api::pages::normalize_page_path(path) {
        Ok(path) => path,
        Err(message) => {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
        }
    };
    let path = normalized.as_str();

    let file_name = match query.file.as_deref() {
        Some(file_name) => file_name,
        None => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: file",
            ));
        }
    };
    if let Err(message) = super::validate_asset_file_name(&file_name) {
        return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
    }

    /*
     * Content-Typeの取得
     */
    let content_type = match req.headers().get(header::CONTENT_TYPE) {
        Some(value) => value.to_str().unwrap_or(""),
        None => "",
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let mime = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
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
     * ページIDの解決
     */
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
        allow_draft: true, allow_deleted: false,
        cleanup_draft: true, expected_version: None,
    };
    let asset_id = match state.db().create_asset_authorized(
        crate::database::AssetPageSelector::Path(path), &file_name, &mime,
        &auth_user, body.as_ref(), &access,
    ) {
        Ok(result) => {
            req.extensions_mut().insert(result.outcome);
            result.value.info.id()
        }
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::asset_access_error(
                &req,
                &err,
                "asset create failed",
            ) {
                return Ok(resp);
            }
            if let Some(DbError::AssetAlreadyExists) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::CONFLICT,
                    "asset already exists",
                ));
            }
            if let Some(DbError::UserNotFound) = err.downcast_ref::<DbError>() {
                return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                    "user not found"));
            }
            return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                "asset create failed"));
        }
    };

    /*
     * レスポンス生成
     */
    let body = json!({
        "id": asset_id.to_string(),
    });
    let location = format!("/api/assets/{}/data", asset_id);

    Ok(HttpResponse::Created()
        .content_type("application/json")
        .insert_header((header::LOCATION, location))
        .insert_header((header::ETAG, asset_id.to_string()))
        .body(body.to_string()))
}

///
/// GET /api/assets?path={page_path}&file={file_name} の実体
///
/// # 概要
/// アセットIDによる取得先へリダイレクトする。
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

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<AssetQuery>::from_query(req.query_string()) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };

    let path = match query.path.as_deref() {
        Some(path) => path,
        None => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };
    let normalized = match crate::rest_api::pages::normalize_page_path(path) {
        Ok(path) => path,
        Err(message) => {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
        }
    };
    let path = normalized.as_str();

    let file_name = match query.file.as_deref() {
        Some(file_name) => file_name,
        None => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: file",
            ));
        }
    };
    if let Err(message) = super::validate_asset_file_name(&file_name) {
        return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
    }

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
     * ページIDの解決
     */
    let asset = match state.db().read_asset_authorized(
        crate::database::AssetSelector::File {
            page: crate::database::AssetPageSelector::Path(path),
            file_name: &file_name,
        },
        &crate::auth::PageAccessPolicy::new(auth.path_prefixes()),
        crate::database::AssetReadMode::Metadata,
    ) {
        Ok(asset) => asset,
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::asset_access_error(
                &req, &err, "asset lookup failed",
            ) { return Ok(resp); }
            return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                "asset lookup failed"));
        }
    };
    req.extensions_mut().insert(
        crate::database::asset_read_target(
            &asset.owner,
            Some(&asset.record.info),
        )
        .map_err(actix_web::error::ErrorInternalServerError)?,
    );
    let asset_id = asset.record.info.id();

    /*
     * レスポンス生成
     */
    let body = json!({
        "id": asset_id.to_string(),
    });
    let location = format!("/api/assets/{}/data", asset_id);

    Ok(HttpResponse::Found()
        .content_type("application/json")
        .insert_header((header::LOCATION, location))
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(body.to_string()))
}
