/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ状態・変更待ちの公開応答モデル
//!

use serde::Serialize;

///
/// 本文・内部IDを含まない通常ページ状態
///
#[derive(Clone, Serialize)]
pub(crate) struct PageState {
    /// 最新revision
    pub(crate) revision: u64,
    /// 旧データでは欠落するsource識別子
    pub(crate) instance_id: Option<String>,
    /// path別の不透明な比較値
    pub(crate) state_token: String,
}

///
/// 状態取得の応答
///
#[derive(Serialize)]
pub(crate) struct GetPageStateResponse {
    /// 正規化した要求path
    pub(crate) path: String,
    /// 通常ページ状態
    #[serde(flatten)]
    pub(crate) state: PageState,
}

///
/// 変更待ちの正常応答
///
#[derive(Serialize)]
pub(crate) struct WaitPageChangeResponse {
    /// 正規化した要求path
    pub(crate) path: String,
    /// changed又はtimeout
    pub(crate) status: &'static str,
    /// updated・replaced・unavailable、timeoutではNone
    pub(crate) change: Option<&'static str>,
    /// 取得不能ならNone
    pub(crate) state: Option<PageState>,
}
