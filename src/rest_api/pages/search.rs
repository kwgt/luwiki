/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ検索APIの実装をまとめたモジュール
//!

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use crate::cmd_args::FtsSearchTarget;
use crate::database::types::PageId;
use crate::fts;
use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

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
)
    -> actix_web::Result<HttpResponse>
{
    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<SearchQuery>::from_query(
        req.query_string()
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
     * 検索の実行と結果の集約
     */
    let mut merged: HashMap<(PageId, u64), fts::FtsSearchResult> =
        HashMap::new();
    for target in targets {
        let results = match fts::search_index(
            state.fts_config(),
            target,
            expression,
            with_deleted,
            all_revision,
        ) {
            Ok(results) => results,
            Err(err) => {
                if err.downcast_ref::<tantivy::query::QueryParserError>()
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
    let mut cache: HashMap<PageId, (String, bool)> = HashMap::new();
    let mut items = Vec::with_capacity(merged.len());
    for result in merged.into_values() {
        let page_id = result.page_id();
        let (path, deleted) = match resolve_page_info(
            state.db(),
            &mut cache,
            &page_id,
        ) {
            Ok(info) => info,
            Err(resp) => return Ok(resp),
        };

        items.push((
            result.score(),
            json!({
                "page_id": page_id.to_string(),
                "revision": result.revision(),
                "score": result.score(),
                "path": path,
                "deleted": deleted,
                "text": result.snippet(),
            }),
        ));
    }

    items.sort_by(|lhs, rhs| {
        rhs.0
            .partial_cmp(&lhs.0)
            .unwrap_or(Ordering::Equal)
    });

    let body = json!(items
        .into_iter()
        .map(|(_, item)| item)
        .collect::<Vec<_>>());

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
            _ => {
                return Err(resp_error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid query parameter: target",
                ));
            }
        }
    }

    if !headings && !body && !code {
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

///
/// ページパスと削除済みフラグの取得
///
/// # 引数
/// * `db` - データベース
/// * `cache` - 取得結果のキャッシュ
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// (ページパス, 削除済みフラグ)
///
fn resolve_page_info(
    db: &crate::database::DatabaseManager,
    cache: &mut HashMap<PageId, (String, bool)>,
    page_id: &PageId,
) -> Result<(String, bool), HttpResponse> {
    if let Some(info) = cache.get(page_id) {
        return Ok(info.clone());
    }

    let index = match db.get_page_index_by_id(page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Err(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
        Err(_) => {
            return Err(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    if index.is_draft() {
        return Err(resp_error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "page lookup failed",
        ));
    }

    let deleted = index.deleted();
    let path = if deleted {
        match index.last_deleted_path() {
            Some(path) => path.to_string(),
            None => index.path(),
        }
    } else {
        match index.current_path() {
            Some(path) => path.to_string(),
            None => index.path(),
        }
    };

    let info = (path, deleted);
    cache.insert(page_id.clone(), info.clone());
    Ok(info)
}
