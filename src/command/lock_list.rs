/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"lock list"の実装
//!

use std::fmt::Write;

use anyhow::Result;
use chrono::SecondsFormat;

use crate::cmd_args::{LockListOpts, LockListSortMode, Options};
use crate::database::{DatabaseManager, LockListEntry};
use super::CommandContext;

///
/// "lock list"サブコマンドのコンテキスト情報をパックした構造体
///
struct LockListCommandContext {
    manager: DatabaseManager,
    sort_mode: LockListSortMode,
    reverse_sort: bool,
    long_info: bool,
}

impl LockListCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &LockListOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            sort_mode: sub_opts.sort_mode(),
            reverse_sort: sub_opts.is_reverse_sort(),
            long_info: sub_opts.is_long_info(),
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for LockListCommandContext {
    fn exec(&self) -> Result<()> {
        let mut locks = self.manager.list_locks()?;
        sort_locks(&mut locks, self.sort_mode, self.reverse_sort);
        println!("{}", format_lock_table(&locks, self.long_info));
        Ok(())
    }
}

///
/// ロック一覧のソート
///
/// # 引数
/// * `locks` - ソート対象のロック情報
/// * `sort_mode` - ソートモード
/// * `reverse_sort` - 逆順ソートの有無
///
fn sort_locks(
    locks: &mut [LockListEntry],
    sort_mode: LockListSortMode,
    reverse_sort: bool,
) {
    locks.sort_by(|left, right| {
        let ord = match sort_mode {
            LockListSortMode::Default => left.token().cmp(&right.token()),
            LockListSortMode::Expire => left.expire().cmp(&right.expire()),
            LockListSortMode::UserName => {
                left.user_name().cmp(&right.user_name())
            }
            LockListSortMode::PagePath => {
                left.page_path().cmp(&right.page_path())
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
/// ロック一覧のテーブル生成
///
/// # 引数
/// * `locks` - ロック情報一覧
/// * `long_info` - 詳細表示の有無
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_lock_table(locks: &[LockListEntry], long_info: bool) -> String {
    /*
     * ヘッダとデータ行の構築
     */
    let mut lines: Vec<Vec<String>> = Vec::with_capacity(locks.len() + 1);

    if long_info {
        let header = ["LOCK_ID", "EXPIRE", "USER", "PATH"];
        lines.push(header.iter().map(|value| value.to_string()).collect());
        for lock in locks {
            lines.push(vec![
                lock.token().to_string(),
                lock.expire().to_rfc3339_opts(SecondsFormat::Secs, true),
                lock.user_name(),
                lock.page_path(),
            ]);
        }
    } else {
        let header = ["LOCK_ID", "PATH"];
        lines.push(header.iter().map(|value| value.to_string()).collect());
        for lock in locks {
            lines.push(vec![
                lock.token().to_string(),
                lock.page_path(),
            ]);
        }
    }

    /*
     * 列幅の計算
     */
    let mut widths = vec![0usize; lines[0].len()];
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
    sub_opts: &LockListOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(LockListCommandContext::new(opts, sub_opts)?))
}
