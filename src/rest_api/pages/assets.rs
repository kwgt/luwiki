/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページアセット関連APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use chrono::SecondsFormat;
use serde_json::json;

use crate::database::DbError;
use crate::database::types::{LockToken, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthUser;
use super::super::resp_error_json;

/// キャッシュ指示ヘッダの固定値
const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// ロック認証ヘッダの名称
const LOCK_AUTH_HEADER: &str = "X-Lock-Authentication";

///
/// GET /api/pages/{page_id}/assets の実体
///
/// # 概要
/// ページに付随するアセットの一覧を取得する。
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
     * アセット情報取得
     */
    let asset_infos = match state.db().list_page_assets(&page_id) {
        Ok(assets) => assets,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset lookup failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    let mut assets = Vec::new();
    for asset in asset_infos {
        /*
         * 削除済みアセットの除外
         */
        if asset.deleted() {
            continue;
        }

        /*
         * ユーザ名の取得
         */
        let user_name = match state.db().get_user_name_by_id(&asset.user())
        {
            Ok(Some(name)) => name,
            Ok(None) => {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user not found",
                ));
            }
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user lookup failed",
                ));
            }
        };

        /*
         * レスポンス用データの整形
         */
        let timestamp = asset
            .timestamp()
            .to_rfc3339_opts(SecondsFormat::Secs, true);

        assets.push(json!({
            "id": asset.id().to_string(),
            "file_name": asset.file_name(),
            "mime_type": asset.mime(),
            "size": asset.size(),
            "timestamp": timestamp,
            "username": user_name,
        }));
    }

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE))
        .insert_header((header::ETAG, page_id.to_string()))
        .body(json!(assets).to_string()))
}

///
/// POST /api/pages/{page_id}/assets/{file_name} の実体
///
/// # 概要
/// ページにアセットをアップロードする。
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
/// * `file_name` - ファイル名
/// * `body` - アセットデータ
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn post(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<(String, String)>,
    body: web::Bytes,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * パス情報の取得
     */
    let (page_id_raw, file_name) = path.into_inner();
    let page_id = match parse_page_id(page_id_raw) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };
    if let Err(message) = crate::rest_api::validate_asset_file_name(
        &file_name,
    ) {
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
    let auth_user = match req.extensions().get::<AuthUser>() {
        Some(user) => user.user_id().to_string(),
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

    if let Some(lock_info) = lock_info {
        if !req.headers().contains_key(LOCK_AUTH_HEADER) {
            return Ok(resp_error_json(
                StatusCode::LOCKED,
                "page locked",
            ));
        }

        let token = match parse_lock_token(&req) {
            Ok(token) => token,
            Err(resp) => return Ok(resp),
        };

        if lock_info.token() != token {
            return Ok(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
        }

        let user_id = match state.db().get_user_id_by_name(&auth_user) {
            Ok(Some(user_id)) => user_id,
            Ok(None) => {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user not found",
                ));
            }
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user lookup failed",
                ));
            }
        };

        if lock_info.user() != user_id {
            return Ok(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock forbidden",
            ));
        }
    }

    /*
     * アセット作成
     */
    let asset_id = match state.db().create_asset(
        &page_id,
        &file_name,
        &mime,
        &auth_user,
        body.as_ref(),
    ) {
        Ok(asset_id) => asset_id,
        Err(err) => {
            if let Some(DbError::AssetAlreadyExists) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::CONFLICT,
                    "asset already exists",
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
            if let Some(DbError::UserNotFound) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "user not found",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset create failed",
            ));
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
/// GET /api/pages/{page_id}/assets/{file_name} の実体
///
/// # 概要
/// アセットIDによる取得先へリダイレクトする。
///
/// # 引数
/// * `state` - 共有状態
/// * `path` - ページID
/// * `file_name` - ファイル名
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn redirect(
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<(String, String)>,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * パス情報の取得
     */
    let (page_id_raw, file_name) = path.into_inner();
    let page_id = match parse_page_id(page_id_raw) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };
    if let Err(message) = crate::rest_api::validate_asset_file_name(
        &file_name,
    ) {
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
     * アセットIDの解決
     */
    let asset_id = match state
        .db()
        .get_asset_id_by_page_file(&page_id, &file_name)
    {
        Ok(Some(asset_id)) => asset_id,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "asset not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset lookup failed",
            ));
        }
    };

    let asset_info = match state.db().get_asset_info_by_id(&asset_id) {
        Ok(Some(info)) => info,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "asset not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset lookup failed",
            ));
        }
    };

    if asset_info.deleted() {
        return Ok(resp_error_json(
            StatusCode::GONE,
            "asset deleted",
        ));
    }

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
        .insert_header((header::ETAG, asset_id.to_string()))
        .body(body.to_string()))
}

///
/// ページIDの解析
///
/// # 引数
/// * `raw` - ページID文字列
///
/// # 戻り値
/// 変換に成功したページIDを返す。
///
fn parse_page_id(raw: String) -> Result<PageId, HttpResponse> {
    match PageId::from_string(&raw) {
        Ok(page_id) => Ok(page_id),
        Err(_) => Err(resp_error_json(
            StatusCode::NOT_FOUND,
            "page not found",
        )),
    }
}

///
/// ロック解除トークンの解析
///
fn parse_lock_token(req: &HttpRequest) -> Result<LockToken, HttpResponse> {
    let raw = match req.headers().get(LOCK_AUTH_HEADER) {
        Some(raw) => raw,
        None => {
            return Err(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
        }
    };

    let raw = match raw.to_str() {
        Ok(raw) => raw.trim(),
        Err(_) => {
            return Err(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
        }
    };

    let mut token_value = None;
    for part in raw.split_whitespace() {
        if let Some(value) = part.strip_prefix("token=") {
            token_value = Some(value);
            break;
        }
    }

    let token = match token_value {
        Some(value) => value,
        None => {
            return Err(resp_error_json(
                StatusCode::FORBIDDEN,
                "lock token invalid",
            ));
        }
    };

    match LockToken::from_string(token) {
        Ok(token) => Ok(token),
        Err(_) => Err(resp_error_json(
            StatusCode::FORBIDDEN,
            "lock token invalid",
        )),
    }
}
