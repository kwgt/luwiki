/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"asset list"の実装
//!

use std::fmt::Write;

use anyhow::Result;
use chrono::{DateTime, Local};

use crate::cmd_args::{AssetListOpts, AssetListSortMode, Options};
use crate::database::{AssetListEntry, DatabaseManager};
use super::CommandContext;

///
/// "asset list"サブコマンドのコンテキスト情報をパックした構造体
///
struct AssetListCommandContext {
    manager: DatabaseManager,
    sort_mode: AssetListSortMode,
    reverse_sort: bool,
    long_info: bool,
}

impl AssetListCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &AssetListOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            sort_mode: sub_opts.sort_mode(),
            reverse_sort: sub_opts.is_reverse_sort(),
            long_info: sub_opts.is_long_info(),
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for AssetListCommandContext {
    fn exec(&self) -> Result<()> {
        let mut assets = self.manager.list_assets()?;
        sort_assets(&mut assets, self.sort_mode, self.reverse_sort);
        println!("{}", format_asset_table(&assets, self.long_info));
        Ok(())
    }
}

///
/// アセット一覧のソート
///
/// # 引数
/// * `assets` - ソート対象のアセット情報
/// * `sort_mode` - ソートモード
/// * `reverse_sort` - 逆順ソートの有無
///
fn sort_assets(
    assets: &mut [AssetListEntry],
    sort_mode: AssetListSortMode,
    reverse_sort: bool,
) {
    assets.sort_by(|left, right| {
        let ord = match sort_mode {
            AssetListSortMode::Default => left.id().cmp(&right.id()),
            AssetListSortMode::Upload => {
                left.timestamp().cmp(&right.timestamp())
            }
            AssetListSortMode::UserName => {
                left.user_name().cmp(&right.user_name())
            }
            AssetListSortMode::MimeType => left.mime().cmp(&right.mime()),
            AssetListSortMode::Size => left.size().cmp(&right.size()),
            AssetListSortMode::Path => {
                let left_path = path_sort_key(left);
                let right_path = path_sort_key(right);
                left_path
                    .cmp(&right_path)
                    .then_with(|| left.file_name().cmp(&right.file_name()))
            }
        };

        if reverse_sort {
            ord.reverse()
        } else {
            ord
        }
    });
}

fn path_sort_key(asset: &AssetListEntry) -> String {
    asset
        .page_path()
        .unwrap_or_else(|| "?????".to_string())
}

///
/// アセット一覧のテーブル生成
///
/// # 引数
/// * `assets` - アセット情報一覧
/// * `long_info` - 詳細表示の有無
///
/// # 戻り値
/// テーブル整形済み文字列を返す。
///
fn format_asset_table(assets: &[AssetListEntry], long_info: bool) -> String {
    /*
     * ヘッダとデータ行の構築
     */
    let mut lines: Vec<Vec<String>> = Vec::with_capacity(assets.len() + 1);

    if long_info {
        let header = ["", "ASSET_ID", "TIMESTAMP", "USER", "MIME", "SIZE", "PATH"];
        lines.push(header.iter().map(|value| value.to_string()).collect());
        for asset in assets {
            lines.push(vec![
                asset_mark(asset),
                asset.id().to_string(),
                format_timestamp(asset.timestamp()),
                asset.user_name(),
                asset.mime(),
                format_size(asset.size()),
                asset_path_display(asset),
            ]);
        }
    } else {
        let header = ["", "ASSET_ID", "MIME", "SIZE", "FILE"];
        lines.push(header.iter().map(|value| value.to_string()).collect());
        for asset in assets {
            lines.push(vec![
                asset_mark(asset),
                asset.id().to_string(),
                asset.mime(),
                format_size(asset.size()),
                asset.file_name(),
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
    let size_index = if long_info { 5usize } else { 3usize };
    if long_info {
        let user_index = 3usize;
        if user_index < widths.len() {
            widths[user_index] = widths[user_index].max(16);
        }
    }
    if size_index < widths.len() {
        widths[size_index] = widths[size_index].saturating_add(2);
    }

    /*
     * 出力文字列の生成
     */
    let mut output = String::new();
    for (row_index, row) in lines.iter().enumerate() {
        let mut line = String::new();
        for (idx, value) in row.iter().enumerate() {
            let padding = if idx + 1 == row.len() { "" } else { "  " };
            if idx == size_index {
                let _ = write!(
                    &mut line,
                    "{:>width$}{}",
                    value,
                    padding,
                    width = widths[idx]
                );
            } else {
                let _ = write!(
                    &mut line,
                    "{:width$}{}",
                    value,
                    padding,
                    width = widths[idx]
                );
            }
        }
        output.push_str(&line);
        if row_index + 1 < lines.len() {
            output.push('\n');
        }
    }

    output
}

fn format_timestamp(timestamp: DateTime<Local>) -> String {
    timestamp.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn format_size(size: u64) -> String {
    let (value, unit) = if size >= 1024 * 1024 * 1024 {
        (size / (1024 * 1024 * 1024), "Gi")
    } else if size >= 1024 * 1024 {
        (size / (1024 * 1024), "Mi")
    } else if size >= 1024 {
        (size / 1024, "Ki")
    } else {
        (size, "B ")
    };

    format!("{}{}", format_number_with_commas(value), unit)
}

fn format_number_with_commas(value: u64) -> String {
    let raw = value.to_string();
    let mut chars: Vec<char> = raw.chars().collect();
    let mut index = chars.len() as isize - 3;
    while index > 0 {
        chars.insert(index as usize, ',');
        index -= 3;
    }
    chars.into_iter().collect()
}

///
/// アセット状態の表示文字列を返す
///
/// # 引数
/// * `asset` - アセット情報
///
/// # 戻り値
/// 状態表示文字列を返す。
///
fn asset_mark(asset: &AssetListEntry) -> String {
    let deleted = asset.deleted();
    let zombie = asset.is_zombie();

    if deleted && zombie {
        "B".to_string()
    } else if deleted {
        "D".to_string()
    } else if zombie {
        "Z".to_string()
    } else {
        " ".to_string()
    }
}

fn asset_path_display(asset: &AssetListEntry) -> String {
    let file_name = asset.file_name();
    let path = match asset.page_path() {
        Some(path) => path,
        None => "?????".to_string(),
    };

    if path == "/" {
        format!("/{}", file_name)
    } else {
        format!("{}/{}", path, file_name)
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &AssetListOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(AssetListCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
    use crate::database::types::AssetId;

    fn build_asset(
        id: &str,
        ts: i64,
        user: &str,
        file: &str,
        mime: &str,
        size: u64,
        path: Option<&str>,
        deleted: bool,
    ) -> AssetListEntry {
        AssetListEntry::new_for_test(
            AssetId::from_string(id).expect("invalid id"),
            file.to_string(),
            mime.to_string(),
            size,
            Local.timestamp_opt(ts, 0).single().unwrap(),
            user.to_string(),
            path.map(|value| value.to_string()),
            deleted,
        )
    }

    #[test]
    fn sort_assets_by_path() {
        let mut assets = vec![
            build_asset(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                2,
                "b",
                "b.bin",
                "application/octet-stream",
                10,
                Some("/b"),
                false,
            ),
            build_asset(
                "01ARZ3NDEKTSV4RRFFQ69G5FA0",
                1,
                "a",
                "a.bin",
                "application/octet-stream",
                5,
                Some("/a"),
                false,
            ),
        ];

        sort_assets(&mut assets, AssetListSortMode::Path, false);
        assert_eq!(assets[0].file_name(), "a.bin");
        assert_eq!(assets[1].file_name(), "b.bin");
    }

    #[test]
    fn format_asset_table_marks_states() {
        let assets = vec![
            build_asset(
                "01ARZ3NDEKTSV4RRFFQ69G5FA0",
                1,
                "user",
                "del.bin",
                "application/octet-stream",
                1,
                Some("/page"),
                true,
            ),
            build_asset(
                "01ARZ3NDEKTSV4RRFFQ69G5FA1",
                1,
                "user",
                "zombie.bin",
                "application/octet-stream",
                1,
                None,
                false,
            ),
            build_asset(
                "01ARZ3NDEKTSV4RRFFQ69G5FA2",
                1,
                "user",
                "both.bin",
                "application/octet-stream",
                1,
                None,
                true,
            ),
        ];

        let output = format_asset_table(&assets, false);
        let mut lines = output.lines();
        let _ = lines.next().expect("header missing");
        let first = lines.next().expect("first row missing");
        let second = lines.next().expect("second row missing");
        let third = lines.next().expect("third row missing");
        assert!(first.starts_with("D"));
        assert!(second.starts_with("Z"));
        assert!(third.starts_with("B"));
    }
}
