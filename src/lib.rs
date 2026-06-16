/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//! ライブラリ用の公開モジュール定義
#![allow(dead_code)]

pub mod auth;
pub mod database;
pub mod export_import;
pub mod audit;
pub mod cmd_args;
pub mod command;
pub mod http_server;
pub mod markdown_source;
pub mod mcp;
pub mod fts;
pub mod rest_api;

pub use database::page_source_exists_for_test;
