/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 監査ログ基盤の骨格を集約するモジュール
//!
#![allow(dead_code)]
#![allow(unused_imports)]

pub(crate) mod buffer;
pub(crate) mod model;
pub(crate) mod retention;
pub(crate) mod rotation;
pub(crate) mod sink;
pub(crate) mod writer;

pub(crate) use sink::AuditSink;
