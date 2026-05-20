/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページID取得APIの実装をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::http::{header, StatusCode};
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

use super::super::resp_error_json;
use crate::database::PagePathResolveState;
use crate::database::types::BearerScope;
use crate::http_server::app_state::AppState;
use crate::rest_api::{CACHE_CONTROL_NO_STORE, require_request_scope};

#[derive(Deserialize)]
struct PageIdQuery {
    path: String,
}

///
/// GET /api/pages/id?path={page_path} の実体
///
/// # 概要
/// ページパスに対応するページIDを取得する
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
    if let Err(resp) = require_request_scope(&req, BearerScope::Read) {
        return Ok(resp);
    }

    /*
     * クエリ取得と検証
     */
    let query = match web::Query::<PageIdQuery>::from_query(
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
     * page path からページ状態を解決
     */
    let page_id = match state.db().resolve_page_state_by_path(&query.path) {
        Ok(PagePathResolveState::Current {
            page_id,
            draft: false,
        }) => page_id,
        Ok(PagePathResolveState::Current { .. })
        | Ok(PagePathResolveState::NotFound) => {
            return Ok(resp_error_json(StatusCode::NOT_FOUND, "page not found"));
        }
        Ok(PagePathResolveState::Deleted) => {
            return Ok(resp_error_json(StatusCode::GONE, "page deleted"));
        }
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "page lookup failed",
            ));
        }
    };

    /*
     * レスポンス生成
     */
    let body = json!({
        "id": page_id.to_string(),
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE))
        .body(body.to_string()))
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
    use chrono::Duration;
    use serde_json::Value;

    use crate::cmd_args::FrontendConfig;
    use crate::database::DatabaseManager;
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        PageId,
        PathPrefixSet,
    };
    use crate::fts::FtsIndexConfig;
    use crate::http_server::app_state::AppState;
    use crate::rest_api::create_api_scope;

    ///
    /// `GET /api/pages/id?path={page_path}` の
    /// 正常系・例外系を確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test rest_api::pages::id::tests::page_id_endpoint_covers_success_and_error_cases -- --test-threads=1`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn page_id_endpoint_covers_success_and_error_cases() {
        /*
         * テスト用アプリの準備
         */
        let app_state = build_test_app_state().expect("build app state failed");
        let normal_page_id = create_page(app_state.db(), "/id/normal")
            .expect("create normal page failed");
        let deleted_page_id = create_page(app_state.db(), "/id/deleted")
            .expect("create deleted page failed");
        create_draft(app_state.db(), "/id/draft")
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
         * 正常系を確認する
         */
        let response = test::call_service(
            &app,
            build_get_request("/api/pages/id?path=/id/normal").to_request(),
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
        assert_eq!(json["id"], normal_page_id.to_string());

        /*
         * 400 / 404 / 410 を確認する
         */
        let bad_request = test::call_service(
            &app,
            build_get_request("/api/pages/id?path=id/invalid").to_request(),
        )
        .await;
        assert_eq!(bad_request.status(), StatusCode::BAD_REQUEST);

        let not_found = test::call_service(
            &app,
            build_get_request("/api/pages/id?path=/id/missing").to_request(),
        )
        .await;
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);

        let deleted = test::call_service(
            &app,
            build_get_request("/api/pages/id?path=/id/deleted").to_request(),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::GONE);

        let draft = test::call_service(
            &app,
            build_get_request("/api/pages/id?path=/id/draft").to_request(),
        )
        .await;
        assert_eq!(draft.status(), StatusCode::NOT_FOUND);
    }

    ///
    /// `GET /api/pages/id?path={page_path}` の
    /// Bearer スコープ判定を確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test rest_api::pages::id::tests::page_id_endpoint_enforces_bearer_read_scope -- --test-threads=1`
    /// で実行する。
    ///
    #[actix_web::test]
    async fn page_id_endpoint_enforces_bearer_read_scope() {
        /*
         * テスト用アプリの準備
         */
        let app_state = build_test_app_state().expect("build app state failed");
        let page_id = create_page(app_state.db(), "/id/bearer")
            .expect("create page failed");
        let read_token = create_bearer_token(
            app_state.db(),
            BearerScopeSet::from_iter([BearerScope::Read]),
        )
        .expect("create read token failed");
        let append_token = create_bearer_token(
            app_state.db(),
            BearerScopeSet::from_iter([BearerScope::Append]),
        )
        .expect("create append token failed");

        let app = test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                    app_state,
                ))))
                .service(create_api_scope(1024 * 1024)),
        )
        .await;

        /*
         * Bearer read で成功することを確認する
         */
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/pages/id?path=/id/bearer")
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", read_token),
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
        assert_eq!(json["id"], page_id.to_string());

        /*
         * Bearer append は拒否されることを確認する
         */
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/pages/id?path=/id/bearer")
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", append_token),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = to_bytes(response.into_body())
            .await
            .expect("read forbidden body failed");
        assert_eq!(
            std::str::from_utf8(&body).expect("decode body failed"),
            r#"{"reason":"forbidden"}"#,
        );
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

    fn create_bearer_token(
        db: &DatabaseManager,
        scopes: BearerScopeSet,
    ) -> anyhow::Result<String> {
        let (plaintext, _) = db.create_bearer_token(
            "user",
            scopes,
            PathPrefixSet::new(),
            Duration::seconds(3600),
            Some("rest api page id test token".to_string()),
        )?;

        Ok(plaintext.expose().to_string())
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
            "rest-api-page-id-{}-{}",
            std::process::id(),
            now.as_nanos(),
        )
    }

    fn build_get_request(uri: &str) -> test::TestRequest {
        let basic = Basic::new("user", Some("pass"));
        let header_value = basic.try_into_value().expect("basic header failed");

        test::TestRequest::get()
            .uri(uri)
            .insert_header((header::AUTHORIZATION, header_value))
    }
}
