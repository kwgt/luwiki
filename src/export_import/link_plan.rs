/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! migrate import 用のリンク解析・書換え計画
//!

use std::collections::HashMap;
use std::path::{Component, Path};

use anyhow::{Result, anyhow, bail};

use crate::database::types::PageId;

use super::model::ExportBundle;

pub(crate) const ABOUT_INVALID_TARGET: &str = "about:invalid";

///
/// リンク問題の種別
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LinkIssueKind {
    TreeExternalPageLink,
    TreeExternalAssetLink,
    AbsolutePageLink,
}

impl LinkIssueKind {
    ///
    /// strict-mode でエラー対象かどうかを返す。
    ///
    /// # 戻り値
    /// エラー対象の場合は true を返す。
    ///
    pub(crate) fn is_strict_error(&self) -> bool {
        matches!(
            self,
            Self::TreeExternalPageLink | Self::AbsolutePageLink
        )
    }

    ///
    /// `about:invalid` 置換対象かどうかを返す。
    ///
    /// # 戻り値
    /// 置換対象の場合は true を返す。
    ///
    pub(crate) fn is_page_rewrite_target(&self) -> bool {
        matches!(
            self,
            Self::TreeExternalPageLink | Self::AbsolutePageLink
        )
    }

    ///
    /// warning コードを返す。
    ///
    /// # 戻り値
    /// warning コード文字列を返す。
    ///
    pub(crate) fn warning_code(&self) -> &'static str {
        match self {
            Self::TreeExternalPageLink => "tree_external_page_link",
            Self::TreeExternalAssetLink => "tree_external_asset_link",
            Self::AbsolutePageLink => "absolute_page_link",
        }
    }
}

///
/// 検出したリンク問題
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinkIssue {
    pub(crate) page: PageId,
    pub(crate) revision: u64,
    pub(crate) kind: LinkIssueKind,
    pub(crate) raw_target: String,
    pub(crate) resolved_path: Option<String>,
}

///
/// 実施予定のリンク書換え
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinkRewriteAction {
    pub(crate) page: PageId,
    pub(crate) revision: u64,
    pub(crate) original_target: String,
    pub(crate) replacement_target: String,
}

///
/// migrate import 用のリンク計画
///
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkRewritePlan {
    pub(crate) issues: Vec<LinkIssue>,
    pub(crate) actions: Vec<LinkRewriteAction>,
}

///
/// migrate import のリンク計画を構築し、必要時はソースを書き換える。
///
/// # 引数
/// * `bundle` - 対象 bundle
/// * `page_final_paths` - page_id ごとの最終配置パス
/// * `destination_root` - migrate 後のルートパス
/// * `strict_mode` - strict-mode 有効時は true
/// * `fix_broken_link` - 破損ページリンクを `about:invalid` へ置換する場合は true
///
/// # 戻り値
/// 生成したリンク計画を返す。
///
pub(crate) fn build_migrate_link_plan(
    bundle: &mut ExportBundle,
    page_final_paths: &HashMap<PageId, String>,
    destination_root: &str,
    strict_mode: bool,
    fix_broken_link: bool,
) -> Result<LinkRewritePlan> {
    /*
     * revision 毎のリンク計画を構築する
     */
    let mut plan = LinkRewritePlan::default();
    for revision in &mut bundle.revisions {
        let base_path = page_final_paths.get(&revision.page).ok_or_else(|| {
            anyhow!(
                "revision page final path is missing: {} revision={}",
                revision.page,
                revision.revision
            )
        })?;

        let scan = with_revision_identity(
            scan_revision_links(base_path, destination_root, &revision.source),
            &revision.page,
            revision.revision,
        );

        if strict_mode {
            if let Some(issue) =
                scan.issues.iter().find(|issue| issue.kind.is_strict_error())
            {
                bail!(
                    "migration link issue is not allowed in strict-mode: {} revision={} kind={} target={}",
                    issue.page,
                    issue.revision,
                    issue.kind.warning_code(),
                    issue.raw_target
                );
            }
        }

        if fix_broken_link && !scan.replacements.is_empty() {
            revision.source =
                apply_replacements(&revision.source, &scan.replacements);
        }

        plan.issues.extend(scan.issues);
        plan.actions.extend(scan.actions);
    }

    Ok(plan)
}

///
/// 1 revision 分のリンク解析結果
///
#[derive(Clone, Debug, Default)]
struct RevisionLinkScan {
    issues: Vec<LinkIssue>,
    actions: Vec<LinkRewriteAction>,
    replacements: Vec<LinkReplacement>,
}

///
/// 文字列置換用の内部表現
///
#[derive(Clone, Debug)]
struct LinkReplacement {
    start: usize,
    end: usize,
    replacement: &'static str,
}

///
/// 1 revision 分のリンクを走査する。
///
/// # 引数
/// * `base_path` - 現 revision の最終配置パス
/// * `destination_root` - migrate 後のルートパス
/// * `source` - Markdown ソース
///
/// # 戻り値
/// 解析結果を返す。
///
fn scan_revision_links(
    base_path: &str,
    destination_root: &str,
    source: &str,
) -> RevisionLinkScan {
    /*
     * 解析結果の初期化
     */
    let mut result = RevisionLinkScan::default();
    let mut iter = source.char_indices().peekable();

    /*
     * Markdown リンクの字句走査
     */
    while let Some((_, ch)) = iter.next() {
        let is_image = if ch == '!' {
            if matches!(iter.peek(), Some((_, '['))) {
                let _ = iter.next();
                true
            } else {
                false
            }
        } else {
            false
        };

        if ch != '[' && !is_image {
            continue;
        }

        if !skip_until_char(&mut iter, ']') {
            continue;
        }

        if !matches!(iter.peek(), Some((_, '('))) {
            continue;
        }
        let _ = iter.next();

        let (raw_target, start, end) = match read_until_paren(&mut iter) {
            Some(parsed) => parsed,
            None => continue,
        };

        let target = raw_target.trim();
        if target.is_empty() {
            continue;
        }

        let issue = if is_asset_link(target) {
            classify_asset_link(base_path, destination_root, target)
        } else {
            classify_page_link(base_path, destination_root, target)
        };

        let Some((kind, resolved_path)) = issue else {
            continue;
        };

        result.issues.push(LinkIssue {
            page: PageId::new(),
            revision: 0,
            kind: kind.clone(),
            raw_target: target.to_string(),
            resolved_path,
        });

        if kind.is_page_rewrite_target() {
            result.actions.push(LinkRewriteAction {
                page: PageId::new(),
                revision: 0,
                original_target: target.to_string(),
                replacement_target: ABOUT_INVALID_TARGET.to_string(),
            });
            result.replacements.push(LinkReplacement {
                start,
                end,
                replacement: ABOUT_INVALID_TARGET,
            });
        }
    }

    result
}

///
/// リンク問題へ page/revision 情報を補う。
///
/// # 引数
/// * `scan` - 解析結果
/// * `page` - 所属 page_id
/// * `revision` - 所属 revision 番号
///
/// # 戻り値
/// page/revision 補完後の結果を返す。
///
fn with_revision_identity(
    mut scan: RevisionLinkScan,
    page: &PageId,
    revision: u64,
) -> RevisionLinkScan {
    for issue in &mut scan.issues {
        issue.page = page.clone();
        issue.revision = revision;
    }
    for action in &mut scan.actions {
        action.page = page.clone();
        action.revision = revision;
    }
    scan
}

///
/// ページリンクの分類を行う。
///
/// # 引数
/// * `base_path` - 現 page の最終配置パス
/// * `destination_root` - migrate 後のルートパス
/// * `target` - リンク先文字列
///
/// # 戻り値
/// 問題がある場合はその種別と解決先を返す。
///
fn classify_page_link(
    base_path: &str,
    destination_root: &str,
    target: &str,
) -> Option<(LinkIssueKind, Option<String>)> {
    if target.starts_with('#') {
        return None;
    }
    if target.contains(' ') || target.contains('\t') || target.contains('\n') {
        return None;
    }
    if is_schema_link(target) {
        return None;
    }

    let path_part = strip_fragment(target);
    if path_part.is_empty() {
        return None;
    }

    let normalized = resolve_page_path(base_path, path_part)?;
    if target.starts_with('/') {
        return Some((LinkIssueKind::AbsolutePageLink, Some(normalized)));
    }
    if !is_path_in_tree(destination_root, &normalized) {
        return Some((LinkIssueKind::TreeExternalPageLink, Some(normalized)));
    }
    None
}

///
/// アセットリンクの分類を行う。
///
/// # 引数
/// * `base_path` - 現 page の最終配置パス
/// * `destination_root` - migrate 後のルートパス
/// * `target` - リンク先文字列
///
/// # 戻り値
/// 問題がある場合はその種別と解決先を返す。
///
fn classify_asset_link(
    base_path: &str,
    destination_root: &str,
    target: &str,
) -> Option<(LinkIssueKind, Option<String>)> {
    let parsed = parse_asset_spec(target)?;
    let resolved = resolve_page_path(base_path, &parsed.path)?;
    if !is_path_in_tree(destination_root, &resolved) {
        return Some((LinkIssueKind::TreeExternalAssetLink, Some(resolved)));
    }
    None
}

///
/// 指定文字まで読み飛ばす。
///
/// # 引数
/// * `iter` - 文字列イテレータ
/// * `target` - 探索対象文字
///
/// # 戻り値
/// 見つかった場合は true を返す。
///
fn skip_until_char<I>(
    iter: &mut std::iter::Peekable<I>,
    target: char,
) -> bool
where
    I: Iterator<Item = (usize, char)>,
{
    while let Some((_, ch)) = iter.next() {
        if ch == target {
            return true;
        }
    }
    false
}

///
/// 閉じ丸括弧までの内容と byte range を取得する。
///
/// # 引数
/// * `iter` - 文字列イテレータ
///
/// # 戻り値
/// `(内容, 開始位置, 終了位置)` を返す。
///
fn read_until_paren<I>(
    iter: &mut std::iter::Peekable<I>,
) -> Option<(String, usize, usize)>
where
    I: Iterator<Item = (usize, char)>,
{
    /*
     * 読み取り状態の初期化
     */
    let mut buf = String::new();
    let mut depth = 0usize;
    let mut start = None;

    /*
     * 閉じ丸括弧まで走査する
     */
    while let Some((idx, ch)) = iter.next() {
        if start.is_none() {
            start = Some(idx);
        }

        match ch {
            '(' => {
                depth += 1;
                buf.push(ch);
            }
            ')' => {
                if depth == 0 {
                    let range_start = start.unwrap_or(idx);
                    return Some((buf, range_start, idx));
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
/// 置換計画をソースへ適用する。
///
/// # 引数
/// * `source` - 元ソース
/// * `replacements` - 置換一覧
///
/// # 戻り値
/// 置換後ソースを返す。
///
fn apply_replacements(
    source: &str,
    replacements: &[LinkReplacement],
) -> String {
    let mut rewritten = source.to_string();
    for replacement in replacements.iter().rev() {
        rewritten.replace_range(
            replacement.start..replacement.end,
            replacement.replacement,
        );
    }
    rewritten
}

///
/// ページ相対パスを解決する。
///
/// # 引数
/// * `base_path` - 基準ページパス
/// * `target_path` - 相対または絶対ターゲット
///
/// # 戻り値
/// 解決済み絶対パスを返す。
///
fn resolve_page_path(
    base_path: &str,
    target_path: &str,
) -> Option<String> {
    if target_path.is_empty() {
        return None;
    }
    if target_path.starts_with('/') {
        return Some(clean_path(target_path));
    }
    if target_path.starts_with('#') {
        return None;
    }

    let normalized_base = if base_path.is_empty() { "/" } else { base_path };
    if target_path == "." {
        return Some(clean_path(normalized_base));
    }

    let base = if normalized_base.ends_with('/') {
        normalized_base.to_string()
    } else {
        format!("{}/", normalized_base)
    };

    Some(clean_path(&format!("{}{}", base, target_path)))
}

///
/// パスを正規化する。
///
/// # 引数
/// * `path_value` - 正規化対象パス
///
/// # 戻り値
/// 正規化済みパスを返す。
///
fn clean_path(path_value: &str) -> String {
    let mut parts = Vec::new();
    for component in Path::new(path_value).components() {
        match component {
            Component::RootDir => parts.clear(),
            Component::CurDir => {}
            Component::ParentDir => {
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            Component::Normal(name) => {
                parts.push(name.to_string_lossy().to_string());
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

///
/// パスが migrate 対象ツリー配下か判定する。
///
/// # 引数
/// * `root` - migrate 後のルートパス
/// * `path` - 判定対象パス
///
/// # 戻り値
/// 配下の場合は true を返す。
///
fn is_path_in_tree(root: &str, path: &str) -> bool {
    if root == "/" {
        return true;
    }
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

///
/// 対象が asset リンクか判定する。
///
/// # 引数
/// * `target` - 判定対象
///
/// # 戻り値
/// asset リンクの場合は true を返す。
///
fn is_asset_link(target: &str) -> bool {
    target.starts_with("asset:")
}

///
/// asset: 指定を分解する。
///
/// # 引数
/// * `raw_spec` - asset: 指定
///
/// # 戻り値
/// 分解できた場合は path/file を返す。
///
fn parse_asset_spec(raw_spec: &str) -> Option<ParsedAssetSpec> {
    if !raw_spec.starts_with("asset:") {
        return None;
    }

    let rest = &raw_spec["asset:".len()..];
    if rest.is_empty() {
        return None;
    }

    if let Some(index) = rest.rfind(':') {
        let file = &rest[index + 1..];
        if file.is_empty() {
            return None;
        }
        return Some(ParsedAssetSpec {
            path: rest[..index].to_string(),
            file: file.to_string(),
        });
    }

    if let Some(index) = rest.rfind('/') {
        let file = &rest[index + 1..];
        if file.is_empty() {
            return None;
        }
        let path = if index == 0 { "." } else { &rest[..index] };
        return Some(ParsedAssetSpec {
            path: path.to_string(),
            file: file.to_string(),
        });
    }

    Some(ParsedAssetSpec {
        path: ".".to_string(),
        file: rest.to_string(),
    })
}

///
/// asset: 指定の分解結果
///
#[derive(Clone, Debug)]
struct ParsedAssetSpec {
    path: String,
    #[allow(dead_code)]
    file: String,
}

///
/// スキーマ付きリンクか判定する。
///
/// # 引数
/// * `target` - 判定対象
///
/// # 戻り値
/// スキーマ付きリンクの場合は true を返す。
///
fn is_schema_link(target: &str) -> bool {
    let mut chars = target.chars().peekable();
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
/// スキーマ文字として許可するか判定する。
///
/// # 引数
/// * `ch` - 判定対象文字
///
/// # 戻り値
/// 許可する場合は true を返す。
///
fn is_schema_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.'
}

///
/// フラグメント部を除去する。
///
/// # 引数
/// * `target` - 元リンク文字列
///
/// # 戻り値
/// `#` より前の文字列を返す。
///
fn strip_fragment(target: &str) -> &str {
    match target.split_once('#') {
        Some((path, _)) => path,
        None => target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::types::{AssetId, UserId};
    use crate::export_import::model::{
        ExportAsset,
        ExportAssetBlob,
        ExportManifest,
        ExportPage,
        ExportRevision,
        ExportType,
        ExportUser,
        ManifestContext,
    };

    #[test]
    fn build_migrate_link_plan_detects_and_rewrites_broken_links() {
        let page_id = PageId::new();
        let user_id = UserId::new();
        let mut bundle = ExportBundle::new(ManifestContext {
            export_type: ExportType::Migrate,
            export_root: "/src".to_string(),
            relocate_prefix: Some("/dst".to_string()),
        });
        bundle.manifest =
            ExportManifest::new(ExportType::Migrate, "/src".to_string());
        bundle.users.push(ExportUser {
            id: user_id.clone(),
            username: "bob".to_string(),
            password: "hash".to_string(),
            salt: [1u8; 16],
            display_name: "Bob".to_string(),
            attributes: crate::database::types::UserAttributeSet::new(),
        });
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: "docs/page".to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: None,
        });
        bundle.revisions.push(ExportRevision {
            page: page_id.clone(),
            revision: 1,
            timestamp: chrono::Local::now(),
            user: user_id.clone(),
            rename: None,
            source: [
                "[rel](../../../outside)",
                "[abs](/src/docs/page)",
                "![asset](asset:../../../other:file.png)",
                "[keep](./child)",
            ]
            .join("\n"),
        });
        bundle.assets.push(ExportAsset {
            id: AssetId::new(),
            page: page_id.clone(),
            file_name: "note.txt".to_string(),
            mime: "text/plain".to_string(),
            size: 5,
            user: user_id,
            timestamp: chrono::Local::now(),
        });
        bundle.asset_blobs.push(ExportAssetBlob {
            asset_id: bundle.assets[0].id.clone(),
            data: b"hello".to_vec(),
        });

        let mut page_final_paths = HashMap::new();
        page_final_paths.insert(page_id, "/dst/docs/page".to_string());

        let plan = build_migrate_link_plan(
            &mut bundle,
            &page_final_paths,
            "/dst",
            false,
            true,
        )
        .expect("build migrate link plan failed");

        assert_eq!(plan.issues.len(), 3);
        assert_eq!(plan.actions.len(), 2);
        assert!(bundle.revisions[0].source.contains("(about:invalid)"));
        assert!(
            bundle.revisions[0]
                .source
                .contains("asset:../../../other:file.png")
        );
        assert!(bundle.revisions[0].source.contains("(./child)"));
    }

    #[test]
    fn build_migrate_link_plan_rejects_strict_mode_page_issues() {
        let page_id = PageId::new();
        let user_id = UserId::new();
        let mut bundle = ExportBundle::new(ManifestContext {
            export_type: ExportType::Migrate,
            export_root: "/src".to_string(),
            relocate_prefix: Some("/dst".to_string()),
        });
        bundle.manifest =
            ExportManifest::new(ExportType::Migrate, "/src".to_string());
        bundle.users.push(ExportUser {
            id: user_id.clone(),
            username: "bob".to_string(),
            password: "hash".to_string(),
            salt: [1u8; 16],
            display_name: "Bob".to_string(),
            attributes: crate::database::types::UserAttributeSet::new(),
        });
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: "".to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: None,
        });
        bundle.revisions.push(ExportRevision {
            page: page_id.clone(),
            revision: 1,
            timestamp: chrono::Local::now(),
            user: user_id,
            rename: None,
            source: "[broken](/outside)".to_string(),
        });

        let mut page_final_paths = HashMap::new();
        page_final_paths.insert(page_id, "/dst".to_string());

        let err = build_migrate_link_plan(
            &mut bundle,
            &page_final_paths,
            "/dst",
            true,
            false,
        )
        .expect_err("strict-mode must reject absolute page link");

        assert!(err.to_string().contains("strict-mode"));
    }
}
