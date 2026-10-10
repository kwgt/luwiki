/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ関連のREST APIを実装するモジュール
//!

pub(crate) mod assets;
pub(crate) mod delete;
pub(crate) mod deleted;
pub(crate) mod id;
pub(crate) mod list;
pub(crate) mod lock;
pub(crate) mod meta;
pub(crate) mod parent;
pub(crate) mod path;
pub(crate) mod revision;
pub(crate) mod search;
pub(crate) mod short;
pub(crate) mod source;
pub(crate) mod template;

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::resp_error_json;
use crate::database::DbError;
use crate::database::types::BearerScope;
use crate::http_server::app_state::AppState;
use crate::rest_api::{AuthContext, require_request_scope};


#[derive(Deserialize)]
struct CreatePageQuery {
    path: String,
}

///
/// POST /api/pages の実体
///
/// # 概要
/// ドラフトページを新規作成する
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `body` - リクエストボディ
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
/// # 注記
/// エラー時はJSON形式で返却する。
/// 処理の流れは
/// クエリ検証、ボディ検証、認証ユーザ取得、
/// 状態取得、ドラフト作成、レスポンス生成の順。
///
pub async fn post(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    body: web::Bytes,
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Create) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<CreatePageQuery>::from_query(
        req.query_string(),
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };

    let mut query = query.into_inner();
    query.path = match normalize_page_path(&query.path) {
        Ok(path) => path,
        Err(message) => {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
        }
    };
    crate::rest_api::audit::remember_path(
        &req,
        crate::wiki_path::NormalizedWikiPath::from_stored(&query.path),
    );


    /*
     * ボディ検証
     */
    if !body.is_empty() {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "request body must be empty",
        ));
    }

    /*
     * 認証ユーザ取得
     */
    let auth_user = match req.extensions().get::<AuthContext>() {
        Some(context) => context.user_id().to_string(),
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
     * ドラフト作成
     */
    let (page_id, lock_info) = match state
        .db()
        .create_draft_authorized(&query.path, &auth_user,
            &crate::auth::PageAccessPolicy::new(auth.path_prefixes()))
    {
        Ok(result) => {
            req.extensions_mut().insert(result.outcome);
            result.value
        },
        Err(err) => {
            if let Some(resp) = crate::rest_api::auth::mutation_access_error(
                &req, &err, "create draft failed",
            ) { return Ok(resp); }
            if let Some(DbError::PageAlreadyExists) =
                err.downcast_ref::<DbError>()
            {
                return Ok(resp_error_json(
                    StatusCode::CONFLICT,
                    "page already exists",
                ));
            }

            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "create draft failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    let body = json!({
        "id": page_id.to_string(),
    });

    let location = format!("/api/pages/{}/meta", page_id);

    Ok(HttpResponse::Created()
        .content_type("application/json")
        .insert_header((header::LOCATION, location))
        .insert_header((header::ETAG, page_id.to_string()))
        .insert_header((
            header::HeaderName::from_static("x-page-lock"),
            self::lock::build_lock_header(&lock_info),
        ))
        .body(body.to_string()))
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
    // CLIの従来配置先末尾記法を保つ入口。RESTは最終値取得を使う。
    if !path.starts_with('/') {
        return Err("path must be absolute");
    }
    if path.contains('\\') {
        return Err("path contains invalid character");
    }
    Ok(())
}

///
/// 通常入力の最終パスを一度だけ取得する
///
/// # 引数
/// * `path` - 解釈済み入力
///
/// # 戻り値
/// dot解決済みパス、または入力エラーを返す。
///
pub(crate) fn normalize_page_path(path: &str) -> Result<String, &'static str> {
    if !path.starts_with('/') {
        return Err("path must be absolute");
    }
    crate::wiki_path::NormalizedWikiPath::parse(path)
        .map(|value| value.as_str().to_string())
        .map_err(|_| "path contains invalid character")
}

///
/// Markdown用のContent-Typeかどうかの判定
///
/// # 引数
/// * `value` - Content-Typeヘッダ値
///
/// # 戻り値
/// 対応しているContent-Typeの場合は`true`を返す。
///
fn is_supported_markdown_content_type(value: &str) -> bool {
    let content_type = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    content_type == "text/markdown"
}
