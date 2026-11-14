/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 状態取得・変更待ちのtool引数を定義する
//!

use rmcp::schemars;
use serde::Deserialize;
use serde_json::Value;

///
/// 最新状態取得の入力
///
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct GetPageStateToolArgs {
    /// 対象の絶対path
    pub(crate) path: String,
}

///
/// 変更待ちの入力
///
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct WaitPageChangeToolArgs {
    /// 対象の絶対path
    pub(crate) path: String,
    /// 取得済みの比較値
    pub(crate) expected_state: String,
    /// 省略時30秒。null等は業務入力不正として拒否する
    #[serde(default = "default_timeout")]
    #[schemars(with = "u64", range(min = 1, max = 30))]
    pub(crate) timeout_seconds: Value,
}

///
/// 省略された期限の値を返す
///
/// # 戻り値
/// JSON数値30。
///
fn default_timeout() -> Value {
    Value::from(30)
}

///
/// 共通handlerへ渡す処理種別
///
pub(crate) enum StateRequest {
    /// 現在状態だけを取得
    Get,
    /// 基準値からの変化を待つ
    Wait {
        /// 基準token
        expected_state: String,
        /// 外部指定の期限
        timeout_seconds: Value,
    },
}
