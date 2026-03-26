/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! token info コマンドの実装
//!

use anyhow::{anyhow, Result};
use chrono::Local;

use super::CommandContext;
use super::common::{format_cli_duration, format_cli_timestamp};
use super::token_list::{
    format_stored_scopes,
    format_token_status,
};
use crate::cmd_args::{Options, TokenInfoOpts};
use crate::database::types::BearerScope;
use crate::database::types::TokenId;
use crate::database::types::BearerScopeSet;
use crate::database::DatabaseManager;

///
/// "token info"サブコマンドのコンテキスト情報をパックした構造体
///
struct TokenInfoCommandContext {
    manager: DatabaseManager,
    token_id: TokenId,
}

impl TokenInfoCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &TokenInfoOpts) -> Result<Self> {
        let token_id = sub_opts.token_id();
        Ok(Self {
            manager: opts.open_database()?,
            token_id: TokenId::from_string(&token_id)
                .map_err(|_| anyhow!("invalid token id: {}", token_id))?,
        })
    }
}

// CommandContextの実装
impl CommandContext for TokenInfoCommandContext {
    fn exec(&self) -> Result<()> {
        /*
         * 管理情報とユーザ名を解決する
         */
        let info = self
            .manager
            .get_bearer_token_info_by_id(&self.token_id)?
            .ok_or_else(|| anyhow!("token not found: {}", self.token_id))?;
        let user_name = self
            .manager
            .get_user_name_by_id(&info.user_id())?
            .ok_or_else(|| anyhow!("user not found for token: {}", self.token_id))?;

        /*
         * 詳細情報を表示する
         */
        print_field("TOKEN ID", &info.token_id().to_string());
        print_field("TOKEN NAME", info.name().as_deref().unwrap_or("-"));
        print_field("USERNAME", &user_name);
        print_field("STATUS", format_token_status(&info, Local::now()));
        print_field("SCOPES", &format_stored_scopes(info.scopes()));
        print_field("PERMISSIONS", &format_effective_permission_list(info.scopes()));
        print_path_prefixes(info.path_prefixes());
        print_field("TTL", &format_ttl(info.ttl()));
        print_timestamps(&[
            ("create", info.created_at()),
            ("update", info.updated_at()),
            ("expire", info.expire_at()),
        ]);
        Ok(())
    }
}

///
/// TTL 表示文字列を生成する
///
/// # 引数
/// * `ttl` - 表示対象の TTL
///
/// # 戻り値
/// 秒数ベースの TTL 表示文字列を返す。
///
fn format_ttl(ttl: chrono::Duration) -> String {
    format_cli_duration(ttl)
}

///
/// 実効権限の詳細表示文字列を生成する
///
/// # 引数
/// * `scopes` - Bearer スコープ集合
///
/// # 戻り値
/// 実効権限を配列風の文字列で返す。
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

    /*
     * 配列風文字列へ整形
     */
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
fn print_path_prefixes(path_prefixes: crate::database::types::PathPrefixSet) {
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
    sub_opts: &TokenInfoOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(TokenInfoCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::format_effective_permission_list;
    use crate::database::types::{BearerScope, BearerScopeSet};

    #[test]
    fn effective_permission_list_expands_write_scope() {
        let scopes = BearerScopeSet::from_iter([BearerScope::Write]);

        assert_eq!(
            format_effective_permission_list(scopes),
            "read, create, delete, update, append"
        );
    }

    #[test]
    fn effective_permission_list_shows_granted_permissions_only() {
        let scopes = BearerScopeSet::from_iter([
            BearerScope::Read,
            BearerScope::Append,
        ]);

        assert_eq!(
            format_effective_permission_list(scopes),
            "read, append"
        );
    }
}
