/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! REST APIの実装を集約するモジュール
//!

pub(crate) mod audit;
#[cfg(test)]
mod audit_boundary_tests;
#[cfg(test)]
mod audit_operation_tests;
#[cfg(test)]
mod auth_contract_tests;
mod assets;
mod auth;
mod hello;
#[cfg(test)]
mod authorization_tests;
#[cfg(test)]
mod group3_tests;
#[cfg(test)]
mod group45_tests;
mod pages;
mod users;

use actix_web::dev::HttpServiceFactory;
use actix_web::http::header;
use actix_web::middleware;
use actix_web::{HttpResponse, web};
use actix_web_httpauth::extractors::basic::Config;
use serde_json::json;

#[allow(unused_imports)]
pub(crate) use crate::auth::AuthContext;
#[allow(unused_imports)]
pub(crate) use auth::require_request_scope;

/// ファイル名で禁止する文字
/// (追加しやすいように集約する)
const FORBIDDEN_FILE_NAME_CHARS: &[char] = &['/', '\\'];

/// キャッシュを禁止させる場合のCache-Controlヘッダ値
pub(crate) const CACHE_CONTROL_NO_STORE: &str = "no-store";

/// Basic認証チャレンジで使用するrealm名
pub(crate) const BASIC_AUTH_REALM: &str = "LuWiki REST API";

/// 条件付きGETを許可する場合のCache-Controlヘッダ値
pub(crate) const CACHE_CONTROL_REVALIDATE_PRIVATE: &str =
    "private, max-age=3600, no-cache";

///
/// Success (200)を返す場合のレスポンスビルド関数
///
/// # 引数
/// * `str` - レスポンスのボディに設定するJSON文字列
///
/// # 戻り値
/// レスポンスオブジェクト
///
fn resp_200<S>(body: S) -> HttpResponse
where
    S: ToString,
{
    HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .content_type("text/plain")
        .body(body.to_string())
}

///
/// JSON形式のエラーレスポンスを返す場合の
/// レスポンスビルド関数
///
/// # 引数
/// * `status` - ステータスコード
/// * `reason` - エラー理由
///
/// # 戻り値
/// レスポンスオブジェクト
///
fn resp_error_json<S>(
    status: actix_web::http::StatusCode,
    reason: S,
) -> HttpResponse
where
    S: ToString,
{
    let body = json!({
        "reason": reason.to_string(),
    });

    HttpResponse::build(status)
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .content_type("application/json")
        .body(body.to_string())
}

///
/// ETagヘッダ値の生成
///
/// # 引数
/// * `value` - ETagに埋め込む値
///
/// # 戻り値
/// RFC上のquoted-string形式のETagを返す。
///
pub(crate) fn build_etag<S>(value: S) -> String
where
    S: AsRef<str>,
{
    format!("\"{}\"", value.as_ref())
}

///
/// If-None-Matchヘッダの照合
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `etag` - 比較対象のETag(クオート付き)
///
/// # 戻り値
/// 一致する場合は`true`を返す。
///
pub(crate) fn if_none_match_matches(
    req: &actix_web::HttpRequest,
    etag: &str,
) -> bool {
    /*
     * ヘッダ値の取得
     */
    let raw = match req.headers().get(header::IF_NONE_MATCH) {
        Some(value) => value,
        None => return false,
    };

    let raw = match raw.to_str() {
        Ok(value) => value,
        Err(_) => return false,
    };

    /*
     * ETag候補との照合
     */
    raw.split(',').map(|part| part.trim()).any(|candidate| {
        candidate == "*"
            || candidate == etag
            || candidate
                .strip_prefix("W/")
                .is_some_and(|weak| weak == etag)
    })
}

///
/// アセット用ファイル名の妥当性チェック
///
/// # 引数
/// * `file_name` - 対象のファイル名
///
/// # 戻り値
/// 検証に成功した場合は`Ok(())`を返す。
///
pub(crate) fn validate_asset_file_name(
    file_name: &str,
) -> Result<(), &'static str> {
    if file_name.is_empty() {
        return Err("file name is empty");
    }

    if file_name
        .chars()
        .any(|ch| FORBIDDEN_FILE_NAME_CHARS.contains(&ch))
    {
        return Err("file name contains invalid character");
    }

    Ok(())
}

///
/// ページパスの妥当性チェック
///
/// # 引数
/// * `path` - 対象のページパス
///
/// # 戻り値
/// 検証に成功した場合は`Ok(())`を返す。
///
pub(crate) fn validate_page_path(path: &str) -> Result<(), &'static str> {
    pages::validate_page_path(path)
}

///
/// REST APIエンドポイントの生成
///
/// # 引数
/// * `payload_limit` - 既存upload resourceのpayload上限（byte）
///
/// # 戻り値
/// 認証を外側に、固定操作の監査境界を内側に持つscope。
///
/// # 概要
/// 旧routeのmethod guardと既存resourceのdefault応答を維持する。
///
pub(crate) fn create_api_scope(
    payload_limit: usize,
) -> impl HttpServiceFactory {
    /*
     * APIスコープの初期設定
     */
    web::scope("/api")
        .app_data(Config::default().realm(BASIC_AUTH_REALM))
        .wrap(actix_web_httpauth::middleware::HttpAuthentication::with_fn(
            auth::validate_authorization,
        ))
        .wrap(middleware::from_fn(auth::append_bearer_expire_header))
        /*
         * 同じ固定resourceでmethodを束ね監査境界をextractor外へ置く
         */
        .service(
            web::resource(audit::REST_RESOURCES[0].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[0].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[0],
                    )
                }))
                .route(web::get().to(hello::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[1].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[1].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[1],
                    )
                }))
                .route(web::post().to(pages::post))
                .route(web::get().to(pages::list::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[2].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[2].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[2],
                    )
                }))
                .route(web::get().to(pages::deleted::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[3].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[3].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[3],
                    )
                }))
                .route(web::get().to(pages::id::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[4].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[4].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[4],
                    )
                }))
                .route(web::get().to(pages::short::get_by_path)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[5].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[5].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[5],
                    )
                }))
                .route(web::get().to(pages::template::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[6].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[6].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[6],
                    )
                }))
                .route(web::get().to(pages::search::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[7].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[7].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[7],
                    )
                }))
                .route(web::get().to(pages::source::get))
                .route(web::put().to(pages::source::put)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[8].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[8].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[8],
                    )
                }))
                .route(web::get().to(pages::meta::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[9].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[9].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[9],
                    )
                }))
                .route(web::get().to(pages::parent::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[10].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[10].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[10],
                    )
                }))
                .route(web::get().to(pages::short::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[11].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[11].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[11],
                    )
                }))
                .route(web::get().to(pages::path::get))
                .route(web::post().to(pages::path::post)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[12].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[12].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[12],
                    )
                }))
                .route(web::post().to(pages::revision::post)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[13].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[13].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[13],
                    )
                }))
                .route(web::get().to(pages::assets::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[14].route)
                .app_data(web::PayloadConfig::new(payload_limit))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[14],
                    )
                }))
                .route(web::post().to(pages::assets::post))
                .route(web::get().to(pages::assets::redirect)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[15].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[15].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[15],
                    )
                }))
                .route(web::post().to(pages::lock::post))
                .route(web::put().to(pages::lock::put))
                .route(web::get().to(pages::lock::get))
                .route(web::delete().to(pages::lock::delete)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[16].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[16].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[16],
                    )
                }))
                .route(web::delete().to(pages::delete::delete)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[17].route)
                .app_data(web::PayloadConfig::new(payload_limit))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[17],
                    )
                }))
                .route(web::post().to(assets::post))
                .route(web::get().to(assets::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[18].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[18].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[18],
                    )
                }))
                .route(web::get().to(assets::data::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[19].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[19].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[19],
                    )
                }))
                .route(web::get().to(assets::meta::get)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[20].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[20].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[20],
                    )
                }))
                .route(web::delete().to(assets::delete::delete)),
        )
        .service(
            web::resource(audit::REST_RESOURCES[21].route)
                .guard(actix_web::guard::fn_guard(|context| {
                    audit::REST_RESOURCES[21].operation(&context.head().method)
                        .is_some()
                }))
                .wrap(middleware::from_fn(|req, next| {
                    audit::request_boundary(
                        req, next, audit::REST_RESOURCES[21],
                    )
                }))
                .route(web::get().to(users::me::get)),
        )
}
