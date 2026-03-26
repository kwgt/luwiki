/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP公開層とサービス層の骨格を集約するモジュール
//!
#![allow(dead_code)]
#![allow(unused_imports)]

pub(crate) mod auth;
pub(crate) mod errors;
pub(crate) mod handler;
pub(crate) mod model;
pub(crate) mod session_manager;
pub(crate) mod server;
pub(crate) mod service;
pub(crate) mod tools;
pub(crate) mod transport;

pub(crate) use transport::{McpEndpoint, create_endpoint};
