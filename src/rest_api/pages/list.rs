/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ一覧取得APIの実装をまとめたモジュール
//!

use std::cmp::Ordering;
use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use crate::database::PageListEntry;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

#[derive(Deserialize)]
struct ListQuery {
    prefix: String,
    forward: Option<String>,
    rewind: Option<String>,
    limit: Option<String>,
    with_deleted: Option<String>,
}

///
/// GET /api/pages?prefix={page_path}[&forward={page_path}][&rewind={page_path}]
/// [&limit={number}][&with_deleted={boolean}] の実体
///
/// # 概要
/// ページ一覧の取得
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<ListQuery>::from_query(
        req.query_string()
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: prefix",
            ));
        }
    };

    if let Err(message) = super::validate_page_path(&query.prefix) {
        return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
    }

    if let Some(forward) = query.forward.as_deref() {
        if let Err(message) = super::validate_page_path(forward) {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
        }
    }

    if let Some(rewind) = query.rewind.as_deref() {
        if let Err(message) = super::validate_page_path(rewind) {
            return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
        }
    }

    if query.forward.is_some() && query.rewind.is_some() {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "invalid query parameter: forward/rewind",
        ));
    }

    let with_deleted = match parse_bool_param(
        "with_deleted",
        query.with_deleted.as_deref(),
    ) {
        Ok(value) => value,
        Err(resp) => return Ok(resp),
    };

    let limit = match parse_limit_param(query.limit.as_deref()) {
        Ok(value) => value,
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
     * ページ一覧の取得
     */
    let entries = match state
        .db()
        .list_page_entries_by_prefix(&query.prefix, with_deleted)
    {
        Ok(entries) => entries,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page list failed",
            ));
        }
    };

    let (forward, cursor) = resolve_cursor(&query);
    let mut filtered = apply_cursor(entries, &query.prefix, &cursor, forward);

    sort_entries(&mut filtered, forward);

    let has_more = filtered.len() > limit;
    if has_more {
        filtered.truncate(limit);
    }

    let anchor = if has_more {
        filtered.last().map(|entry| entry.path())
    } else {
        None
    };

    let items = filtered
        .into_iter()
        .map(|entry| {
            let timestamp = entry.timestamp()
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string();
            json!({
                "page_id": entry.id().to_string(),
                "path": entry.path(),
                "deleted": entry.deleted(),
                "last_update": {
                    "revision": entry.latest_revision(),
                    "timestamp": timestamp,
                    "username": entry.user_name(),
                },
            })
        })
        .collect::<Vec<_>>();

    let body = if let Some(anchor) = anchor {
        json!({
            "items": items,
            "has_more": true,
            "anchor": anchor,
        })
    } else {
        json!({
            "items": items,
            "has_more": false,
        })
    };

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(body.to_string()))
}

fn resolve_cursor(query: &ListQuery) -> (bool, String) {
    if let Some(rewind) = query.rewind.as_deref() {
        return (false, rewind.to_string());
    }

    if let Some(forward) = query.forward.as_deref() {
        return (true, forward.to_string());
    }

    (true, query.prefix.clone())
}

fn apply_cursor(
    entries: Vec<PageListEntry>,
    prefix: &str,
    cursor: &str,
    forward: bool,
) -> Vec<PageListEntry> {
    entries
        .into_iter()
        .filter(|entry| entry.path() != prefix)
        .filter(|entry| {
            let path = entry.path();
            if forward {
                path.as_str() > cursor
            } else {
                path.as_str() < cursor
            }
        })
        .collect()
}

fn sort_entries(entries: &mut [PageListEntry], forward: bool) {
    entries.sort_by(|left, right| {
        let ord = match left.path().cmp(&right.path()) {
            Ordering::Equal => left.id().cmp(&right.id()),
            other => other,
        };
        if forward {
            ord
        } else {
            ord.reverse()
        }
    });
}

fn parse_bool_param(
    name: &str,
    raw: Option<&str>,
) -> Result<bool, HttpResponse> {
    match raw {
        None => Ok(false),
        Some(value) => match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(resp_error_json(
                StatusCode::BAD_REQUEST,
                format!("invalid query parameter: {}", name),
            )),
        },
    }
}

fn parse_limit_param(raw: Option<&str>) -> Result<usize, HttpResponse> {
    match raw {
        None => Ok(50),
        Some(value) => match value.parse::<usize>() {
            Ok(limit) if limit > 0 => Ok(limit),
            _ => Err(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: limit",
            )),
        },
    }
}
