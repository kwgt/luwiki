/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! HTTPサーバに関する処理を集約するモジュール
//!

#[cfg(test)]
mod startup_audit_tests;
#[cfg(test)]
mod lifecycle_tests;
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
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
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
/// * `key_path` - 分離した秘密鍵ファイルのパス
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
    key_path: Option<PathBuf>,
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
    let audit_sink = initialize_audit_sink(audit_config.as_ref())?;
    let retention = AuditRetentionPolicy::new(
        audit_config.as_ref().unwrap().retention,
    );
    let state = web::Data::new(Arc::new(RwLock::new(AppState::new(
        manager,
        frontend_config,
        fts_config,
        template_root,
        wiki_title,
        wiki_icon,
        asset_limit_size,
        Some(audit_sink.clone()),
    ))));
    let mcp_session_manager =
        mcp_endpoint.as_ref().map(|_| ManagedSessionManager::new());

    let server = match create_server(
        addr,
        port,
        state.clone(),
        asset_limit_size,
        use_tls,
        cert_path,
        cert_is_explicit,
        key_path,
        mcp_endpoint,
        mcp_session_manager.clone(),
    ) {
        Ok(server) => server,
        Err(error) => return complete_shutdown(
            Err(error), None, Ok(()), &audit_sink,
        ),
    };


    /*
     * MCP session sweep task を Tokio runtime 上で起動する
     */
    if let Some(manager) = mcp_session_manager.as_ref() {
        let _guard = rt.enter();
        manager.start_background_sweep();
    }

    #[allow(unused)]
    let has_external_shutdown = shutdown_signal.is_some();

    /*
     * 同期sink保守と既存lock cleanupへ停止通知/JoinHandleを保持する
     */
    let (stop_tx, stop_rx) = watch::channel(false);
    let retention_task = rt.spawn(audit_retention_task(
        audit_sink.clone(), retention, stop_rx.clone(),
    ));
    let cleanup_task = rt.spawn(lock_cleanup_task(state.clone(), stop_rx));
    let signal_task = if let Some(signal) = shutdown_signal {
        let _guard = rt.enter();
        Some(shutdown_signal_hook(server.handle(), signal))
    } else { None };
    let handle = server.handle();

    /*
     * 起動callback失敗は未poll serverとstopを並行に処理する
     */
    let startup_error = on_started.and_then(|callback| callback().err());
    let startup_failed = startup_error.is_some();
    info!("HTTP server start");
    let (server_result, background_result) = rt.block_on(async {
        #[cfg(target_os = "windows")]
        if !has_external_shutdown && !startup_failed {
            windows_event_fook(handle.clone());
        }
        let result = if startup_failed {
            let (result, ()) = tokio::join!(server, handle.stop(true));
            result
        } else {
            server.await
        };

        /*
         * 稼働要求終了後に補助taskを停止しguardなしで完了待ちする
         */
        let _ = stop_tx.send(true);
        let (retention, cleanup) = tokio::join!(retention_task, cleanup_task);

        /*
         * 同期MCP業務/recordをstate barrierで待ちqueued業務を止める
         */
        let gate_failed = match state.write() {
            Ok(mut state) => { state.begin_shutdown(); false }
            Err(_) => true,
        };
        let session_failed = if let Some(manager) = mcp_session_manager {
            manager.shutdown().await.is_err()
        } else { false };
        if let Some(task) = signal_task {
            task.abort();
            let _ = task.await;
        }
        let background = if retention.is_err() || cleanup.is_err()
            || gate_failed || session_failed
        {
            Err(anyhow!("background task completion failed"))
        } else { Ok(()) };
        (result.map_err(anyhow::Error::from), background)
    });

    /*
     * server Errでも保留appendとwriter最終flushを試み終了Resultへ返す
     */
    complete_shutdown(
        server_result, startup_error, background_result, &audit_sink,
    )
}

///
/// 監査保持の固定内部周期（秒）
///
const AUDIT_MAINTENANCE_SECS: u64 = 60;

///
/// 終了時に確認した独立した失敗
///
#[derive(Debug)]
struct ShutdownFailure {
    /// server.await又はbindの元error
    server: Option<anyhow::Error>,
    /// 起動callbackの元error
    startup: Option<anyhow::Error>,
    /// 補助taskの完了error
    background: Option<anyhow::Error>,
    /// 保留append/writer flush/lockのerror
    audit: Option<anyhow::Error>,
}

impl std::fmt::Display for ShutdownFailure {
    ///
    /// 内部FS pathや例外本文を出さず失敗種別を表示する
    ///
    /// # 引数
    /// * `formatter` - 診断出力先
    ///
    /// # 戻り値
    /// 固定診断の書込結果。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>)
        -> std::fmt::Result
    {
        write!(formatter,
            "shutdown failed: server={} startup={} background={} audit={}",
            self.server.is_some(), self.startup.is_some(),
            self.background.is_some(), self.audit.is_some())
    }
}

impl std::error::Error for ShutdownFailure {}

///
/// 稼働要求と補助task終了後に最終flushと結果統合を行う
///
/// # 引数
/// * `server` - 元server/bind結果
/// * `startup` - 起動callback error
/// * `background` - 補助task完了結果
/// * `sink` - 終了後も保持した共通sink参照
///
/// # 戻り値
/// 全成功ならOk、独立した元失敗を保持した終了Err。
///
/// # 概要
/// server/bind失敗でも監査最終flushを試み、成功終了としない。
///
fn complete_shutdown(
    server: Result<()>,
    startup: Option<anyhow::Error>,
    background: Result<()>,
    sink: &Arc<RwLock<AuditSink>>,
) -> Result<()> {
    /*
     * DB/state guardやtask joinを跨がず同期flushを試行する
     */
    let audit = match sink.write() {
        Ok(mut sink) => sink.flush(),
        Err(_) => Err(anyhow!("audit shutdown lock unavailable")),
    };
    let failure = ShutdownFailure {
        server: server.err(), startup,
        background: background.err(), audit: audit.err(),
    };

    /*
     * 独立した元errorを保持し固定診断だけを出す
     */
    if failure.server.is_some() || failure.startup.is_some()
        || failure.background.is_some() || failure.audit.is_some()
    {
        warn!("server shutdown failed");
        Err(anyhow::Error::new(failure))
    } else {
        info!("HTTP server exit");
        Ok(())
    }
}

///
/// 保持処理を同sink lockで一周期だけ実行する
///
/// # 引数
/// * `sink` - 共通writer参照
/// * `policy` - 既存保持設定
///
/// # 戻り値
/// 正常列挙ならtrue、dir/lock障害ならfalse。
///
/// # 概要
/// 個別削除失敗は既存固定診断/継続、周期障害も次周期へ進む。
///
fn audit_maintenance_tick(
    sink: &Arc<RwLock<AuditSink>>,
    policy: &AuditRetentionPolicy,
) -> bool {
    /*
     * 同期lock内で既存plan/executeを直列化する
     */
    match sink.write() {
        Ok(mut sink) => match sink.maintenance(policy, Utc::now()) {
            Ok(_) => true,
            Err(_) => {
                warn!("audit maintenance failed");
                false
            }
        },
        Err(_) => {
            warn!("audit maintenance lock unavailable");
            false
        }
    }
}

///
/// 固定60秒周期で共通監査の保持taskを実行する
///
/// # 引数
/// * `sink` - 共通sink
/// * `policy` - 既存保持設定
/// * `stop` - 明示終了通知
///
/// # 戻り値
/// 通知後に同期処理を完了して返る。
///
async fn audit_retention_task(
    sink: Arc<RwLock<AuditSink>>,
    policy: AuditRetentionPolicy,
    stop: watch::Receiver<bool>,
) {
    audit_retention_loop(
        sink, policy, stop, Duration::from_secs(AUDIT_MAINTENANCE_SECS),
    ).await;
}

///
/// 次周期継続と完了待ちを検証できる内部保持loop
///
/// # 引数
/// * `sink` - 共通sink
/// * `policy` - 既存保持設定
/// * `stop` - 終了通知
/// * `period` - 内部周期。製品呼出しは常に60秒
///
/// # 戻り値
/// 通知後に完了。公開設定は追加しない。
///
/// # 概要
/// lockをawaitへ持ち越さず、失敗でも次周期の待機を継続する。
///
async fn audit_retention_loop(
    sink: Arc<RwLock<AuditSink>>,
    policy: AuditRetentionPolicy,
    mut stop: watch::Receiver<bool>,
    period: Duration,
) {
    /*
     * 起動保持処理と重複せず最初の定期tickを60秒後にする
     */
    let mut interval = time::interval_at(time::Instant::now() + period, period);
    loop {
        tokio::select! {
            biased;
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break; }
            }
            _ = interval.tick() => {
                audit_maintenance_tick(&sink, &policy);
            }
        }
    }
}

///
/// REST/MCP共通sinkを既存設定から一度準備する
///
/// # 引数
/// * `config` - 常に必要な既存監査設定
///
/// # 戻り値
/// 共通Arc、又は固定診断による起動失敗。
///
/// # 概要
/// MCP公開有無に依存せず出力準備・起動保持処理を完了する。
///
fn initialize_audit_sink(
    config: Option<&AuditLogConfig>,
) -> Result<Arc<RwLock<AuditSink>>> {
    /*
     * CLIと同じ既存正値契約を内側の起動入口でも確認する
     */
    let config = config.ok_or_else(|| anyhow!("audit config missing"))?;
    if config.retention <= ChronoDuration::zero() || config.rotate_size == 0 {
        return Err(anyhow!("audit config invalid"));
    }

    /*
     * 初回request以前にdir/open依存失敗を検出する
     */
    let mut writer = AuditWriter::new(AuditWriterConfig {
        output_dir: config.output_dir.clone(),
        rotation_policy: AuditRotationPolicy::new(config.rotate_size),
    });
    writer.prepare()
        .map_err(|_| anyhow!("audit output preparation failed"))?;
    run_audit_retention(config)
        .map_err(|_| anyhow!("audit startup retention failed"))?;

    Ok(Arc::new(RwLock::new(AuditSink::new(
        AppendAuditBuffer::new(), writer,
    ))))
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
/// * `key_path` - 分離した秘密鍵ファイルのパス
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
    key_path: Option<PathBuf>,
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
        let app = match mcp_endpoint.as_ref() {
            Some(endpoint) => app
                .service(crate::mcp::transport::create_scope(
                    endpoint.clone(),
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
        let tls_config = tls::load_server_config(
            &cert_path,
            cert_is_explicit,
            key_path.as_deref(),
        )?;
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
fn shutdown_signal_hook(
    handle: ServerHandle, signal: ShutdownSignal,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _ = signal.await;
        handle.stop(true).await;
    })
}

///
/// ロック期限切れ監視タスク
///
/// # 概要
/// 一定間隔で期限切れロックの削除を実行する。
///
/// # 引数
/// * `state` - アプリケーション状態
/// * `stop` - 明示終了通知
///
/// # 戻り値
/// 通知後に処理を完了して返る。
///
async fn lock_cleanup_task(
    state: web::Data<Arc<RwLock<AppState>>>,
    mut stop: watch::Receiver<bool>,
) {
    /*
     * 監視間隔の初期化
     */
    let mut interval = time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            biased;
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { break; }
                continue;
            }
            _ = interval.tick() => {}
        }

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
        if result.is_err() {
            warn!("lock cleanup failed");
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
