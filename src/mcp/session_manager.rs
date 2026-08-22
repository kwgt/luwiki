/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP の stateful session 管理を補強する wrapper を定義するモジュール
//!

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::Stream;
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::streamable_http_server::session::{
    ServerSseMessage,
    SessionId,
    SessionManager,
};
use rmcp::transport::streamable_http_server::session::local::{
    LocalSessionManager,
    LocalSessionManagerError,
};
use tokio::sync::{RwLock, oneshot};
use tokio::task::JoinHandle;

const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(30 * 60);
const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_MAX_SESSIONS: usize = 64;

///
/// session 管理の設定値
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SessionManagerConfig {
    /// idle TTL
    idle_ttl: Duration,

    /// sweep 実行間隔
    sweep_interval: Duration,

    /// Heartbeat 実行間隔
    heartbeat_interval: Duration,

    /// 同時保持する最大 session 数
    max_sessions: usize,
}

impl Default for SessionManagerConfig {
    ///
    /// 既定設定の生成
    ///
    /// # 戻り値
    /// 既定値で初期化した設定を返す。
    ///
    fn default() -> Self {
        Self {
            idle_ttl: DEFAULT_IDLE_TTL,
            sweep_interval: DEFAULT_SWEEP_INTERVAL,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            max_sessions: DEFAULT_MAX_SESSIONS,
        }
    }
}

impl SessionManagerConfig {
    ///
    /// session 管理設定の生成
    ///
    /// # 引数
    /// * `idle_ttl` - idle TTL
    /// * `sweep_interval` - sweep 実行間隔
    /// * `max_sessions` - 同時保持する最大 session 数
    ///
    /// # 戻り値
    /// 生成した設定を返す。
    ///
    pub(crate) fn new(
        idle_ttl: Duration,
        sweep_interval: Duration,
        max_sessions: usize,
    ) -> Self {
        Self {
            idle_ttl,
            sweep_interval,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            max_sessions,
        }
    }

    ///
    /// Heartbeat 実行間隔を指定して session 管理設定を生成する
    ///
    /// # 引数
    /// * `idle_ttl` - idle TTL
    /// * `sweep_interval` - sweep 実行間隔
    /// * `heartbeat_interval` - Heartbeat 実行間隔
    /// * `max_sessions` - 同時保持する最大 session 数
    ///
    /// # 戻り値
    /// 生成した設定を返す。
    ///
    #[cfg(test)]
    pub(crate) fn new_with_heartbeat_interval(
        idle_ttl: Duration,
        sweep_interval: Duration,
        heartbeat_interval: Duration,
        max_sessions: usize,
    ) -> Self {
        Self {
            idle_ttl,
            sweep_interval,
            heartbeat_interval,
            max_sessions,
        }
    }
}

///
/// session metadata
///
#[derive(Clone, Copy, Debug)]
struct SessionMeta {
    /// session 生成時刻
    created_at: Instant,

    /// 最終アクセス時刻
    last_access_at: Instant,

    /// close 中フラグ
    closing: bool,
}

impl SessionMeta {
    ///
    /// metadata の生成
    ///
    /// # 引数
    /// * `now` - 現在時刻
    ///
    /// # 戻り値
    /// 生成した metadata を返す。
    ///
    fn new(now: Instant) -> Self {
        Self {
            created_at: now,
            last_access_at: now,
            closing: false,
        }
    }
}

type SharedSessionMetadata = Arc<RwLock<HashMap<SessionId, SessionMeta>>>;

///
/// Heartbeat task の停止を管理する guard
///
struct HeartbeatGuard {
    /// 停止通知 sender
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,

    /// Heartbeat task
    task: Mutex<Option<JoinHandle<()>>>,
}

impl HeartbeatGuard {
    ///
    /// guard を生成する
    ///
    /// # 引数
    /// * `stop_tx` - 停止通知 sender
    /// * `task` - Heartbeat task
    ///
    /// # 戻り値
    /// 生成した guard を返す。
    ///
    fn new(stop_tx: oneshot::Sender<()>, task: JoinHandle<()>) -> Self {
        Self {
            stop_tx: Mutex::new(Some(stop_tx)),
            task: Mutex::new(Some(task)),
        }
    }
}

impl Drop for HeartbeatGuard {
    ///
    /// Heartbeat task へ停止を通知する
    ///
    /// # 戻り値
    /// なし
    ///
    fn drop(&mut self) {
        if let Some(stop_tx) = self
            .stop_tx
            .lock()
            .expect("heartbeat stop sender mutex poisoned")
            .take()
        {
            let _ = stop_tx.send(());
        }
        if let Some(task) = self
            .task
            .lock()
            .expect("heartbeat task mutex poisoned")
            .take()
        {
            task.abort();
        }
    }
}

///
/// Heartbeat 付き SSE stream
///
struct HeartbeatStream<S> {
    /// 元 stream
    inner: Pin<Box<S>>,

    /// Heartbeat task guard
    _heartbeat: HeartbeatGuard,
}

impl<S> HeartbeatStream<S>
where
    S: Stream<Item = ServerSseMessage> + Send + Sync + 'static,
{
    ///
    /// Heartbeat 付き stream を生成する
    ///
    /// # 引数
    /// * `stream` - 元 stream
    /// * `metadata` - session metadata
    /// * `id` - session ID
    /// * `interval` - Heartbeat 実行間隔
    ///
    /// # 戻り値
    /// 生成した stream を返す。
    ///
    fn new(
        stream: S,
        metadata: SharedSessionMetadata,
        id: SessionId,
        interval: Duration,
    ) -> Self {
        Self {
            inner: Box::pin(stream),
            _heartbeat: spawn_session_heartbeat(metadata, id, interval),
        }
    }
}

impl<S> Stream for HeartbeatStream<S>
where
    S: Stream<Item = ServerSseMessage> + Send + Sync + 'static,
{
    type Item = ServerSseMessage;

    ///
    /// 元 stream の poll を透過する
    ///
    /// # 引数
    /// * `self` - stream
    /// * `cx` - task context
    ///
    /// # 戻り値
    /// 元 stream の poll 結果を返す。
    ///
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.inner.as_mut().poll_next(cx)
    }
}

///
/// session Heartbeat task を起動する
///
/// # 引数
/// * `metadata` - session metadata
/// * `id` - session ID
/// * `interval` - Heartbeat 実行間隔
///
/// # 戻り値
/// Heartbeat task guard を返す。
///
fn spawn_session_heartbeat(
    metadata: SharedSessionMetadata,
    id: SessionId,
    interval: Duration,
) -> HeartbeatGuard {
    let interval = if interval.is_zero() {
        Duration::from_millis(1)
    } else {
        interval
    };
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);

        /*
         * 初回 tick を消費して以後の周期実行へ入る
         */
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if !heartbeat_touch_session(
                        &metadata,
                        &id,
                        Instant::now(),
                        interval,
                    ).await {
                        break;
                    }
                }
                _ = &mut stop_rx => {
                    break;
                }
            }
        }
    });

    HeartbeatGuard::new(stop_tx, task)
}

///
/// Heartbeat tick により session の最終アクセス時刻を更新する
///
/// # 引数
/// * `metadata` - session metadata
/// * `id` - session ID
/// * `now` - 現在時刻
/// * `min_interval` - 最小更新間隔
///
/// # 戻り値
/// Heartbeat を継続できる場合は `true` を返す。
///
async fn heartbeat_touch_session(
    metadata: &RwLock<HashMap<SessionId, SessionMeta>>,
    id: &SessionId,
    now: Instant,
    min_interval: Duration,
) -> bool {
    let mut metadata = metadata.write().await;
    match metadata.get_mut(id) {
        Some(meta) if meta.closing => false,
        Some(meta) => {
            if now.duration_since(meta.last_access_at) >= min_interval {
                meta.last_access_at = now;
            }
            true
        }
        None => false,
    }
}

///
/// MCP session 管理失敗
///
#[derive(Debug)]
pub(crate) enum ManagedSessionManagerError {
    Backend(LocalSessionManagerError),
}

impl std::fmt::Display for ManagedSessionManagerError {
    ///
    /// エラー内容を文字列化する
    ///
    /// # 引数
    /// * `f` - フォーマッタ
    ///
    /// # 戻り値
    /// 整形結果を返す。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(error) => {
                write!(f, "session manager backend error: {error}")
            }
        }
    }
}

impl std::error::Error for ManagedSessionManagerError {}

impl From<LocalSessionManagerError> for ManagedSessionManagerError {
    ///
    /// backend error を wrapper error へ変換する
    ///
    /// # 引数
    /// * `error` - backend error
    ///
    /// # 戻り値
    /// 変換後の error を返す。
    ///
    fn from(error: LocalSessionManagerError) -> Self {
        Self::Backend(error)
    }
}

///
/// `LocalSessionManager` を補強する session manager
///
pub(crate) struct ManagedSessionManager {
    /// RMCP 標準の local session manager
    inner: LocalSessionManager,

    /// session metadata
    metadata: SharedSessionMetadata,

    /// 管理設定
    config: SessionManagerConfig,

    /// background sweep task
    sweep_task: Mutex<Option<JoinHandle<()>>>,
}

impl ManagedSessionManager {
    ///
    /// 既定設定で manager を生成する
    ///
    /// # 戻り値
    /// 共有可能な manager を返す。
    ///
    pub(crate) fn new() -> Arc<Self> {
        Self::new_with_config(SessionManagerConfig::default())
    }

    ///
    /// 指定設定で manager を生成する
    ///
    /// # 引数
    /// * `config` - session 管理設定
    ///
    /// # 戻り値
    /// 共有可能な manager を返す。
    ///
    pub(crate) fn new_with_config(
        config: SessionManagerConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: LocalSessionManager::default(),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            config,
            sweep_task: Mutex::new(None),
        })
    }

    ///
    /// background sweep task を起動する
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn start_background_sweep(self: &Arc<Self>) {
        /*
         * 二重起動を避ける
         */
        {
            let guard = self
                .sweep_task
                .lock()
                .expect("session sweep task mutex poisoned");
            if guard.is_some() {
                return;
            }
        }

        let weak = Arc::downgrade(self);
        let interval = self.config.sweep_interval;
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            /*
             * 初回 tick を消費して以後の周期実行へ入る
             */
            ticker.tick().await;
            loop {
                ticker.tick().await;

                /*
                 * manager が生存している間だけ sweep する
                 */
                let Some(manager) = weak.upgrade() else {
                    break;
                };
                manager.sweep_expired_sessions().await;
            }
        });

        let mut guard = self
            .sweep_task
            .lock()
            .expect("session sweep task mutex poisoned");
        *guard = Some(handle);
    }

    ///
    /// session metadata を登録する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `now` - 現在時刻
    ///
    /// # 戻り値
    /// なし
    ///
    async fn insert_metadata(&self, id: SessionId, now: Instant) {
        let mut metadata = self.metadata.write().await;
        metadata.insert(id, SessionMeta::new(now));
    }

    ///
    /// session の最終アクセス時刻を更新する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `now` - 現在時刻
    ///
    /// # 戻り値
    /// なし
    ///
    async fn touch_session(&self, id: &SessionId, now: Instant) {
        let mut metadata = self.metadata.write().await;
        if let Some(meta) = metadata.get_mut(id) {
            if !meta.closing {
                meta.last_access_at = now;
            }
        }
    }

    ///
    /// SSE stream に Heartbeat を付与する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `stream` - 元 stream
    ///
    /// # 戻り値
    /// Heartbeat 付き stream を返す。
    ///
    fn with_heartbeat<S>(
        &self,
        id: &SessionId,
        stream: S,
    ) -> HeartbeatStream<S>
    where
        S: Stream<Item = ServerSseMessage> + Send + Sync + 'static,
    {
        HeartbeatStream::new(
            stream,
            self.metadata.clone(),
            id.clone(),
            self.config.heartbeat_interval,
        )
    }

    ///
    /// session close 開始を記録する
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// close を続行すべき場合は `true` を返す。
    ///
    async fn begin_close(&self, id: &SessionId) -> bool {
        let mut metadata = self.metadata.write().await;
        match metadata.get_mut(id) {
            Some(meta) if meta.closing => false,
            Some(meta) => {
                meta.closing = true;
                true
            }
            None => false,
        }
    }

    ///
    /// session metadata を削除する
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// なし
    ///
    async fn remove_metadata(&self, id: &SessionId) {
        let mut metadata = self.metadata.write().await;
        metadata.remove(id);
    }

    ///
    /// close 対象の session を実際に閉じる
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// close 結果を返す。
    ///
    async fn close_tracked_session(
        &self,
        id: &SessionId,
    ) -> Result<(), ManagedSessionManagerError> {
        if !self.begin_close(id).await {
            return Ok(());
        }

        /*
         * backend close 実行後に metadata を破棄する
         */
        let close_result = self.inner.close_session(id).await;
        self.remove_metadata(id).await;
        close_result.map_err(ManagedSessionManagerError::from)
    }

    ///
    /// idle TTL を超過した session 一覧を取得する
    ///
    /// # 引数
    /// * `now` - 判定時刻
    ///
    /// # 戻り値
    /// close 対象の session ID 一覧を返す。
    ///
    async fn collect_expired_sessions(&self, now: Instant) -> Vec<SessionId> {
        let metadata = self.metadata.read().await;
        metadata
            .iter()
            .filter_map(|(id, meta)| {
                if meta.closing {
                    return None;
                }
                if now.duration_since(meta.last_access_at) >= self.config.idle_ttl
                {
                    return Some(id.clone());
                }
                None
            })
            .collect()
    }

    ///
    /// LRU eviction 対象 session を選択する
    ///
    /// # 戻り値
    /// eviction 対象があれば session ID を返す。
    ///
    async fn select_lru_session(&self) -> Option<SessionId> {
        let metadata = self.metadata.read().await;
        metadata
            .iter()
            .filter(|(_, meta)| !meta.closing)
            .min_by_key(|(_, meta)| (meta.last_access_at, meta.created_at))
            .map(|(id, _)| id.clone())
    }

    ///
    /// 保持上限を超えないよう session を整理する
    ///
    /// # 戻り値
    /// 整理に成功した場合は `Ok(())` を返す。
    ///
    async fn evict_lru_if_needed(
        &self,
    ) -> Result<(), ManagedSessionManagerError> {
        loop {
            let should_evict = {
                let metadata = self.metadata.read().await;
                metadata.values().filter(|meta| !meta.closing).count()
                    >= self.config.max_sessions
            };

            if !should_evict {
                return Ok(());
            }

            let Some(session_id) = self.select_lru_session().await else {
                return Ok(());
            };
            self.close_tracked_session(&session_id).await?;
        }
    }

    ///
    /// TTL 超過 session を sweep する
    ///
    /// # 戻り値
    /// なし
    ///
    async fn sweep_expired_sessions(&self) {
        let expired = self.collect_expired_sessions(Instant::now()).await;
        for session_id in expired {
            let _ = self.close_tracked_session(&session_id).await;
        }
    }

    #[cfg(test)]
    ///
    /// metadata 件数を取得する
    ///
    /// # 戻り値
    /// metadata 件数を返す。
    ///
    async fn metadata_len(&self) -> usize {
        self.metadata.read().await.len()
    }

    #[cfg(test)]
    ///
    /// 指定 session の metadata を取得する
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// metadata があれば返す。
    ///
    async fn metadata_of(&self, id: &SessionId) -> Option<SessionMeta> {
        self.metadata.read().await.get(id).copied()
    }

    #[cfg(test)]
    ///
    /// 指定 session の最終アクセス時刻を直接上書きする
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `at` - 上書き後の時刻
    ///
    /// # 戻り値
    /// なし
    ///
    async fn overwrite_last_access_at(&self, id: &SessionId, at: Instant) {
        let mut metadata = self.metadata.write().await;
        if let Some(meta) = metadata.get_mut(id) {
            meta.last_access_at = at;
        }
    }
}

impl Drop for ManagedSessionManager {
    ///
    /// background task を停止する
    ///
    /// # 戻り値
    /// なし
    ///
    fn drop(&mut self) {
        let mut guard = self
            .sweep_task
            .lock()
            .expect("session sweep task mutex poisoned");
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }
}

impl SessionManager for ManagedSessionManager {
    type Error = ManagedSessionManagerError;
    type Transport = <LocalSessionManager as SessionManager>::Transport;

    ///
    /// session を生成する
    ///
    /// # 戻り値
    /// 生成した session ID と transport を返す。
    ///
    async fn create_session(
        &self,
    ) -> Result<(SessionId, Self::Transport), Self::Error> {
        /*
         * 期限切れ sweep と上限制御を先に実施する
         */
        self.sweep_expired_sessions().await;
        self.evict_lru_if_needed().await?;

        /*
         * backend session 生成後に metadata を登録する
         */
        let now = Instant::now();
        let (session_id, transport) = self.inner.create_session().await?;
        self.insert_metadata(session_id.clone(), now).await;
        Ok((session_id, transport))
    }

    ///
    /// initialize request を session へ転送する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `message` - initialize request
    ///
    /// # 戻り値
    /// initialize 応答を返す。
    ///
    async fn initialize_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, Self::Error> {
        let response = self.inner.initialize_session(id, message).await?;
        self.touch_session(id, Instant::now()).await;
        Ok(response)
    }

    ///
    /// session の存在を確認する
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// 利用可能な session が存在すれば `true` を返す。
    ///
    async fn has_session(
        &self,
        id: &SessionId,
    ) -> Result<bool, Self::Error> {
        let metadata = self.metadata.read().await;
        Ok(matches!(metadata.get(id), Some(meta) if !meta.closing))
    }

    ///
    /// session を close する
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// close 結果を返す。
    ///
    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        self.close_tracked_session(id).await
    }

    ///
    /// request 単位の SSE stream を生成する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `message` - 転送する request
    ///
    /// # 戻り値
    /// 生成した SSE stream を返す。
    ///
    async fn create_stream(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<
        impl Stream<Item = ServerSseMessage> + Send + Sync + 'static,
        Self::Error,
    > {
        let stream = self.inner.create_stream(id, message).await?;
        self.touch_session(id, Instant::now()).await;
        Ok(self.with_heartbeat(id, stream))
    }

    ///
    /// client message を受理する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `message` - client message
    ///
    /// # 戻り値
    /// 受理結果を返す。
    ///
    async fn accept_message(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        self.inner.accept_message(id, message).await?;
        self.touch_session(id, Instant::now()).await;
        Ok(())
    }

    ///
    /// standalone SSE stream を生成する
    ///
    /// # 引数
    /// * `id` - session ID
    ///
    /// # 戻り値
    /// 生成した SSE stream を返す。
    ///
    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<
        impl Stream<Item = ServerSseMessage> + Send + Sync + 'static,
        Self::Error,
    > {
        let stream = self.inner.create_standalone_stream(id).await?;
        self.touch_session(id, Instant::now()).await;
        Ok(self.with_heartbeat(id, stream))
    }

    ///
    /// SSE stream を再開する
    ///
    /// # 引数
    /// * `id` - session ID
    /// * `last_event_id` - 再開基点の event ID
    ///
    /// # 戻り値
    /// 再開した SSE stream を返す。
    ///
    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<
        impl Stream<Item = ServerSseMessage> + Send + Sync + 'static,
        Self::Error,
    > {
        let stream = self.inner.resume(id, last_event_id).await?;
        self.touch_session(id, Instant::now()).await;
        Ok(self.with_heartbeat(id, stream))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use futures::stream;
    use rmcp::transport::streamable_http_server::session::SessionManager;

    use super::{
        HeartbeatStream,
        ManagedSessionManager,
        ServerSseMessage,
        SessionManagerConfig,
    };

    ///
    /// session 作成時に metadata が生成されることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::create_session_registers_metadata`
    /// で実行する。
    ///
    #[tokio::test]
    async fn create_session_registers_metadata() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new(
                Duration::from_secs(300),
                Duration::from_secs(300),
                4,
            ),
        );

        let (session_id, _transport) = manager
            .create_session()
            .await
            .expect("create session failed");
        assert_eq!(manager.metadata_len().await, 1);
        assert!(manager.metadata_of(&session_id).await.is_some());
    }

    ///
    /// `has_session()` だけでは最終アクセス時刻が更新されないことを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::has_session_does_not_touch_last_access`
    /// で実行する。
    ///
    #[tokio::test]
    async fn has_session_does_not_touch_last_access() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new(
                Duration::from_secs(300),
                Duration::from_secs(300),
                4,
            ),
        );
        let (session_id, _transport) = manager
            .create_session()
            .await
            .expect("create session failed");
        let old_time = Instant::now() - Duration::from_secs(120);
        manager
            .overwrite_last_access_at(&session_id, old_time)
            .await;

        assert!(
            manager
                .has_session(&session_id)
                .await
                .expect("has session failed")
        );
        let meta = manager
            .metadata_of(&session_id)
            .await
            .expect("metadata missing");
        assert_eq!(meta.last_access_at, old_time);
    }

    ///
    /// active stream 生存中に Heartbeat が
    /// 最終アクセス時刻を更新することを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::active_stream_heartbeat_touches_last_access`
    /// で実行する。
    ///
    #[tokio::test]
    async fn active_stream_heartbeat_touches_last_access() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_millis(20),
                4,
            ),
        );
        let (session_id, _transport) = manager
            .create_session()
            .await
            .expect("create session failed");
        let stream = HeartbeatStream::new(
            stream::pending::<ServerSseMessage>(),
            manager.metadata.clone(),
            session_id.clone(),
            Duration::from_millis(20),
        );
        let old_time = Instant::now() - Duration::from_secs(120);
        manager
            .overwrite_last_access_at(&session_id, old_time)
            .await;

        tokio::time::sleep(Duration::from_millis(70)).await;

        let meta = manager
            .metadata_of(&session_id)
            .await
            .expect("metadata missing");
        assert!(meta.last_access_at > old_time);
        drop(stream);
    }

    ///
    /// stream drop 後に Heartbeat が停止することを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::heartbeat_stops_after_stream_drop`
    /// で実行する。
    ///
    #[tokio::test]
    async fn heartbeat_stops_after_stream_drop() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_millis(20),
                4,
            ),
        );
        let (session_id, _transport) = manager
            .create_session()
            .await
            .expect("create session failed");
        let stream = HeartbeatStream::new(
            stream::pending::<ServerSseMessage>(),
            manager.metadata.clone(),
            session_id.clone(),
            Duration::from_millis(20),
        );

        drop(stream);
        let frozen_time = Instant::now() - Duration::from_secs(120);
        manager
            .overwrite_last_access_at(&session_id, frozen_time)
            .await;

        tokio::time::sleep(Duration::from_millis(70)).await;

        let meta = manager
            .metadata_of(&session_id)
            .await
            .expect("metadata missing");
        assert_eq!(meta.last_access_at, frozen_time);
    }

    ///
    /// close 中 session は Heartbeat で更新されないことを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::heartbeat_does_not_touch_closing_session`
    /// で実行する。
    ///
    #[tokio::test]
    async fn heartbeat_does_not_touch_closing_session() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_millis(20),
                4,
            ),
        );
        let (session_id, _) = manager
            .create_session()
            .await
            .expect("create session failed");
        let stream = HeartbeatStream::new(
            stream::pending::<ServerSseMessage>(),
            manager.metadata.clone(),
            session_id.clone(),
            Duration::from_millis(20),
        );
        let old_time = Instant::now() - Duration::from_secs(120);
        manager
            .overwrite_last_access_at(&session_id, old_time)
            .await;

        assert!(manager.begin_close(&session_id).await);
        tokio::time::sleep(Duration::from_millis(70)).await;

        let meta = manager
            .metadata_of(&session_id)
            .await
            .expect("metadata missing");
        assert_eq!(meta.last_access_at, old_time);
        drop(stream);
        manager.remove_metadata(&session_id).await;
    }

    ///
    /// metadata 削除済み session への Heartbeat が
    /// session を復活させないことを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::heartbeat_does_not_restore_removed_metadata`
    /// で実行する。
    ///
    #[tokio::test]
    async fn heartbeat_does_not_restore_removed_metadata() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_millis(20),
                4,
            ),
        );
        let (session_id, _) = manager
            .create_session()
            .await
            .expect("create session failed");
        let stream = HeartbeatStream::new(
            stream::pending::<ServerSseMessage>(),
            manager.metadata.clone(),
            session_id.clone(),
            Duration::from_millis(20),
        );

        manager.remove_metadata(&session_id).await;
        tokio::time::sleep(Duration::from_millis(70)).await;

        assert!(manager.metadata_of(&session_id).await.is_none());
        drop(stream);
    }

    ///
    /// TTL 超過 session が sweep で回収されることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::sweep_closes_expired_session`
    /// で実行する。
    ///
    #[tokio::test]
    async fn sweep_closes_expired_session() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new(
                Duration::from_millis(50),
                Duration::from_secs(300),
                4,
            ),
        );
        let (session_id, _) = manager
            .create_session()
            .await
            .expect("create session failed");
        let old_time = Instant::now() - Duration::from_secs(1);
        manager
            .overwrite_last_access_at(&session_id, old_time)
            .await;

        manager.sweep_expired_sessions().await;

        assert!(
            !manager
                .has_session(&session_id)
                .await
                .expect("has session failed")
        );
        assert!(manager.metadata_of(&session_id).await.is_none());
    }

    ///
    /// session 上限超過時に LRU が eviction されることを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::create_session_evicts_lru`
    /// で実行する。
    ///
    #[tokio::test]
    async fn create_session_evicts_lru() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new(
                Duration::from_secs(300),
                Duration::from_secs(300),
                1,
            ),
        );
        let (first_session_id, _) = manager
            .create_session()
            .await
            .expect("create first session failed");
        manager
            .overwrite_last_access_at(
                &first_session_id,
                Instant::now() - Duration::from_secs(10),
            )
            .await;

        let (second_session_id, _) = manager
            .create_session()
            .await
            .expect("create second session failed");

        assert_ne!(first_session_id, second_session_id);
        assert!(
            !manager
                .has_session(&first_session_id)
                .await
                .expect("has first session failed")
        );
        assert!(
            manager
                .has_session(&second_session_id)
                .await
                .expect("has second session failed")
        );
    }

    ///
    /// active stream 中でも session 上限超過時に
    /// LRU eviction が機能することを確認する。
    ///
    /// # 注記
    /// `cargo test mcp::session_manager::tests::active_stream_does_not_prevent_lru_eviction`
    /// で実行する。
    ///
    #[tokio::test]
    async fn active_stream_does_not_prevent_lru_eviction() {
        let manager = ManagedSessionManager::new_with_config(
            SessionManagerConfig::new_with_heartbeat_interval(
                Duration::from_secs(300),
                Duration::from_secs(300),
                Duration::from_millis(20),
                1,
            ),
        );
        let (first_session_id, _) = manager
            .create_session()
            .await
            .expect("create first session failed");
        let stream = HeartbeatStream::new(
            stream::pending::<ServerSseMessage>(),
            manager.metadata.clone(),
            first_session_id.clone(),
            Duration::from_millis(20),
        );

        let (second_session_id, _) = manager
            .create_session()
            .await
            .expect("create second session failed");

        assert_ne!(first_session_id, second_session_id);
        assert!(
            !manager
                .has_session(&first_session_id)
                .await
                .expect("has first session failed")
        );
        assert!(manager.metadata_of(&first_session_id).await.is_none());
        assert!(
            manager
                .has_session(&second_session_id)
                .await
                .expect("has second session failed")
        );

        tokio::time::sleep(Duration::from_millis(70)).await;

        assert!(manager.metadata_of(&first_session_id).await.is_none());
        drop(stream);
    }
}
