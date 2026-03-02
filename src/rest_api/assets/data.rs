/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセットデータ取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{StatusCode, header};
use actix_web::{HttpRequest, HttpResponse, web};

use super::super::resp_error_json;
use crate::database::types::AssetId;
use crate::http_server::app_state::AppState;
use crate::rest_api::{
    CACHE_CONTROL_NO_STORE, CACHE_CONTROL_REVALIDATE_PRIVATE, build_etag, if_none_match_matches,
};

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
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
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
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "asset not found"));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset lookup failed",
            ));
        }
    };

    if asset_info.deleted() {
        return Ok(resp_error_json(StatusCode::GONE, "asset deleted"));
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
    let content_disposition = header::ContentDisposition {
        disposition: header::DispositionType::Attachment,
        parameters: vec![header::DispositionParam::Filename(
            asset_info.file_name().to_string(),
        )],
    };

    if let Some(instance_id) = asset_info.instance_id() {
        let etag = build_etag(instance_id.to_string());
        if if_none_match_matches(&req, &etag) {
            return Ok(HttpResponse::NotModified()
                .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_REVALIDATE_PRIVATE))
                .insert_header((header::ETAG, etag))
                .finish());
        }

        return Ok(HttpResponse::Ok()
            .content_type(asset_info.mime())
            .insert_header((header::CONTENT_DISPOSITION, content_disposition))
            .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_REVALIDATE_PRIVATE))
            .insert_header((header::ETAG, etag))
            .body(data));
    }

    Ok(HttpResponse::Ok()
        .content_type(asset_info.mime())
        .insert_header((header::CONTENT_DISPOSITION, content_disposition))
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
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
        Err(_) => Err(resp_error_json(StatusCode::NOT_FOUND, "asset not found")),
    }
}
