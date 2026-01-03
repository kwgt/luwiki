/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! API HELLOの実装を行うモジュール
//!

use std::sync::{Arc, RwLock};

use actix_web::{web, HttpResponse};

use crate::http_server::app_state::AppState;
use super::resp_200;

///
/// GET /api/hello の実体
///
/// # 概要
/// アプリケーション動作確認用APIとして、単純に文字列"hello"を返す
///
/// # 引数
/// * `_state` - 共有状態
///
/// # APIレスポンスの種別
/// text/plain
///
/// # 戻り値
/// actix-webのレスポンスオブジェクト
///
pub async fn get(
    _state: web::Data<Arc<RwLock<AppState>>>,
)
    -> actix_web::Result<HttpResponse>
{
    Ok(resp_200("hello"))
}
