/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 埋め込み静的ファイルの配信を行うモジュール
//!

use std::borrow::Cow;

use actix_web::{HttpResponse, http::header, mime, web};
use mime_guess::MimeGuess;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

///
/// 埋め込み済みファイルの取得
///
/// # 引数
/// * `path` - 取得対象のパス
///
/// # 戻り値
/// 取得できた場合はバイナリを返す。
///
pub(crate) fn get_embedded_file(path: &str) -> Option<Cow<'static, [u8]>> {
    FrontendAssets::get(path).map(|file| file.data)
}

///
/// ファイルパスからMIMEを推定する
///
/// # 引数
/// * `path` - ファイルパス
///
/// # 戻り値
/// 推定したMIMEを返す。
///
fn guess_mime(path: &str) -> mime::Mime {
    MimeGuess::from_path(path).first_or_octet_stream()
}

///
/// 静的ファイル配信ハンドラ
///
pub(crate) async fn get(path: web::Path<String>) -> HttpResponse {
    let path = path.into_inner();
    let data = match get_embedded_file(&path) {
        Some(data) => data,
        None => return HttpResponse::NotFound().finish(),
    };

    let mime = guess_mime(&path);
    HttpResponse::Ok()
        .insert_header((
            header::CACHE_CONTROL,
            "public, max-age=31536000, immutable",
        ))
        .content_type(mime.as_ref())
        .body(data)
}
