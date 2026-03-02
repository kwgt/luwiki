/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ユーザ情報APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{StatusCode, header};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde_json::json;

use crate::http_server::app_state::AppState;
use crate::rest_api::{AuthUser, CACHE_CONTROL_NO_STORE, resp_error_json};

///
/// GET /api/users/me の実体
///
/// # 概要
/// 認証済みユーザ自身の情報を取得する。
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> actix_web::Result<HttpResponse> {
    /*
     * 認証済みユーザ名の取得
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
     * ユーザ情報取得
     */
    let user_info = match state.db().get_user_info_by_name(&auth_user) {
        Ok(Some(user_info)) => user_info,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user query failed",
            ));
        }
    };

    let user_id = user_info.id().to_string();
    let timestamp = user_info.timestamp();
    let timestamp_iso = timestamp.format("%Y-%m-%dT%H:%M:%S").to_string();
    let body = json!({
        "id": user_id,
        "username": user_info.username(),
        "display_name": user_info.display_name(),
        "timestamp": timestamp_iso,
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(body.to_string()))
}
