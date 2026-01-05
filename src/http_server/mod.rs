/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! HTTPサーバに関する処理を集約するモジュール
//!

pub(crate) mod app_state;
pub(crate) mod logger;
pub(crate) mod page_view;
pub(crate) mod static_files;

use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Result;
use actix_web::{web, App, HttpResponse, HttpServer};
use actix_web::dev::ServiceResponse;
use actix_web::http::StatusCode;
use actix_web::middleware::{ErrorHandlerResponse, ErrorHandlers};
use actix_web::dev::Server;
use log::{info, warn};
use tokio::runtime::Builder;
use tokio::time;

use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::rest_api;

use self::app_state::AppState;
use self::logger::AccessLogger;

fn payload_too_large_handler<B>(
    res: ServiceResponse<B>,
) -> actix_web::Result<ErrorHandlerResponse<B>> {
    let (req, _) = res.into_parts();
    let body = serde_json::json!({
        "reason": "payload too large",
    });
    let resp = HttpResponse::build(StatusCode::PAYLOAD_TOO_LARGE)
        .content_type("application/json")
        .body(body.to_string())
        .map_into_right_body();
    Ok(ErrorHandlerResponse::Response(
        ServiceResponse::new(req, resp),
    ))
}

pub(crate) fn run(
    addr: String,
    port: u16,
    manager: DatabaseManager,
    frontend_config: FrontendConfig,
) -> Result<()> {
    /*
     * Tokioランタイムの構築
     */
    let rt = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime failed");

    /*
     * サーバインスタンスの生成
     */
    let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
        manager,
        frontend_config,
    ))));
    let server = create_server(addr, port, state.clone())?;

    /*
     * ロック期限切れ監視タスクの起動
     */
    rt.spawn(lock_cleanup_task(state));

    /*
     * Tokioランタイムでのサーバの起動
     */
    info!("HTTP server start");

    match rt.block_on(async {server.await}) {
        Ok(()) => {
            info!("HTTP server exit");
            Ok(())
        }

        Err(err) => {
            info!("HTTP server failed");
            Err(err.into())
        }
    }
}

///
/// HTTPサーバーの生成
///
/// # 引数
/// * `addr` - サーバーをバインドさせるアドレス
/// * `port` - サーバーをバインドさせるポート番号
/// * `manager` - データベースマネージャ
///
fn create_server(
    addr: String,
    port: u16,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<Server> {
    let payload_limit = 10 * 1024 * 1024;
    let server = HttpServer::new(move || {
        App::new()
            // ロガーの設定
            .wrap(AccessLogger::new())
            .wrap(ErrorHandlers::new().handler(
                StatusCode::PAYLOAD_TOO_LARGE,
                payload_too_large_handler,
            ))

            // REST APIエンドポイント設定
            .app_data(state.clone())
            .app_data(web::PayloadConfig::new(payload_limit))
            .service(rest_api::create_api_scope())

            // Wiki閲覧用エンドポイント設定
            .route("/", web::get().to(page_view::get_root_redirect))
            .route("/wiki", web::get().to(page_view::get_root))
            .route("/wiki/{page_path:.*}", web::get().to(page_view::get))
            .route("/edit", web::get().to(page_view::get_edit_root))
            .route("/edit/{page_path:.*}", web::get().to(page_view::get_edit))

            // 静的ファイル配信
            .route("/static/{file:.*}", web::get().to(static_files::get))

            // root空間に展開されるその他のエンドポイント設定
            //.servcie(...)
    })
    .bind(format!("{}:{}", addr, port))?;

    Ok(server.run())
}

///
/// ロック期限切れ監視タスク
///
async fn lock_cleanup_task(state: web::Data<Arc<RwLock<AppState>>>) {
    let mut interval = time::interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        let result = {
            let state = match state.read() {
                Ok(state) => state,
                Err(_) => {
                    warn!("lock cleanup skipped: state lock failed");
                    continue;
                }
            };
            state.db().cleanup_expired_locks()
        };

        if let Err(err) = result {
            warn!("lock cleanup failed: {}", err);
        }
    }
}
