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
use serde_json::{Map, Value, json};

use super::super::resp_error_json;
use crate::database::short_id::encode_page_short_id;
use crate::database::types::{BearerScope, PageId, PageIndex, RenameInfo};
use crate::http_server::app_state::AppState;
use crate::rest_api::{CACHE_CONTROL_NO_STORE, require_request_scope};

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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<GetMetaQuery>::from_query(
        req.query_string(),
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
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
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
    let (page, lock, _) = match state.db().read_authorized_page_meta(
        &page_id, revision, true, true, &policy,
    ) {
        Ok(result) => {
            crate::rest_api::audit::remember_page(&req, &result.0);
            result
        },
        Err(err) => {
            if let Some(resp) =
                crate::rest_api::auth::page_access_error_for_request(&req, &err)
            {
                return Ok(resp);
            }
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR, "page lookup failed"));
        }
    };
    let locked = lock.is_some();
    let page_index = page.index;
    if page_index.is_draft() {
        let page_info = build_page_info(&page_index, &page_id, locked);
        let mut body = Map::new();
        body.insert("page_info".to_string(), page_info);
        return Ok(HttpResponse::Ok()
            .content_type("application/json")
            .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
            .body(Value::Object(body).to_string()));
    }
    let revision = page.revision;
    let page_source = match page.source {
        Some(source) => source,
        None => return Ok(resp_error_json(
            StatusCode::NOT_FOUND, "page source not found")),
    };
    let user_name = match page.user_name {
        Some(name) => name,
        None => return Ok(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR, "user not found")),
    };

    /*
     * レスポンス生成
     */
    let timestamp = page_source
        .timestamp()
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    let mut revision_info = Map::new();
    revision_info.insert("revision".to_string(), json!(revision));
    revision_info.insert("timestamp".to_string(), json!(timestamp));
    revision_info.insert("username".to_string(), json!(user_name));

    let rename = page_source.rename();
    let rename_info = match build_rename_info(&rename) {
        Ok(rename_info) => rename_info,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "link refs serialize failed",
            ));
        }
    };

    if let Some(rename_info) = rename_info {
        revision_info.insert("rename_info".to_string(), rename_info);
    }

    let page_info = build_page_info(&page_index, &page_id, locked);

    let mut body = Map::new();
    body.insert("page_info".to_string(), page_info);
    body.insert("revision_info".to_string(), Value::Object(revision_info));

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(Value::Object(body).to_string()))
}

///
/// ページパス情報オブジェクトを構築する
///
/// # 引数
/// * `page_index` - ページインデックス
///
/// # 戻り値
/// パス情報のJSON値
///
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

///
/// ページメタ情報内のページ情報オブジェクトを構築する
///
/// # 引数
/// * `page_index` - ページインデックス
/// * `page_id` - ページID
/// * `locked` - ロック状態
///
/// # 戻り値
/// ページ情報のJSON値
///
fn build_page_info(
    page_index: &PageIndex,
    page_id: &PageId,
    locked: bool,
) -> Value {
    json!({
        "path": build_path_info(page_index),
        "short_path": build_short_path_info(page_index, page_id),
        "revision_scope": {
            "latest": page_index.latest(),
            "oldest": page_index.earliest(),
        },
        "rename_revisions": page_index.rename_revisions(),
        "deleted": page_index.deleted(),
        "locked": locked,
    })
}

///
/// ページ短縮パス情報オブジェクトを構築する
///
/// # 引数
/// * `page_index` - ページインデックス
/// * `page_id` - ページID
///
/// # 戻り値
/// 短縮パス情報のJSON値
///
fn build_short_path_info(page_index: &PageIndex, page_id: &PageId) -> Value {
    /*
     * 短縮URL対象可否の判定
     */
    if page_index.is_draft() || page_index.deleted() {
        return json!({
            "kind": "unavailable",
        });
    }

    json!({
        "kind": "available",
        "value": encode_page_short_id(page_id),
    })
}

///
/// リネーム情報オブジェクトを構築する
///
/// # 引数
/// * `rename` - リネーム情報
///
/// # 戻り値
/// リネーム情報がある場合はJSON値を返す。
///
fn build_rename_info(rename: &RenameInfo) -> Result<Option<Value>, serde_json::Error> {
    if rename.is_removed_by_migrate() {
        return Ok(Some(json!({
            "kind": "removed_by_migrate",
        })));
    }

    if !rename.is_active() {
        return Ok(None);
    }

    let Some(to) = rename.to() else {
        return Ok(None);
    };

    let link_refs = serde_json::to_value(rename.link_refs().unwrap_or_default())?;
    Ok(Some(json!({
        "kind": "active",
        "from": rename.from(),
        "to": to,
        "link_refs": link_refs,
    })))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::build_rename_info;
    use crate::database::types::RenameInfo;

    #[test]
    fn build_rename_info_returns_active_payload() {
        let rename = RenameInfo::new(
            Some("/from".to_string()),
            "/to".to_string(),
            BTreeMap::from([("/ref".to_string(), None)]),
        );

        let value = build_rename_info(&rename)
            .expect("build active rename info failed")
            .expect("active rename info missing");

        assert_eq!(
            value,
            json!({
                "kind": "active",
                "from": "/from",
                "to": "/to",
                "link_refs": {
                    "/ref": null,
                },
            })
        );
    }

    #[test]
    fn build_rename_info_returns_active_payload_without_from() {
        let rename =
            RenameInfo::new(None, "/to".to_string(), BTreeMap::new());

        let value = build_rename_info(&rename)
            .expect("build active rename info failed")
            .expect("active rename info missing");

        assert_eq!(
            value,
            json!({
                "kind": "active",
                "from": null,
                "to": "/to",
                "link_refs": {},
            })
        );
    }

    #[test]
    fn build_rename_info_returns_removed_by_migrate_payload() {
        let value = build_rename_info(&RenameInfo::removed_by_migrate())
            .expect("build removed rename info failed")
            .expect("removed rename info missing");

        assert_eq!(
            value,
            json!({
                "kind": "removed_by_migrate",
            })
        );
    }
}
