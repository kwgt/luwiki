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
use super::common::{format_cli_duration, format_cli_timestamp};
use crate::cmd_args::{Options, TokenCreateOpts};
use crate::database::types::{
    BearerScope,
    BearerScopeSet,
    BearerTokenInfo,
    BearerTokenPlaintext,
    PathPrefixSet,
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

    /// path prefix 制約集合
    path_prefixes: PathPrefixSet,
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
            path_prefixes: sub_opts.path_prefixes()?,
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
            self.path_prefixes.clone(),
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
    print_field("TOKEN ID", &info.token_id().to_string());
    print_field("TOKEN NAME", info.name().as_deref().unwrap_or("-"));
    print_field("USERNAME", user_name);
    print_field("SCOPES", &format_scopes(info));
    print_field(
        "PERMISSIONS",
        &format_effective_permission_list(info.scopes()),
    );
    print_field("TTL", &format_cli_duration(info.ttl()));
    print_path_prefixes(info.path_prefixes());
    print_timestamps(&[
        ("create", info.created_at()),
        ("expire", info.expire_at()),
    ]);
    if info.path_prefixes().allows_all() {
        println!("WARNING:");
        println!("    - token allows access to all paths");
    }
    println!();
    println!("TOKEN VALUE:");
    println!("    {}", plaintext.expose());
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
/// 実効権限の詳細表示文字列を生成する
///
/// # 引数
/// * `scopes` - Bearer スコープ集合
///
/// # 戻り値
/// 実効権限名のカンマ区切り文字列を返す。
///
fn format_effective_permission_list(scopes: BearerScopeSet) -> String {
    /*
     * 実効権限名の収集
     */
    let mut permissions = Vec::new();
    for (required, label) in [
        (BearerScope::Read, "read"),
        (BearerScope::Create, "create"),
        (BearerScope::Delete, "delete"),
        (BearerScope::Update, "update"),
        (BearerScope::Append, "append"),
    ] {
        if scopes.allows(required) {
            permissions.push(label);
        }
    }

    permissions.join(", ")
}

///
/// 単一値フィールドを整形出力する
///
/// # 引数
/// * `label` - 表示ラベル
/// * `value` - 表示値
///
/// # 戻り値
/// なし
///
fn print_field(label: &str, value: &str) {
    println!("{:<13} {}", format!("{}:", label), value);
}

///
/// path prefix 制約を整形出力する
///
/// # 引数
/// * `path_prefixes` - 表示対象の path prefix 制約集合
///
/// # 戻り値
/// なし
///
fn print_path_prefixes(path_prefixes: PathPrefixSet) {
    println!("PATH PREFIXES:");

    if path_prefixes.allows_all() {
        println!("    - all");
        return;
    }

    for path_prefix in path_prefixes.iter() {
        println!("    - {}", path_prefix);
    }
}

///
/// タイムスタンプ群を整形出力する
///
/// # 引数
/// * `timestamps` - ラベル付きタイムスタンプ一覧
///
/// # 戻り値
/// なし
///
fn print_timestamps(timestamps: &[(&str, chrono::DateTime<chrono::Local>)]) {
    println!("TIMESTAMPS:");
    for (label, timestamp) in timestamps {
        println!("    {}: {}", label, format_cli_timestamp(*timestamp));
    }
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
