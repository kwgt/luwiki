/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! Wiki表示用のエンドポイントを提供するモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::{HttpResponse, web, http::header};

use crate::http_server::app_state::AppState;
use crate::http_server::static_files;
use crate::rest_api;

const ASSET_MAX_BYTES: u64 = 10 * 1024 * 1024;

///
/// ルートページ表示
///
pub(crate) async fn get_root(
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_by_path(String::new(), data).await
}

///
/// 任意ページ表示
///
pub(crate) async fn get(
    path: web::Path<String>,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_by_path(path.into_inner(), data).await
}

///
/// 編集画面(ルート)
///
pub(crate) async fn get_edit_root(
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_edit_by_path(String::new(), data).await
}

///
/// 編集画面
///
pub(crate) async fn get_edit(
    path: web::Path<String>,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_edit_by_path(path.into_inner(), data).await
}

async fn get_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    let page_path = normalize_path(&raw_path);
    if let Err(reason) = rest_api::validate_page_path(&page_path) {
        return HttpResponse::BadRequest().body(reason);
    }

    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let page_id = match state.db().get_page_id_by_path(&page_path) {
        Ok(Some(page_id)) => page_id,
        Ok(None) => {
            let edit_path = build_edit_redirect_path(&page_path);
            return HttpResponse::Found()
                .insert_header((header::LOCATION, edit_path))
                .finish();
        }
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => return HttpResponse::NotFound().finish(),
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let revision = page_index.latest();
    render_page_html(&state, &page_id.to_string(), &revision.to_string())
}

async fn get_edit_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    let page_path = normalize_path(&raw_path);
    if let Err(reason) = rest_api::validate_page_path(&page_path) {
        return HttpResponse::BadRequest().body(reason);
    }

    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let mut page_id = String::new();
    let mut revision = String::new();
    match state.db().get_page_id_by_path(&page_path) {
        Ok(Some(id)) => {
            let page_index = match state.db().get_page_index_by_id(&id) {
                Ok(Some(index)) => index,
                Ok(None) => return HttpResponse::NotFound().finish(),
                Err(_) => return HttpResponse::InternalServerError().finish(),
            };
            page_id = id.to_string();
            revision = page_index.latest().to_string();
        }
        Ok(None) => {}
        Err(_) => return HttpResponse::InternalServerError().finish(),
    }

    render_page_html(&state, &page_id, &revision)
}

fn normalize_path(raw_path: &str) -> String {
    let trimmed = raw_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }

    format!("/{}", trimmed)
}

fn build_edit_redirect_path(page_path: &str) -> String {
    if page_path == "/" {
        return "/edit/".to_string();
    }

    format!("/edit{}", page_path)
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_page_html(
    state: &AppState,
    page_id: &str,
    revision: &str,
) -> HttpResponse {
    let template = match static_files::get_embedded_file("index.html") {
        Some(data) => data,
        None => return HttpResponse::InternalServerError().finish(),
    };

    let mut html = match String::from_utf8(template.into_owned()) {
        Ok(html) => html,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    html = html.replace("{{PAGE_ID}}", page_id);
    html = html.replace("{{REVISION}}", revision);
    html = html.replace(
        "{{FRONTEND_UI_FONT}}",
        &escape_html_attribute(state.frontend_config().ui_font()),
    );
    html = html.replace(
        "{{FRONTEND_MD_FONT_SANS}}",
        &escape_html_attribute(state.frontend_config().md_font_sans()),
    );
    html = html.replace(
        "{{FRONTEND_MD_FONT_SERIF}}",
        &escape_html_attribute(state.frontend_config().md_font_serif()),
    );
    html = html.replace(
        "{{FRONTEND_MD_FONT_MONO}}",
        &escape_html_attribute(state.frontend_config().md_font_mono()),
    );
    html = html.replace(
        "{{FRONTEND_MD_CODE_FONT}}",
        &escape_html_attribute(state.frontend_config().md_code_font()),
    );
    html = html.replace(
        "{{ASSET_MAX_BYTES}}",
        &ASSET_MAX_BYTES.to_string(),
    );

    HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, "no-store, no-cache"))
        .content_type("text/html; charset=utf-8")
        .body(html)
}
