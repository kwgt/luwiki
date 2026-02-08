/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセットデータ取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpResponse, web};

use crate::database::types::AssetId;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

/// キャッシュ指示ヘッダの固定値
const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";

///
/// GET /api/assets/{asset_id}/data の実体
///
/// # 概要
/// アセットの本体データを取得する。
///
/// # 引数
/// * `state` - 共有状態
/// * `path` - アセットID
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
     * アセットデータ取得
     */
    let data = match state.db().read_asset_data(&asset_id) {
        Ok(data) => data,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset read failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    let etag = asset_id.to_string();
    let content_disposition = header::ContentDisposition {
        disposition: header::DispositionType::Attachment,
        parameters: vec![header::DispositionParam::Filename(
            asset_info.file_name().to_string(),
        )],
    };

    Ok(HttpResponse::Ok()
        .content_type(asset_info.mime())
        .insert_header((header::CONTENT_DISPOSITION, content_disposition))
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE))
        .insert_header((header::ETAG, etag))
        .body(data))
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
