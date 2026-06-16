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
use actix_web::{HttpRequest, HttpResponse, web};
use serde::Serialize;

use super::super::resp_error_json;
use crate::database::types::BearerScope;
use crate::http_server::app_state::AppState;
use crate::rest_api::require_request_scope;

///
/// テンプレート一覧のレスポンスエントリ
///
#[derive(Serialize)]
struct TemplateEntry {
    page_id: String,
    name: String,
    description: Option<String>,
    macro_expand: Option<bool>,
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
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> actix_web::Result<HttpResponse> {
    if let Err(resp) = require_request_scope(&req, BearerScope::Read) {
        return Ok(resp);
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

    let candidates = match state.db().list_template_candidates() {
        Ok(entries) => entries,
        Err(_) => {
            return Ok(resp_error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "template list failed",
            ));
        }
    };

    let mut entries: Vec<TemplateEntry> = candidates
        .into_iter()
        .map(|candidate| TemplateEntry {
            page_id: candidate.page_id().to_string(),
            name: candidate.name().to_string(),
            description: candidate.description().map(str::to_string),
            macro_expand: candidate.macro_expand(),
        })
        .collect();

    /*
     * レスポンス生成
     */
    entries.sort_by(|left, right| {
        left.name.cmp(&right.name).then_with(|| left.page_id.cmp(&right.page_id))
    });
    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .body(
            serde_json::to_string(&entries)
                .unwrap_or_else(|_| "[]".to_string()),
        ))
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
    use crate::database::types::PageId;
    use crate::fts::FtsIndexConfig;
    use crate::http_server::app_state::AppState;
    use crate::rest_api::create_api_scope;

    #[actix_web::test]
    async fn template_endpoint_lists_front_matter_candidates() {
        let app_state = build_test_app_state().expect("build app state failed");
        let visible = create_template_page(
            app_state.db(),
            "/templates/visible",
            "議事録B",
            Some("visible description"),
            Some(true),
        )
        .expect("create visible template failed");
        let hidden = create_template_page(
            app_state.db(),
            "/templates/hidden",
            "議事録A",
            Some("hidden description"),
            Some(false),
        )
        .expect("create hidden template failed");
        create_normal_page(app_state.db(), "/templates/normal")
            .expect("create normal page failed");

        app_state
            .db()
            .rename_page("/templates/visible", "/moved/visible-template")
            .expect("rename template failed");
        app_state
            .db()
            .delete_page_by_id(&hidden)
            .expect("delete template failed");

        let app = test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                    app_state,
                ))))
                .service(create_api_scope(1024 * 1024)),
        )
        .await;

        let response = test::call_service(
            &app,
            build_get_request("/api/pages/template").to_request(),
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
        let array = json.as_array().expect("entries should be array");
        assert_eq!(array.len(), 1);
        assert_eq!(array[0]["page_id"], visible.to_string());
        assert_eq!(array[0]["name"], "議事録B");
        assert_eq!(array[0]["description"], "visible description");
        assert_eq!(array[0]["macro_expand"], true);
    }

    #[actix_web::test]
    async fn template_endpoint_returns_empty_array_without_template_root() {
        let app_state = build_test_app_state().expect("build app state failed");

        let app = test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                    app_state,
                ))))
                .service(create_api_scope(1024 * 1024)),
        )
        .await;

        let response = test::call_service(
            &app,
            build_get_request("/api/pages/template").to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body())
            .await
            .expect("read body failed");
        assert_eq!(
            std::str::from_utf8(&body).expect("decode body failed"),
            "[]",
        );
    }

    #[actix_web::test]
    async fn template_endpoint_uses_rebuilt_candidates_after_legacy_import() {
        let app_state = build_test_app_state().expect("build app state failed");
        let front_matter_id = create_template_page(
            app_state.db(),
            "/templates/front-matter",
            "front matter 優先",
            Some("front matter description"),
            Some(true),
        )
        .expect("create front matter template failed");
        let legacy_id = create_normal_page(app_state.db(), "/templates/legacy-only")
            .expect("create legacy-only page failed");
        let outside_id = app_state
            .db()
            .create_page(
                "/outside/front-matter",
                "user",
                "---\nwiki:\n  template:\n    name: ルート外\n---\n# outside\n".to_string(),
            )
            .expect("create outside template failed");

        app_state
            .db()
            .remove_template_candidate_by_page_id(&front_matter_id)
            .expect("remove front matter candidate failed");
        app_state
            .db()
            .rebuild_template_candidates_with_legacy(Some("/templates"))
            .expect("rebuild template candidates with legacy failed");

        let app = test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(Arc::new(RwLock::new(
                    app_state,
                ))))
                .service(create_api_scope(1024 * 1024)),
        )
        .await;

        let response = test::call_service(
            &app,
            build_get_request("/api/pages/template").to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body())
            .await
            .expect("read body failed");
        let json: Value =
            serde_json::from_slice(&body).expect("parse json failed");
        let array = json.as_array().expect("entries should be array");

        assert_eq!(array.len(), 3);
        assert_eq!(array[0]["name"], "front matter 優先");
        assert_eq!(array[0]["page_id"], front_matter_id.to_string());
        assert_eq!(array[0]["description"], "front matter description");
        assert_eq!(array[0]["macro_expand"], true);
        assert_eq!(array[1]["name"], "legacy-only");
        assert_eq!(array[1]["page_id"], legacy_id.to_string());
        assert_eq!(array[1]["description"], Value::Null);
        assert_eq!(array[1]["macro_expand"], Value::Null);
        assert_eq!(array[2]["name"], "ルート外");
        assert_eq!(array[2]["page_id"], outside_id.to_string());
        assert_eq!(array[2]["description"], Value::Null);
        assert_eq!(array[2]["macro_expand"], Value::Null);
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

    fn create_template_page(
        db: &DatabaseManager,
        path: &str,
        name: &str,
        description: Option<&str>,
        macro_expand: Option<bool>,
    ) -> anyhow::Result<PageId> {
        let description_block = match description {
            Some(description) => format!("    description: {}\n", description),
            None => String::new(),
        };
        let macro_expand_block = match macro_expand {
            Some(value) => format!("    macro_expand: {}\n", value),
            None => String::new(),
        };
        let page_id = db.create_page(
            path,
            "user",
            format!(
                "---\nwiki:\n  template:\n    name: {}\n{}{}---\n# {}\n",
                name, description_block, macro_expand_block, name
            ),
        )?;
        db.sync_template_candidate_for_page(&page_id)?;
        Ok(page_id)
    }

    fn create_normal_page(
        db: &DatabaseManager,
        path: &str,
    ) -> anyhow::Result<PageId> {
        db.create_page(path, "user", format!("# {}", path))
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
            "rest-api-template-{}-{}",
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
