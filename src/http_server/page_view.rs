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

use crate::database::short_id::decode_page_short_id;
use crate::http_server::app_state::AppState;
use crate::http_server::static_files;
use crate::wiki_path::NormalizedWikiPath;

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
/// 短縮URLによるページ表示
///
/// # 引数
/// * `path` - 短縮ID
/// * `data` - アプリケーション状態
///
/// # 戻り値
/// 短縮URL解決結果のHTTPレスポンス
///
pub(crate) async fn get_short(
    path: web::Path<String>,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * 短縮IDの復元
     */
    let short_id = path.into_inner();
    let page_id = match decode_page_short_id(&short_id) {
        // 短縮URLは既存ページ専用導線であり、未存在時に編集画面へは遷移しない
        Ok(page_id) => page_id,
        Err(_) => return short_url_not_found(),
    };

    /*
     * ページ状態の取得
     */
    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => return short_url_not_found(),
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    /*
     * 削除済みページの判定
     */
    if page_index.deleted() {
        return short_url_gone();
    }

    let current_path = match state.db().get_current_page_path_by_id(&page_id) {
        Ok(Some(info)) => info,
        Ok(None) => return short_url_not_found(),
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    if !current_path.short_url_available() {
        return short_url_not_found();
    }

    /*
     * current path へのリダイレクト
     */
    let location = build_wiki_location(current_path.current_path());

    HttpResponse::Found()
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .insert_header((header::LOCATION, location))
        .finish()
}

///
/// 短縮URL解決失敗時の 404 応答を返す
///
/// # 戻り値
/// 不正 `short_id` および未存在 `page_id` を共通で扱う 404 応答を返す。
///
fn short_url_not_found() -> HttpResponse {
    HttpResponse::NotFound().finish()
}

///
/// 短縮URL解決時の 410 応答を返す
///
/// # 戻り値
/// 削除済みページを共通で扱う 410 応答を返す。
///
fn short_url_gone() -> HttpResponse {
    HttpResponse::Gone().finish()
}

///
/// current path から閲覧URLを構築する
///
/// # 引数
/// * `current_path` - current path
///
/// # 戻り値
/// `/wiki/{current_path}` 形式の閲覧URLを返す。
///
fn build_wiki_location(current_path: &str) -> String {
    /*
     * ルートページの特別扱い
     */
    if current_path == "/" {
        return "/wiki/".to_string();
    }

    /*
     * セグメントごとのURLエンコード
     */
    let mut encoded_segments = Vec::new();
    for segment in current_path.trim_start_matches('/').split('/') {
        let mut encoded = String::new();
        for byte in segment.bytes() {
            if byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~')
            {
                encoded.push(byte as char);
            } else {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
        encoded_segments.push(encoded);
    }

    format!("/wiki/{}", encoded_segments.join("/"))
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
/// # 概要
/// 解釈済みURLのtailを共通解決してから既存の表示状態を取得する。
///
async fn get_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = match normalize_path(&raw_path) {
        Ok(path) => path,
        Err(error) => return HttpResponse::BadRequest().body(error.to_string()),
    };

    /*
     * ページ情報の取得
     */
    let state = match data.read() {
        Ok(state) => state,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let page_id = match state.db().get_page_id_by_path(&page_path) {
        Ok(Some(page_id)) => page_id,
        Ok(None) => return redirect_missing_view_page_to_edit(&page_path),
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
/// # 概要
/// 共通入力規則を確認してから既存のrevision画面処理を行う。
///
async fn get_rev_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = match normalize_path(&raw_path) {
        Ok(path) => path,
        Err(error) => return HttpResponse::BadRequest().body(error.to_string()),
    };

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
/// # 概要
/// 入力表記を共通解決し、不正表記を編集画面へ引き継がない。
///
async fn get_edit_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    let page_path = match normalize_path(&raw_path) {
        Ok(path) => path,
        Err(error) => return HttpResponse::BadRequest().body(error.to_string()),
    };

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
/// # 概要
/// 一覧URLの入力表記を検証し、一覧データの認可接続はMS3で行う。
///
async fn get_pages_by_path(
    raw_path: String,
    data: web::Data<Arc<RwLock<AppState>>>,
) -> HttpResponse {
    /*
     * パスの検証
     */
    if let Err(error) = normalize_path(&raw_path) {
        return HttpResponse::BadRequest().body(error.to_string());
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
/// 正規化済みのページパス、または入力不正を返す。
/// Actix解釈済みのtailへ一度だけrootを結合し、再decodeしない。
///
fn normalize_path(raw_path: &str) -> anyhow::Result<String> {
    let absolute = format!("/{}", raw_path);
    NormalizedWikiPath::parse(&absolute)
        .map(NormalizedWikiPath::into_string)
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
/// 通常閲覧URLで未存在ページへアクセスした場合の編集画面遷移を返す
///
/// # 引数
/// * `page_path` - 未存在だったページパス
///
/// # 戻り値
/// 通常閲覧URL専用の編集画面リダイレクト応答を返す。
///
fn redirect_missing_view_page_to_edit(page_path: &str) -> HttpResponse {
    let edit_path = build_edit_redirect_path(page_path);
    HttpResponse::Found()
        .insert_header((header::LOCATION, edit_path))
        .finish()
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
        "{{WIKI_ICON_URL}}",
        if state.wiki_icon().is_some() {
            "/wiki-icon"
        } else {
            ""
        },
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


#[cfg(test)]
mod path_input_tests {
    use super::normalize_path;

    ///
    /// 直接URLのtail解決と禁止表記、二重decodeなしを検証する
    ///
    /// # 注記
    /// root、dot、leading slash、末尾/、%表記を入力して比較する。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn browser_tail_keeps_raw_validation_and_single_root_join() {
        assert_eq!(normalize_path("").unwrap(), "/");
        assert_eq!(normalize_path("docs/../a").unwrap(), "/a");
        assert_eq!(normalize_path("docs/%2E%2E/a").unwrap(),
            "/docs/%2E%2E/a");
        assert!(normalize_path("/docs").is_err());
        assert!(normalize_path("docs/").is_err());
        assert!(normalize_path("../docs").is_err());
    }
    ///
    /// 実際のActix routeとPath extractorで直接URL解釈を確認する
    ///
    /// # 概要
    /// productionの閲覧handlerを隔離stateで使い、解決先IDを確認する。
    ///
    /// # 注記
    /// 一回decodeとraw禁止をin-process HTTP requestで検証する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[actix_web::test]
    async fn browser_http_path_decodes_once_before_common_resolution() {
        use std::sync::{Arc, RwLock};

        use actix_web::{App, test, web};
        use tempfile::tempdir;

        use crate::database::DatabaseManager;
        use crate::fts::FtsIndexConfig;
        use crate::http_server::app_state::AppState;
        use crate::http_server::FrontendConfig;

        /*
         * 閲覧先のroot、dot解決先、percent文字列の保存ページを用意する
         */
        let dir = tempdir().unwrap();
        let manager = DatabaseManager::open(
            dir.path().join("direct-url.redb"), dir.path().join("assets"),
        ).unwrap();
        manager.add_user("url_test", "pass", None).unwrap();
        let mut ids = Vec::new();
        for path in ["/", "/a", "/docs/%2E%2E/a"] {
            let id = manager.create_page(path, "url_test", "body".to_string())
                .unwrap();
            ids.push(id.to_string());
        }
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            manager, FrontendConfig::default(),
            FtsIndexConfig::new(dir.path().join("fts")),
            None, "URL test".to_string(), None, 1024, None,
        ))));
        let app = test::init_service(App::new().app_data(state)
            .route("/wiki/{page_path:.*}", web::get().to(super::get)))
            .await;

        /*
         * HTTP解釈後のdot解決とpercent保持が正しい保存IDを選ぶ
         */
        for (uri, id) in [
            ("/wiki/", &ids[0]),
            ("/wiki/docs/%2E%2E/a", &ids[1]),
            ("/wiki/docs/%252E%252E/a", &ids[2]),
        ] {
            let response = test::call_service(&app,
                test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(response.status(), 200);
            let body = test::read_body(response).await;
            assert!(std::str::from_utf8(&body).unwrap().contains(id));
        }

        /*
         * leading区切りを消さず禁止文字とroot越えも拒否する
         */
        for uri in [
            "/wiki/%2Fdocs", "/wiki/docs/", "/wiki/docs/%20a",
            "/wiki/docs/%00a", "/wiki/%2E%2E/a",
        ] {
            let response = test::call_service(&app,
                test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(response.status(), 400, "{uri}");
        }
    }

}
