/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ短縮パス取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::short_id::encode_page_short_id;
use crate::database::PagePathResolveState;
use crate::database::types::{BearerScope, PageId};
use crate::http_server::app_state::AppState;
use crate::rest_api::{CACHE_CONTROL_NO_STORE, require_request_scope};

#[derive(Deserialize)]
struct ShortPathQuery {
    path: String,
}

///
/// GET /api/pages/short?path={page_path} の実体
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn get_by_path(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> actix_web::Result<HttpResponse> {
    if let Err(resp) = require_request_scope(&req, BearerScope::Read) {
        return Ok(resp);
    }

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<ShortPathQuery>::from_query(
        req.query_string(),
    ) {
        Ok(query) => query,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::BAD_REQUEST,
                "invalid query parameter: path",
            ));
        }
    };

    if let Err(message) = super::validate_page_path(&query.path) {
        return Ok(resp_error_json(StatusCode::BAD_REQUEST, message));
    }

    /*
     * path 形式の追加制約確認
     */
    if query.path.starts_with("/wiki/") {
        return Ok(resp_error_json(
            StatusCode::BAD_REQUEST,
            "path must not be wiki view path",
        ));
    }

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
     * page path から短縮URL対象を解決
     */
    let resolved = match state.db().resolve_page_state_by_path(&query.path) {
        Ok(resolved) => resolved,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    let page_id = match resolved {
        PagePathResolveState::Current { page_id, draft: false } => page_id,
        PagePathResolveState::Current { .. }
        | PagePathResolveState::NotFound => {
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
        }
        PagePathResolveState::Deleted => {
            return Ok(resp_error_json(StatusCode::GONE, "page deleted"));
        }
    };

    /*
     * レスポンス生成
     */
    Ok(build_short_path_response(&page_id))
}

///
/// GET /api/pages/{page_id}/short の実体
///
/// # 概要
/// ページIDに対応する短縮用パス断片を取得する。
///
/// # 引数
/// * `req` - HTTPリクエスト
/// * `state` - 共有状態
/// * `path` - ページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn get(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<String>,
) -> actix_web::Result<HttpResponse> {
    if let Err(resp) = require_request_scope(&req, BearerScope::Read) {
        return Ok(resp);
    }

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
     * ページ状態取得
     */
    let page_index = match state.db().get_page_index_by_id(&page_id) {
        Ok(Some(index)) => index,
        Ok(None) => {
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    if page_index.deleted() {
        return Ok(resp_error_json(StatusCode::GONE, "page deleted"));
    }

    /*
     * レスポンス生成
     */
    Ok(build_short_path_response(&page_id))
}

///
/// 短縮用パス断片取得APIの正常レスポンスを生成
///
/// # 引数
/// * `page_id` - レスポンスへ反映するページID
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
fn build_short_path_response(page_id: &PageId) -> HttpResponse {
    let body = json!({
        "short_path": encode_page_short_id(page_id),
    });

    HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(body.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, RwLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use actix_web::body::to_bytes;
    use actix_web::http::{StatusCode, header};
    use actix_web::http::header::TryIntoHeaderValue;
    use actix_web::test;
    use actix_web_httpauth::headers::authorization::Basic;
    use serde_json::Value;

    use crate::cmd_args::FrontendConfig;
    use crate::database::DatabaseManager;
    use crate::database::short_id::encode_page_short_id;
    use crate::database::types::PageId;
    use crate::fts::FtsIndexConfig;
    use crate::http_server::app_state::AppState;
    use crate::rest_api::create_api_scope;

    ///
    /// `GET /api/pages/{page_id}/short` と
    /// `GET /api/pages/short?path={page_path}` の
    /// 正常系・例外系を確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test rest_api::pages::short::tests::page_short_endpoints_cover_success_and_error_cases -- --test-threads=1`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn page_short_endpoints_cover_success_and_error_cases() {
        /*
         * テスト用アプリの準備
         */
        let app_state = build_test_app_state().expect("build app state failed");
        let normal_page_id = create_page(
            app_state.db(),
            "/short/normal",
        )
        .expect("create normal page failed");
        let deleted_page_id = create_page(
            app_state.db(),
            "/short/deleted",
        )
        .expect("create deleted page failed");
        create_draft(app_state.db(), "/short/draft")
            .expect("create draft page failed");
        delete_page(app_state.db(), &deleted_page_id)
            .expect("delete page failed");

        let app = test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                    app_state,
                ))))
                .service(create_api_scope(1024 * 1024)),
        )
        .await;

        let expected_short_id = encode_page_short_id(&normal_page_id);

        /*
         * page_id 基準 API の正常系を確認する
         */
        let response = test::call_service(
            &app,
            build_get_request(&format!(
                "/api/pages/{}/short",
                normal_page_id
            ))
            .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&header::HeaderValue::from_static("no-store")),
        );

        let body = to_bytes(response.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        assert_eq!(json["short_path"], expected_short_id);

        /*
         * page_id 基準 API の 404 / 410 を確認する
         */
        let not_found = test::call_service(
            &app,
            build_get_request(
                "/api/pages/01ARZ3NDEKTSV4RRFFQ69G5FAZ/short",
            )
            .to_request(),
        )
        .await;
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

        let gone = test::call_service(
            &app,
            build_get_request(&format!(
                "/api/pages/{}/short",
                deleted_page_id
            ))
            .to_request(),
        )
        .await;
        assert_eq!(gone.status(), StatusCode::GONE);

        /*
         * path 基準 API の正常系を確認する
         */
        let by_path = test::call_service(
            &app,
            build_get_request("/api/pages/short?path=/short/normal")
                .to_request(),
        )
        .await;
        assert_eq!(by_path.status(), StatusCode::OK);

        let body = to_bytes(by_path.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        assert_eq!(json["short_path"], expected_short_id);

        /*
         * path 基準 API の 400 / 404 / 410 を確認する
         */
        let bad_request = test::call_service(
            &app,
            build_get_request("/api/pages/short?path=/wiki/short/normal")
                .to_request(),
        )
        .await;
        assert_eq!(bad_request.status(), StatusCode::BAD_REQUEST);

        let not_found = test::call_service(
            &app,
            build_get_request("/api/pages/short?path=/short/missing")
                .to_request(),
        )
        .await;
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

        let deleted = test::call_service(
            &app,
            build_get_request("/api/pages/short?path=/short/deleted")
                .to_request(),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::GONE);

        let draft = test::call_service(
            &app,
            build_get_request("/api/pages/short?path=/short/draft")
                .to_request(),
        )
        .await;
        assert_eq!(draft.status(), StatusCode::NOT_FOUND);
    }

    ///
    /// `GET /api/pages/{page_id}/meta` の
    /// `short_path` 返却状態を確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test rest_api::pages::short::tests::page_meta_returns_available_and_unavailable_short_path -- --test-threads=1`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn page_meta_returns_available_and_unavailable_short_path() {
        /*
         * テスト用アプリの準備
         */
        let app_state = build_test_app_state().expect("build app state failed");
        let normal_page_id = create_page(
            app_state.db(),
            "/meta/normal",
        )
        .expect("create normal page failed");
        let deleted_page_id = create_page(
            app_state.db(),
            "/meta/deleted",
        )
        .expect("create deleted page failed");
        let draft_page_id = create_draft(app_state.db(), "/meta/draft")
            .expect("create draft page failed");
        delete_page(app_state.db(), &deleted_page_id)
            .expect("delete page failed");

        let app = test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                    app_state,
                ))))
                .service(create_api_scope(1024 * 1024)),
        )
        .await;

        /*
         * 通常ページは available を返すことを確認する
         */
        let response = test::call_service(
            &app,
            build_get_request(&format!("/api/pages/{}/meta", normal_page_id))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        assert_eq!(json["page_info"]["short_path"]["kind"], "available");
        assert_eq!(
            json["page_info"]["short_path"]["value"],
            encode_page_short_id(&normal_page_id),
        );

        /*
         * revision 指定時も current 状態基準で available を返すことを確認する
         */
        let response = test::call_service(
            &app,
            build_get_request(&format!(
                "/api/pages/{}/meta?rev=1",
                normal_page_id
            ))
            .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        assert_eq!(json["page_info"]["short_path"]["kind"], "available");

        /*
         * ドラフトと削除済みページは unavailable を返すことを確認する
         */
        let draft_response = test::call_service(
            &app,
            build_get_request(&format!("/api/pages/{}/meta", draft_page_id))
                .to_request(),
        )
        .await;
        assert_eq!(draft_response.status(), StatusCode::OK);

        let body = to_bytes(draft_response.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        assert_eq!(json["page_info"]["short_path"]["kind"], "unavailable");
        assert!(json["page_info"]["short_path"]["value"].is_null());

        let deleted_response = test::call_service(
            &app,
            build_get_request(&format!(
                "/api/pages/{}/meta",
                deleted_page_id
            ))
            .to_request(),
        )
        .await;
        assert_eq!(deleted_response.status(), StatusCode::OK);

        let body = to_bytes(deleted_response.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        assert_eq!(json["page_info"]["short_path"]["kind"], "unavailable");
        assert!(json["page_info"]["short_path"]["value"].is_null());
    }

    fn build_test_app_state() -> anyhow::Result<AppState> {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");

        let manager = DatabaseManager::open(&db_path, &asset_path)?;
        manager.add_user("user", "pass", None)?;
        manager.ensure_default_root("user")?;

        Ok(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(base_dir.join("fts-index")),
            None,
            "LuWiki Test".to_string(),
            None,
            1024 * 1024,
            None,
        ))
    }

    fn create_page(
        db: &DatabaseManager,
        path: &str,
    ) -> anyhow::Result<PageId> {
        db.create_page(path, "user", format!("# {}", path))
    }

    fn create_draft(
        db: &DatabaseManager,
        path: &str,
    ) -> anyhow::Result<PageId> {
        db.create_draft_page(path, "user").map(|(page_id, _)| page_id)
    }

    fn delete_page(
        db: &DatabaseManager,
        page_id: &PageId,
    ) -> anyhow::Result<()> {
        db.delete_page_by_id(page_id)
    }

    fn prepare_test_dirs() -> (PathBuf, PathBuf) {
        let base = Path::new("tests")
            .join("tmp")
            .join(unique_suffix());
        fs::create_dir_all(&base).expect("create test dir failed");
        (base.clone(), base.join("database.redb"))
    }

    fn unique_suffix() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time error");
        format!(
            "rest-api-short-{}-{}",
            std::process::id(),
            now.as_nanos()
        )
    }

    fn build_get_request(uri: &str) -> test::TestRequest {
        let basic = Basic::new("user", Some("pass"))
            .try_into_value()
            .expect("build basic auth header failed");
        test::TestRequest::get()
            .uri(uri)
            .insert_header((header::AUTHORIZATION, basic))
    }
}
