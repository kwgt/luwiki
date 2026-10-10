/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 同txn確定情報と全REST操作の監査補足を検証する
//!

use actix_web::http::{Method, StatusCode, header};
use actix_web::{App, test};
use serde_json::{Value, json};

use super::audit_boundary_tests::{fixture, records};
use crate::audit::model::{AuditRecord, AuditResult};

///
/// 当該公開要求の最終レコードを照合する
///
/// # 引数
/// * `rows` - 明示flushした実レコード
/// * `operation` - 正本固定operation
/// * `target` - 正本対象
/// * `revision` - 正本単一revision
/// * `summary` - 正本補足
///
/// # 戻り値
/// 対応表へ登録するレコード。
///
fn check_record(
    rows: Vec<AuditRecord>,
    operation: &str,
    target: Option<&str>,
    revision: Option<u64>,
    summary: Option<&str>,
) -> AuditRecord {
    let row = rows.last().unwrap().clone();
    assert_eq!(row.operation.as_str(), operation);
    assert_eq!(row.target_path.as_deref(), target, "{operation}");
    assert_eq!(row.revision, revision, "{operation}");
    assert_eq!(row.summary.as_deref(), summary, "{operation}");
    row
}

///
/// 秘密を含まない実失敗レコードをレビュー用へ保存する
///
/// # 引数
/// * `label` - fixture固定名
/// * `rows` - flush後に読み取った実JSONL
///
/// # 戻り値
/// なし。運用データは使用しない。
///
fn save_failure_records(label: &str, rows: Vec<AuditRecord>) {
    let json = serde_json::to_string_pretty(&rows).unwrap();
    std::fs::write(format!("/tmp/ms4-g4-failure-{label}.json"), json).unwrap();
}

macro_rules! request {
    ($app:expr, $method:expr, $uri:expr) => {
        request!($app, $method, $uri, "", &[] as &[(&str, &str)])
    };
    ($app:expr, $method:expr, $uri:expr, $body:expr, $headers:expr) => {
        request_with_auth!($app, $method, $uri, $body, $headers,
            "Basic dXNlcjpwYXNz")
    };
}

macro_rules! request_with_auth {
    ($app:expr, $method:expr, $uri:expr, $body:expr, $headers:expr,
        $authorization:expr) => {{
        let mut builder = test::TestRequest::default()
            .method($method).uri($uri)
            .insert_header((header::AUTHORIZATION, $authorization))
            .insert_header((header::CONTENT_TYPE, "text/markdown"))
            .set_payload($body.to_string());
        let headers: &[(&str, &str)] = $headers;
        for (name, value) in headers {
            builder = builder.insert_header((*name, *value));
        }
        test::call_service(&$app, builder.to_request()).await
    }};
}

///
/// 全32アクションの成功補足を実JSONLへ照合する
///
/// # 概要
/// 同じ隔離AppState/sinkへ32公開要求を送り対象・補足を照合する。
///
/// # 注記
/// source/revision/lock/assetは実DB操作の成立値だけを期待値に使う。
///
#[actix_web::test]
async fn rest_operation_all_r32_success_metadata_matches_contract() {
    run_r32_success(false).await;
}

///
/// 実Bearerの全32成功を同じrunnerでactor/tokenと照合する
///
/// # 注記
/// Basicと同じ対象/行順/全fieldを保ち、fixture複製をしない。
///
#[actix_web::test]
async fn rest_operation_all_r32_bearer_metadata_matches_contract() {
    run_r32_success(true).await;
}

///
/// 全R32の実操作と監査を認証modeだけ変えて照合する
///
/// # 引数
/// * use_bearer - 実Bearerを使う場合true
///
/// # 戻り値
/// なし。期待不一致でtest失敗。
///
/// # 概要
/// 同じDB準備/操作/期待値をBasic/Bearerへ適用し原JSONLを保存する。
///
async fn run_r32_success(use_bearer: bool) {
    /*
     * 実操作の対象と履歴・削除・assetを隔離DBに用意する
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let db = guard.db();
    let actor = db.get_user_info_by_name("user").unwrap().unwrap().id();
    let (authorization, expected_token, secret) = if use_bearer {
        let (token, info) = db.create_bearer_token(
            "user", crate::database::types::BearerScopeSet::all(),
            crate::database::types::PathPrefixSet::new(),
            chrono::Duration::hours(1), None,
        ).unwrap();
        (format!("Bearer {}", token.expose()), Some(info.token_id()),
            Some(token.expose().to_string()))
    } else { ("Basic dXNlcjpwYXNz".to_string(), None, None) };
    macro_rules! request {
        ($app:expr, $method:expr, $uri:expr) => {
            request!($app, $method, $uri, "", &[] as &[(&str, &str)])
        };
        ($app:expr, $method:expr, $uri:expr, $body:expr, $headers:expr) => {
            request_with_auth!($app, $method, $uri, $body, $headers,
                authorization.as_str())
        };
    }
    let mut pages = std::collections::BTreeMap::new();
    for path in [
        "/audit", "/audit/a", "/audit/parent", "/audit/parent/child",
        "/audit/move", "/audit/deleted", "/audit/history", "/audit/compact",
        "/audit/assetowner", "/audit/lockpage", "/audit/delete",
        "/audit/delete/child",
    ] {
        let page = db.create_page(path, "user", "initial".into()).unwrap();
        pages.insert(path, page);
    }
    db.put_page(&pages["/audit/history"], "user", "second".into(), false)
        .unwrap();
    db.put_page(&pages["/audit/compact"], "user", "second".into(), false)
        .unwrap();
    db.put_page(&pages["/audit/compact"], "user", "third".into(), false)
        .unwrap();
    db.delete_page_by_id(&pages["/audit/deleted"]).unwrap();
    let asset = db.create_asset(
        &pages["/audit/assetowner"], "never_record_filename.bin",
        "text/plain", "user", b"secret_body_123",
    ).unwrap();
    drop(guard);
    let app = test::init_service(App::new().app_data(state.clone())
        .service(super::create_api_scope(1024))).await;
    let mut evidence = Vec::<Value>::new();

    macro_rules! checked {
        ($r:expr, $op:expr, $p:expr, $v:expr, $s:expr) => {{
            let all = records(&dir, &sink);
            assert_eq!(all.len(), evidence.len() + 1, "{}", $r);
            let row = check_record(all, $op, $p, $v, $s);
            assert_eq!(row.result, AuditResult::Success, "{}", $r);
            assert_eq!(row.user_id, actor);
            assert_eq!(row.token_id, expected_token);
            evidence.push(json!({ "r": $r, "record": row }));
        }};
    }

    /*
     * 対象を持たない共通参照と新規draft、一覧・path入力
     */
    assert!(request!(app, Method::GET, "/api/hello").status().is_success());
    checked!("R01", "GET /api/hello", None, None, None);
    assert!(request!(app, Method::GET, "/api/users/me").status().is_success());
    checked!("R02", "GET /api/users/me", None, None, None);
    let response = request!(app, Method::POST, "/api/pages?path=/audit/draft");
    assert!(response.status().is_success());
    checked!("R03", "POST /api/pages", Some("/audit/draft"), None, None);

    let response =
        request!(app, Method::GET, "/api/pages?prefix=/audit&limit=2");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    checked!("R04", "GET /api/pages", Some("/audit"), None,
        Some(&format!("count={} has_more={}",
            body["items"].as_array().unwrap().len(), body["has_more"])));
    let response = request!(app, Method::GET,
        "/api/pages/deleted?path=/audit/deleted");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    checked!("R05", "GET /api/pages/deleted", Some("/audit/deleted"), None,
        Some(&format!("count={}", body.as_array().unwrap().len())));
    assert!(request!(app, Method::GET, "/api/pages/id?path=/audit/a")
        .status().is_success());
    checked!("R06", "GET /api/pages/id", Some("/audit/a"), None, None);
    assert!(request!(app, Method::GET, "/api/pages/short?path=/audit/a")
        .status().is_success());
    checked!("R07", "GET /api/pages/short", Some("/audit/a"), None, None);
    let response = request!(app, Method::GET, "/api/pages/template");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    checked!("R08", "GET /api/pages/template", None, None,
        Some(&format!("count={}", body.as_array().unwrap().len())));

    /*
     * 実source取得VとcommitV、検索の実返却件数
     */
    let uri = format!("/api/pages/{}/source", pages["/audit/a"]);
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R10", "GET /api/pages/{page_id}/source",
        Some("/audit/a"), Some(1), None);
    assert!(request!(app, Method::PUT, &uri, "needle secret_body_123", &[])
        .status().is_success());
    checked!("R11", "PUT /api/pages/{page_id}/source",
        Some("/audit/a"), Some(2), Some("amend=false"));
    let response = request!(app, Method::GET, "/api/pages/search?expr=needle");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    checked!("R09", "GET /api/pages/search", None, None,
        Some(&format!("count={}", body.as_array().unwrap().len())));

    /*
     * source/metaのrevisionとparentの指定対象を区別する
     */
    let uri = format!("/api/pages/{}/meta", pages["/audit/a"]);
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R12", "GET /api/pages/{page_id}/meta",
        Some("/audit/a"), Some(2), None);
    let uri = format!("/api/pages/{}/parent", pages["/audit/parent/child"]);
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R13", "GET /api/pages/{page_id}/parent",
        Some("/audit/parent/child"), None, None);
    let uri = format!("/api/pages/{}/short", pages["/audit/a"]);
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R14", "GET /api/pages/{page_id}/short",
        Some("/audit/a"), None, None);
    let uri = format!("/api/pages/{}/path", pages["/audit/a"]);
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R15", "GET /api/pages/{page_id}/path",
        Some("/audit/a"), None, None);

    /*
     * shared POSTの移動/復元と指定revisionを最新Vと混同しない
     */
    let uri = format!("/api/pages/{}/path?rename_to=/audit/moved",
        pages["/audit/move"]);
    assert!(request!(app, Method::POST, &uri).status().is_success());
    checked!("R16", "POST /api/pages/{page_id}/path",
        Some("/audit/move"), Some(2),
        Some("action=rename destination=/audit/moved"));
    let uri = format!("/api/pages/{}/path?restore_to=/audit/restored",
        pages["/audit/deleted"]);
    assert!(request!(app, Method::POST, &uri).status().is_success());
    checked!("R17", "POST /api/pages/{page_id}/path",
        Some("/audit/deleted"), Some(1),
        Some("action=restore destination=/audit/restored"));
    let uri = format!("/api/pages/{}/revision?rollback_to=1",
        pages["/audit/history"]);
    assert!(request!(app, Method::POST, &uri).status().is_success());
    checked!("R18", "POST /api/pages/{page_id}/revision",
        Some("/audit/history"), Some(1), Some("action=rollback"));
    let uri = format!("/api/pages/{}/revision?keep_from=2",
        pages["/audit/compact"]);
    assert!(request!(app, Method::POST, &uri).status().is_success());
    checked!("R19", "POST /api/pages/{page_id}/revision",
        Some("/audit/compact"), Some(2), Some("action=compaction"));

    /*
     * asset ownerだけを記録しfile名/body/Locationを転記しない
     */
    let uri = format!("/api/pages/{}/assets", pages["/audit/assetowner"]);
    let response = request!(app, Method::GET, &uri);
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    checked!("R20", "GET /api/pages/{page_id}/assets",
        Some("/audit/assetowner"), None,
        Some(&format!("count={}", body.as_array().unwrap().len())));
    let uri = format!("/api/pages/{}/assets/second_secret.bin",
        pages["/audit/assetowner"]);
    assert!(request!(app, Method::POST, &uri, "secret_body_123", &[])
        .status().is_success());
    checked!("R21", "POST /api/pages/{page_id}/assets/{file_name}",
        Some("/audit/assetowner"), None, None);
    let uri = format!("/api/pages/{}/assets/never_record_filename.bin",
        pages["/audit/assetowner"]);
    let response = request!(app, Method::GET, &uri);
    assert_eq!(response.status(), StatusCode::FOUND);
    assert!(response.headers().contains_key(header::LOCATION));
    checked!("R22", "GET /api/pages/{page_id}/assets/{file_name}",
        Some("/audit/assetowner"), None, None);

    /*
     * lock lifecycleの資格情報を保持せず同txn targetだけ使う
     */
    let uri = format!("/api/pages/{}/lock", pages["/audit/lockpage"]);
    let response = request!(app, Method::POST, &uri);
    assert!(response.status().is_success());
    let mut lock_header = response.headers().get("X-Page-Lock")
        .unwrap().to_str().unwrap().to_string();
    checked!("R23", "POST /api/pages/{page_id}/lock",
        Some("/audit/lockpage"), None, None);
    let response = request!(app, Method::PUT, &uri, "",
        &[("X-Lock-Authentication", lock_header.as_str())]);
    assert!(response.status().is_success());
    lock_header = response.headers().get("X-Page-Lock").unwrap()
        .to_str().unwrap().to_string();
    checked!("R24", "PUT /api/pages/{page_id}/lock",
        Some("/audit/lockpage"), None, None);
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R25", "GET /api/pages/{page_id}/lock",
        Some("/audit/lockpage"), None, None);
    assert!(request!(app, Method::DELETE, &uri, "",
        &[("X-Lock-Authentication", lock_header.as_str())])
        .status().is_success());
    checked!("R26", "DELETE /api/pages/{page_id}/lock",
        Some("/audit/lockpage"), None, None);
    let uri = format!("/api/pages/{}?recursive=true", pages["/audit/delete"]);
    assert!(request!(app, Method::DELETE, &uri).status().is_success());
    checked!("R27", "DELETE /api/pages/{page_id}",
        Some("/audit/delete"), None, Some("recursive=true"));

    /*
     * path指定assetとID指定assetは同じowner/null revision
     */
    assert!(request!(app, Method::POST,
        "/api/assets?path=/audit/assetowner&file=third_secret.bin",
        "secret_body_123", &[]).status().is_success());
    checked!("R28", "POST /api/assets",
        Some("/audit/assetowner"), None, None);
    let response = request!(app, Method::GET,
        "/api/assets?path=/audit/assetowner&file=never_record_filename.bin");
    assert_eq!(response.status(), StatusCode::FOUND);
    checked!("R29", "GET /api/assets",
        Some("/audit/assetowner"), None, None);
    let uri = format!("/api/assets/{asset}/data");
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R30", "GET /api/assets/{asset_id}/data",
        Some("/audit/assetowner"), None, None);
    let uri = format!("/api/assets/{asset}/meta");
    assert!(request!(app, Method::GET, &uri).status().is_success());
    checked!("R31", "GET /api/assets/{asset_id}/meta",
        Some("/audit/assetowner"), None, None);
    let uri = format!("/api/assets/{asset}");
    assert!(request!(app, Method::DELETE, &uri).status().is_success());
    checked!("R32", "DELETE /api/assets/{asset_id}",
        Some("/audit/assetowner"), None, None);

    /*
     * 全32欄を記録しsecret/raw補足と余分な公開要求投入を検出する
     */
    assert_eq!(evidence.len(), 32);
    let all = records(&dir, &sink);
    assert_eq!(all.len(), 32);
    let json = serde_json::to_string_pretty(&evidence).unwrap();
    for forbidden in [
        "secret_body_123", "never_record_filename", "second_secret",
        "third_secret", "token=", "expire=", "needle",
    ] {
        assert!(!json.contains(forbidden), "{forbidden}");
    }
    assert!(!json.contains("dXNlcjpwYXNz"));
    if let Some(secret) = secret { assert!(!json.contains(&secret)); }
    let mode = if use_bearer { "bearer" } else { "basic" };
    let output = std::path::Path::new("tmp/ms5-evidence/2.2-2.3");
    std::fs::create_dir_all(output).unwrap();
    std::fs::write(output.join(format!("r32-{mode}.json")), &json).unwrap();
    let raw = std::fs::read(crate::audit::rotation::active_log_path(
        &dir.path().join("audit"),
    )).unwrap();
    std::fs::write(output.join(format!("r32-{mode}.jsonl")), raw).unwrap();
    if !use_bearer {
        std::fs::write("/tmp/ms4-g4-r32-success-records.json", json).unwrap();
    }
}

///
/// 同snapshotのknown read/lock失敗と304の対象を維持する
///
/// # 概要
/// 不在source/不在lock/lock資格情報エラー/親拒否を実JSONLで確認する。
///
/// # 注記
/// unknown IDだけをnullとしraw指定revと資格情報は記録しない。
///
#[actix_web::test]
async fn rest_operation_read_lock_failures_and_cache_keep_known_targets() {
    /*
     * knownページとassetを実DBに準備する
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let page = guard.db().create_page("/known", "user", "source".into())
        .unwrap();
    guard.db().create_page("/parent", "user", "source".into()).unwrap();
    let child = guard.db().create_page(
        "/parent/child", "user", "source".into(),
    ).unwrap();
    let asset = guard.db().create_asset(
        &page, "private_filename.bin", "text/plain", "user", b"private_bytes",
    ).unwrap();
    let (token, _) = guard.db().create_bearer_token(
        "user", crate::database::types::BearerScopeSet::all(),
        crate::database::types::PathPrefixSet::from_iter(["/parent/child"]),
        chrono::Duration::minutes(30), None,
    ).unwrap();
    let authorization = format!("Bearer {}", token.expose());
    drop(guard);
    let app = test::init_service(App::new().app_data(state.clone())
        .service(super::create_api_scope(1024))).await;

    /*
     * sourceとassetの304にも同じ実対象を残す
     */
    for (uri, operation, revision) in [
        (format!("/api/pages/{page}/source"),
            "GET /api/pages/{page_id}/source", Some(1)),
        (format!("/api/assets/{asset}/data"),
            "GET /api/assets/{asset_id}/data", None),
    ] {
        let response = request!(app, Method::GET, &uri);
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers().get(header::ETAG).unwrap()
            .to_str().unwrap().to_string();
        let response = request!(app, Method::GET, &uri, "",
            &[("If-None-Match", etag.as_str())]);
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        check_record(records(&dir, &sink), operation,
            Some("/known"), revision, None);
    }

    /*
     * 未取得source revisionをraw指定値から作らない
     */
    for suffix in ["source", "meta"] {
        let uri = format!("/api/pages/{page}/{suffix}?rev=999");
        let response = request!(app, Method::GET, &uri);
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let operation = format!("GET /api/pages/{{page_id}}/{suffix}");
        check_record(records(&dir, &sink), &operation,
            Some("/known"), None, Some("failure=not_found"));
    }
    let response = request!(app, Method::GET, "/api/pages/badid/source");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    check_record(records(&dir, &sink), "GET /api/pages/{page_id}/source",
        None, None, Some("failure=not_found"));

    /*
     * lock無し404/重複取得409/不正token403でもtargetを失わない
     */
    let uri = format!("/api/pages/{page}/lock");
    assert_eq!(request!(app, Method::GET, &uri).status(),
        StatusCode::NOT_FOUND);
    check_record(records(&dir, &sink), "GET /api/pages/{page_id}/lock",
        Some("/known"), None, Some("failure=not_found"));
    assert!(request!(app, Method::POST, &uri).status().is_success());
    assert_eq!(request!(app, Method::POST, &uri).status(),
        StatusCode::CONFLICT);
    check_record(records(&dir, &sink), "POST /api/pages/{page_id}/lock",
        Some("/known"), None, Some("failure=conflict"));
    for (method, operation) in [
        (Method::PUT, "PUT /api/pages/{page_id}/lock"),
        (Method::DELETE, "DELETE /api/pages/{page_id}/lock"),
    ] {
        let response = request!(app, method, &uri, "",
            &[("X-Lock-Authentication", "private_invalid_token")]);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        check_record(records(&dir, &sink), operation,
            Some("/known"), None, Some("failure=operation_denied"));
    }

    /*
     * 親拒否は指定対象ではなく候補path、HTTP404でもtyped deny
     */
    let uri = format!("/api/pages/{child}/parent");
    let response = request!(app, Method::GET, &uri, "",
        &[("Authorization", authorization.as_str())]);
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let row = check_record(records(&dir, &sink),
        "GET /api/pages/{page_id}/parent", Some("/parent"), None,
        Some("failure=path_prefix_denied denied_side=parent"));
    assert_eq!(row.result, AuditResult::PathPrefixDenied);

    /*
     * 認可前にsourceを読まず指定対象pathだけを記録する
     */
    let uri = format!("/api/pages/{page}/source");
    let response = request!(app, Method::GET, &uri, "",
        &[("Authorization", authorization.as_str())]);
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    check_record(records(&dir, &sink), "GET /api/pages/{page_id}/source",
        Some("/known"), None, Some("failure=path_prefix_denied"));
    let json = serde_json::to_string(&records(&dir, &sink)).unwrap();
    for secret in [token.expose(), "private_bytes", "private_filename",
        "private_invalid_token"]
    {
        assert!(!json.contains(secret));
    }
    save_failure_records("read-lock", records(&dir, &sink));
}

///
/// move拒否側/no-op/recursiveと指定revision失敗を実証する
///
/// # 概要
/// source/destinationを同txnのpolicy判断点で記録し候補Vを除外する。
///
/// # 注記
/// rename/restoreの拒否4種、no-op成功、再帰成功、指定V不正を比較する。
///
#[actix_web::test]
async fn rest_operation_move_sides_noop_and_revision_failures() {
    /*
     * 移動/復元対象と双方片側のみ許可するtokenを用意する
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let db = guard.db();
    let source = db.create_page("/source", "user", "text".into()).unwrap();
    let restore = db.create_page("/restore", "user", "text".into()).unwrap();
    db.delete_page_by_id(&restore).unwrap();
    let recursive = db.create_page("/tree", "user", "text".into()).unwrap();
    db.create_page("/tree/child", "user", "text".into()).unwrap();
    let make_token = |prefix| db.create_bearer_token(
        "user", crate::database::types::BearerScopeSet::all(),
        crate::database::types::PathPrefixSet::from_iter([prefix]),
        chrono::Duration::minutes(30), None,
    ).unwrap().0;
    let only_destination = make_token("/destination");
    let only_source = make_token("/source");
    let only_restore = make_token("/restore");
    drop(guard);
    let root = state.read().unwrap().db().get_page_id_by_path("/")
        .unwrap().unwrap();
    let app = test::init_service(App::new().app_data(state.clone())
        .service(super::create_api_scope(1024))).await;

    /*
     * root拒否でも同txnで既知の指定対象を失わない
     */
    let uri = format!("/api/pages/{root}/path?rename_to=/destination");
    assert_eq!(request!(app, Method::POST, &uri).status(),
        StatusCode::BAD_REQUEST);
    check_record(records(&dir, &sink), "POST /api/pages/{page_id}/path",
        Some("/"), None, Some("failure=invalid_input action=rename"));

    /*
     * raw入力のbasename再利用前後で拒否側の実pathを選ぶ
     */
    for (page, action, token, denied_path, side) in [
        (&source, "rename", &only_destination, "/source", "source"),
        (&source, "rename", &only_source, "/destination", "destination"),
        (&restore, "restore", &only_destination, "/restore", "source"),
        (&restore, "restore", &only_restore, "/destination", "destination"),
    ] {
        let authorization = format!("Bearer {}", token.expose());
        let uri = format!("/api/pages/{page}/path?{action}_to=/destination");
        let response = request!(app, Method::POST, &uri, "",
            &[("Authorization", authorization.as_str())]);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let summary = format!(
            "failure=path_prefix_denied action={action} denied_side={side}",
        );
        check_record(records(&dir, &sink), "POST /api/pages/{page_id}/path",
            Some(denied_path), None, Some(&summary));
    }

    /*
     * no-opは204で既知latest、再帰は起点一件とcommitted V
     */
    let uri = format!("/api/pages/{source}/path?rename_to=/source");
    assert_eq!(request!(app, Method::POST, &uri).status(),
        StatusCode::NO_CONTENT);
    check_record(records(&dir, &sink), "POST /api/pages/{page_id}/path",
        Some("/source"), Some(1), Some("action=rename destination=/source"));
    let before = records(&dir, &sink).len();
    let uri = format!(
        "/api/pages/{recursive}/path?rename_to=/moved&recursive=true",
    );
    assert!(request!(app, Method::POST, &uri).status().is_success());
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), before + 1);
    check_record(rows, "POST /api/pages/{page_id}/path", Some("/tree"),
        Some(2), Some("action=rename destination=/moved"));

    /*
     * invalid指定Vと未確定actionをraw queryから捏造しない
     */
    for (query, action) in [("rollback_to=999", "rollback"),
        ("keep_from=999", "compaction")]
    {
        let uri = format!("/api/pages/{source}/revision?{query}");
        assert_eq!(request!(app, Method::POST, &uri).status(),
            StatusCode::BAD_REQUEST);
        check_record(records(&dir, &sink), "POST /api/pages/{page_id}/revision",
            Some("/source"), None,
            Some(&format!("failure=invalid_input action={action}")));
    }
    let uri = format!("/api/pages/{source}/revision?rollback_to=1&keep_from=1");
    assert_eq!(request!(app, Method::POST, &uri).status(),
        StatusCode::BAD_REQUEST);
    check_record(records(&dir, &sink), "POST /api/pages/{page_id}/revision",
        None, None, Some("failure=invalid_input"));
    save_failure_records("move-revision", records(&dir, &sink));
}

///
/// asset zombie/owner不在/upload失敗とcommit後500を区別する
///
/// # 概要
/// 業務DB/FSの失敗phaseを実JSONLで照合し秘密値を排除する。
///
/// # 注記
/// 登録/commit前注入/回収失敗、既知owner、commit後候補同期失敗を比較する。
///
#[actix_web::test]
async fn rest_operation_asset_and_postcommit_failure_metadata() {
    /*
     * 既知ownerとzombie、失敗対象sourceを隔離DBに作る
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let db = guard.db();
    let page = db.create_page("/owner", "user", "text".into()).unwrap();
    let source = db.create_page("/postcommit", "user", "text".into()).unwrap();
    let zombie = db.create_asset(&page, "zombie", "text/plain",
        "user", b"private_payload").unwrap();
    db.force_update_asset_info_for_test(&zombie, |info| info.clear_page_id())
        .unwrap();
    drop(guard);
    let app = test::init_service(App::new().app_data(state.clone())
        .service(super::create_api_scope(1024))).await;

    /*
     * owner未解決/zombieはNULL、原始IDやfile名をsummaryへ出さない
     */
    for (suffix, operation) in [
        ("/data", "GET /api/assets/{asset_id}/data"),
        ("/meta", "GET /api/assets/{asset_id}/meta"),
    ] {
        let uri = format!("/api/assets/{zombie}{suffix}");
        assert_eq!(request!(app, Method::GET, &uri).status(), StatusCode::GONE);
        check_record(records(&dir, &sink), operation, None, None,
            Some("failure=gone"));
    }
    let uri = format!("/api/assets/{zombie}");
    assert_eq!(request!(app, Method::DELETE, &uri).status(), StatusCode::GONE);
    check_record(records(&dir, &sink), "DELETE /api/assets/{asset_id}",
        None, None, Some("failure=gone"));
    let uri = format!("/api/pages/{page}/assets/missing");
    assert_eq!(request!(app, Method::GET, &uri).status(),
        StatusCode::NOT_FOUND);
    check_record(records(&dir, &sink),
        "GET /api/pages/{page_id}/assets/{file_name}",
        Some("/owner"), None, Some("failure=not_found"));

    /*
     * 登録/commit試行前/回収失敗の注入位置はnative commit Errと区別
     */
    for (fault, phase) in [
        (1, "pre_commit"), (2, "pre_commit"), (3, "cleanup"),
    ] {
        state.read().unwrap().db().set_asset_upload_failure_for_test(fault);
        let uri = format!("/api/pages/{page}/assets/private_file_{fault}");
        let response =
            request!(app, Method::POST, &uri, "private_payload", &[]);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let summary = format!(
            "failure=internal_error phase={phase} committed=false",
        );
        check_record(records(&dir, &sink),
            "POST /api/pages/{page_id}/assets/{file_name}",
            Some("/owner"), None, Some(&summary));
    }
    state.read().unwrap().db().set_asset_upload_failure_for_test(0);

    /*
     * commit後候補同期500でも実commitVを保持する
     */
    state.read().unwrap().db()
        .set_resource_candidate_sync_failure_for_test(true);
    let uri = format!("/api/pages/{source}/source?amend=false");
    let response = request!(app, Method::PUT, &uri, "private_source", &[]);
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    check_record(records(&dir, &sink), "PUT /api/pages/{page_id}/source",
        Some("/postcommit"), Some(2),
        Some(concat!(
            "failure=internal_error amend=false ",
            "phase=post_commit committed=true",
        )));
    state.read().unwrap().db()
        .set_resource_candidate_sync_failure_for_test(false);

    /*
     * source amend成功は既知commitVと確定指定だけ
     */
    let uri = format!("/api/pages/{source}/source?amend=true");
    assert!(request!(app, Method::PUT, &uri, "amended_source", &[])
        .status().is_success());
    check_record(records(&dir, &sink), "PUT /api/pages/{page_id}/source",
        Some("/postcommit"), Some(2), Some("amend=true"));
    let json = serde_json::to_string(&records(&dir, &sink)).unwrap();
    for secret in ["private_payload", "private_file", "private_source",
        "amended_source", dir.path().to_str().unwrap()]
    {
        assert!(!json.contains(secret));
    }
    save_failure_records("asset-commit", records(&dir, &sink));
}

///
/// cleanup後404と実source取得後500のknown対象を失わない
///
/// # 概要
/// same write txnのbefore値をcleanup後の不在/ユーザ不存在へ保持する。
///
/// # 注記
/// meta/lockのexpired draftとauthor欠損metaを実JSONLへ照合する。
///
#[actix_web::test]
async fn rest_operation_cleanup_and_read_failure_keep_before_targets() {
    /*
     * expired draftと他ユーザのsourceを隔離DBに用意する
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let db = guard.db();
    let (meta, _) = db.create_draft_page("/expired/meta", "user").unwrap();
    let (lock, _) = db.create_draft_page("/expired/lock", "user").unwrap();
    db.expire_page_lock_for_test(&meta).unwrap();
    db.expire_page_lock_for_test(&lock).unwrap();
    db.add_user("author", "pass", None).unwrap();
    let source = db.create_page("/missing/author", "author", "text".into())
        .unwrap();
    db.delete_user("author").unwrap();
    let checkpoint = db.create_page("/commit/checkpoint", "user", "text".into())
        .unwrap();
    drop(guard);
    let app = test::init_service(App::new().app_data(state.clone())
        .service(super::create_api_scope(1024))).await;

    /*
     * cleanupがdraft indexを除去してもbefore pathを保持する
     */
    for (id, suffix, target) in [
        (&meta, "meta", "/expired/meta"),
        (&lock, "lock", "/expired/lock"),
    ] {
        let uri = format!("/api/pages/{id}/{suffix}");
        assert_eq!(request!(app, Method::GET, &uri).status(),
            StatusCode::NOT_FOUND);
        check_record(records(&dir, &sink),
            &format!("GET /api/pages/{{page_id}}/{suffix}"),
            Some(target), None, Some("failure=not_found"));
        assert!(state.read().unwrap().db().get_page_index_by_id(id)
            .unwrap().is_none());
    }

    /*
     * author lookupの500でもsource実取得Vは既知情報として残る
     */
    let uri = format!("/api/pages/{source}/meta");
    assert_eq!(request!(app, Method::GET, &uri).status(),
        StatusCode::INTERNAL_SERVER_ERROR);
    check_record(records(&dir, &sink), "GET /api/pages/{page_id}/meta",
        Some("/missing/author"), Some(1), Some("failure=internal_error"));

    /*
     * commit checkpointのErrでもbefore target/source Vを保持する
     */
    state.read().unwrap().db().set_meta_commit_failure_for_test(true);
    let uri = format!("/api/pages/{checkpoint}/meta");
    assert_eq!(request!(app, Method::GET, &uri).status(),
        StatusCode::INTERNAL_SERVER_ERROR);
    check_record(records(&dir, &sink), "GET /api/pages/{page_id}/meta",
        Some("/commit/checkpoint"), Some(1), Some("failure=internal_error"));
    state.read().unwrap().db().set_meta_commit_failure_for_test(false);
    save_failure_records("cleanup-read", records(&dir, &sink));
}
