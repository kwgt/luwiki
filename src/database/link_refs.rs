/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページソースからリンク参照情報を抽出するモジュール
//!

use std::collections::BTreeMap;
use std::path::{Component, Path};

use anyhow::Result;
use redb::{ReadableTable, Table};

use crate::database::types::PageId;

use super::schema::PAGE_PATH_TABLE;

///
/// ページソースからリンク参照情報を生成
///
/// # 概要
/// MarkdownリンクからWiki内部リンクを抽出して解決する
///
/// # 引数
/// * `txn` - 書き込みトランザクション
/// * `base_path` - 基準となるページパス
/// * `source` - ページソース
///
/// # 戻り値
/// リンク参照情報を返す。
///
/// # 注記
/// 正規表現では括弧の入れ子や`![]()`の除外が複雑になるため、
/// 字句走査による抽出を行っている。
///
/// 抽出ルールは以下の通り。
///  - 対象は`[]()`のリンクのみ
///  - `![]()`は除外
///  - スキーマ指定リンク（`xxx:`）は除外
///  - 相対パスは`base_path`基準で正規化し、`.`/`..`を解決する
///  - 未存在ページは`None`として記録する
///
pub(in crate::database) fn build_link_refs(
    txn: &redb::WriteTransaction,
    base_path: &str,
    source: &str,
) -> Result<BTreeMap<String, Option<PageId>>> {
    let table = txn.open_table(PAGE_PATH_TABLE)?;
    build_link_refs_with_table(&table, base_path, source)
}

///
/// ページパステーブルを用いてリンク参照情報を生成する
///
/// # 引数
/// * `path_table` - ページパステーブル
/// * `base_path` - 基準となるページパス
/// * `source` - ページソース
///
/// # 戻り値
/// リンク参照情報を返す。
///
pub(in crate::database) fn build_link_refs_with_table<'txn>(
    path_table: &Table<'txn, String, PageId>,
    base_path: &str,
    source: &str,
) -> Result<BTreeMap<String, Option<PageId>>> {
    build_link_refs_with_resolver(
        base_path,
        source,
        |path| resolve_page_id_with_table(path_table, path),
    )
}

///
/// ページID解決を行うクロージャでリンク参照情報を生成する
///
/// # 引数
/// * `base_path` - 基準となるページパス
/// * `source` - ページソース
/// * `resolve` - ページID解決クロージャ
///
/// # 戻り値
/// リンク参照情報を返す。
///
fn build_link_refs_with_resolver<F>(
    base_path: &str,
    source: &str,
    mut resolve: F,
) -> Result<BTreeMap<String, Option<PageId>>>
where
    F: FnMut(&str) -> Result<Option<PageId>>,
{
    /*
     * 参照一覧の初期化
     */
    let mut refs = BTreeMap::new();
    let mut chars = source.chars().peekable();

    /*
     * Markdownリンクの抽出
     */
    while let Some(ch) = chars.next() {
        if ch == '!' {
            if matches!(chars.peek(), Some('[')) {
                // 画像リンクは対象外のため末尾まで読み飛ばす
                skip_until_link_end(&mut chars);
            }
            continue;
        }

        if ch != '[' {
            // リンク開始以外の文字は無視する
            continue;
        }

        if !skip_until_char(&mut chars, ']') {
            // ラベル終端が無い場合はリンクとして扱わない
            continue;
        }

        if !matches!(chars.peek(), Some('(')) {
            // 直後がURL部でないものは対象外とする
            continue;
        }
        let _ = chars.next();

        let raw_link = match read_until_paren(&mut chars) {
            Some(link) => link,
            // 閉じ括弧が無い場合は不正リンクとして除外する
            None => continue,
        };

        let raw_link = raw_link.trim();
        if raw_link.is_empty() {
            // URLが空のリンクは対象外とする
            continue;
        }

        if is_schema_link(raw_link) {
            // スキーマ付きリンクは外部参照として除外する
            continue;
        }

        if let Some(normalized) = normalize_page_path(base_path, raw_link) {
            let page_id = resolve(&normalized)?;
            refs.insert(normalized, page_id);
        }
    }

    Ok(refs)
}

///
/// 指定文字が現れるまで読み進める
///
/// # 引数
/// * `iter` - 文字列イテレータ
/// * `target` - 探索対象の文字
///
/// # 戻り値
/// 指定文字が見つかった場合は`true`を返す。
///
fn skip_until_char<I>(iter: &mut std::iter::Peekable<I>, target: char) -> bool
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = iter.next() {
        if ch == target {
            return true;
        }
    }

    false
}

///
/// 閉じ丸括弧までの文字列を取得
///
/// # 引数
/// * `iter` - 文字列イテレータ
///
/// # 戻り値
/// 取得した文字列を返す。閉じ丸括弧が見つからない場合は`None`を返す。
///
fn read_until_paren<I>(iter: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = char>,
{
    /*
     * 取得結果の初期化
     */
    let mut buf = String::new();
    let mut depth = 0usize;

    /*
     * 閉じ括弧までの読み込み
     */
    while let Some(ch) = iter.next() {
        match ch {
            '(' => {
                depth += 1;
                buf.push(ch);
            }
            ')' => {
                if depth == 0 {
                    return Some(buf);
                }
                depth -= 1;
                buf.push(ch);
            }
            _ => buf.push(ch),
        }
    }

    None
}

///
/// Markdownリンクの末尾まで読み飛ばす
///
/// # 引数
/// * `iter` - 文字列イテレータ
///
/// # 戻り値
/// なし
///
fn skip_until_link_end<I>(iter: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    if !skip_until_char(iter, ']') {
        return;
    }

    if !matches!(iter.peek(), Some('(')) {
        return;
    }
    let _ = iter.next();
    let _ = read_until_paren(iter);
}

///
/// スキーマ指定リンクかどうかの判定
///
/// # 引数
/// * `link` - 判定対象のリンク
///
/// # 戻り値
/// スキーマ指定リンクの場合は`true`を返す。
///
fn is_schema_link(link: &str) -> bool {
    let mut chars = link.chars().peekable();
    let mut had_char = false;

    while let Some(ch) = chars.next() {
        if ch == ':' {
            return had_char;
        }

        if ch == '/' || ch.is_whitespace() {
            return false;
        }

        if !is_schema_char(ch) {
            return false;
        }

        had_char = true;
    }

    false
}

///
/// スキーマ指定の文字として許可するかの判定
///
/// # 引数
/// * `ch` - 判定対象の文字
///
/// # 戻り値
/// 許可する文字の場合は`true`を返す。
///
fn is_schema_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.'
}

///
/// ページパスの正規化
///
/// # 引数
/// * `base_path` - 基準となるページパス
/// * `link` - 対象リンク
///
/// # 戻り値
/// 正規化したパスを返す。対象外の場合は`None`を返す。
///
fn normalize_page_path(base_path: &str, link: &str) -> Option<String> {
    /*
     * 事前の判定
     */
    if link.starts_with('/') {
        return Some(cleanup_path(link));
    }

    if link.starts_with('#') {
        return None;
    }

    if link.contains(' ') || link.contains('\t') || link.contains('\n') {
        return None;
    }

    /*
     * 相対パスの解決
     */
    let trimmed = base_path.trim_end_matches('/');
    let base = if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("{}/", trimmed)
    };

    Some(cleanup_path(&format!("{}{}", base, link)))
}

///
/// ページパスの正規化処理
///
/// # 引数
/// * `path` - 対象パス
///
/// # 戻り値
/// 正規化済みのパスを返す。
///
fn cleanup_path(path: &str) -> String {
    /*
     * パスセグメントの収集
     */
    let mut result = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => result.clear(),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.is_empty() {
                    result.pop();
                }
            }
            Component::Normal(name) => result.push(name.to_string_lossy()),
            _ => {}
        }
    }

    /*
     * 正規化パスの構築
     */
    let mut normalized = String::from("/");
    normalized.push_str(&result.join("/"));
    normalized
}

///
/// パスからページIDを解決
///
/// # 引数
/// * `txn` - 書き込みトランザクション
/// * `path` - ページパス
///
/// # 戻り値
/// 解決できたページIDを返す。存在しない場合は`None`を返す。
///
#[allow(dead_code)]
fn resolve_page_id(
    txn: &redb::WriteTransaction,
    path: &str,
) -> Result<Option<PageId>> {
    let table = txn.open_table(PAGE_PATH_TABLE)?;
    resolve_page_id_with_table(&table, path)
}

///
/// ページパステーブルからページIDを解決する
///
/// # 引数
/// * `table` - ページパステーブル
/// * `path` - ページパス
///
/// # 戻り値
/// 解決できたページIDを返す。存在しない場合は`None`を返す。
///
fn resolve_page_id_with_table<'txn>(
    table: &Table<'txn, String, PageId>,
    path: &str,
) -> Result<Option<PageId>> {
    /*
     * インデックス参照
     */
    let key = path.to_string();
    let entry = match table.get(&key)? {
        Some(entry) => entry.value(),
        None => return Ok(None),
    };

    Ok(Some(entry))
}
