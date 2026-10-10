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

use super::super::resp_error_json;
use crate::database::types::{AssetId, BearerScope};
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

/// ロック認証ヘッダの名称

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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Delete) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

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
    let auth_user = auth.user_id().to_string();
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
    match state.db().delete_asset_authorized(
        crate::database::AssetSelector::Id(&asset_id), &access,
    ) {
        Ok(result) => { req.extensions_mut().insert(result.outcome); }
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::asset_access_error(
                &req, &err, "asset delete failed",
            ) { return Ok(resp); }
            return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                "asset delete failed"));
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
