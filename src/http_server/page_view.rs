/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! Wiki表示用のエンドポイントを提供するモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::{http::header, HttpResponse, web};

use crate::http_server::app_state::AppState;
use crate::http_server::static_files;
use crate::rest_api;

///
/// ルートページ表示
///
pub(crate) async fn get_root(
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_by_path(String::new(), data).await
}

///
/// ルートパスのリダイレクト
///
pub(crate) async fn get_root_redirect() -> HttpResponse {
    HttpResponse::Found()
        .insert_header((header::LOCATION, "/wiki/"))
        .finish()
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

///
/// 検索画面
///
pub(crate) async fn get_search(
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    render_page_html(&state, "", "")
}

///
/// ページ一覧画面(ルート)
///
pub(crate) async fn get_pages_root(
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_pages_by_path(String::new(), data).await
}

///
/// ページ一覧画面
///
pub(crate) async fn get_pages(
    path: web::Path<String>,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_pages_by_path(path.into_inner(), data).await
}

///
/// リビジョン管理画面(ルート)
///
pub(crate) async fn get_rev_root(
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_rev_by_path(String::new(), data).await
}

///
/// リビジョン管理画面
///
pub(crate) async fn get_rev(
    path: web::Path<String>,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    get_rev_by_path(path.into_inner(), data).await
}

///
/// パス指定でWikiページを表示する
///
/// # 引数
/// * `raw_path` - リクエストされたページパス
/// * `data` - アプリケーション状態
///
/// # 戻り値
/// 表示結果のHTTPレスポンス
///
async fn get_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = normalize_path(&raw_path);
    if let Err(reason) = rest_api::validate_page_path(&page_path) {
        return HttpResponse::BadRequest().body(reason);
    }

    /*
     * ページ情報の取得
     */
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

    /*
     * 最新リビジョンの画面を返す
     */
    let revision = page_index.latest();
    render_page_html(&state, &page_id.to_string(), &revision.to_string())
}

///
/// パス指定でリビジョン画面を表示する
///
/// # 引数
/// * `raw_path` - リクエストされたページパス
/// * `data` - アプリケーション状態
///
/// # 戻り値
/// 表示結果のHTTPレスポンス
///
async fn get_rev_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = normalize_path(&raw_path);
    if let Err(reason) = rest_api::validate_page_path(&page_path) {
        return HttpResponse::BadRequest().body(reason);
    }

    /*
     * ページ情報の取得
     */
    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let page_id = match state.db().get_page_id_by_path(&page_path) {
        Ok(Some(page_id)) => page_id,
        Ok(None) => return HttpResponse::NotFound().finish(),
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => return HttpResponse::NotFound().finish(),
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    /*
     * 最新リビジョンの画面を返す
     */
    let revision = page_index.latest();
    render_page_html(&state, &page_id.to_string(), &revision.to_string())
}

///
/// パス指定で編集画面を表示する
///
/// # 引数
/// * `raw_path` - リクエストされたページパス
/// * `data` - アプリケーション状態
///
/// # 戻り値
/// 表示結果のHTTPレスポンス
///
async fn get_edit_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = normalize_path(&raw_path);
    if let Err(reason) = rest_api::validate_page_path(&page_path) {
        return HttpResponse::BadRequest().body(reason);
    }

    /*
     * ページ情報の取得
     */
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

    /*
     * 編集画面を返す
     */
    render_page_html(&state, &page_id, &revision)
}

///
/// パス指定でページ一覧画面を表示する
///
/// # 引数
/// * `raw_path` - リクエストされたページパス
/// * `data` - アプリケーション状態
///
/// # 戻り値
/// 表示結果のHTTPレスポンス
///
async fn get_pages_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = normalize_path(&raw_path);
    if let Err(reason) = rest_api::validate_page_path(&page_path) {
        return HttpResponse::BadRequest().body(reason);
    }

    /*
     * アプリケーション状態の取得
     */
    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    /*
     * 一覧画面を返す
     */
    render_page_html(&state, "", "")
}

///
/// リクエストパスを正規化する
///
/// # 引数
/// * `raw_path` - 正規化前のパス
///
/// # 戻り値
/// 正規化済みのページパス
///
fn normalize_path(raw_path: &str) -> String {
    let trimmed = raw_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }

    format!("/{}", trimmed)
}

///
/// 未存在ページ用の編集画面リダイレクト先を構築する
///
/// # 引数
/// * `page_path` - 対象ページパス
///
/// # 戻り値
/// 編集画面へのパス
///
fn build_edit_redirect_path(page_path: &str) -> String {
    if page_path == "/" {
        return "/edit/".to_string();
    }

    format!("/edit{}", page_path)
}

///
/// HTML属性値として安全な文字列に変換する
///
/// # 引数
/// * `value` - 変換対象文字列
///
/// # 戻り値
/// エスケープ済み文字列
///
fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

///
/// フロントエンドHTMLを組み立てて返す
///
/// # 引数
/// * `state` - アプリケーション状態
/// * `page_id` - 埋め込むページID
/// * `revision` - 埋め込むリビジョン番号
///
/// # 戻り値
/// 画面表示用のHTTPレスポンス
///
fn render_page_html(
    state: &AppState,
    page_id: &str,
    revision: &str,
) -> HttpResponse {
    /*
     * HTMLテンプレートの読み込み
     */
    let template = match static_files::get_embedded_file("index.html") {
        Some(data) => data,
        None => return HttpResponse::InternalServerError().finish(),
    };

    let mut html = match String::from_utf8(template.into_owned()) {
        Ok(html) => html,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    /*
     * プレースホルダの置換
     */
    html = html.replace("{{PAGE_ID}}", page_id);
    html = html.replace("{{REVISION}}", revision);
    html = html.replace(
        "{{WIKI_TITLE}}",
        &escape_html_attribute(state.wiki_title()),
    );
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
        &state.asset_limit_size().to_string(),
    );

    /*
     * HTMLレスポンスの返却
     */
    HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, "no-store, no-cache"))
        .content_type("text/html; charset=utf-8")
        .body(html)
}
