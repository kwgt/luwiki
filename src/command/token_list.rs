/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! token list コマンドの実装
//!

use std::fmt::Write;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};

use super::CommandContext;
use super::common::format_cli_timestamp;
use crate::cmd_args::{Options, TokenListOpts};
use crate::database::types::{BearerScope, BearerTokenInfo, UserId};
use crate::database::{DatabaseManager, DbError};

///
/// "token list"サブコマンドのコンテキスト情報をパックした構造体
///
struct TokenListCommandContext {
    manager: DatabaseManager,
    long_info: bool,
    user_name: Option<String>,
    revoked_only: bool,
    expired_only: bool,
}

///
/// 一覧表示用の1行分データ
///
struct TokenListRow {
    state: String,
    token_id: String,
    user_name: String,
    created_at: DateTime<Local>,
    updated_at: DateTime<Local>,
    expire_at: DateTime<Local>,
    revoked: bool,
    name: Option<String>,
}

impl TokenListCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &TokenListOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            long_info: sub_opts.is_long_info(),
            user_name: sub_opts.user_name().or_else(|| sub_opts.target_user_name()),
            revoked_only: sub_opts.is_revoked(),
            expired_only: sub_opts.is_expired(),
        })
    }

    ///
    /// 表示対象トークン一覧の取得
    ///
    fn collect_rows(&self) -> Result<Vec<TokenListRow>> {
        let now = Local::now();
        let user_id = self.resolve_user_id()?;
        let tokens = self.manager.filter_bearer_tokens(
            user_id.as_ref(),
            self.revoked_only,
            self.expired_only,
            now,
        )?;

        let mut rows = Vec::with_capacity(tokens.len());
        for token in tokens {
            let user_name = self
                .manager
                .get_user_name_by_id(&token.user_id())?
                .ok_or_else(|| anyhow!(DbError::UserNotFound))?;
            rows.push(TokenListRow {
                state: format_token_state(&token, now),
                token_id: token.token_id().to_string(),
                user_name,
                created_at: token.created_at(),
                updated_at: token.updated_at(),
                expire_at: token.expire_at(),
                revoked: token.revoked(),
                name: token.name(),
            });
        }

        rows.sort_by(|left, right| left.token_id.cmp(&right.token_id));
        Ok(rows)
    }

    ///
    /// ユーザ名フィルタをユーザIDへ解決する
    ///
    fn resolve_user_id(&self) -> Result<Option<UserId>> {
        let Some(user_name) = &self.user_name else {
            return Ok(None);
        };

        let user_id = self
            .manager
            .get_user_id_by_name(user_name)?
            .ok_or_else(|| anyhow!(DbError::UserNotFound))?;
        Ok(Some(user_id))
    }
}

// CommandContextの実装
impl CommandContext for TokenListCommandContext {
    fn exec(&self) -> Result<()> {
        let rows = self.collect_rows()?;
        if self.long_info {
            println!("{}", format_token_long_table(&rows));
        } else {
            println!("{}", format_token_table(&rows));
        }
        Ok(())
    }
}

///
/// トークン一覧テーブルの生成
///
/// # 引数
/// * `rows` - 一覧表示用データ
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_token_table(rows: &[TokenListRow]) -> String {
    let mut lines: Vec<Vec<String>> = Vec::with_capacity(rows.len() + 1);
    let header = ["STAT", "TOKEN_ID", "USER", "EXPIRE_AT"];
    lines.push(header.iter().map(|value| value.to_string()).collect());

    for row in rows {
        lines.push(vec![
            row.state.clone(),
            row.token_id.clone(),
            row.user_name.clone(),
            format_cli_timestamp(row.expire_at),
        ]);
    }

    format_table_lines(&lines)
}

///
/// 詳細トークン一覧テーブルの生成
///
/// # 引数
/// * `rows` - 一覧表示用データ
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_token_long_table(rows: &[TokenListRow]) -> String {
    let mut lines: Vec<Vec<String>> = Vec::with_capacity(rows.len() + 1);
    let header = [
        "STAT",
        "TOKEN_ID",
        "USER",
        "EXPIRE_AT",
        "CREATED_AT",
        "UPDATED_AT",
        "REVOKED",
        "NAME",
    ];
    lines.push(header.iter().map(|value| value.to_string()).collect());

    for row in rows {
        lines.push(vec![
            row.state.clone(),
            row.token_id.clone(),
            row.user_name.clone(),
            format_cli_timestamp(row.expire_at),
            format_cli_timestamp(row.created_at),
            format_cli_timestamp(row.updated_at),
            row.revoked.to_string(),
            row.name.clone().unwrap_or_default(),
        ]);
    }

    format_table_lines(&lines)
}

///
/// 行データから固定幅テーブル文字列を生成
///
/// # 引数
/// * `lines` - 1行ごとの表示データ
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_table_lines(lines: &[Vec<String>]) -> String {
    if lines.is_empty() {
        return String::new();
    }

    let mut widths = vec![0usize; lines[0].len()];
    for row in lines {
        for (idx, value) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(value.len());
        }
    }

    let mut output = String::new();
    for (row_index, row) in lines.iter().enumerate() {
        let mut line = String::new();
        for (idx, value) in row.iter().enumerate() {
            let padding = if idx + 1 == row.len() { "" } else { "  " };
            let _ = write!(
                &mut line,
                "{:width$}{}",
                value,
                padding,
                width = widths[idx]
            );
        }
        output.push_str(&line);
        if row_index + 1 < lines.len() {
            output.push('\n');
        }
    }

    output
}

///
/// 状態表示欄の生成
///
/// # 引数
/// * `token` - Bearerトークン管理情報
/// * `now` - 期限切れ判定に用いる現在時刻
///
/// # 戻り値
/// 固定幅の状態表示文字列を返す。
///
fn format_token_state(token: &BearerTokenInfo, now: DateTime<Local>) -> String {
    let scopes = token.scopes();
    let read_mark = if scopes.contains(BearerScope::Read) { 'r' } else { '-' };
    let write_mark = if scopes.contains(BearerScope::Write) { 'w' } else { '-' };
    let revoked_mark = if token.revoked() { 'v' } else { '-' };
    let expired_mark = if token.expire_at() <= now { 'e' } else { '-' };
    format!("{}{}{}{}", read_mark, write_mark, revoked_mark, expired_mark)
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &TokenListOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(TokenListCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Local};

    use super::format_token_state;
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        BearerTokenInfo,
        TokenId,
        UserId,
    };

    #[test]
    fn token_state_marks_assigned_scopes_and_revoked_state() {
        let now = Local::now();
        let info = BearerTokenInfo::new_for_test(
            TokenId::new(),
            UserId::new(),
            BearerScopeSet::from_iter([BearerScope::Write]),
            now,
            now,
            Duration::days(30),
            now + Duration::days(30),
            true,
            None,
        );

        assert_eq!(format_token_state(&info, now), "-wv-");
    }

    #[test]
    fn token_state_marks_read_scope_without_write_scope() {
        let now = Local::now();
        let info = BearerTokenInfo::new_for_test(
            TokenId::new(),
            UserId::new(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            now,
            now,
            Duration::days(30),
            now + Duration::days(30),
            false,
            None,
        );

        assert_eq!(format_token_state(&info, now), "r---");
    }
}
