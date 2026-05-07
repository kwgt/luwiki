/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! Wikiアイコン画像配信を行うモジュール
//!

use std::fs;
use std::sync::{Arc, RwLock};

use actix_web::{HttpResponse, http::header, web};
use mime_guess::MimeGuess;

use super::app_state::AppState;

/// キャッシュ抑止用のCache-Controlヘッダ値
const CACHE_CONTROL_NO_STORE_NO_CACHE: &str = "no-store, no-cache";

///
/// GET /wiki-icon の実体
///
/// # 引数
/// * `state` - 共有状態
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub(crate) async fn get(
    state: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * 共有状態取得
     */
    let state = match state.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let path = match state.wiki_icon() {
        Some(path) => path.clone(),
        None => return HttpResponse::NotFound().finish(),
    };

    /*
     * アイコン画像の読み込み
     */
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) => {
            return if err.kind() == std::io::ErrorKind::NotFound {
                HttpResponse::NotFound().finish()
            } else {
                HttpResponse::InternalServerError().finish()
            };
        }
    };

    /*
     * MIME推定とレスポンス返却
     */
    let mime = MimeGuess::from_path(&path).first_or_octet_stream();
    HttpResponse::Ok()
        .insert_header((
            header::CACHE_CONTROL,
            CACHE_CONTROL_NO_STORE_NO_CACHE,
        ))
        .content_type(mime.as_ref())
        .body(data)
}
