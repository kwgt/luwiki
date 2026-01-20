/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ関連のREST APIを実装するモジュール
//!

pub(crate) mod source;
pub(crate) mod meta;
pub(crate) mod lock;
pub(crate) mod path;
pub(crate) mod assets;
pub(crate) mod delete;
pub(crate) mod deleted;
pub(crate) mod parent;
pub(crate) mod search;
pub(crate) mod template;
pub(crate) mod revision;

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use crate::database::DbError;
use crate::http_server::app_state::AppState;
use crate::rest_api::AuthUser;
use super::resp_error_json;

/// ページパスで禁止する文字(追加しやすいように集約する)
const FORBIDDEN_PATH_CHARS: &[char] = &['\\'];

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
/// 処理の流れはクエリ検証、ボディ検証、認証ユーザ取得、
/// 状態取得、ドラフト作成、レスポンス生成の順。
///
pub async fn post(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    body: web::Bytes,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<CreatePageQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };

    if let Err(message) = validate_page_path(&query.path) {
        return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
    }

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
    let auth_user = match req.extensions().get::<AuthUser>() {
        Some(user) => user.user_id().to_string(),
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
    let (page_id, lock_info) = match state.db().create_draft_page(
        &query.path,
        auth_user,
    ) {
        Ok(result) => result,
        Err(err) => {
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
    if !path.starts_with('/') {
        return Err("path must be absolute");
    }

    if path.chars().any(|ch| FORBIDDEN_PATH_CHARS.contains(&ch)) {
        return Err("path contains invalid character");
    }

    Ok(())
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
