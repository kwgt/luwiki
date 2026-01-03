/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user list"の実装
//!

use std::fmt::Write;

use anyhow::Result;
use chrono::SecondsFormat;

use crate::cmd_args::{UserListOpts, Options, UserListSortMode};
use crate::database::types::UserInfo;
use crate::database::DatabaseManager;
use super::CommandContext;

///
/// "user list"サブコマンドのコンテキスト情報をパックした構造体
///
struct UserListCommandContext {
    manager: DatabaseManager,
    sort_mode: UserListSortMode,
    reverse_sort: bool,
}

impl UserListCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &UserListOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            sort_mode: sub_opts.sort_mode(),
            reverse_sort: sub_opts.is_reverse_sort(),
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for UserListCommandContext {
    fn exec(&self) -> Result<()> {
        let mut users = self.manager.list_users()?;
        sort_users(&mut users, self.sort_mode, self.reverse_sort);
        println!("{}", format_user_table(&users));
        Ok(())
    }
}

///
/// ユーザ一覧のソート
///
/// # 引数
/// * `users` - ソート対象のユーザ情報
/// * `sort_mode` - ソートモード
/// * `reverse_sort` - 逆順ソートの有無
///
fn sort_users(
    users: &mut [UserInfo],
    sort_mode: UserListSortMode,
    reverse_sort: bool,
) {
    users.sort_by(|left, right| {
        let ord = match sort_mode {
            UserListSortMode::Default => left.id().cmp(&right.id()),
            UserListSortMode::UserName => {
                left.username().cmp(&right.username())
            }
            UserListSortMode::DisplayName => {
                left.display_name().cmp(&right.display_name())
            }
            UserListSortMode::LastUpdate => {
                left.timestamp().cmp(&right.timestamp())
            }
        };

        if reverse_sort {
            ord.reverse()
        } else {
            ord
        }
    });
}

///
/// ユーザ一覧のテーブル生成
///
/// # 引数
/// * `users` - ユーザ情報一覧
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_user_table(users: &[UserInfo]) -> String {
    /*
     * ヘッダとデータ行の構築
     */
    let mut lines = Vec::with_capacity(users.len() + 1);

    let header = ["USER_ID", "TIMESTAMP", "USER_NAME", "DISPLAY_NAME"];
    lines.push(header.map(|value| value.to_string()));

    for user in users {
        lines.push([
            user.id().to_string(),
            user.timestamp()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            user.username(),
            user.display_name(),
        ]);
    }

    /*
     * 列幅の計算
     */
    let mut widths = vec![0usize; header.len()];
    for row in &lines {
        for (idx, value) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(value.len());
        }
    }

    /*
     * 出力文字列の生成
     */
    let mut output = String::new();
    for (row_index, row) in lines.iter().enumerate() {
        let mut line = String::new();
        for (idx, value) in row.iter().enumerate() {
            let _ = write!(
                &mut line,
                "{:width$}{}",
                value,
                if idx + 1 == row.len() { "" } else { "  " },
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
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &UserListOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(UserListCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
    use crate::database::types::{UserId, UserInfo};

    fn build_user(id: &str, ts: i64, name: &str, display: &str) -> UserInfo {
        UserInfo::new_for_test(
            UserId::from_string(id).expect("invalid id"),
            Local.timestamp_opt(ts, 0).single().unwrap(),
            name,
            display,
        )
    }

    #[test]
    fn sort_users_by_id_default() {
        let mut users = vec![
            build_user("01ARZ3NDEKTSV4RRFFQ69G5FAV", 2, "b", "bb"),
            build_user("01ARZ3NDEKTSV4RRFFQ69G5FA0", 1, "a", "aa"),
        ];

        sort_users(&mut users, UserListSortMode::Default, false);
        assert_eq!(users[0].username(), "a");
        assert_eq!(users[1].username(), "b");
    }

    #[test]
    fn sort_users_reverse_by_name() {
        let mut users = vec![
            build_user("01ARZ3NDEKTSV4RRFFQ69G5FAV", 2, "b", "bb"),
            build_user("01ARZ3NDEKTSV4RRFFQ69G5FA0", 1, "a", "aa"),
        ];

        sort_users(&mut users, UserListSortMode::UserName, true);
        assert_eq!(users[0].username(), "b");
        assert_eq!(users[1].username(), "a");
    }

    #[test]
    fn format_user_table_has_header() {
        let users = vec![build_user(
            "01ARZ3NDEKTSV4RRFFQ69G5FA0",
            1,
            "user",
            "display",
        )];
        let output = format_user_table(&users);
        let mut lines = output.lines();
        let header = lines.next().expect("header missing");
        assert!(header.contains("USER_ID"));
        assert!(header.contains("TIMESTAMP"));
        assert!(header.contains("USER_NAME"));
        assert!(header.contains("DISPLAY_NAME"));
    }
}
