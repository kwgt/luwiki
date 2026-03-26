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
use crate::database::types::{
    BearerScope,
    BearerScopeSet,
    BearerTokenInfo,
    PathPrefixSet,
    UserId,
};
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
    scope: String,
    path: String,
    token_id: String,
    user_name: String,
    created_at: DateTime<Local>,
    expire_at: DateTime<Local>,
    status: String,
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
        /*
         * フィルタ条件に一致するトークンを取得する
         */
        let now = Local::now();
        let user_id = self.resolve_user_id()?;
        let tokens = self.manager.filter_bearer_tokens(
            user_id.as_ref(),
            self.revoked_only,
            self.expired_only,
            now,
        )?;

        /*
         * CLI 表示行へ整形する
         */
        let mut rows = Vec::with_capacity(tokens.len());
        for token in tokens {
            let user_name = self
                .manager
                .get_user_name_by_id(&token.user_id())?
                .ok_or_else(|| anyhow!(DbError::UserNotFound))?;
            rows.push(TokenListRow {
                scope: format_effective_permissions(token.scopes()),
                path: format_path_access_marker(token.path_prefixes()),
                token_id: token.token_id().to_string(),
                user_name,
                created_at: token.created_at(),
                expire_at: token.expire_at(),
                status: format_token_status(&token, now).to_string(),
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
    let header = ["SCOPE", "PATH", "ID", "USER", "NAME", "EXPIRES"];
    lines.push(header.iter().map(|value| value.to_string()).collect());

    for row in rows {
        lines.push(vec![
            row.scope.clone(),
            row.path.clone(),
            row.token_id.clone(),
            row.user_name.clone(),
            row.name.clone().unwrap_or_default(),
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
        "SCOPE",
        "PATH",
        "ID",
        "USER",
        "NAME",
        "EXPIRES",
        "CREATE",
        "STATUS",
    ];
    lines.push(header.iter().map(|value| value.to_string()).collect());

    for row in rows {
        lines.push(vec![
            row.scope.clone(),
            row.path.clone(),
            row.token_id.clone(),
            row.user_name.clone(),
            row.name.clone().unwrap_or_default(),
            format_cli_timestamp(row.expire_at),
            format_cli_timestamp(row.created_at),
            row.status.clone(),
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
/// 実効権限表示文字列の生成
///
/// # 引数
/// * `scopes` - Bearer スコープ集合
///
/// # 戻り値
/// `rcdua` 形式の実効権限文字列を返す。
///
pub(crate) fn format_effective_permissions(scopes: BearerScopeSet) -> String {
    let mut output = String::with_capacity(5);
    for (required, marker) in [
        (BearerScope::Read, 'r'),
        (BearerScope::Create, 'c'),
        (BearerScope::Delete, 'd'),
        (BearerScope::Update, 'u'),
        (BearerScope::Append, 'a'),
    ] {
        output.push(if scopes.allows(required) { marker } else { '-' });
    }

    output
}

///
/// path 制約有無の一覧表示文字列の生成
///
/// # 引数
/// * `path_prefixes` - path prefix 制約集合
///
/// # 戻り値
/// 全領域アクセス可なら `*`、制約ありなら `L` を返す。
///
pub(crate) fn format_path_access_marker(
    path_prefixes: PathPrefixSet,
) -> String {
    if path_prefixes.allows_all() {
        "*".to_string()
    } else {
        "L".to_string()
    }
}

///
/// path prefix 詳細表示文字列の生成
///
/// # 引数
/// * `path_prefixes` - path prefix 制約集合
///
/// # 戻り値
/// 全領域アクセス可または詳細 prefix 群を返す。
///
pub(crate) fn format_path_prefixes_detail(
    path_prefixes: PathPrefixSet,
) -> String {
    if path_prefixes.allows_all() {
        "all".to_string()
    } else {
        path_prefixes.iter().collect::<Vec<_>>().join(",")
    }
}

///
/// 保存スコープ表示文字列の生成
///
/// # 引数
/// * `scopes` - Bearer スコープ集合
///
/// # 戻り値
/// 保存値としてのスコープをカンマ区切り文字列で返す。
///
pub(crate) fn format_stored_scopes(scopes: BearerScopeSet) -> String {
    scopes
        .iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

///
/// 状態表示文字列の生成
///
/// # 引数
/// * `token` - Bearerトークン管理情報
/// * `now` - 期限切れ判定に用いる現在時刻
///
/// # 戻り値
/// `alive`、`expired`、`revoked` のいずれかを返す。
///
pub(crate) fn format_token_status(
    token: &BearerTokenInfo,
    now: DateTime<Local>,
) -> &'static str {
    if token.revoked() {
        "revoked"
    } else if token.expire_at() <= now {
        "expired"
    } else {
        "alive"
    }
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

    use super::{
        format_effective_permissions,
        format_path_access_marker,
        format_path_prefixes_detail,
        format_token_status,
    };
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        BearerTokenInfo,
        PathPrefixSet,
        TokenId,
        UserId,
    };

    #[test]
    fn effective_permissions_expands_write_scope_to_rcdua() {
        let scopes = BearerScopeSet::from_iter([BearerScope::Write]);
        assert_eq!(format_effective_permissions(scopes), "rcdua");
    }

    #[test]
    fn effective_permissions_marks_only_granted_decomposed_scopes() {
        let scopes = BearerScopeSet::from_iter([
            BearerScope::Read,
            BearerScope::Append,
        ]);
        assert_eq!(format_effective_permissions(scopes), "r---a");
    }

    #[test]
    fn path_display_marks_all_access_and_limited_access() {
        assert_eq!(format_path_access_marker(PathPrefixSet::new()), "*");
        assert_eq!(
            format_path_access_marker(PathPrefixSet::from_iter(["/docs"])),
            "L",
        );
    }

    #[test]
    fn path_detail_formats_all_access_and_prefix_list() {
        assert_eq!(format_path_prefixes_detail(PathPrefixSet::new()), "all");
        assert_eq!(
            format_path_prefixes_detail(PathPrefixSet::from_iter([
                "/docs",
                "/notes",
            ])),
            "/docs,/notes",
        );
    }

    #[test]
    fn token_status_prioritizes_revoked_over_expired() {
        let now = Local::now();
        let info = BearerTokenInfo::new_for_test(
            TokenId::new(),
            UserId::new(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            now,
            now,
            Duration::days(30),
            now - Duration::minutes(1),
            true,
            PathPrefixSet::new(),
            None,
        );

        assert_eq!(format_token_status(&info, now), "revoked");
    }
}
