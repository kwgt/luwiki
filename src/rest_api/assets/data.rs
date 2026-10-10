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
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};

use super::super::resp_error_json;
use crate::database::types::{AssetId, BearerScope};
use crate::http_server::app_state::AppState;
use crate::rest_api::{
    build_etag,
    if_none_match_matches,
    require_request_scope,
    CACHE_CONTROL_NO_STORE,
    CACHE_CONTROL_REVALIDATE_PRIVATE,
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
    let auth = match require_request_scope(&req, BearerScope::Read) {
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
    let asset = match state.db().read_asset_authorized(
        crate::database::AssetSelector::Id(&asset_id),
        &crate::auth::PageAccessPolicy::new(auth.path_prefixes()),
        crate::database::AssetReadMode::Data { max_bytes: None },
    ) {
        Ok(asset) => asset,
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::asset_access_error(
                &req, &err, "asset lookup failed",
            ) { return Ok(resp); }
            return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
                "asset read failed"));
        }
    };
    let target = crate::database::asset_read_target(
        &asset.owner,
        Some(&asset.record.info),
    )
    .map_err(actix_web::error::ErrorInternalServerError)?;
    req.extensions_mut().insert(target);
    let asset_info = asset.record.info;
    let data = match asset.data {
        Some(data) => data,
        None => return Ok(resp_error_json(StatusCode::INTERNAL_SERVER_ERROR,
            "asset read failed")),
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
                .insert_header((
                    header::CACHE_CONTROL,
                    CACHE_CONTROL_REVALIDATE_PRIVATE,
                ))
                .insert_header((header::ETAG, etag))
                .finish());
        }

        return Ok(HttpResponse::Ok()
            .content_type(asset_info.mime())
            .insert_header((header::CONTENT_DISPOSITION, content_disposition))
            .insert_header((
                header::CACHE_CONTROL,
                CACHE_CONTROL_REVALIDATE_PRIVATE,
            ))
            .insert_header((header::ETAG, etag))
            .body(data));
    }

    Ok(HttpResponse::Ok()
        .content_type(asset_info.mime())
        .insert_header((header::CONTENT_DISPOSITION, content_disposition))
        .insert_header((
            header::CACHE_CONTROL,
            CACHE_CONTROL_NO_STORE,
        ))
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
