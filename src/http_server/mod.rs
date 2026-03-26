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
pub(crate) mod tls;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use actix_web::dev::Server;
#[cfg(target_os = "windows")]
use actix_web::dev::ServerHandle;
use actix_web::dev::ServiceResponse;
use actix_web::http::StatusCode;
use actix_web::middleware::{ErrorHandlerResponse, ErrorHandlers};
use actix_web::{App, HttpResponse, HttpServer, web};
use anyhow::{Result, anyhow};
use chrono::{Duration as ChronoDuration, Utc};
use log::{info, warn};
use tokio::runtime::Builder;
#[cfg(target_os = "windows")]
use tokio::signal::windows::{ctrl_close, ctrl_logoff, ctrl_shutdown};
use tokio::time;

use crate::audit::buffer::AppendAuditBuffer;
use crate::audit::retention::{
    AuditRetentionPolicy,
    build_retention_plan,
    execute_retention_plan,
};
use crate::audit::rotation::AuditRotationPolicy;
use crate::audit::sink::AuditSink;
use crate::audit::writer::{AuditWriter, AuditWriterConfig};
use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::fts::FtsIndexConfig;
use crate::mcp::McpEndpoint;
use crate::mcp::session_manager::ManagedSessionManager;
use crate::rest_api;

use self::app_state::AppState;
use self::logger::AccessLogger;

///
/// HTTP サーバが利用する監査ログ設定
///
#[derive(Clone, Debug)]
pub(crate) struct AuditLogConfig {
    /// 監査ログ出力先ディレクトリ
    output_dir: PathBuf,

    /// 監査ログ保持期間
    retention: ChronoDuration,

    /// 監査ログローテーション閾値サイズ
    rotate_size: u64,
}

impl AuditLogConfig {
    ///
    /// 監査ログ設定の生成
    ///
    /// # 引数
    /// * `output_dir` - 監査ログ出力先ディレクトリ
    /// * `retention` - 監査ログ保持期間
    /// * `rotate_size` - ローテーション閾値サイズ(バイト)
    ///
    /// # 戻り値
    /// 生成した監査ログ設定を返す。
    ///
    pub(crate) fn new(
        output_dir: PathBuf,
        retention: ChronoDuration,
        rotate_size: u64,
    ) -> Self {
        Self {
            output_dir,
            retention,
            rotate_size,
        }
    }
}

///
/// ペイロード超過時のエラーレスポンスを生成する
///
/// # 引数
/// * `res` - 元のサービスレスポンス
///
/// # 戻り値
/// エラーハンドラのレスポンス
///
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
    Ok(ErrorHandlerResponse::Response(ServiceResponse::new(
        req, resp,
    )))
}

///
/// HTTPサーバを起動する
///
/// # 概要
/// アプリケーション状態を初期化し、
/// サーバと補助タスクを起動する。
///
/// # 引数
/// * `addr` - バインド先アドレス
/// * `port` - バインド先ポート番号
/// * `manager` - データベースマネージャ
/// * `frontend_config` - フロントエンド設定
/// * `fts_config` - 全文検索設定
/// * `template_root` - テンプレートルート
/// * `wiki_title` - Wikiタイトル
/// * `asset_limit_size` - アセット上限サイズ(バイト)
/// * `use_tls` - TLSを使用する場合は`true`
/// * `cert_path` - 証明書ファイルパス
/// * `cert_is_explicit` - 証明書パスが明示指定なら`true`
/// * `audit_config` - 監査ログ設定
/// * `mcp_endpoint` - 公開するMCP endpoint情報
///
/// # 戻り値
/// 起動処理に成功した場合は`Ok(())`
///
pub(crate) fn run(
    addr: String,
    port: u16,
    manager: DatabaseManager,
    frontend_config: FrontendConfig,
    fts_config: FtsIndexConfig,
    template_root: Option<String>,
    wiki_title: String,
    asset_limit_size: u64,
    use_tls: bool,
    cert_path: PathBuf,
    cert_is_explicit: bool,
    audit_config: Option<AuditLogConfig>,
    mcp_endpoint: Option<McpEndpoint>,
) -> Result<()> {
    info!(
        "{} {} start",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    );

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
    let audit_sink = if mcp_endpoint.is_some() {
        let config = audit_config
            .clone()
            .ok_or_else(|| anyhow!("audit config missing for MCP"))?;
        run_audit_retention(&config)?;
        Some(Arc::new(RwLock::new(build_audit_sink(&config))))
    } else {
        None
    };
    let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
        manager,
        frontend_config,
        fts_config,
        template_root,
        wiki_title,
        asset_limit_size,
        audit_sink,
    ))));
    let mcp_session_manager =
        mcp_endpoint.map(|_| ManagedSessionManager::new());

    /*
     * MCP session sweep task を Tokio runtime 上で起動する
     */
    if let Some(manager) = mcp_session_manager.as_ref() {
        let _guard = rt.enter();
        manager.start_background_sweep();
    }

    let server = create_server(
        addr,
        port,
        state.clone(),
        asset_limit_size,
        use_tls,
        cert_path,
        cert_is_explicit,
        mcp_endpoint,
        mcp_session_manager,
    )?;

    /*
     * ロック期限切れ監視タスクの起動
     */
    rt.spawn(lock_cleanup_task(state));

    /*
     * Tokioランタイムでのサーバの起動
     */
    info!("HTTP server start");

    match rt.block_on(async {
        #[cfg(target_os = "windows")]
        windows_event_fook(server.handle());

        server.await
    }) {
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
/// 監査ログ投入入口を設定値から生成する
///
/// # 引数
/// * `config` - 監査ログ設定
///
/// # 戻り値
/// 設定値で初期化した監査ログ投入入口を返す。
///
fn build_audit_sink(config: &AuditLogConfig) -> AuditSink {
    let writer = AuditWriter::new(AuditWriterConfig {
        output_dir: config.output_dir.clone(),
        rotation_policy: AuditRotationPolicy::new(config.rotate_size),
    });

    AuditSink::new(AppendAuditBuffer::new(), writer)
}

///
/// 起動時の保持期間超過ログ削除を実行する
///
/// # 引数
/// * `config` - 監査ログ設定
///
/// # 戻り値
/// 保持削除処理に成功した場合は `Ok(())` を返す。
///
fn run_audit_retention(config: &AuditLogConfig) -> Result<()> {
    /*
     * 出力先がまだ存在しない場合は何もしない
     */
    if !config.output_dir.exists() {
        return Ok(());
    }

    /*
     * 保持削除計画の構築と実行
     */
    let policy = AuditRetentionPolicy::new(config.retention);
    let plan = build_retention_plan(&config.output_dir, &policy, Utc::now())?;
    let _ = execute_retention_plan(plan);

    Ok(())
}

///
/// HTTPサーバーの生成
///
/// # 引数
/// * `addr` - サーバーをバインドさせるアドレス
/// * `port` - サーバーをバインドさせるポート番号
/// * `state` - アプリケーション状態
/// * `asset_limit_size` - アセット上限サイズ(バイト)
/// * `use_tls` - TLSを利用する場合は`true`
/// * `cert_path` - 証明書ファイルパス
/// * `cert_is_explicit` - 証明書パスが明示指定なら`true`
/// * `mcp_endpoint` - 公開するMCP endpoint情報
///
/// # 戻り値
/// 生成したHTTPサーバ
///
fn create_server(
    addr: String,
    port: u16,
    state: web::Data<Arc<RwLock<AppState>>>,
    asset_limit_size: u64,
    use_tls: bool,
    cert_path: PathBuf,
    cert_is_explicit: bool,
    mcp_endpoint: Option<McpEndpoint>,
    mcp_session_manager: Option<Arc<ManagedSessionManager>>,
) -> Result<Server> {
    /*
     * サーバ設定の構築
     */
    let payload_limit = asset_limit_size as usize;
    let server = HttpServer::new(move || {
        let app = App::new()
            // ロガーの設定
            .wrap(AccessLogger::new())
            .wrap(
                ErrorHandlers::new()
                    .handler(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        payload_too_large_handler,
                    ),
            )
            // REST APIエンドポイント設定
            .app_data(state.clone())
            .service(rest_api::create_api_scope(payload_limit))
            // Wiki閲覧用エンドポイント設定
            .route("/", web::get().to(page_view::get_root_redirect))
            .route("/wiki", web::get().to(page_view::get_root))
            .route("/wiki/{page_path:.*}", web::get().to(page_view::get))
            .route("/edit", web::get().to(page_view::get_edit_root))
            .route("/edit/{page_path:.*}", web::get().to(page_view::get_edit))
            .route("/search", web::get().to(page_view::get_search))
            .route("/pages", web::get().to(page_view::get_pages_root))
            .route("/pages/{page_path:.*}", web::get().to(page_view::get_pages))
            .route("/rev", web::get().to(page_view::get_rev_root))
            .route("/rev/{page_path:.*}", web::get().to(page_view::get_rev))
            // 静的ファイル配信
            .route("/static/{file:.*}", web::get().to(static_files::get));

        /*
         * MCP endpoint を必要時のみ登録する
         */
        let app = match mcp_endpoint {
            Some(endpoint) => app
                .service(crate::mcp::transport::create_scope(
                    endpoint,
                    state.clone(),
                    mcp_session_manager
                        .as_ref()
                        .expect("mcp session manager missing")
                        .clone(),
                )),
            None => app,
        };

        app

        // root空間に展開されるその他のエンドポイント設定
        //.servcie(...)
    });

    /*
     * バインド方式の選択
     */
    let bind_addr = format!("{}:{}", addr, port);
    let server = if use_tls {
        let tls_config = tls::load_server_config(&cert_path, cert_is_explicit)?;
        server.bind_rustls_0_23(bind_addr, tls_config)?
    } else {
        server.bind(bind_addr)?
    };

    Ok(server.shutdown_timeout(10).run())
}

///
/// Windows用イベントフック
///
/// # 引数
/// * `handle` - フックを登録するハンドル
///
/// # 戻り値
/// なし
///
#[cfg(target_os = "windows")]
fn windows_event_fook(handle: ServerHandle) {
    let mut close = ctrl_close().expect("failed to install CLOSE handler");
    let mut logoff = ctrl_logoff().expect("failed to install LOGOFF handler");
    let mut shutdown =
        ctrl_shutdown().expect("failed to install SHUTDOWN handler");

    tokio::spawn(async move {
        // どれか来たら終了
        tokio::select! {
            _ = close.recv() => info!("caught CLOSE event"),
            _ = logoff.recv() => info!("caught LOGOFF event"),
            _ = shutdown.recv() => info!("caught SHUTDOWN event"),
        };

        handle.stop(true).await;
    });
}

///
/// ロック期限切れ監視タスク
///
/// # 概要
/// 一定間隔で期限切れロックの削除を実行する。
///
/// # 引数
/// * `state` - アプリケーション状態
///
/// # 戻り値
/// なし
///
async fn lock_cleanup_task(state: web::Data<Arc<RwLock<AppState>>>) {
    /*
     * 監視間隔の初期化
     */
    let mut interval = time::interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        /*
         * 期限切れロックの削除
         */
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

        /*
         * エラーの記録
         */
        if let Err(err) = result {
            warn!("lock cleanup failed: {}", err);
        }
    }
}
