/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"page list"の実装
//!

use std::fmt::Write;

use anyhow::Result;
use chrono::SecondsFormat;

use crate::cmd_args::{PageListOpts, Options, PageListSortMode};
use crate::database::{DatabaseManager, PageListEntry};
use super::CommandContext;

///
/// "page list"サブコマンドのコンテキスト情報をパックした構造体
///
struct PageListCommandContext {
    manager: DatabaseManager,
    sort_mode: PageListSortMode,
    reverse_sort: bool,
    long_info: bool,
}

impl PageListCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &PageListOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            sort_mode: sub_opts.sort_mode(),
            reverse_sort: sub_opts.is_reverse_sort(),
            long_info: sub_opts.is_long_info(),
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for PageListCommandContext {
    fn exec(&self) -> Result<()> {
        let mut pages = self.manager.list_pages()?;
        sort_pages(&mut pages, self.sort_mode, self.reverse_sort);
        println!("{}", format_page_table(&pages, self.long_info));
        Ok(())
    }
}

///
/// ページ一覧のソート
///
/// # 引数
/// * `pages` - ソート対象のページ情報
/// * `sort_mode` - ソートモード
/// * `reverse_sort` - 逆順ソートの有無
///
fn sort_pages(
    pages: &mut [PageListEntry],
    sort_mode: PageListSortMode,
    reverse_sort: bool,
) {
    pages.sort_by(|left, right| {
        let ord = match sort_mode {
            PageListSortMode::Default => left.id().cmp(&right.id()),
            PageListSortMode::UserName => {
                left.user_name().cmp(&right.user_name())
            }
            PageListSortMode::PagePath => left.path().cmp(&right.path()),
            PageListSortMode::LastUpdate => {
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
/// ページ一覧のテーブル生成
///
/// # 引数
/// * `pages` - ページ情報一覧
/// * `long_info` - 詳細表示の有無
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_page_table(pages: &[PageListEntry], long_info: bool) -> String {
    /*
     * ヘッダとデータ行の構築
     */
    let mut lines: Vec<Vec<String>> = Vec::with_capacity(pages.len() + 1);

    if long_info {
        let header = ["", "PAGE_ID", "TIMESTAMP", "USER", "REV", "PATH"];
        lines.push(header.iter().map(|value| value.to_string()).collect());
        for page in pages {
            let (timestamp, user, revision) = if page.is_draft() {
                ("***".to_string(), "***".to_string(), "***".to_string())
            } else {
                (
                    page.timestamp()
                        .to_rfc3339_opts(SecondsFormat::Secs, true),
                    page.user_name(),
                    page.latest_revision().to_string(),
                )
            };
            lines.push(vec![
                state_mark(page),
                page.id().to_string(),
                timestamp,
                user,
                revision,
                format_page_path(page),
            ]);
        }
    } else {
        let header = ["", "PAGE_ID", "PATH"];
        lines.push(header.iter().map(|value| value.to_string()).collect());
        for page in pages {
            lines.push(vec![
                state_mark(page),
                page.id().to_string(),
                format_page_path(page),
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
/// 状態表示の文字列を返す
fn state_mark(page: &PageListEntry) -> String {
    if page.deleted() {
        "D".to_string()
    } else if page.is_draft() {
        "d".to_string()
    } else if page.is_locked() {
        "L".to_string()
    } else {
        " ".to_string()
    }
}

fn format_page_path(page: &PageListEntry) -> String {
    let path = page.path();
    if page.deleted() {
        format!("[{}]", path)
    } else {
        path
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &PageListOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(PageListCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
    use crate::database::types::PageId;

    fn build_page(
        id: &str,
        ts: i64,
        user: &str,
        path: &str,
        rev: u64,
        deleted: bool,
        draft: bool,
        locked: bool,
    ) -> PageListEntry {
        PageListEntry::new_for_test(
            PageId::from_string(id).expect("invalid id"),
            path.to_string(),
            rev,
            Local.timestamp_opt(ts, 0).single().unwrap(),
            user.to_string(),
            deleted,
            draft,
            locked,
        )
    }

    #[test]
    fn sort_pages_by_path() {
        let mut pages = vec![
            build_page(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                2,
                "b",
                "/b",
                1,
                false,
                false,
                false,
            ),
            build_page(
                "01ARZ3NDEKTSV4RRFFQ69G5FA0",
                1,
                "a",
                "/a",
                1,
                false,
                false,
                false,
            ),
        ];

        sort_pages(&mut pages, PageListSortMode::PagePath, false);
        assert_eq!(pages[0].path(), "/a");
        assert_eq!(pages[1].path(), "/b");
    }

    #[test]
    fn format_page_table_has_header() {
        let pages = vec![build_page(
            "01ARZ3NDEKTSV4RRFFQ69G5FA0",
            1,
            "user",
            "/page",
            1,
            false,
            false,
            false,
        )];
        let output = format_page_table(&pages, true);
        let mut lines = output.lines();
        let header = lines.next().expect("header missing");
        assert!(header.contains("PAGE_ID"));
        assert!(header.contains("TIMESTAMP"));
        assert!(header.contains("USER"));
        assert!(header.contains("REV"));
        assert!(header.contains("PATH"));
    }

    #[test]
    fn format_page_table_has_short_header() {
        let pages = vec![build_page(
            "01ARZ3NDEKTSV4RRFFQ69G5FA0",
            1,
            "user",
            "/page",
            1,
            false,
            false,
            false,
        )];
        let output = format_page_table(&pages, false);
        let mut lines = output.lines();
        let header = lines.next().expect("header missing");
        assert!(header.contains("PAGE_ID"));
        assert!(header.contains("PATH"));
        assert!(!header.contains("TIMESTAMP"));
    }

    #[test]
    fn format_page_table_marks_deleted() {
        let pages = vec![
            build_page(
                "01ARZ3NDEKTSV4RRFFQ69G5FA0",
                1,
                "user",
                "/deleted",
                1,
                true,
                false,
                false,
            ),
            build_page(
                "01ARZ3NDEKTSV4RRFFQ69G5FA1",
                1,
                "user",
                "/active",
                1,
                false,
                false,
                false,
            ),
        ];
        let output = format_page_table(&pages, false);
        let mut lines = output.lines();
        let _ = lines.next().expect("header missing");
        let first = lines.next().expect("first row missing");
        let second = lines.next().expect("second row missing");
        assert!(first.starts_with("D"));
        assert!(second.starts_with(" "));
    }
}
