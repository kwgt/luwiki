/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ検索APIの実装をまとめたモジュール
//!

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::types::{BearerScope, PageId};
use crate::fts::{self, FtsSearchTarget};
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

#[derive(Deserialize)]
struct SearchQuery {
    expr: String,
    target: Option<String>,
    with_deleted: Option<String>,
    all_revision: Option<String>,
}

///
/// GET /api/pages/search?expr={expression} の実体
///
/// # 概要
/// ページの全文検索を実行する
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
) -> actix_web::Result<HttpResponse> {
    let auth = match require_request_scope(&req, BearerScope::Read) {
        Ok(auth) => auth,
        Err(resp) => return Ok(resp),
    };
    let policy = crate::auth::PageAccessPolicy::new(auth.path_prefixes());

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<SearchQuery>::from_query(
        req.query_string(),
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: expr",
            ));
        }
    };

    let expression = query.expr.trim();
    if expression.is_empty() {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "invalid query parameter: expr",
        ));
    }

    let targets = match parse_target_param(query.target.as_deref()) {
        Ok(targets) => targets,
        Err(resp) => return Ok(resp),
    };

    let with_deleted = match parse_bool_param(
        "with_deleted",
        query.with_deleted.as_deref(),
    ) {
        Ok(value) => value,
        Err(resp) => return Ok(resp),
    };
    let all_revision = match parse_bool_param(
        "all_revision",
        query.all_revision.as_deref(),
    ) {
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
     * 返却状態の解決まで同じDB snapshotを保持する
     */
    let snapshot = match state.db().authorized_search_snapshot(
        with_deleted, &policy,
    ) {
        Ok(snapshot) => snapshot,
        Err(_) => return Ok(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR, "search snapshot failed")),
    };
    let page_ids = snapshot.page_ids();

    /*
     * 検索の実行と結果の集約
     */
    let mut merged: HashMap<(PageId, u64), fts::FtsSearchResult> =
        HashMap::new();
    for target in targets {
        let results = match fts::search_index_for_page_ids(
            state.fts_config(),
            target,
            expression,
            with_deleted,
            all_revision,
            &page_ids,
        ) {
            Ok(results) => results,
            Err(err) => {
                if err
                    .downcast_ref::<tantivy::query::QueryParserError>()
                    .is_some()
                {
                    return Ok(resp_error_json(
                        StatusCode::BAD_REQUEST,
                        "invalid query parameter: expr",
                    ));
                }
                return Ok(resp_error_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "search failed",
                ));
            }
        };

        merge_results(&mut merged, results);
    }

    /*
     * パス情報の取得とレスポンス生成
     */
    let mut results = merged.into_values().collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right.score().total_cmp(&left.score())
            .then_with(|| left.page_id().cmp(&right.page_id()))
            .then_with(|| left.revision().cmp(&right.revision()))
    });
    results.truncate(100);
    let mut items = Vec::with_capacity(results.len());
    for result in results {
        let page_id = result.page_id();
        let (path, deleted) = match snapshot.resolve_result(
            &page_id, result.revision(), all_revision,
        ) {
            Ok(info) => info,
            Err(_) => return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR, "page lookup failed")),
        };
        items.push(json!({
            "page_id": page_id.to_string(),
            "revision": result.revision(),
            "score": result.score(),
            "path": path,
            "deleted": deleted,
            "text": result.snippet(),
        }));
    }
    crate::rest_api::audit::update_metadata(&req, |metadata| {
        metadata.count = Some(items.len());
    });
    let body = json!(items);

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .body(body.to_string()))
}

///
/// 検索対象のパラメータを解析する
///
/// # 引数
/// * `raw` - クエリーパラメータ値
///
/// # 戻り値
/// 検索対象一覧
///
fn parse_target_param(
    raw: Option<&str>,
) -> Result<Vec<FtsSearchTarget>, HttpResponse> {
    if raw.is_none() {
        return Ok(vec![FtsSearchTarget::Body]);
    }

    let raw = raw.unwrap();
    let mut headings = false;
    let mut body = false;
    let mut code = false;
    let mut front_matter = false;
    for item in raw.split(',') {
        let item = item.trim();
        if item.is_empty() {
            return Err(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: target",
            ));
        }

        match item {
            "headings" => headings = true,
            "body" => body = true,
            "code" => code = true,
            "front_matter" => front_matter = true,
            _ => {
                return Err(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: target",
                ));
            }
        }
    }

    if !headings && !body && !code && !front_matter {
        return Err(resp_error_json(
            StatusCode::BAD_REQUEST,
            "invalid query parameter: target",
        ));
    }

    let mut targets = Vec::new();
    if headings {
        targets.push(FtsSearchTarget::Headings);
    }
    if body {
        targets.push(FtsSearchTarget::Body);
    }
    if code {
        targets.push(FtsSearchTarget::Code);
    }
    if front_matter {
        targets.push(FtsSearchTarget::FrontMatter);
    }

    Ok(targets)
}

///
/// 真偽値パラメータを解析する
///
/// # 引数
/// * `name` - クエリーパラメータ名
/// * `raw` - クエリーパラメータ値
///
/// # 戻り値
/// 解析結果
///
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

///
/// 検索結果のマージ処理
///
/// # 引数
/// * `merged` - マージ先
/// * `results` - 追加対象
///
/// # 戻り値
/// なし
///
fn merge_results(
    merged: &mut HashMap<(PageId, u64), fts::FtsSearchResult>,
    results: Vec<fts::FtsSearchResult>,
) {
    for result in results {
        let key = (result.page_id(), result.revision());
        let replace = match merged.get(&key) {
            Some(existing) => result.score() > existing.score(),
            None => true,
        };
        if replace {
            merged.insert(key, result);
        }
    }
}
