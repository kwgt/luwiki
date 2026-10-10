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

use super::super::resp_error_json;
use crate::database::DbError;
use crate::database::types::{BearerScope, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthContext;
use crate::rest_api::{CACHE_CONTROL_NO_STORE, require_request_scope};
/// ロック認証ヘッダの名称
/// 孤立した user_id を表示する際の代替ユーザ名

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
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

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
    let (owner, asset_infos) = match state.db().list_assets_authorized(
        crate::database::AssetPageSelector::Id(&page_id),
        &crate::auth::PageAccessPolicy::new(auth.path_prefixes()),
    ) {
        Ok(result) => result,
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::asset_access_error(
                &req, &err, "asset lookup failed",
            ) { return Ok(resp); }
            return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                "asset lookup failed"));
        }
    };
    req.extensions_mut().insert(crate::database::asset_read_target(&owner, None)
        .map_err(actix_web::error::ErrorInternalServerError)?);

    /*
     * レスポンス生成
     */
    let mut assets = Vec::new();
    for record in asset_infos {
        let asset = record.info;
        /*
         * 削除済みアセットの除外
         */
        if asset.deleted() {
            continue;
        }

        /*
         * ユーザ名の取得
         */
        let user_name = record.user_name;

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

    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.count = Some(assets.len());
    });
    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Update) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    /*
     * パス情報の取得
     */
    let (page_id_raw, file_name) = path.into_inner();
    let page_id = match parse_page_id(page_id_raw) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };
    if let Err(message) =
        crate::rest_api::validate_asset_file_name(&file_name)
    {
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
     * ページ情報取得
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
        crate::database::AssetPageSelector::Id(&page_id), &file_name, &mime,
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
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<(String, String)>,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    /*
     * パス情報の取得
     */
    let (page_id_raw, file_name) = path.into_inner();
    let page_id = match parse_page_id(page_id_raw) {
        Ok(page_id) => page_id,
        Err(resp) => return Ok(resp),
    };
    if let Err(message) =
        crate::rest_api::validate_asset_file_name(&file_name)
    {
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
    let asset = match state.db().read_asset_authorized(
        crate::database::AssetSelector::File {
            page: crate::database::AssetPageSelector::Id(&page_id),
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
        Err(_) => Err(resp_error_json(StatusCode::NOT_FOUND, "page not found")),
    }
}
