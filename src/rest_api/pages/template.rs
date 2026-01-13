/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! テンプレート一覧取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpResponse, web};
use serde::Serialize;

use crate::http_server::app_state::AppState;
use super::super::resp_error_json;

///
/// テンプレート一覧のレスポンスエントリ
///
#[derive(Serialize)]
struct TemplateEntry {
    page_id: String,
    name: String,
}

///
/// テンプレートの候補判定
///
/// # 引数
/// * `root` - テンプレートルート
/// * `path` - ページパス
///
/// # 戻り値
/// テンプレート候補の場合は`true`を返す。
///
fn is_direct_child(root: &str, path: &str) -> bool {
    let normalized_root = if root.len() > 1 {
        root.trim_end_matches('/')
    } else {
        root
    };

    /*
     * ルート階層の判定
     */
    if normalized_root == "/" {
        if !path.starts_with('/') || path == "/" {
            return false;
        }
        let rest = &path[1..];
        return !rest.is_empty() && !rest.contains('/');
    }

    /*
     * 子ページ候補の判定
     */
    if !path.starts_with(normalized_root) {
        return false;
    }

    let rest = &path[normalized_root.len()..];
    if !rest.starts_with('/') {
        return false;
    }

    let child = &rest[1..];
    !child.is_empty() && !child.contains('/')
}

///
/// テンプレート名の抽出
///
/// # 引数
/// * `path` - ページパス
///
/// # 戻り値
/// パス末尾の要素を返す。
///
fn extract_template_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

///
/// GET /api/pages/template の実体
///
/// # 概要
/// テンプレートページの一覧を取得する
///
/// # 引数
/// * `state` - 共有状態
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
/// # 注記
/// エラー時はJSON形式で返却する。
/// 処理の流れは状態取得、テンプレート判定、
/// ページ収集、レスポンス生成の順。
///
pub async fn get(
    state: web::Data<Arc<RwLock<AppState>>>,
)
    -> actix_web::Result<HttpResponse>
{
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
     * テンプレート機能の有効判定
     */
    let template_root = match state.template_root() {
        Some(path) => path.to_string(),
        None => {
            return Ok(resp_error_json(
                StatusCode::NOT_FOUND,
                "template feature disabled",
            ));
        }
    };

    /*
     * ページ一覧の収集
     */
    let mut entries: Vec<TemplateEntry> = Vec::new();
    let pages = match state.db().list_pages() {
        Ok(pages) => pages,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "template list failed",
            ));
        }
    };

    for page in pages {
        if page.deleted() || page.is_draft() {
            continue;
        }
        let path = page.path();
        if !is_direct_child(&template_root, &path) {
            continue;
        }
        entries.push(TemplateEntry {
            page_id: page.id().to_string(),
            name: extract_template_name(&path),
        });
    }

    /*
     * レスポンス生成
     */
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .body(
            serde_json::to_string(&entries)
                .unwrap_or_else(|_| "[]".to_string()),
        ))
}
