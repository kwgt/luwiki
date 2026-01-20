/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! REST APIの実装を集約するモジュール
//!

mod hello;
mod assets;
mod pages;

use std::sync::{Arc, RwLock};

use actix_web::dev::{HttpServiceFactory, ServiceRequest};
use actix_web::error::{ErrorInternalServerError, ErrorUnauthorized};
use actix_web::http::header;
use actix_web::{HttpMessage, HttpResponse, web};
use actix_web_httpauth::extractors::basic::BasicAuth;
use serde_json::json;

use crate::http_server::app_state::AppState;

/// ファイル名で禁止する文字(追加しやすいように集約する)
const FORBIDDEN_FILE_NAME_CHARS: &[char] = &['/', '\\'];

/// キャッシュを禁止させる場合のCache-Controlヘッダのテンプレート
const NO_CACHE_TEMPLATE: &str = concat!(
    "no-store, ",
    "no-cache, ",
    "must-revalidate, ",
    "max-age=0",
);

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
        .insert_header((header::CACHE_CONTROL, NO_CACHE_TEMPLATE))
        .content_type("text/plain")
        .body(body.to_string())
}

///
/// JSON形式のエラーレスポンスを返す場合のレスポンスビルド関数
///
/// # 引数
/// * `status` - ステータスコード
/// * `reason` - エラー理由
///
/// # 戻り値
/// レスポンスオブジェクト
///
fn resp_error_json<S>(status: actix_web::http::StatusCode, reason: S)
    -> HttpResponse
where
    S: ToString,
{
    let body = json!({
        "reason": reason.to_string(),
    });

    HttpResponse::build(status)
        .insert_header((header::CACHE_CONTROL, NO_CACHE_TEMPLATE))
        .content_type("application/json")
        .body(body.to_string())
}

///
/// 認証済みユーザ情報
///
pub(crate) struct AuthUser {
    user_id: String,
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

impl AuthUser {
    ///
    /// 認証済みユーザ情報の生成
    ///
    /// # 引数
    /// * `user_id` - ユーザID
    ///
    /// # 戻り値
    /// 生成したユーザ情報を返す。
    ///
    pub(crate) fn new(user_id: String) -> Self {
        Self { user_id }
    }

    ///
    /// ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// ユーザIDを返す。
    ///
    pub(crate) fn user_id(&self) -> &str {
        &self.user_id
    }
}

///
/// Basic認証の検証
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `credentials` - 認証情報
///
/// # 戻り値
/// 認証に成功した場合はリクエストをそのまま返す。
///
pub(crate) async fn validate_basic_auth(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, (actix_web::Error, ServiceRequest)> {
    let data = match req
        .app_data::<web::Data<Arc<RwLock<AppState>>>>()
    {
        Some(data) => data.clone(),
        None => return Err((ErrorInternalServerError("state not found"), req)),
    };

    let password = match credentials.password() {
        Some(password) => password.to_owned(),
        None => return Err((ErrorUnauthorized("unauthorized"), req)),
    };

    let username = credentials.user_id().to_string();

    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return Err((
            ErrorInternalServerError("state lock failed"),
            req
        )),
    };
    let ok = match state.db().verify_user(&username, &password) {
        Ok(ok) => ok,
        Err(_) => return Err((
            ErrorInternalServerError("auth failed"),
            req
        )),
    };

    if !ok {
        return Err((ErrorUnauthorized("unauthorized"), req));
    }

    req.extensions_mut().insert(AuthUser::new(username));

    Ok(req)
}

///
/// REST APIエンドポイントの生成
///
pub(crate) fn create_api_scope() -> impl HttpServiceFactory {
    web::scope("/api")
        .wrap(actix_web_httpauth::middleware::HttpAuthentication::basic(
            validate_basic_auth
        ))
        .route("/hello", web::get().to(hello::get))
        .route("/pages", web::post().to(pages::post))
        .route("/pages/deleted", web::get().to(pages::deleted::get))
        .route("/pages/search", web::get().to(pages::search::get))
        .route("/pages/template", web::get().to(pages::template::get))
        .route("/pages/{page_id}/source", web::get().to(pages::source::get))
        .route("/pages/{page_id}/source", web::put().to(pages::source::put))
        .route("/pages/{page_id}/meta", web::get().to(pages::meta::get))
        .route("/pages/{page_id}/parent", web::get().to(pages::parent::get))
        .route("/pages/{page_id}/path", web::get().to(pages::path::get))
        .route("/pages/{page_id}/path", web::post().to(pages::path::post))
        .route("/pages/{page_id}/revision", web::post().to(pages::revision::post))
        .route("/pages/{page_id}/assets", web::get().to(pages::assets::get))
        .route(
            "/pages/{page_id}/assets/{file_name}",
            web::post().to(pages::assets::post),
        )
        .route(
            "/pages/{page_id}/assets/{file_name}",
            web::get().to(pages::assets::redirect),
        )
        .route("/pages/{page_id}/lock", web::post().to(pages::lock::post))
        .route("/pages/{page_id}/lock", web::put().to(pages::lock::put))
        .route("/pages/{page_id}/lock", web::get().to(pages::lock::get))
        .route("/pages/{page_id}/lock", web::delete().to(pages::lock::delete))
        .route("/pages/{page_id}", web::delete().to(pages::delete::delete))
        .route("/assets", web::post().to(assets::post))
        .route("/assets", web::get().to(assets::get))
        .route("/assets/{asset_id}/data", web::get().to(assets::get_data))
        .route("/assets/{asset_id}/meta", web::get().to(assets::get_meta))
        .route("/assets/{asset_id}", web::delete().to(assets::delete))
}
