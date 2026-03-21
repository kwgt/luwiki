/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! token create コマンドの実装
//!

use std::cell::RefCell;

use anyhow::Result;

use super::CommandContext;
use super::common::format_cli_timestamp;
use crate::cmd_args::{Options, TokenCreateOpts};
use crate::database::types::{
    BearerScopeSet,
    BearerTokenInfo,
    BearerTokenPlaintext,
};
use crate::database::DatabaseManager;

///
/// "token create"サブコマンドのコンテキスト情報をパックした構造体
///
struct TokenCreateCommandContext {
    /// データベースマネージャオブジェクト
    manager: RefCell<DatabaseManager>,

    /// 発行対象ユーザ名
    user_name: String,

    /// 付与スコープ
    scopes: BearerScopeSet,

    /// TTL
    ttl: chrono::Duration,

    /// 任意のトークン名
    name: Option<String>,
}

impl TokenCreateCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &TokenCreateOpts) -> Result<Self> {
        Ok(Self {
            manager: RefCell::new(opts.open_database()?),
            user_name: sub_opts.user_name(),
            scopes: sub_opts.scopes()?,
            ttl: sub_opts.ttl_duration()?,
            name: sub_opts.normalized_name(),
        })
    }
}

// CommandContextの実装
impl CommandContext for TokenCreateCommandContext {
    fn exec(&self) -> Result<()> {
        let manager = self.manager.borrow_mut();
        let (plaintext, info) = manager.create_bearer_token(
            &self.user_name,
            self.scopes.clone(),
            self.ttl,
            self.name.clone(),
        )?;

        print_created_token(&self.user_name, &plaintext, &info);
        Ok(())
    }
}

///
/// 作成したトークン情報の出力
///
/// # 引数
/// * `user_name` - 発行対象ユーザ名
/// * `plaintext` - 発行したトークン平文
/// * `info` - 作成された管理情報
///
fn print_created_token(
    user_name: &str,
    plaintext: &BearerTokenPlaintext,
    info: &BearerTokenInfo,
) {
    println!("token_id: {}", info.token_id());
    println!("user_name: {}", user_name);
    println!("scopes: {}", format_scopes(info));
    println!("created_at: {}", format_cli_timestamp(info.created_at()));
    println!("expire_at: {}", format_cli_timestamp(info.expire_at()));
    println!("token: {}", plaintext.expose());
}

///
/// スコープ表示文字列の生成
///
/// # 引数
/// * `info` - Bearerトークン管理情報
///
/// # 戻り値
/// カンマ区切りのスコープ表示文字列を返す。
///
fn format_scopes(info: &BearerTokenInfo) -> String {
    info.scopes()
        .iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &TokenCreateOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(TokenCreateCommandContext::new(opts, sub_opts)?))
}
