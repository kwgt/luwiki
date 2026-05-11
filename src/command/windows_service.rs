/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! Windowsサービス実行に関する処理
//!

use std::ffi::OsString;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Result, anyhow};
use log::error;
use tokio::sync::oneshot;
use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl,
    ServiceControlAccept,
    ServiceExitCode,
    ServiceState,
    ServiceStatus,
    ServiceType,
};
use windows_service::service_control_handler::{
    self,
    ServiceControlHandlerResult,
    ServiceStatusHandle,
};
use windows_service::service_dispatcher;

use super::run::RunCommandContext;

const SERVICE_NAME: &str = env!("CARGO_PKG_NAME");
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const START_WAIT_HINT: Duration = Duration::from_secs(10);
const STOP_WAIT_HINT: Duration = Duration::from_secs(10);

static RUN_CONTEXT: OnceLock<RunCommandContext> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

///
/// Windowsサービスとして実行する
///
/// # 引数
/// * `context` - runコマンド実行コンテキスト
///
/// # 戻り値
/// サービス起動に成功した場合は`Ok(())`を返す。
///
pub(crate) fn run(context: RunCommandContext) -> Result<()> {
    RUN_CONTEXT
        .set(context)
        .map_err(|_| anyhow!("windows service context already initialized"))?;

    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

///
/// Windowsサービスのメイン処理
///
/// # 引数
/// * `_args` - SCMから渡された引数
///
fn service_main(_args: Vec<OsString>) {
    if let Err(err) = run_service() {
        error!("windows service failed: {}", err);
    }
}

///
/// Windowsサービス本体を実行する
///
/// # 戻り値
/// サービス実行に成功した場合は`Ok(())`を返す。
///
fn run_service() -> Result<()> {
    let context = RUN_CONTEXT
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("windows service context missing"))?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));
    let status_handle = register_service_handler(shutdown_tx)?;

    status_handle.set_service_status(build_start_pending_status())?;

    let started_handle = status_handle;
    let on_started = Arc::new(move || {
        started_handle.set_service_status(build_running_status())?;
        Ok(())
    });

    let result = context.run_server(Some(shutdown_rx), Some(on_started));
    status_handle.set_service_status(build_stopped_status())?;
    result
}

///
/// サービス制御ハンドラを登録する
///
/// # 引数
/// * `shutdown_tx` - 停止通知送信口
///
/// # 戻り値
/// 登録済みサービスステータスハンドルを返す。
///
fn register_service_handler(
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
) -> Result<ServiceStatusHandle> {
    let status_handle_slot = Arc::new(Mutex::new(None::<ServiceStatusHandle>));
    let handler_status_handle = status_handle_slot.clone();
    let event_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Interrogate => {
                ServiceControlHandlerResult::NoError
            }

            ServiceControl::Stop | ServiceControl::Shutdown => {
                if let Ok(handle) = handler_status_handle.lock() {
                    if let Some(handle) = *handle {
                        let _ = handle
                            .set_service_status(build_stop_pending_status());
                    }
                }

                if let Ok(mut tx) = shutdown_tx.lock() {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(());
                    }
                }

                ServiceControlHandlerResult::NoError
            }

            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle =
        service_control_handler::register(SERVICE_NAME, event_handler)?;
    if let Ok(mut slot) = status_handle_slot.lock() {
        *slot = Some(status_handle);
    }

    Ok(status_handle)
}

///
/// StartPending状態を生成する
///
fn build_start_pending_status() -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: START_WAIT_HINT,
        process_id: None,
    }
}

///
/// Running状態を生成する
///
fn build_running_status() -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP
            | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}

///
/// StopPending状態を生成する
///
fn build_stop_pending_status() -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::StopPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: STOP_WAIT_HINT,
        process_id: None,
    }
}

///
/// Stopped状態を生成する
///
fn build_stopped_status() -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}
