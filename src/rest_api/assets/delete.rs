/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセット削除APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};

use crate::database::DbError;
use crate::database::types::AssetId;
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthUser;
use super::super::resp_error_json;

/// ロック認証ヘッダの名称
const LOCK_AUTH_HEADER: &str = "X-Lock-Authentication";

///
/// DELETE /api/assets/{asset_id} の実体
///
/// # 概要
/// アセットを削除する。
///
/// # 引数
/// * `state` - 共有状態
/// * `path` - アセットID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn delete(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * アセットID解析
     */
    let asset_id = match parse_asset_id(path.into_inner()) {
        Ok(asset_id) => asset_id,
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
     * アセット情報取得
     */
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
     * ロック検証
     */
    if !asset_info.is_zombie() {
        if let Some(page_id) = asset_info.page_id() {
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

                let auth_user = match req.extensions().get::<AuthUser>() {
                    Some(user) => user.user_id().to_string(),
                    None => {
                        return Ok(resp_error_json(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "auth context missing",
                        ));
                    }
                };

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
        }
    }

    /*
     * アセット削除
     */
    match state.db().delete_asset(&asset_id) {
        Ok(()) => {}
        Err(err) => {
            if let Some(DbError::AssetNotFound) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::NOT_FOUND,
                    "asset not found",
                ));
            }
            if let Some(DbError::AssetDeleted) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::GONE,
                    "asset deleted",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset delete failed",
            ));
        }
    }

    /*
     * レスポンス生成
     */
    Ok(HttpResponse::NoContent().finish())
}

///
/// アセットIDの解析
///
/// # 引数
/// * `raw` - アセットID文字列
///
/// # 戻り値
/// 変換に成功したアセットIDを返す。
///
fn parse_asset_id(raw: String) -> Result<AssetId, HttpResponse> {
    match AssetId::from_string(&raw) {
        Ok(asset_id) => Ok(asset_id),
        Err(_) => Err(resp_error_json(
            StatusCode::NOT_FOUND,
            "asset not found",
        )),
    }
}

///
/// ロック解除トークンの解析
///
fn parse_lock_token(req: &HttpRequest) -> Result<crate::database::types::LockToken, HttpResponse> {
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

    match crate::database::types::LockToken::from_string(token) {
        Ok(token) => Ok(token),
        Err(_) => Err(resp_error_json(
            StatusCode::FORBIDDEN,
            "lock token invalid",
        )),
    }
}
