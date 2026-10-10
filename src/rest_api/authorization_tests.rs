/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! RESTの入力・scope・ページ参照認可を隔離fixtureで検証する
//!

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use actix_web::{
    http::{Method, header},
    test, web,
};
use chrono::Duration;
use serde_json::Value;

use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::database::types::{BearerScope, BearerScopeSet, PathPrefixSet};
use crate::fts::FtsIndexConfig;
use crate::http_server::app_state::AppState;

///
/// テスト用のDB/asset/FTSを隔離する
///
/// # 戻り値
/// 一時ディレクトリとアプリ状態。
///
/// # 概要
/// プロセスと時刻で一時領域を分けDB/assets/FTSを初期化する。
///
pub(super) fn fixture() -> (PathBuf, AppState) {
    /*
     * 隔離DB/assets/FTSとユーザを初期化する
     */
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = PathBuf::from(format!(
        "tests/tmp/rest-ms3-{}-{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(&base).unwrap();
    let db =
        DatabaseManager::open(base.join("db"), base.join("assets")).unwrap();
    db.add_user("user", "pass", None).unwrap();
    db.ensure_default_root("user").unwrap();
    let state = AppState::new(
        db,
        FrontendConfig::default(),
        FtsIndexConfig::new(base.join("fts")),
        None,
        "test".into(),
        None,
        1024 * 1024,
        None,
    );
    (base, state)
}

///
/// scope/prefix指定のBearerを作る
///
/// # 引数
/// * `db` - 隔離DB
/// * `scopes` - 許可scope
/// * `prefixes` - 許可パス
///
/// # 戻り値
/// テスト用token文字列。
///
pub(super) fn token(
    db: &DatabaseManager,
    scopes: &[BearerScope],
    prefixes: &[&str],
) -> String {
    db.create_bearer_token(
        "user",
        BearerScopeSet::from_iter(scopes.iter().copied()),
        PathPrefixSet::from_iter(prefixes.iter().copied()),
        Duration::hours(1),
        None,
    )
    .unwrap()
    .0
    .expose()
    .to_string()
}

///
/// ページ参照の認可、状態、304、親例外と履歴を確認する
///
/// # 戻り値
/// テスト結果。
///
/// # 注記
/// current/deleted/draftを作り、許可/非許可の同じAPIを比較する。
///
#[actix_web::test]
async fn page_reads_guard_state_history_and_parent() {
    /*
     * 通常、履歴移動、削除、draftのfixtureを作る
     */
    let (base, state) = fixture();
    let db = state.db();
    db.create_page("/allowed", "user", "parent".to_string())
        .unwrap();
    db.create_page("/second/page", "user", "second".to_string())
        .unwrap();
    let page = db
        .create_page("/outside/history", "user", "old source".to_string())
        .unwrap();
    db.rename_page("/outside/history", "/allowed/history")
        .unwrap();
    let outside = db
        .create_page("/allowed2/page", "user", "secret".to_string())
        .unwrap();
    let deleted = db
        .create_page("/outside/deleted", "user", "deleted source".to_string())
        .unwrap();
    db.delete_page_by_id(&deleted).unwrap();
    let draft = db.create_draft_page("/outside/draft", "user").unwrap().0;
    let read = token(db, &[BearerScope::Read], &["/allowed", "/second"]);
    let update = token(db, &[BearerScope::Update], &["/allowed"]);
    let root = token(db, &[BearerScope::Read], &["/"]);
    let leaf = token(db, &[BearerScope::Read], &["/allowed/history"]);
    let shared = Arc::new(RwLock::new(state));
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(shared.clone()))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;

    /*
     * 本文、path、short、meta、parent、lockは返却より先に拒否する
     */
    for id in [&outside, &deleted, &draft] {
        for endpoint in ["source", "path", "short", "meta", "parent", "lock"] {
            let uri = format!("/api/pages/{}/{}", id, endpoint);
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(&uri)
                    .insert_header((
                        header::AUTHORIZATION,
                        format!("Bearer {}", read),
                    ))
                    .insert_header((header::IF_NONE_MATCH, "*"))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status().as_u16(), 403, "{}", uri);
            let body: Value = test::read_body_json(resp).await;
            assert_eq!(body["reason"], "forbidden");
        }
    }
    for uri in [
        "/api/pages/id?path=/allowed2/page",
        "/api/pages/short?path=/outside/deleted",
        "/api/pages/id?path=/outside/missing",
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", read),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 403);
    }

    /*
     * dot最終値と過去ソース/metaを保持する
     */
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/pages/id?path=/outside/../allowed/history")
            .insert_header((header::AUTHORIZATION, format!("Bearer {}", read)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], page.to_string());
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/pages/id?path=/second/page")
            .insert_header((header::AUTHORIZATION, format!("Bearer {}", read)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200);
    for suffix in ["source?rev=1", "meta?rev=1"] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/api/pages/{}/{}", page, suffix))
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", read),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200);
        if suffix.starts_with("source") {
            assert_eq!(test::read_body(resp).await.as_ref(), b"old source");
        } else {
            let body: Value = test::read_body_json(resp).await;
            assert_eq!(body["revision_info"]["revision"], 1);
            assert_eq!(body["page_info"]["path"]["value"], "/allowed/history");
        }
    }
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/pages/{}/meta?rev=2", page))
            .insert_header((header::AUTHORIZATION, format!("Bearer {}", read)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(
        body["revision_info"]["rename_info"]["from"],
        "/outside/history"
    );
    assert_eq!(
        body["revision_info"]["rename_info"]["to"],
        "/allowed/history"
    );
    assert_eq!(
        body["revision_info"]["rename_info"]["link_refs"],
        serde_json::json!({})
    );
    /*
     * path拒否はtoken欠落/不正のlock理由より先に返す
     */
    for method in [Method::PUT, Method::DELETE] {
        let resp = test::call_service(
            &app,
            test::TestRequest::default()
                .method(method)
                .uri(&format!("/api/pages/{}/lock", outside))
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", update),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 403);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["reason"], "forbidden");
    }
    for recursive in [false, true] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!(
                    "/api/pages/{}/parent?recursive={}",
                    page, recursive
                ))
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", leaf),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 404);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["reason"], "parent not found");
    }
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/pages/{}/source", deleted))
            .insert_header((header::AUTHORIZATION, format!("Bearer {}", root)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200);
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/pages/00000000000000000000000000/path")
            .insert_header((header::AUTHORIZATION, format!("Bearer {}", read)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 404);
    drop(app);
    drop(shared);
    fs::remove_dir_all(base).unwrap();
}

///
/// scope細分とlock OR/action分岐を検証する
///
/// # 戻り値
/// テスト結果。
///
/// # 注記
/// scope単独tokenで全更新actionの入口と別scope拒否を比較する。
///
#[actix_web::test]
async fn scopes_are_action_specific_and_lock_or() {
    /*
     * 単独scopeのtokenと既存ページを準備する
     */
    let (base, state) = fixture();
    let db = state.db();
    let page = db
        .create_page("/page", "user", "source".to_string())
        .unwrap();
    let mut tokens = Vec::new();
    for scope in [
        BearerScope::Read,
        BearerScope::Create,
        BearerScope::Update,
        BearerScope::Delete,
        BearerScope::Write,
    ] {
        tokens.push(token(db, &[scope], &[]));
    }
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(Arc::new(RwLock::new(state))))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;

    /*
     * hello/read非包含と更新action別scopeを比較する
     */
    for (index, token) in tokens.iter().enumerate() {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/hello")
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", token),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status().as_u16(),
            if index == 0 || index == 4 { 200 } else { 403 }
        );
    }
    let actions = [
        (Method::POST, "/api/pages?path=/new".to_string(), 1),
        (Method::PUT, format!("/api/pages/{}/source", page), 2),
        (
            Method::POST,
            format!("/api/pages/{}/path?rename_to=/renamed", page),
            2,
        ),
        (
            Method::POST,
            format!("/api/pages/{}/path?restore_to=/restored", page),
            1,
        ),
        (
            Method::POST,
            format!("/api/pages/{}/revision?rollback_to=1", page),
            2,
        ),
        (
            Method::POST,
            format!("/api/pages/{}/revision?keep_from=1", page),
            3,
        ),
        (
            Method::POST,
            format!("/api/pages/{}/assets/test.txt", page),
            2,
        ),
        (
            Method::POST,
            "/api/assets?path=/page&file=test.txt".to_string(),
            2,
        ),
        (Method::DELETE, format!("/api/pages/{}", page), 3),
        (Method::DELETE, "/api/assets/not-an-id".to_string(), 3),
    ];
    for (method, uri, required) in actions {
        for index in 0..4 {
            if index == required {
                continue;
            }
            let resp = test::call_service(
                &app,
                test::TestRequest::default()
                    .method(method.clone())
                    .uri(&uri)
                    .insert_header((
                        header::AUTHORIZATION,
                        format!("Bearer {}", tokens[index]),
                    ))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status().as_u16(), 403, "{} scope {}", uri, index);
        }
    }

    /*
     * create/update単独でlock取得・延長しupdateで解放する
     */
    for scope in [1, 2, 4] {
        let uri = format!("/api/pages/{}/lock", page);
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", tokens[scope]),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 204);
        let lock = resp.headers().get("x-page-lock").unwrap().clone();
        let resp = test::call_service(
            &app,
            test::TestRequest::put()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", tokens[scope]),
                ))
                .insert_header(("X-Lock-Authentication", lock))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 204);
        let lock = resp.headers().get("x-page-lock").unwrap().clone();
        let resp = test::call_service(
            &app,
            test::TestRequest::delete()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", tokens[2]),
                ))
                .insert_header(("X-Lock-Authentication", lock))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 204);
    }
    /*
     * 公開細分scope単独で保存・移動・履歴操作・upload・削除・復元する
     */
    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/pages?path=/scope-created")
            .insert_header((
                header::AUTHORIZATION,
                format!("Bearer {}", tokens[1]),
            ))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 201);
    let lock = resp.headers().get("x-page-lock").unwrap().clone();
    let body: Value = test::read_body_json(resp).await;
    let created = body["id"].as_str().unwrap();
    let resp = test::call_service(
        &app,
        test::TestRequest::put()
            .uri(&format!("/api/pages/{}/source", created))
            .insert_header((
                header::AUTHORIZATION,
                format!("Bearer {}", tokens[2]),
            ))
            .insert_header((header::CONTENT_TYPE, "text/markdown"))
            .insert_header(("X-Lock-Authentication", lock))
            .set_payload("scope source")
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 204);
    for (uri, scope) in [
        (
            format!("/api/pages/{}/path?rename_to=/scope-renamed", created),
            2,
        ),
        (format!("/api/pages/{}/revision?rollback_to=1", created), 2),
        (format!("/api/pages/{}/revision?keep_from=1", created), 3),
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", tokens[scope]),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 204, "{}", uri);
    }
    for uri in [
        format!("/api/pages/{}/assets/a.txt", created),
        "/api/assets?path=/scope-renamed&file=b.txt".to_string(),
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", tokens[2]),
                ))
                .insert_header((header::CONTENT_TYPE, "text/plain"))
                .set_payload("asset")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 201, "{}", uri);
    }
    let resp = test::call_service(
        &app,
        test::TestRequest::delete()
            .uri(&format!("/api/pages/{}", created))
            .insert_header((
                header::AUTHORIZATION,
                format!("Bearer {}", tokens[3]),
            ))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 204);
    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri(&format!(
                "/api/pages/{}/path?restore_to=/scope-restored",
                created
            ))
            .insert_header((
                header::AUTHORIZATION,
                format!("Bearer {}", tokens[1]),
            ))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 204);
    drop(app);
    fs::remove_dir_all(base).unwrap();
}

///
/// raw禁止とdot解決をREST全通常入力で確認する
///
/// # 戻り値
/// テスト結果。
///
/// # 注記
/// raw禁止入力が縮約で正常扱いされないことを比較する。
///
#[actix_web::test]
async fn normal_inputs_reject_raw_invalid_before_dot_resolution() {
    let (base, state) = fixture();
    let bearer =
        token(state.db(), &[BearerScope::Read, BearerScope::Write], &[]);
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(Arc::new(RwLock::new(state))))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;
    /*
     * 通常path/prefix/cursorのraw禁止を検証する
     */
    for input in [
        "/a//../b",
        "/a/",
        "/../b",
        "/a%20/../b",
        "/a%E3%80%80/../b",
        "/a%5Cb",
    ] {
        for (method, uri) in [
            (Method::GET, format!("/api/pages/id?path={}", input)),
            (Method::GET, format!("/api/pages/short?path={}", input)),
            (Method::GET, format!("/api/pages/deleted?path={}", input)),
            (Method::GET, format!("/api/pages?prefix={}", input)),
            (
                Method::GET,
                format!("/api/pages?prefix=/&forward={}", input),
            ),
            (Method::GET, format!("/api/pages?prefix=/&rewind={}", input)),
            (Method::POST, format!("/api/pages?path={}", input)),
            (Method::GET, format!("/api/assets?path={}&file=f", input)),
            (Method::POST, format!("/api/assets?path={}&file=f", input)),
        ] {
            let resp = test::call_service(
                &app,
                test::TestRequest::default()
                    .method(method)
                    .uri(&uri)
                    .insert_header((
                        header::AUTHORIZATION,
                        format!("Bearer {}", bearer),
                    ))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status().as_u16(), 400, "{}", uri);
        }
    }
    drop(app);
    fs::remove_dir_all(base).unwrap();
}

///
/// hello/meは制約Bearerでもページguardを適用せずreadを要求する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 関係しないprefixのreadとupdate単独を同じHTTP入口で比較する。
///
/// # 注記
/// readでは両200とmeの主体を確認し、update単独は両403とする。
///
#[actix_web::test]
async fn constrained_hello_and_me_require_read_without_page_guard() {
    /*
     * ページ対象と無関係なprefixを持つ細分scope tokenを準備する
     */
    let (base, state) = fixture();
    let read = token(state.db(), &[BearerScope::Read], &["/unrelated"]);
    let update = token(state.db(), &[BearerScope::Update], &["/unrelated"]);
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(Arc::new(RwLock::new(state))))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;
    /*
     * readのみ200となり、path制約による拒否が発生しないことを確認する
     */
    for (bearer, expected) in [(&read, 200), (&update, 403)] {
        for uri in ["/api/hello", "/api/users/me"] {
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(uri)
                    .insert_header((
                        header::AUTHORIZATION,
                        format!("Bearer {}", bearer),
                    ))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status().as_u16(), expected, "{}", uri);
            if expected == 200 && uri.ends_with("/me") {
                let body: Value = test::read_body_json(resp).await;
                assert_eq!(body["username"], "user");
                assert!(body["id"].as_str().is_some());
            }
        }
    }
    drop(app);
    fs::remove_dir_all(base).unwrap();
}
