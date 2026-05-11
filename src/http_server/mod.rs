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
pub(crate) mod wiki_icon;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use actix_web::dev::Server;
use actix_web::dev::ServerHandle;
use actix_web::dev::ServiceResponse;
use actix_web::http::StatusCode;
use actix_web::middleware::{ErrorHandlerResponse, ErrorHandlers};
use actix_web::{App, HttpResponse, HttpServer, web};
use anyhow::{Result, anyhow};
use chrono::{Duration as ChronoDuration, Utc};
use log::{info, warn};
use tokio::runtime::Builder;
use tokio::sync::oneshot;
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

/// 外部停止通知
pub(crate) type ShutdownSignal = oneshot::Receiver<()>;

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
/// * `wiki_icon` - Wikiアイコン画像ファイルのパス
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
    wiki_icon: Option<PathBuf>,
    asset_limit_size: u64,
    use_tls: bool,
    cert_path: PathBuf,
    cert_is_explicit: bool,
    audit_config: Option<AuditLogConfig>,
    mcp_endpoint: Option<McpEndpoint>,
    shutdown_signal: Option<ShutdownSignal>,
    on_started: Option<Arc<dyn Fn() -> Result<()> + Send + Sync>>,
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
        wiki_icon,
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
    let has_external_shutdown = shutdown_signal.is_some();

    /*
     * ロック期限切れ監視タスクの起動
     */
    rt.spawn(lock_cleanup_task(state));

    /*
     * 外部停止通知待ちタスクの起動
     */
    if let Some(signal) = shutdown_signal {
        let _guard = rt.enter();
        shutdown_signal_hook(server.handle(), signal);
    }

    /*
     * 起動完了通知
     */
    if let Some(callback) = on_started {
        callback()?;
    }

    /*
     * Tokioランタイムでのサーバの起動
     */
    info!("HTTP server start");

    match rt.block_on(async {
        #[cfg(target_os = "windows")]
        if !has_external_shutdown {
            windows_event_fook(server.handle());
        }

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
            .route("/w/{short_id}", web::get().to(page_view::get_short))
            .route("/wiki", web::get().to(page_view::get_root))
            .route("/wiki/{page_path:.*}", web::get().to(page_view::get))
            .route("/edit", web::get().to(page_view::get_edit_root))
            .route("/edit/{page_path:.*}", web::get().to(page_view::get_edit))
            .route("/search", web::get().to(page_view::get_search))
            .route("/pages", web::get().to(page_view::get_pages_root))
            .route("/pages/{page_path:.*}", web::get().to(page_view::get_pages))
            .route("/rev", web::get().to(page_view::get_rev_root))
            .route("/rev/{page_path:.*}", web::get().to(page_view::get_rev))
            .route("/wiki-icon", web::get().to(wiki_icon::get))
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
/// 外部停止通知を受け付けるフック
///
/// # 引数
/// * `handle` - 停止対象サーバハンドル
/// * `signal` - 外部停止通知受信口
///
fn shutdown_signal_hook(handle: ServerHandle, signal: ShutdownSignal) {
    tokio::spawn(async move {
        let _ = signal.await;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use actix_web::{http::StatusCode, web};
    use reqwest::Client;
    use tempfile::tempdir;

    use super::create_server;
    use crate::cmd_args::FrontendConfig;
    use crate::database::DatabaseManager;
    use crate::fts::FtsIndexConfig;
    use crate::http_server::app_state::AppState;

    ///
    /// MCP 無効時は `/mcp` が公開されないことを確認する。
    ///
    #[actix_web::test]
    async fn server_without_mcp_does_not_expose_mcp_endpoint() {
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_dir = dir.path().join("assets");
        let index_dir = dir.path().join("fts");
        std::fs::create_dir_all(&asset_dir).expect("create assets dir failed");
        std::fs::create_dir_all(&index_dir).expect("create fts dir failed");

        let manager = DatabaseManager::open(&db_path, &asset_dir)
            .expect("open database failed");
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_dir),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        ))));

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("bind test listener failed");
        let address = listener
            .local_addr()
            .expect("resolve listener address failed");
        drop(listener);

        let server = create_server(
            "127.0.0.1".to_string(),
            address.port(),
            state,
            1024 * 1024,
            false,
            std::path::PathBuf::new(),
            false,
            None,
            None,
        )
        .expect("create server failed");
        let handle = server.handle();
        actix_web::rt::spawn(server);

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client failed");
        let response = client
            .get(format!("http://{}/mcp", address))
            .send()
            .await
            .expect("send mcp request failed");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        std::mem::drop(client);
        std::mem::drop(handle.stop(true));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    ///
    /// `wiki_icon` 未設定時は `/wiki-icon` が 404 を返すことを確認する。
    ///
    #[actix_web::test]
    async fn server_without_wiki_icon_returns_not_found() {
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_dir = dir.path().join("assets");
        let index_dir = dir.path().join("fts");
        fs::create_dir_all(&asset_dir).expect("create assets dir failed");
        fs::create_dir_all(&index_dir).expect("create fts dir failed");

        let manager = DatabaseManager::open(&db_path, &asset_dir)
            .expect("open database failed");
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_dir),
            None,
            "LUWIKI".to_string(),
            None,
            1024 * 1024,
            None,
        ))));

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("bind test listener failed");
        let address = listener
            .local_addr()
            .expect("resolve listener address failed");
        drop(listener);

        let server = create_server(
            "127.0.0.1".to_string(),
            address.port(),
            state,
            1024 * 1024,
            false,
            PathBuf::new(),
            false,
            None,
            None,
        )
        .expect("create server failed");
        let handle = server.handle();
        actix_web::rt::spawn(server);

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client failed");
        let response = client
            .get(format!("http://{}/wiki-icon", address))
            .send()
            .await
            .expect("send wiki icon request failed");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        std::mem::drop(client);
        std::mem::drop(handle.stop(true));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    ///
    /// `wiki_icon` 設定時は `/wiki-icon` が画像を返すことを確認する。
    ///
    #[actix_web::test]
    async fn server_with_wiki_icon_returns_image_data() {
        let dir = tempdir().expect("tempdir failed");
        let db_path = dir.path().join("database.redb");
        let asset_dir = dir.path().join("assets");
        let index_dir = dir.path().join("fts");
        let icon_path = dir.path().join("wiki-icon.png");
        fs::create_dir_all(&asset_dir).expect("create assets dir failed");
        fs::create_dir_all(&index_dir).expect("create fts dir failed");
        let icon_bytes = b"png".to_vec();
        fs::write(&icon_path, &icon_bytes).expect("write icon failed");

        let manager = DatabaseManager::open(&db_path, &asset_dir)
            .expect("open database failed");
        let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
            manager,
            FrontendConfig::default(),
            FtsIndexConfig::new(index_dir),
            None,
            "LUWIKI".to_string(),
            Some(icon_path),
            1024 * 1024,
            None,
        ))));

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("bind test listener failed");
        let address = listener
            .local_addr()
            .expect("resolve listener address failed");
        drop(listener);

        let server = create_server(
            "127.0.0.1".to_string(),
            address.port(),
            state,
            1024 * 1024,
            false,
            PathBuf::new(),
            false,
            None,
            None,
        )
        .expect("create server failed");
        let handle = server.handle();
        actix_web::rt::spawn(server);

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("build reqwest client failed");
        let response = client
            .get(format!("http://{}/wiki-icon", address))
            .send()
            .await
            .expect("send wiki icon request failed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("Content-Type")
                .expect("missing content-type")
                .to_str()
                .expect("content-type decode failed"),
            "image/png"
        );
        assert_eq!(
            response
                .headers()
                .get("Cache-Control")
                .expect("missing cache-control")
                .to_str()
                .expect("cache-control decode failed"),
            "no-store, no-cache"
        );
        let body = response.bytes().await.expect("read body failed");
        assert_eq!(body.as_ref(), icon_bytes.as_slice());

        std::mem::drop(client);
        std::mem::drop(handle.stop(true));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
