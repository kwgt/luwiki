/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! HTTPサーバが共有する状態をまとめたモジュール
//!

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::audit::AuditSink;
use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::fts::FtsIndexConfig;

///
/// 状態要求と監査の完了を停止処理へ通知するlease
///
pub(crate) struct PageStateLease(Arc<AtomicUsize>);

impl Drop for PageStateLease {
    ///
    /// 監査guard破棄後に要求数を減らす
    ///
    /// # 戻り値
    /// なし。
    ///
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

///
/// HTTPサーバの共有状態
///
pub(crate) struct AppState {
    /// データベースマネージャ
    db: DatabaseManager,

    /// frontend設定
    frontend_config: FrontendConfig,

    /// MCPとブラウザで共有する解決済みauthority
    mcp_authority: String,

    /// FTS設定
    fts_config: FtsIndexConfig,

    /// テンプレートルート
    #[allow(unused)]
    template_root: Option<String>,

    /// Wikiタイトル
    wiki_title: String,

    /// Wikiアイコン画像ファイルのパス
    wiki_icon: Option<PathBuf>,

    /// アセットサイズ上限
    asset_limit_size: u64,

    /// 監査ログ投入入口
    audit_sink: Option<Arc<RwLock<AuditSink>>>,

    /// HTTP終了後のqueued MCP処理を拒否する内部印
    shutting_down: bool,

    /// WebSocket待機を直ちに終了させる通知
    page_events_stop: tokio::sync::watch::Sender<bool>,

    /// 接続上限をRAIIで管理する
    pub(crate) page_events_slots: Arc<tokio::sync::Semaphore>,

    /// 非同期状態要求の監査完了までの生存数
    page_state_requests: Arc<AtomicUsize>,
}

impl AppState {
    ///
    /// 共有状態オブジェクトの生成
    ///
    /// # 引数
    /// * `db` - 所有させるデータベースマネージャオブジェクト
    /// * `frontend_config` - frontend設定
    /// * `fts_config` - FTS設定
    /// * `template_root` - テンプレートルート
    /// * `wiki_title` - Wikiタイトル
    /// * `wiki_icon` - Wikiアイコン画像ファイルのパス
    /// * `asset_limit_size` - アセットサイズ上限(バイト単位)
    /// * `audit_sink` - 監査ログ投入入口
    /// * `mcp_authority` - runで検証・解決済みのauthority
    ///
    /// # 戻り値
    /// 生成したオブジェクトを返す。
    ///
    pub(crate) fn new(
        db: DatabaseManager,
        frontend_config: FrontendConfig,
        fts_config: FtsIndexConfig,
        template_root: Option<String>,
        wiki_title: String,
        wiki_icon: Option<PathBuf>,
        asset_limit_size: u64,
        audit_sink: Option<Arc<RwLock<AuditSink>>>,
        mcp_authority: String,
    ) -> Self {
        Self {
            db,
            frontend_config,
            mcp_authority,
            fts_config,
            template_root,
            wiki_title,
            wiki_icon,
            asset_limit_size,
            audit_sink,
            shutting_down: false,
            page_events_stop: tokio::sync::watch::channel(false).0,
            page_events_slots: Arc::new(tokio::sync::Semaphore::new(64)),
            page_state_requests: Arc::new(AtomicUsize::new(0)),
        }
    }

    ///
    /// runで解決したauthorityを返す
    ///
    /// # 戻り値
    /// MCPとブラウザが共有するauthority。
    ///
    pub(crate) fn mcp_authority(&self) -> &str {
        &self.mcp_authority
    }

    ///
    /// exclusive state barrierで終了処理を開始する
    ///
    /// # 戻り値
    /// なし。server終了後だけ呼び、保存値を変更しない。
    ///
    pub(crate) fn begin_shutdown(&mut self) {
        self.shutting_down = true;
        self.page_events_stop.send_replace(true);
    }

    ///
    /// 接続待機へ終了通知を渡す
    ///
    /// # 戻り値
    /// shutdownの購読。
    ///
    pub(crate) fn page_events_shutdown(
        &self,
    ) -> tokio::sync::watch::Receiver<bool> {
        self.page_events_stop.subscribe()
    }

    ///
    /// queued MCPを実行してよいかを取得する
    ///
    /// # 戻り値
    /// 終了処理中ならtrue。
    ///
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    ///
    /// 停止gateと同じread guard内で非同期要求を登録する
    ///
    /// # 戻り値
    /// 受付可能なら監査完了まで保持するlease、停止中ならNone。
    ///
    pub(crate) fn begin_page_state_request(&self) -> Option<PageStateLease> {
        if self.shutting_down {
            return None;
        }
        self.page_state_requests.fetch_add(1, Ordering::SeqCst);
        Some(PageStateLease(self.page_state_requests.clone()))
    }

    ///
    /// 最終flush前に終了待ちする非同期要求数を取得する
    ///
    /// # 戻り値
    /// 監査投入が終わっていない要求数。
    ///
    pub(crate) fn page_state_request_count(&self) -> usize {
        self.page_state_requests.load(Ordering::SeqCst)
    }

    ///
    /// データベースマネージャオブジェクトへのアクセサ
    ///
    /// # 戻り値
    /// データベースマネージャオブジェクトへの参照を返す。
    ///
    pub(crate) fn db<'a>(&'a self) -> &'a DatabaseManager {
        &self.db
    }

    ///
    /// frontend設定へのアクセサ
    ///
    pub(crate) fn frontend_config<'a>(&'a self) -> &'a FrontendConfig {
        &self.frontend_config
    }

    ///
    /// FTS設定へのアクセサ
    ///
    /// # 戻り値
    /// FTS設定への参照を返す。
    ///
    pub(crate) fn fts_config<'a>(&'a self) -> &'a FtsIndexConfig {
        &self.fts_config
    }

    ///
    /// テンプレートルートへのアクセサ
    ///
    /// # 戻り値
    /// テンプレートルートが設定されている場合は参照を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn template_root<'a>(&'a self) -> Option<&'a str> {
        self.template_root.as_deref()
    }

    ///
    /// Wikiタイトルへのアクセサ
    ///
    /// # 戻り値
    /// Wikiタイトルへの参照を返す。
    ///
    pub(crate) fn wiki_title<'a>(&'a self) -> &'a str {
        &self.wiki_title
    }

    ///
    /// Wikiアイコン画像ファイルパスへのアクセサ
    ///
    /// # 戻り値
    /// Wikiアイコン画像ファイルのパスが存在する場合は参照を返す。
    ///
    pub(crate) fn wiki_icon<'a>(&'a self) -> Option<&'a PathBuf> {
        self.wiki_icon.as_ref()
    }

    ///
    /// アセットサイズ上限へのアクセサ
    ///
    /// # 戻り値
    /// アップロード可能なアセットサイズ上限(バイト単位)を返す。
    ///
    pub(crate) fn asset_limit_size(&self) -> u64 {
        self.asset_limit_size
    }

    ///
    /// 監査ログ投入入口へのアクセサ
    ///
    /// # 戻り値
    /// 監査ログ投入入口が存在する場合は共有参照を返す。
    ///
    pub(crate) fn audit_sink(&self) -> Option<Arc<RwLock<AuditSink>>> {
        self.audit_sink.clone()
    }

    ///
    /// データベースマネージャオブジェクトへのアクセサ
    ///
    /// # 戻り値
    /// データベースマネージャオブジェクトへの参照を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn db_mut<'a>(&'a mut self) -> &'a mut DatabaseManager {
        &mut self.db
    }
}
