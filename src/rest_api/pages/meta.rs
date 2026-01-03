/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページメタ情報取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpRequest, HttpResponse, web};
use chrono::SecondsFormat;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::database::types::{PageId, PageIndex};
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

/// キャッシュ指示ヘッダの固定値
const CACHE_CONTROL_IMMUTABLE: &str = "public, max-age=31536000, immutable";

#[derive(Deserialize)]
struct GetMetaQuery {
    rev: Option<String>,
}

///
/// GET /api/pages/{page_id}/meta の実体
///
/// # 概要
/// ページのメタ情報を取得する
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
/// # 注記
/// エラー時はJSON形式で返却する。
/// 処理の流れはクエリ検証、ID解析、状態取得、
/// ページ情報取得、メタ情報取得、レスポンス生成の順。
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<GetMetaQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: rev",
            ));
        }
    };

    let revision = match query.rev.as_deref() {
        Some(raw) => match raw.parse::<u64>() {
            Ok(revision) => Some(revision),
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: rev",
                ));
            }
        },
        None => None,
    };

    /*
     * ページID解析
     */
    let page_id_raw = path.into_inner();
    let page_id = match PageId::from_string(&page_id_raw) {
        Ok(page_id) => page_id,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
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
     * ページ情報取得
     */
    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    /*
     * ページソース取得
     */
    if page_index.is_draft() {
        let locked = match state.db().get_page_lock_info(&page_id) {
            Ok(info) => info.is_some(),
            Err(_) => {
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "lock lookup failed",
                ));
            }
        };

        let page_info = json!({
            "path": build_path_info(&page_index),
            "revision_scope": {
                "latest": 0,
                "oldest": 0,
            },
            "rename_revisions": [],
            "deleted": page_index.deleted(),
            "locked": locked,
        });

        let mut body = Map::new();
        body.insert("page_info".to_string(), page_info);

        let etag = format!("\"{}:draft\"", page_id);

        return Ok(HttpResponse::Ok()
            .content_type("application/json")
            .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE))
            .insert_header((header::ETAG, etag))
            .body(Value::Object(body).to_string()));
    }

    let revision = revision.unwrap_or(page_index.latest());

    let page_source = match state.db().get_page_source(&page_id, revision) {
        Ok(Some(source)) => source,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "page source not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page source lookup failed",
            ));
        }
    };

    /*
     * ロック情報の取得
     */
    let locked = match state.db().get_page_lock_info(&page_id) {
        Ok(info) => info.is_some(),
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "lock lookup failed",
            ));
        }
    };

    /*
     * ユーザ名の取得
     */
    let user_name = match state.db().get_user_name_by_id(&page_source.user()) {
        Ok(Some(name)) => name,
        Ok(None) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user not found",
            ));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "user lookup failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    let timestamp = page_source.timestamp()
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    let mut revision_info = Map::new();
    revision_info.insert("revision".to_string(), json!(revision));
    revision_info.insert("timestamp".to_string(), json!(timestamp));
    revision_info.insert("username".to_string(), json!(user_name));

    if let Some(rename) = page_source.rename() {
        if let Some(from) = rename.from() {
            let link_refs = match serde_json::to_value(rename.link_refs()) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(resp_error_json(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "link refs serialize failed",
                    ));
                }
            };

            revision_info.insert(
                "rename_info".to_string(),
                json!({
                    "from": from,
                    "to": rename.to(),
                    "link_refs": link_refs,
                }),
            );
        }
    }

    let page_info = json!({
        "path": build_path_info(&page_index),
        "revision_scope": {
            "latest": page_index.latest(),
            "oldest": page_index.earliest(),
        },
        "rename_revisions": page_index.rename_revisions(),
        "deleted": page_index.deleted(),
        "locked": locked,
    });

    let mut body = Map::new();
    body.insert("page_info".to_string(), page_info);
    body.insert(
        "revision_info".to_string(),
        Value::Object(revision_info),
    );

    let etag = format!("\"{}:{}\"", page_id, revision);

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_IMMUTABLE))
        .insert_header((header::ETAG, etag))
        .body(Value::Object(body).to_string()))
}

fn build_path_info(page_index: &PageIndex) -> Value {
    let kind = if page_index.deleted() {
        "last_deleted"
    } else {
        "current"
    };

    json!({
        "kind": kind,
        "value": page_index.path(),
    })
}
