/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//! ライブラリ用の公開モジュール定義
#![allow(dead_code)]

pub mod database;

pub use database::page_source_exists_for_test;
