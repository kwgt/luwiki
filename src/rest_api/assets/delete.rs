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
use actix_web::{HttpResponse, web};

use crate::database::DbError;
use crate::database::types::AssetId;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

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

            if lock_info.is_some() {
                return Ok(resp_error_json(
                    StatusCode::LOCKED,
                    "page locked",
                ));
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
