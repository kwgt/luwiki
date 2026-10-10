/*
 * Light weight and small wiki system for local use
 *
 * Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MS5の実認証・scope・属性・期限通知を監査と照合する隔離fixture
//!

use std::fs;
use std::sync::{Arc, RwLock};

use actix_web::http::{Method, header};
use actix_web::{App, test, web};
use chrono::{DateTime, Duration, Local};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

use crate::audit::AuditSink;
use crate::audit::buffer::AppendAuditBuffer;
use crate::audit::model::{AuditRecord, AuditResult};
use crate::audit::rotation::{AuditRotationPolicy, active_log_path};
use crate::audit::writer::{AuditWriter, AuditWriterConfig};
use crate::cmd_args::FrontendConfig;
use crate::database::{
    DatabaseManager, rewrite_bearer_token_timestamps_for_test,
};
use crate::database::types::{
    BearerScope, BearerScopeSet, PathPrefixSet, TokenId,
    UserAttribute, UserAttributeSet,
};
use crate::fts::FtsIndexConfig;
use crate::http_server::app_state::AppState;

/// fixtureだけが所有する共通監査sink
type SharedSink = Arc<RwLock<AuditSink>>;
/// 製品と同じrequest state形式
type SharedState = web::Data<Arc<RwLock<AppState>>>;

///
/// 実DB発行値を保存するfixture token。秘密はartifactへ出さない。
///
struct IssuedToken {
    /// headerにだけ使用する平文
    plaintext: String,
    /// 検証済みIDの独立期待値
    id: TokenId,
    /// 発行時の期限
    expire_at: DateTime<Local>,
}

///
/// 単独scopeと指定prefixで実tokenを発行する
///
/// # 引数
/// * db - 隔離DB
/// * user - fixtureユーザ名
/// * scope - 単独scope
/// * prefixes - 保存するprefix集合
///
/// # 戻り値
/// header平文とDB発行ID/期限。
///
fn issue(
    db: &DatabaseManager, user: &str, scope: BearerScope,
    prefixes: &[&str],
) -> IssuedToken {
    let (token, info) = db.create_bearer_token(
        user, BearerScopeSet::from_iter([scope]),
        PathPrefixSet::from_iter(prefixes.iter().copied()),
        Duration::hours(1), None,
    ).unwrap();
    IssuedToken {
        plaintext: token.expose().to_string(), id: info.token_id(),
        expire_at: info.expire_at(),
    }
}

///
/// config/CLIを介さず隔離DBと共通sinkをAppStateへ渡す
///
/// # 引数
/// * dir - 隔離領域
/// * db - 準備済みDB
///
/// # 戻り値
/// request stateと同じsink Arc。
///
fn state(dir: &TempDir, db: DatabaseManager) -> (SharedState, SharedSink) {
    let sink = Arc::new(RwLock::new(AuditSink::new(
        AppendAuditBuffer::new(), AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().join("audit"),
            rotation_policy: AuditRotationPolicy::new(1024 * 1024),
        }),
    )));
    let state = AppState::new(
        db, FrontendConfig::default(),
        FtsIndexConfig::new(dir.path().join("fts")), None,
        "fixture".into(), None, 1024, Some(sink.clone()),
    );
    (web::Data::new(Arc::new(RwLock::new(state))), sink)
}

///
/// fixture明示flush後の実JSONLを読む
///
/// # 引数
/// * dir - 隔離領域
/// * sink - 共通sink
///
/// # 戻り値
/// 投入済み全record。正常停止flush証拠ではない。
///
fn rows(dir: &TempDir, sink: &SharedSink) -> Vec<AuditRecord> {
    sink.write().unwrap().flush().unwrap();
    let path = active_log_path(&dir.path().join("audit"));
    if !path.exists() { return Vec::new(); }
    fs::read_to_string(path).unwrap().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect()
}

///
/// 秘密非記録を検査して原JSONLとケース結果をworkspaceへ保存する
///
/// # 引数
/// * dir - 隔離領域
/// * sink - 共通sink
/// * label - 固定fixture名
/// * cases - HTTP/DB確認結果（平文を含めない）
/// * secrets - headerにだけ使用した平文など
///
/// # 戻り値
/// なし。保存内容は旧9fieldの実record。
///
/// # 概要
/// 実writer byte列とHTTP/DBケースを秘密非記録確認後に保存する。
///
fn save(
    dir: &TempDir, sink: &SharedSink, label: &str,
    cases: &[Value], secrets: &[&str],
) {
    let records = rows(dir, sink);
    let bytes = fs::read(active_log_path(&dir.path().join("audit"))).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    for secret in secrets { assert!(!text.contains(secret)); }
    assert!(!text.contains("Authorization"));
    assert_eq!(text.lines().count(), records.len());
    let output = std::path::Path::new("tmp/ms5-evidence/2.1");
    fs::create_dir_all(output).unwrap();
    let case_bytes = serde_json::to_vec_pretty(cases).unwrap();
    let case_text = std::str::from_utf8(&case_bytes).unwrap();
    for secret in secrets { assert!(!case_text.contains(secret)); }
    for record in text.lines() {
        let value: Value = serde_json::from_str(record).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 9);
    }
    fs::write(output.join(format!("{label}.jsonl")), bytes).unwrap();
    fs::write(output.join(format!("{label}-cases.json")),
        case_bytes).unwrap();
}

/// 実scope認証/監査境界へ要求を一度渡す
macro_rules! request {
    ($app:expr, $method:expr, $uri:expr, $auth:expr) => {
        test::call_service(&$app,
            test::TestRequest::default().method($method).uri($uri)
                .insert_header((header::AUTHORIZATION, $auth)).to_request(),
        ).await
    };
}

///
/// Basic/no-prefix/root/multi-prefixと実主体/tokenを照合する
///
/// # 概要
/// 同じsource3対象とpath制約除外hello/meを4認証modeで比較する。
///
/// # 注記
/// multiは完全一致/配下だけ許可し似たprefixを403にする。
/// HTTPと実JSONLを発行IDへ比較し、原証拠をworkspaceへ保存する。
///
#[actix_web::test]
async fn ms5_auth_prefix_modes_keep_actor_and_token() {
    /*
     * 実ユーザと3対象、prefix3modeのBearerを準備する
     */
    let dir = tempdir().unwrap();
    let db = DatabaseManager::open(
        dir.path().join("db"), dir.path().join("assets"),
    ).unwrap();
    db.add_user("user", "pass", None).unwrap();
    let actor = db.get_user_info_by_name("user").unwrap().unwrap().id();
    let mut pages = Vec::new();
    let paths = ["/docs/a", "/team/b", "/docs2/outside"];
    for path in paths {
        pages.push(db.create_page(path, "user", "private_ms5_source".into())
            .unwrap());
    }
    let none = issue(&db, "user", BearerScope::Read, &[]);
    let root = issue(&db, "user", BearerScope::Read, &["/"]);
    let multi = issue(&db, "user", BearerScope::Read, &["/docs", "/team"]);
    let (state, sink) = state(&dir, db);
    let app = test::init_service(App::new().app_data(state)
        .service(super::create_api_scope(1024))).await;
    let mut cases = Vec::new();
    let basic = "Basic dXNlcjpwYXNz".to_string();
    let modes = [
        ("basic", basic, None, false),
        ("none", format!("Bearer {}", none.plaintext),
            Some(none.id.clone()), false),
        ("root", format!("Bearer {}", root.plaintext),
            Some(root.id.clone()), false),
        ("multi", format!("Bearer {}", multi.plaintext),
            Some(multi.id.clone()), true),
    ];

    /*
     * 公開sourceと制約除外readを実認証で通し各一行を確認する
     */
    for (mode, auth, token, restricted) in modes {
        for (index, page) in pages.iter().enumerate() {
            let uri = format!("/api/pages/{page}/source");
            let response = request!(app, Method::GET, &uri, auth.as_str());
            let denied = restricted && index == 2;
            assert_eq!(response.status().as_u16(), if denied { 403 } else {
                200
            });
            assert!(!response.headers().contains_key("X-Bearer-Expire"));
            let records = rows(&dir, &sink);
            let record = records.last().unwrap();
            assert_eq!(records.len(), cases.len() + 1);
            assert_eq!(record.user_id, actor);
            assert_eq!(record.token_id, token);
            assert_eq!(record.result, if denied {
                AuditResult::PathPrefixDenied
            } else { AuditResult::Success });
            assert_eq!(record.operation.as_str(),
                "GET /api/pages/{page_id}/source");
            assert_eq!(record.target_path.as_deref(), Some(paths[index]));
            assert_eq!(record.revision, if denied { None } else { Some(1) });
            assert_eq!(record.summary.as_deref(), if denied {
                Some("failure=path_prefix_denied")
            } else { None });
            cases.push(json!({"mode":mode,"source_index":index,
                "status":response.status().as_u16(),"actor":actor,
                "token_id":token,"result":record.result}));
        }
        for uri in ["/api/hello", "/api/users/me"] {
            let response = request!(app, Method::GET, uri, auth.as_str());
            assert_eq!(response.status().as_u16(), 200);
            let records = rows(&dir, &sink);
            let record = records.last().unwrap();
            assert_eq!(records.len(), cases.len() + 1);
            assert_eq!(record.user_id, actor);
            assert_eq!(record.token_id, token);
            assert_eq!(record.result, AuditResult::Success);
            assert_eq!(record.target_path, None);
            assert_eq!(record.revision, None);
            cases.push(json!({"mode":mode,"operation":record.operation,
                "status":200,"actor":actor,"token_id":token}));
        }
    }
    save(&dir, &sink, "prefix", &cases, &[
        &none.plaintext, &root.plaintext, &multi.plaintext,
        "dXNlcjpwYXNz", "private_ms5_source",
    ]);
}

///
/// ReadOnly優先とNoBasicAuth/誤認証の監査除外を比較する
///
/// # 概要
/// Basic/read/write Bearerの更新拒否をtyped resultで確認する。
///
/// # 注記
/// NoBasicのBasicは401/追加ゼロ、Bearerは成功。原JSONLを保存する。
///
#[actix_web::test]
async fn ms5_auth_attributes_separate_authentication_and_authorization() {
    /*
     * ReadOnly/NoBasicの実属性と検証済みtokenを用意する
     */
    let dir = tempdir().unwrap();
    let db = DatabaseManager::open(
        dir.path().join("db"), dir.path().join("assets"),
    ).unwrap();
    db.add_user("user", "pass", None).unwrap();
    db.add_user("nobasic", "pass", None).unwrap();
    db.update_user_with_attributes("user", None, None,
        Some(UserAttributeSet::from_iter([UserAttribute::ReadOnly])))
        .unwrap();
    db.update_user_with_attributes("nobasic", None, None,
        Some(UserAttributeSet::from_iter([UserAttribute::NoBasicAuth])))
        .unwrap();
    let actor = db.get_user_info_by_name("user").unwrap().unwrap().id();
    let other = db.get_user_info_by_name("nobasic").unwrap().unwrap().id();
    let read = issue(&db, "user", BearerScope::Read, &[]);
    let write = issue(&db, "user", BearerScope::Write, &[]);
    let no_basic = issue(&db, "nobasic", BearerScope::Read, &[]);
    let (state, sink) = state(&dir, db);
    let app = test::init_service(App::new().app_data(state)
        .service(super::create_api_scope(1024))).await;
    let mut cases = Vec::new();

    /*
     * readだけのtokenでも書込結果はscopeよりReadOnlyを優先する
     */
    for (mode, auth, token) in [
        ("basic", "Basic dXNlcjpwYXNz".to_string(), None),
        ("read", format!("Bearer {}", read.plaintext), Some(read.id.clone())),
        ("write", format!("Bearer {}", write.plaintext),
            Some(write.id.clone())),
    ] {
        for (method, uri, denied) in [
            (Method::GET, "/api/hello", false),
            (Method::POST, "/api/pages?path=/readonly-new", true),
        ] {
            let response = request!(app, method, uri, auth.as_str());
            assert_eq!(response.status().as_u16(), if denied { 403 } else {
                200
            });
            assert!(!response.headers().contains_key("X-Bearer-Expire"));
            let records = rows(&dir, &sink);
            let record = records.last().unwrap();
            assert_eq!(records.len(), cases.len() + 1);
            assert_eq!(record.user_id, actor);
            assert_eq!(record.token_id, token);
            assert_eq!(record.result, if denied {
                AuditResult::ReadOnlyDenied
            } else { AuditResult::Success });
            cases.push(json!({"mode":mode,"denied":denied,
                "status":response.status().as_u16(),
                "result":record.result,"actor":actor,"token_id":token}));
        }
    }
    let before = rows(&dir, &sink);
    for (case, auth) in [
        ("nobasic-basic", "Basic bm9iYXNpYzpwYXNz"),
        ("wrong-basic", "Basic dXNlcjp3cm9uZw=="),
        ("invalid-bearer", "Bearer invalid"),
    ] {
        let response = request!(app, Method::GET, "/api/hello", auth);
        assert_eq!(response.status().as_u16(), 401);
        assert!(!response.headers().contains_key("X-Bearer-Expire"));
        assert_eq!(rows(&dir, &sink), before);
        cases.push(json!({"mode":case,"status":401,"audit_additions":0}));
    }
    let auth = format!("Bearer {}", no_basic.plaintext);
    let response = request!(app, Method::GET, "/api/hello", auth.as_str());
    assert_eq!(response.status().as_u16(), 200);
    let records = rows(&dir, &sink);
    assert_eq!(records.len(), 7);
    assert_eq!(records[6].user_id, other);
    assert_eq!(records[6].token_id, Some(no_basic.id.clone()));
    assert_eq!(records[6].result, AuditResult::Success);
    cases.push(json!({"mode":"no-basic-bearer","status":200,
        "excluded_auth_failures":3,"actor":other,"token_id":no_basic.id}));
    save(&dir, &sink, "attributes", &cases, &[
        &read.plaintext, &write.plaintext, &no_basic.plaintext,
        "dXNlcjpwYXNz", "bm9iYXNpYzpwYXNz", "dXNlcjp3cm9uZw==",
    ]);
}

///
/// TTL実DB更新と通知/非通知・認可失敗・Basic除外を確認する
///
/// # 概要
/// 時間境界をsleepせず、既存helperで閉じた隔離DBを事前準備する。
///
/// # 注記
/// 成功/403でも延長時だけ通知、expired401は監査追加ゼロ。
///
#[actix_web::test]
async fn ms5_auth_ttl_header_matches_db_and_excludes_basic() {
    /*
     * DBを閉じてから既存test helperで時刻だけを準備する
     */
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("db");
    let assets = dir.path().join("assets");
    let db = DatabaseManager::open(&db_path, &assets).unwrap();
    db.add_user("user", "pass", None).unwrap();
    let actor = db.get_user_info_by_name("user").unwrap().unwrap().id();
    let extend = issue(&db, "user", BearerScope::Read, &[]);
    let steady = issue(&db, "user", BearerScope::Read, &[]);
    let denied = issue(&db, "user", BearerScope::Update, &[]);
    let expired = issue(&db, "user", BearerScope::Read, &[]);
    drop(db);
    let now = Local::now();
    for issued in [&extend, &denied] {
        rewrite_bearer_token_timestamps_for_test(
            &db_path, &assets, &issued.id.to_string(),
            now - Duration::minutes(40), now - Duration::minutes(40),
            now + Duration::minutes(20),
        ).unwrap();
    }
    rewrite_bearer_token_timestamps_for_test(
        &db_path, &assets, &expired.id.to_string(),
        now - Duration::hours(2), now - Duration::hours(2),
        now - Duration::minutes(1),
    ).unwrap();
    let db = DatabaseManager::open(&db_path, &assets).unwrap();
    let (state, sink) = state(&dir, db);
    let app = test::init_service(App::new().app_data(state.clone())
        .service(super::create_api_scope(1024))).await;
    let mut cases = Vec::new();

    /*
     * 実middlewareとDB期限を比較し、同token二回目は延長しない
     */
    for (label, auth, token, status, extended) in [
        ("basic", "Basic dXNlcjpwYXNz".to_string(), None, 200, false),
        ("steady", format!("Bearer {}", steady.plaintext),
            Some(steady.id.clone()), 200, false),
        ("extended", format!("Bearer {}", extend.plaintext),
            Some(extend.id.clone()), 200, true),
        ("same-again", format!("Bearer {}", extend.plaintext),
            Some(extend.id.clone()), 200, false),
        ("denied-extended", format!("Bearer {}", denied.plaintext),
            Some(denied.id.clone()), 403, true),
    ] {
        let before = token.as_ref().map(|id| state.read().unwrap().db()
            .get_bearer_token_info_by_id(id).unwrap().unwrap().expire_at());
        let response = request!(app, Method::GET, "/api/hello", auth.as_str());
        assert_eq!(response.status().as_u16(), status);
        let after = token.as_ref().map(|id| state.read().unwrap().db()
            .get_bearer_token_info_by_id(id).unwrap().unwrap().expire_at());
        let notify = response.headers().get("X-Bearer-Expire")
            .map(|value| value.to_str().unwrap().to_string());
        if extended {
            assert!(after.unwrap() > before.unwrap());
            assert_eq!(notify, Some(after.unwrap()
                .format("%Y-%m-%dT%H:%M:%S").to_string()));
        } else {
            assert_eq!(after, before);
            assert_eq!(notify, None);
        }
        let records = rows(&dir, &sink);
        let record = records.last().unwrap();
        assert_eq!(records.len(), cases.len() + 1);
        assert_eq!(record.user_id, actor);
        assert_eq!(record.token_id, token);
        assert_eq!(record.result, if status == 403 {
            AuditResult::ScopeDenied
        } else { AuditResult::Success });
        cases.push(json!({"case":label,"status":status,
            "token_id":token,"actor":actor,"before":before,
            "after":after,"header":notify,"result":record.result}));
    }
    assert_eq!(state.read().unwrap().db()
        .get_bearer_token_info_by_id(&steady.id).unwrap().unwrap().expire_at(),
        steady.expire_at);
    let before = rows(&dir, &sink);
    let auth = format!("Bearer {}", expired.plaintext);
    let response = request!(app, Method::GET, "/api/hello", auth.as_str());
    assert_eq!(response.status().as_u16(), 401);
    assert!(!response.headers().contains_key("X-Bearer-Expire"));
    assert_eq!(rows(&dir, &sink), before);
    cases.push(json!({"case":"expired","status":401,
        "audit_additions":0,"header":null}));
    save(&dir, &sink, "ttl", &cases, &[
        &extend.plaintext, &steady.plaintext, &denied.plaintext,
        &expired.plaintext, "dXNlcjpwYXNz",
    ]);
}
