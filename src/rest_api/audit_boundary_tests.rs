/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! REST監査境界と既存HTTP契約を隔離fixtureで検証する
//!

use std::fs;
use std::sync::{Arc, Mutex, RwLock};

use actix_web::body::BoxBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::{Method, StatusCode, header};
use actix_web::middleware::{Next, from_fn};
use actix_web::{App, Error, HttpMessage, HttpRequest, HttpResponse, test, web};
use chrono::Local;
use tempfile::{TempDir, tempdir};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

use super::audit::{self, REST_RESOURCES};
use crate::audit::AuditSink;
use crate::audit::buffer::AppendAuditBuffer;
use crate::audit::model::{AuditOperation, AuditRecord, AuditResult};
use crate::audit::rotation::{AuditRotationPolicy, active_log_path};
use crate::audit::writer::{AuditWriter, AuditWriterConfig};
use crate::auth::{AuthContext, AuthUser};
use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::database::types::{
    BearerScopeSet, PathPrefixSet, UserAttribute, UserAttributeSet, UserId,
};
use crate::fts::FtsIndexConfig;
use crate::http_server::app_state::AppState;

/// 既存AppStateと共有する隔離監査sink
type SharedSink = Arc<RwLock<AuditSink>>;
/// 既存AppStateの共有形式
type SharedState = web::Data<Arc<RwLock<AppState>>>;

///
/// DB/assets/FTS/監査出力と実認証ユーザを隔離する
///
/// # 戻り値
/// 一時領域、既存AppState、共有sink。
///
/// # 概要
/// lifecycleを変更せずテストのAppStateへsinkを明示注入する。
///
pub(super) fn fixture() -> (TempDir, SharedState, SharedSink) {
    /*
     * 運用設定を使用せず全出力を同じ一時領域へ限定する
     */
    let dir = tempdir().unwrap();
    let db = DatabaseManager::open(
        dir.path().join("db"), dir.path().join("assets"),
    ).unwrap();
    db.add_user("user", "pass", None).unwrap();
    db.ensure_default_root("user").unwrap();
    let sink = Arc::new(RwLock::new(AuditSink::new(
        AppendAuditBuffer::new(),
        AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().join("audit"),
            rotation_policy: AuditRotationPolicy::new(1024 * 1024),
        }),
    )));

    /*
     * 製品と同じArc参照をfixture状態へ渡す
     */
    let state = AppState::new(
        db, FrontendConfig::default(),
        FtsIndexConfig::new(dir.path().join("fts")),
        None, "test".into(), None, 1024, Some(sink.clone()),
    );
    (dir, web::Data::new(Arc::new(RwLock::new(state))), sink)
}

///
/// 明示flush後のJSONLレコードを読む
///
/// # 引数
/// * `dir` - 隔離領域
/// * `sink` - 共有sink
///
/// # 戻り値
/// 永続化済みレコード。未投入なら空。
///
pub(super) fn records(dir: &TempDir, sink: &SharedSink) -> Vec<AuditRecord> {
    sink.write().unwrap().flush().unwrap();
    let path = active_log_path(&dir.path().join("audit"));
    if !path.exists() { return Vec::new(); }
    fs::read_to_string(path).unwrap().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect()
}

///
/// Basicで全登録methodを呼び固定30操作と一要求一投入を確認する
///
/// # 概要
/// handler結果を再解析せずresource境界の実JSONL件数を比較する。
///
/// # 注記
/// 全descriptorを登録済みscope経由で呼びactorとoperationを照合する。
///
#[actix_web::test]
async fn rest_boundary_covers_all_registered_methods_once() {
    /*
     * 実Basic認証と既存公開routingを同じAppで実行する
     */
    let (dir, state, sink) = fixture();
    let actor = state.read().unwrap().db()
        .get_user_info_by_name("user").unwrap().unwrap().id();
    let app = test::init_service(
        App::new().app_data(state.clone())
            .service(super::create_api_scope(1024)),
    ).await;
    let mut expected = Vec::new();

    /*
     * 固定method/routeを実要求へ使い、未解決IDはhandlerへ任せる
     */
    for operation in crate::audit::model::RestAuditOperation::ALL {
        let (method, route) = operation.as_str().split_once(' ').unwrap();
        let uri = route.replace("{page_id}", "badid")
            .replace("{asset_id}", "badid").replace("{file_name}", "file");
        let response = test::call_service(&app,
            test::TestRequest::default()
                .method(Method::from_bytes(method.as_bytes()).unwrap())
                .uri(&uri)
                .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
                .to_request(),
        ).await;
        expected.push((operation, response.status()));
    }

    /*
     * 全要求につき一行、固定actor/operationだけを出力する
     */
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 30);
    for (record, (operation, status)) in rows.iter().zip(expected) {
        assert_eq!(record.operation, AuditOperation::Rest(operation));
        assert_eq!(record.user_id, actor);
        assert_eq!(record.token_id, None);
        assert_eq!(record.result, audit::status_result(status));
    }
}

///
/// 認証失敗・未登録route・未対応methodを監査へ入れない
///
/// # 概要
/// 旧.routeの404と旧明示resourceの405、動的fallbackを区別する。
///
/// # 注記
/// scope/resource guardと同resource複数methodのHTTP互換を確認する。
///
#[actix_web::test]
async fn rest_boundary_preserves_auth_and_method_routing() {
    /*
     * 既存認証challengeと固定resource guardを実行する
     */
    let (dir, state, sink) = fixture();
    let app = test::init_service(
        App::new().app_data(state)
            .service(super::create_api_scope(1024)),
    ).await;
    let unauthorized = test::call_service(&app,
        test::TestRequest::get().uri("/api/hello").to_request(),
    ).await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert!(unauthorized.headers().contains_key(header::WWW_AUTHENTICATE));

    /*
     * 旧.routeはmethod guard不一致で404、旧resourceは405を維持する
     */
    for (uri, method, expected) in [
        ("/api/unknown", Method::GET, 404),
        ("/api/hello", Method::PATCH, 404),
        ("/api/assets", Method::PATCH, 405),
        ("/api/pages/id", Method::PUT, 404),
    ] {
        let response = test::call_service(&app,
            test::TestRequest::default().method(method).uri(uri)
                .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
                .to_request(),
        ).await;
        assert_eq!(response.status().as_u16(), expected);
        if expected == 405 {
            assert!(response.headers().contains_key(header::ALLOW));
        }
    }
    assert!(records(&dir, &sink).is_empty());

    /*
     * 静的routeのguard不一致でも動的DELETE fallbackを維持する
     */
    let response = test::call_service(&app,
        test::TestRequest::delete().uri("/api/pages/id")
            .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
            .to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].operation.as_str(), "DELETE /api/pages/{page_id}");
}

///
/// 実Bearerのscope/ReadOnly拒否を一般403から区別する
///
/// # 概要
/// 新request guardの型付き補足をscope認証から境界まで通す。
///
/// # 注記
/// scope不一致とReadOnlyの2要求でID/token/resultを照合する。
///
#[actix_web::test]
async fn rest_boundary_records_typed_scope_and_readonly_denials() {
    /*
     * 認証済みBearer主体と異なる拒否理由を準備する
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let (token, info) = guard.db().create_bearer_token(
        "user", BearerScopeSet::from_iter([
            crate::database::types::BearerScope::Update,
        ]), PathPrefixSet::new(), chrono::Duration::minutes(30), None,
    ).unwrap();
    drop(guard);
    let app = test::init_service(
        App::new().app_data(state.clone())
            .service(super::create_api_scope(1024)),
    ).await;
    let response = test::call_service(&app,
        test::TestRequest::get().uri("/api/hello")
            .insert_header((
                header::AUTHORIZATION, format!("Bearer {}", token.expose()),
            ))
            .to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    /*
     * 同じtokenでも認証時に取得したReadOnly属性を優先する
     */
    state.read().unwrap().db().update_user_with_attributes(
        "user", None, None,
        Some(UserAttributeSet::from_iter([UserAttribute::ReadOnly])),
    ).unwrap();
    let response = test::call_service(&app,
        test::TestRequest::post().uri("/api/pages?path=/new")
            .insert_header((
                header::AUTHORIZATION, format!("Bearer {}", token.expose()),
            ))
            .to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].result, AuditResult::ScopeDenied);
    assert_eq!(rows[1].result, AuditResult::ReadOnlyDenied);
    assert!(rows.iter().all(|row| row.token_id == Some(info.token_id())));
}

///
/// extractor以前のpayload失敗とhandler入力失敗を一度記録する
///
/// # 概要
/// 本文の先読みをせずBytes extractorが返す413を境界で捕捉する。
///
/// # 注記
/// handler未実行の413と入力400の実JSONLを照合する。
///
#[actix_web::test]
async fn rest_boundary_records_payload_and_input_failures() {
    /*
     * 既存upload resourceのpayload設定をそのまま使う
     */
    let (dir, state, sink) = fixture();
    let app = test::init_service(
        App::new().app_data(state)
            .service(super::create_api_scope(4)),
    ).await;
    let response = test::call_service(&app,
        test::TestRequest::post().uri("/api/assets?path=/")
            .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
            .set_payload("private_body_exceeds_limit").to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response = test::call_service(&app,
        test::TestRequest::post().uri("/api/pages")
            .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
            .to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].result, AuditResult::InvalidInput);
    assert_eq!(rows[0].summary.as_deref(), Some("failure=payload_too_large"));
    assert_eq!(rows[1].result, AuditResult::InvalidInput);
    let json = fs::read_to_string(
        active_log_path(&dir.path().join("audit")),
    ).unwrap();
    assert!(!json.contains("private_body"));
    assert!(!json.contains("dXNlcjpwYXNz"));
}

///
/// fixture専用の検証済み主体とexpiry補足を設定する
///
/// # 引数
/// * `req` - fixture要求
/// * `next` - 後続service
///
/// # 戻り値
/// 後続応答。実認証は別fixtureで検証する。
///
async fn fixture_actor(
    req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    req.extensions_mut().insert(AuthContext::new_with_actor(
        AuthUser::new("fixture".into()), BearerScopeSet::all(),
        PathPrefixSet::new(), UserAttributeSet::new(),
        None, None, UserId::new(),
    ));
    req.extensions_mut().insert(
        super::auth::BearerExpireHeaderValue::new(Local::now()),
    );
    next.call(req).await
}

///
/// fixture内で後続serviceから直接Errを返す
///
/// # 引数
/// * `req` - fixture要求
/// * `next` - 未使用の後続service
///
/// # 戻り値
/// ResponseError由来の固定403。
///
async fn next_error(
    _req: ServiceRequest,
    _next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    Err(actix_web::error::ErrorForbidden("private_error_body"))
}

///
/// Next Errを一度記録し元のerror/body/expiry契約を保つ
///
/// # 概要
/// inner serviceが直接Errになる経路を外側expiryまで通す。
///
/// # 注記
/// 最終403はOperationDenied、expiryなし、秘密はJSONLへ出ない。
///
#[actix_web::test]
async fn rest_boundary_records_next_error_and_preserves_expiry_skip() {
    /*
     * outer actor/expiry→boundary→Next Errの順序を構成する
     */
    let (dir, state, sink) = fixture();
    let app = test::init_service(
        App::new().app_data(state)
            .wrap(from_fn(super::auth::append_bearer_expire_header))
            .wrap(from_fn(fixture_actor))
            .service(web::resource("/hello")
                .wrap(from_fn(next_error))
                .wrap(from_fn(|req, next| {
                    audit::request_boundary(req, next, REST_RESOURCES[0])
                }))
                .route(web::get().to(|| async { HttpResponse::Ok() })),
            ),
    ).await;
    let response = test::call_service(&app,
        test::TestRequest::get().uri("/hello").to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.response().error().is_some());
    assert!(!response.headers().contains_key(
        super::auth::X_BEARER_EXPIRE_HEADER,
    ));
    assert_eq!(test::read_body(response).await, "private_error_body");
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].result, AuditResult::OperationDenied);
    assert_eq!(rows[0].summary.as_deref(), Some("failure=operation_denied"));
}

///
/// fixtureのstatusを型付き補足付きで返す
///
/// # 引数
/// * `req` - 要求
/// * `status` - fixture status
///
/// # 戻り値
/// 指定statusの応答。
///
async fn status_handler(
    req: HttpRequest, status: web::Path<u16>,
) -> HttpResponse {
    let status = StatusCode::from_u16(status.into_inner()).unwrap();
    if status == StatusCode::NOT_FOUND {
        let denied = crate::database::PathPrefixDenied {
            path: "/private_parent".into(), parent_candidate: true,
        };
        return super::auth::page_access_error_for_request(
            &req, &anyhow::Error::new(denied),
        ).unwrap();
    }
    HttpResponse::build(status).insert_header(("X-Fixture", "kept")).finish()
}

///
/// 302/304/415/親404を同じ境界で分類する
///
/// # 概要
/// body再読取やhandler直接投入なしで公開statusとtyped denyを統合する。
///
/// # 注記
/// 各応答を一度記録し通常expiry/headerと親404の分類を確認する。
///
#[actix_web::test]
async fn rest_boundary_records_redirect_cache_and_parent_failure() {
    /*
     * fixed parent descriptorのfixture serviceを構成する
     */
    let (dir, state, sink) = fixture();
    let parent = REST_RESOURCES.iter()
        .find(|descriptor| descriptor.route.ends_with("/parent"))
        .copied().unwrap();
    let app = test::init_service(
        App::new().app_data(state)
            .wrap(from_fn(super::auth::append_bearer_expire_header))
            .wrap(from_fn(fixture_actor))
            .service(web::resource("/{status}")
                .wrap(from_fn(move |req, next| {
                    audit::request_boundary(req, next, parent)
                }))
                .route(web::get().to(status_handler)),
            ),
    ).await;

    /*
     * 302/304成功と415/親404失敗を通常応答経路で分類する
     */
    for status in [302, 304, 415, 404] {
        let response = test::call_service(&app,
            test::TestRequest::get().uri(&format!("/{status}")).to_request(),
        ).await;
        assert_eq!(response.status().as_u16(), status);
        assert!(response.headers().contains_key(
            super::auth::X_BEARER_EXPIRE_HEADER,
        ));
        if status != 404 {
            assert_eq!(response.headers().get("X-Fixture").unwrap(), "kept");
        }
    }
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].result, AuditResult::Success);
    assert_eq!(rows[1].result, AuditResult::Success);
    assert_eq!(rows[2].result, AuditResult::InvalidInput);
    assert_eq!(rows[3].result, AuditResult::PathPrefixDenied);
    assert_eq!(rows[3].summary.as_deref(),
        Some("failure=path_prefix_denied denied_side=parent"));
}

///
/// sink書込失敗とlock poisonでもHTTP成功を保つ
///
/// # 概要
/// 元のHTTP応答が監査失敗によって変更されないことを実証する。
///
/// # 注記
/// 出力dirをfileへ置換し、別fixtureではsink lockだけをpoisonする。
///
#[actix_web::test]
async fn rest_boundary_sink_failures_preserve_business_response() {
    /*
     * 書込先を通常fileにして業務と独立した監査障害を注入する
     */
    for poison in [false, true] {
        let (dir, state, sink) = fixture();
        let subscriber = WarningSubscriber::default();
        let captured = subscriber.clone();
        let _dispatch = tracing::subscriber::set_default(subscriber);
        if poison {
            let cloned = sink.clone();
            let _ = std::thread::spawn(move || {
                let _guard = cloned.write().unwrap();
                panic!("fixture sink poison");
            }).join();
        } else {
            fs::write(dir.path().join("audit"), "blocked").unwrap();
        }

        /*
         * Basic成功応答と既存cache headerをそのまま返す
         */
        let app = test::init_service(
            App::new().app_data(state)
                .service(super::create_api_scope(1024)),
        ).await;
        let response = test::call_service(&app,
            test::TestRequest::get().uri("/api/hello")
                .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
                .to_request(),
        ).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store");
        assert_eq!(test::read_body(response).await, "hello");
        let text = captured.0.lock().unwrap().join("\n");
        assert_eq!(text.matches("REST audit record failed").count(), 1);
        assert!(!text.contains(&dir.path().display().to_string()));
        assert!(!text.contains("blocked"));
    }
}

///
/// 既存tracingだけでfixture内の固定warningを収集するsubscriber
///
#[derive(Clone, Default)]
struct WarningSubscriber(
    /// 収集した固定診断field
    Arc<Mutex<Vec<String>>>,
);

impl Subscriber for WarningSubscriber {
    ///
    /// warning eventを有効にする
    ///
    /// # 引数
    /// * `metadata` - event情報
    ///
    /// # 戻り値
    /// warningならtrue。
    ///
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }

    ///
    /// fixtureのspan IDを生成する
    ///
    /// # 引数
    /// * `span` - 未使用のspan属性
    ///
    /// # 戻り値
    /// fixture固定ID。
    ///
    fn new_span(&self, _span: &Attributes<'_>) -> Id { Id::from_u64(1) }

    ///
    /// 未使用span値を受け取る
    ///
    /// # 引数
    /// * `span` - 未使用ID
    /// * `values` - 未使用値
    ///
    /// # 戻り値
    /// なし。
    ///
    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    ///
    /// 未使用span依存を受け取る
    ///
    /// # 引数
    /// * `span` - 未使用ID
    /// * `follows` - 未使用依存ID
    ///
    /// # 戻り値
    /// なし。
    ///
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    ///
    /// eventの固定fieldをメモリへ収集する
    ///
    /// # 引数
    /// * `event` - 診断event
    ///
    /// # 戻り値
    /// なし。
    ///
    fn event(&self, event: &Event<'_>) {
        let mut message = String::new();
        event.record(&mut WarningVisitor(&mut message));
        self.0.lock().unwrap().push(message);
    }

    ///
    /// 未使用span開始を受け取る
    ///
    /// # 引数
    /// * `span` - 未使用ID
    ///
    /// # 戻り値
    /// なし。
    ///
    fn enter(&self, _span: &Id) {}

    ///
    /// 未使用span終了を受け取る
    ///
    /// # 引数
    /// * `span` - 未使用ID
    ///
    /// # 戻り値
    /// なし。
    ///
    fn exit(&self, _span: &Id) {}
}

///
/// event fieldを固定warningの検証文字列へ取り出すvisitor
///
struct WarningVisitor<'a>(
    /// fixtureの診断出力先
    &'a mut String,
);

impl Visit for WarningVisitor<'_> {
    ///
    /// fieldの診断表現を収集する
    ///
    /// # 引数
    /// * `field` - eventの固定field
    /// * `value` - 診断値
    ///
    /// # 戻り値
    /// なし。
    ///
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!("{}={value:?};", field.name()));
    }
}

///
/// fixtureの旧constructorを実認証済みactorと推測しない
///
/// # 引数
/// * `req` - fixture要求
/// * `next` - 後続service
///
/// # 戻り値
/// 後続応答。
///
async fn legacy_actor(
    req: ServiceRequest,
    next: Next<BoxBody>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    req.extensions_mut().insert(AuthContext::new(
        AuthUser::new("user".into()), BearerScopeSet::all(),
        PathPrefixSet::new(), None,
    ));
    next.call(req).await
}

///
/// sink未設定と未検証actorを記録成功と扱わない
///
/// # 概要
/// G5前の既存状態と互換constructor要求のHTTP応答を維持する。
///
/// # 注記
/// handler成功でも監査fileや推測actorレコードを生成しない。
///
#[actix_web::test]
async fn rest_boundary_missing_sink_and_actor_do_not_fabricate_records() {
    /*
     * 互換constructorは不変IDを持たないので投入しない
     */
    let (dir, state, sink) = fixture();
    let app = test::init_service(
        App::new().app_data(state)
            .wrap(from_fn(legacy_actor))
            .service(web::resource("/hello")
                .wrap(from_fn(|req, next| {
                    audit::request_boundary(req, next, REST_RESOURCES[0])
                }))
                .route(web::get().to(|| async { HttpResponse::Ok() })),
            ),
    ).await;
    let response = test::call_service(&app,
        test::TestRequest::get().uri("/hello").to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(records(&dir, &sink).is_empty());

    /*
     * 既存fixtureのsink=Noneを変更せずHTTP結果だけ維持する
     */
    let (base, state) = super::authorization_tests::fixture();
    let app = test::init_service(
        App::new().app_data(web::Data::new(Arc::new(RwLock::new(state))))
            .service(super::create_api_scope(1024)),
    ).await;
    let response = test::call_service(&app,
        test::TestRequest::get().uri("/api/hello")
            .insert_header((header::AUTHORIZATION, "Basic dXNlcjpwYXNz"))
            .to_request(),
    ).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!base.join("audit").exists());
    drop(app);
    fs::remove_dir_all(base).unwrap();
}

///
/// 実Basic/Bearer主体と一投入・認証失敗除外をJSONLで照合する
///
/// # 概要
/// 同じユーザの成功2件とscope/lock拒否を共通sinkへ記録する。
///
/// # 注記
/// 不変actor、Basic null、検証済みtoken ID、秘密非記録を確認し、
/// CP4用artifactを隔離fixtureの実JSONLから保存する。
///
#[actix_web::test]
async fn rest_cross_auth_jsonl_keeps_actor_and_excludes_auth_failures() {
    /*
     * 実ユーザIDとread専用Bearerを既存認証入口へ渡す
     */
    let (dir, state, sink) = fixture();
    let guard = state.read().unwrap();
    let actor = guard.db().get_user_info_by_name("user")
        .unwrap().unwrap().id();
    let (token, info) = guard.db().create_bearer_token(
        "user", BearerScopeSet::from_iter([
            crate::database::types::BearerScope::Read,
        ]), PathPrefixSet::new(), chrono::Duration::minutes(30), None,
    ).unwrap();
    drop(guard);
    let locked = state.read().unwrap().db()
        .create_page("/cross/locked", "user", "source".into()).unwrap();
    state.read().unwrap().db().acquire_page_lock(&locked, "user").unwrap();
    let locked_uri = format!("/api/pages/{locked}/path?rename_to=/cross/new");
    let bearer = format!("Bearer {}", token.expose());
    let app = test::init_service(App::new().app_data(state)
        .service(super::create_api_scope(1024))).await;

    /*
     * 成功/認可拒否は各一件、認証失敗は追加ゼロを確認する
     */
    for (authorization, method, uri, status) in [
        ("Basic dXNlcjpwYXNz", Method::GET, "/api/hello", 200),
        (bearer.as_str(), Method::GET, "/api/hello", 200),
        (bearer.as_str(), Method::POST, "/api/pages?path=/new", 403),
        ("Basic dXNlcjpwYXNz", Method::POST, locked_uri.as_str(), 423),
    ] {
        let response = test::call_service(&app,
            test::TestRequest::default().method(method).uri(uri)
                .insert_header((header::AUTHORIZATION, authorization))
                .to_request(),
        ).await;
        assert_eq!(response.status().as_u16(), status);
    }
    let rows = records(&dir, &sink);
    assert_eq!(rows.len(), 4);
    for (authorization, status) in [
        ("Basic dXNlcjp3cm9uZw==", 401), ("Bearer invalid", 401),
    ] {
        let response = test::call_service(&app,
            test::TestRequest::get().uri("/api/hello")
                .insert_header((header::AUTHORIZATION, authorization))
                .to_request(),
        ).await;
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(records(&dir, &sink), rows);
    }

    /*
     * 業務の実認証値と固定監査fieldを独立に比較する
     */
    assert!(rows.iter().all(|row| row.user_id == actor));
    assert_eq!(rows[0].token_id, None);
    assert_eq!(rows[1].token_id, Some(info.token_id()));
    assert_eq!(rows[2].token_id, Some(info.token_id()));
    assert_eq!(rows[0].result, AuditResult::Success);
    assert_eq!(rows[1].result, AuditResult::Success);
    assert_eq!(rows[2].result, AuditResult::ScopeDenied);
    assert_eq!(rows[0].operation.as_str(), "GET /api/hello");
    assert_eq!(rows[1].operation, rows[0].operation);
    assert_eq!(rows[2].operation.as_str(), "POST /api/pages");
    assert_eq!(rows[2].summary.as_deref(), Some("failure=scope_denied"));
    assert_eq!(rows[3].result, AuditResult::Locked);
    assert_eq!(rows[3].target_path.as_deref(), Some("/cross/locked"));
    assert_eq!(rows[3].revision, None);
    assert_eq!(rows[3].summary.as_deref(),
        Some("failure=locked action=rename"));
    assert_eq!(rows[3].token_id, None);
    let bytes = fs::read(active_log_path(&dir.path().join("audit"))).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    for secret in [token.expose(), "dXNlcjpwYXNz", "Authorization", "pass"] {
        assert!(!text.contains(secret));
    }
    assert_eq!(text.lines().count(), 4);
    fs::write("/tmp/ms4-g6-cross-auth.jsonl", bytes).unwrap();
}
