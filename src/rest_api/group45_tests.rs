/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 更新/assetの認可前返却・副作用を隔離REST fixtureで確認する
//!

use std::fs;
use std::sync::{Arc, RwLock};

use actix_web::{
    http::{Method, header},
    test, web,
};
use serde_json::{Value, json};

use super::authorization_tests::{fixture, token};
use crate::database::types::{AssetId, BearerScope, PageId};

///
/// 全更新入口のpath拒否がlock/draft/assetを変更しない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// 正常なscope/body/token形式で範囲外だけを403にし、DB/bytesを照合する。
///
#[actix_web::test]
async fn denied_updates_preserve_page_lock_asset_and_files() {
    /*
     * 隔離DBとHTTP入口を準備する
     */
    let (dir, state, sink) = super::audit_boundary_tests::fixture();
    let shared = state.get_ref().clone();
    let guard = shared.read().unwrap();
    let db = guard.db();
    let actor = db.get_user_info_by_name("user").unwrap().unwrap().id();
    let page = db.create_page(
        "/outside/page", "user", "private_ms5_denied_source".into(),
    ).unwrap();
    let lock = db.acquire_page_lock(&page, "user").unwrap();
    let asset = db.create_asset(
        &page, "data.txt", "text/plain", "user", b"secret",
    ).unwrap();
    let (token, info) = db.create_bearer_token(
        "user", crate::database::types::BearerScopeSet::from_iter([
            BearerScope::Write,
        ]), crate::database::types::PathPrefixSet::from_iter(["/allow"]),
        chrono::Duration::hours(1), None,
    ).unwrap();
    let bearer = token.expose().to_string();
    let token_id = info.token_id();
    crate::fts::rebuild_index(guard.fts_config(), &[]).unwrap();
    crate::fts::update_pages_index(
        guard.fts_config(), db, &[page.clone()], false,
    ).unwrap();
    drop(guard);
    let app = test::init_service(
        actix_web::App::new().app_data(state.clone())
            .service(super::create_api_scope(1024 * 1024)),
    ).await;
    let mut cases = Vec::new();
    /*
     * 全更新の拒否と保存状態の保全を照合する
     */
    for (method, uri, body, operation, target, summary) in [
        (Method::PUT, format!("/api/pages/{page}/source"), "new",
            "PUT /api/pages/{page_id}/source", "/outside/page",
            "failure=path_prefix_denied amend=false"),
        (Method::POST,
            format!("/api/pages/{page}/path?rename_to=/allow/moved"), "",
            "POST /api/pages/{page_id}/path", "/outside/page",
            "failure=path_prefix_denied action=rename denied_side=source"),
        (Method::POST,
            format!("/api/pages/{page}/path?restore_to=/allow/restored"), "",
            "POST /api/pages/{page_id}/path", "/outside/page",
            "failure=path_prefix_denied action=restore denied_side=source"),
        (Method::POST, format!("/api/pages/{page}/revision?rollback_to=1"), "",
            "POST /api/pages/{page_id}/revision", "/outside/page",
            "failure=path_prefix_denied action=rollback"),
        (Method::POST, format!("/api/pages/{page}/revision?keep_from=1"), "",
            "POST /api/pages/{page_id}/revision", "/outside/page",
            "failure=path_prefix_denied action=compaction"),
        (Method::DELETE, format!("/api/pages/{page}"), "",
            "DELETE /api/pages/{page_id}", "/outside/page",
            "failure=path_prefix_denied recursive=false"),
        (Method::DELETE, format!("/api/pages/{page}?recursive=true"), "",
            "DELETE /api/pages/{page_id}", "/outside/page",
            "failure=path_prefix_denied recursive=true"),
        (Method::POST, format!("/api/pages/{page}/assets/new.txt"), "data",
            "POST /api/pages/{page_id}/assets/{file_name}", "/outside/page",
            "failure=path_prefix_denied"),
        (Method::POST, "/api/assets?path=/outside/page&file=new.txt".into(),
            "data", "POST /api/assets", "/outside/page",
            "failure=path_prefix_denied"),
        (Method::DELETE, format!("/api/assets/{asset}"), "",
            "DELETE /api/assets/{asset_id}", "/outside/page",
            "failure=path_prefix_denied"),
        (Method::POST, "/api/pages?path=/outside/new".into(), "",
            "POST /api/pages", "/outside/new", "failure=path_prefix_denied"),
    ] {
        /*
         * 認証TTL/監査writerを除く業務stateと関連fileだけを保存する
         */
        let before = {
            let guard = shared.read().unwrap();
            business_snapshot(guard.db(), &page, &asset)
        };
        let files = business_files(dir.path());
        let resp = test::call_service(
            &app,
            test::TestRequest::default().method(method).uri(&uri)
                .insert_header((header::AUTHORIZATION,
                    format!("Bearer {}", bearer)))
                .insert_header((header::CONTENT_TYPE, "text/markdown"))
                .set_payload(body).to_request(),
        ).await;
        assert_eq!(resp.status().as_u16(), 403, "{operation}");
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["reason"], "forbidden");
        let after = {
            let guard = shared.read().unwrap();
            business_snapshot(guard.db(), &page, &asset)
        };
        assert!(before == after, "business changed for {operation}");
        assert!(files == business_files(dir.path()),
            "asset/FTS files changed for {operation}");
        let rows = super::audit_boundary_tests::records(&dir, &sink);
        assert_eq!(rows.len(), cases.len() + 1);
        let row = rows.last().unwrap();
        assert_eq!(row.operation.as_str(), operation);
        assert_eq!(row.result,
            crate::audit::model::AuditResult::PathPrefixDenied);
        assert_eq!(row.target_path.as_deref(), Some(target));
        assert_eq!(row.revision, None);
        assert_eq!(row.summary.as_deref(), Some(summary));
        assert_eq!(row.user_id, actor);
        assert_eq!(row.token_id, Some(token_id.clone()));
        cases.push(json!({"operation":operation,"status":403,
            "business_unchanged":true,"asset_fts_files_unchanged":true,
            "result":row.result,"target":target,"revision":null,
            "actor":actor,"token_id":token_id}));
    }
    /*
     * 拒否後の本文/lock/asset/fileを比較する
     */
    {
        let state = shared.read().unwrap();
        let db = state.db();
        assert_eq!(
            db.get_page_index_by_id(&page).unwrap().unwrap().path(),
            "/outside/page"
        );
        assert_eq!(
            db.get_page_source(&page, 1).unwrap().unwrap().source(),
            "private_ms5_denied_source"
        );
        assert_eq!(
            db.get_page_lock_info(&page).unwrap().unwrap().token(),
            lock.token()
        );
        assert!(!db.get_asset_info_by_id(&asset).unwrap().unwrap().deleted());
        assert_eq!(db.read_asset_data(&asset).unwrap(), b"secret");
        assert_eq!(db.list_page_assets(&page).unwrap().len(), 1);
        assert!(db.get_page_id_by_path("/outside/new").unwrap().is_none());
    }
    let rows = super::audit_boundary_tests::records(&dir, &sink);
    let encoded = serde_json::to_string(&rows).unwrap();
    let lock_plaintext = lock.token().to_string();
    for secret in [bearer.as_str(), lock_plaintext.as_str(),
        "data.txt", "new.txt", "secret", "private_ms5_denied_source"]
    {
        assert!(!encoded.contains(secret));
    }
    let output = std::path::Path::new("tmp/ms5-evidence/2.2-2.3");
    fs::create_dir_all(output).unwrap();
    fs::write(output.join("denied-business-cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap()).unwrap();
    fs::write(output.join("denied-business.jsonl"), fs::read(
        crate::audit::rotation::active_log_path(&dir.path().join("audit")),
    ).unwrap()).unwrap();
    drop(app);
    drop(shared);
    drop(state);
    drop(sink);

}

///
/// 認証情報を除いたページ/lock/asset業務stateを比較用に取得する
///
/// # 引数
/// * db - 隔離DB
/// * page - 起点page ID
/// * asset - 既存asset ID
///
/// # 戻り値
/// 内部比較だけのsnapshot。本文/資格情報をartifactへ保存しない。
///
fn business_snapshot(
    db: &crate::database::DatabaseManager, page: &PageId, asset: &AssetId,
) -> Value {
    json!({
        "index": db.get_page_index_by_id(page).unwrap(),
        "source": db.get_page_source(page, 1).unwrap(),
        "lock": db.get_page_lock_info(page).unwrap(),
        "asset": db.get_asset_info_by_id(asset).unwrap(),
        "assets": db.list_page_assets(page).unwrap(),
        "new_draft": db.get_page_id_by_path("/outside/new").unwrap(),
    })
}

///
/// 既存assetと実FTSのfile集合/bytesを内部比較する
///
/// # 引数
/// * root - fixture領域
///
/// # 戻り値
/// 内部file比較値。DB/監査fileと内部pathをartifactへ含めない。
///
/// # 概要
/// subtreeだけを巡回し、増減/内容変更を固定sorted集合で比較する。
///
fn business_files(root: &std::path::Path)
    -> Vec<(std::path::PathBuf, Vec<u8>)>
{
    /*
     * 認証DB・writerを除きassets/FTSの完成済みindexだけを比較する
     */
    let mut pending = vec![root.join("assets"), root.join("fts")];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        if !path.exists() { continue; }
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else { files.push((path.clone(), fs::read(path).unwrap())); }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}


///
/// asset情報/Location/ETag/304/bodyより先にownerを認可する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// 同じassetへの全参照形式を比較し、metadataはbytes不在でも取得できる。
///
#[actix_web::test]
async fn asset_reads_guard_before_redirect_cache_and_body() {
    /*
     * 隔離DBとHTTP入口を準備する
     */
    let (base, state) = fixture();
    let db = state.db();
    let page = db
        .create_page("/outside/page", "user", "source".into())
        .unwrap();
    let asset = db
        .create_asset(&page, "data.txt", "text/plain", "user", b"secret")
        .unwrap();
    let denied = token(db, &[BearerScope::Read], &["/allow"]);
    let allowed = token(db, &[BearerScope::Read], &["/outside"]);
    let shared = Arc::new(RwLock::new(state));
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(shared.clone()))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;
    for uri in [
        format!("/api/assets/{}/data", asset),
        format!("/api/assets/{}/meta", asset),
        format!("/api/pages/{}/assets", page),
        format!("/api/pages/{}/assets/data.txt", page),
        "/api/assets?path=/outside/page&file=data.txt".into(),
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", denied),
                ))
                .insert_header((header::IF_NONE_MATCH, "*"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 403, "{}", uri);
        assert!(resp.headers().get(header::LOCATION).is_none());
        assert!(resp.headers().get(header::ETAG).is_none());
        assert_eq!(
            test::read_body(resp).await.as_ref(),
            b"{\"reason\":\"forbidden\"}"
        );
    }
    {
        let state = shared.read().unwrap();
        let bytes = state.db().read_asset_data(&asset).unwrap();
        assert_eq!(bytes, b"secret");
        // 保存fileをなくしてmetadata/redirectがbytesを読まないことを確認する。
        let file = state.db().asset_file_path(&asset);
        fs::remove_file(file).unwrap();
    }
    for (uri, status) in [
        (format!("/api/assets/{}/meta", asset), 200),
        (format!("/api/pages/{}/assets/data.txt", page), 302),
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", allowed),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), status);
    }
    drop(app);
    drop(shared);
    fs::remove_dir_all(base).unwrap();
}

///
/// no-opでも両側認可/lockを維持し不要FTSを呼ばない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// FTS不在fixtureで204を確認し、有効lock時は旧423を維持する。
///
#[actix_web::test]
async fn rename_noop_skips_fts_but_keeps_lock_check() {
    /*
     * 隔離DBとHTTP入口を準備する
     */
    let (base, state) = fixture();
    let db = state.db();
    let page = db
        .create_page("/allow/page", "user", "body".into())
        .unwrap();
    let bearer = token(db, &[BearerScope::Update], &["/allow"]);
    let shared = Arc::new(RwLock::new(state));
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(shared.clone()))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;
    for locked in [false, true] {
        if locked {
            shared
                .read()
                .unwrap()
                .db()
                .acquire_page_lock(&page, "user")
                .unwrap();
        }
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&format!("/api/pages/{}/path?rename_to=/allow/page", page))
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", bearer),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), if locked { 423 } else { 204 });
    }
    assert_eq!(
        shared
            .read()
            .unwrap()
            .db()
            .get_page_index_by_id(&page)
            .unwrap()
            .unwrap()
            .latest(),
        1
    );
    drop(app);
    drop(shared);
    fs::remove_dir_all(base).unwrap();
}
