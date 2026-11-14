/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 一要求内の初期見出し同一性と編集差分を管理する。
//!

use std::ops::Range;

use super::{
    EditPageOperation, McpError, McpErrorCode, McpService, ParsedHeading,
};

#[cfg(test)]
mod tests;

///
/// 現在本文に適用する半開byte範囲の差分。
///
pub(super) struct TextEdit {
    /// 置換前の範囲。
    pub(super) range: Range<usize>,
    /// 範囲へ挿入する文字列。
    pub(super) replacement: String,
}

impl TextEdit {
    ///
    /// 差分を生成する。
    ///
    /// # 引数
    /// * `range` - 置換範囲
    /// * `replacement` - 新文字列
    ///
    /// # 戻り値
    /// 編集差分。
    ///
    pub(super) fn new(range: Range<usize>, replacement: String) -> Self {
        Self { range, replacement }
    }
}

///
/// 初期見出しの単調な追跡状態。
///
enum HeadingState {
    /// 元見出しの現在範囲を保持する。
    Alive(Range<usize>),
    /// 節操作によって削除された。
    Deleted,
    /// 構文や見出し行変更で対応を証明できなくなった。
    Unresolvable,
}

///
/// 初期identityに対応する見出し。
///
struct TrackedHeading {
    /// 初期の見出しレベル。
    level: u32,
    /// 現在範囲または終端状態。
    state: HeadingState,
}

impl TrackedHeading {
    ///
    /// 初期見出しから追跡対象を生成する。
    ///
    /// # 引数
    /// * `heading` - 初期見出し
    ///
    /// # 戻り値
    /// Alive状態の元対象。
    ///
    fn new(heading: &ParsedHeading) -> Self {
        Self {
            level: heading.level,
            state: HeadingState::Alive(
                heading.heading_start..heading.content_start,
            ),
        }
    }

    ///
    /// 差分で元対象の位置または終端状態を更新する。
    ///
    /// # 引数
    /// * `edit` - 適用差分
    /// * `destructive` - 節操作による除去か
    ///
    /// # 戻り値
    /// 成功、または位置の桁溢れ。
    ///
    /// # 概要
    /// 先頭は右、末尾は左affinityとし、終端状態を復活させない。
    ///
    fn update(&mut self, edit: &TextEdit, destructive: bool)
        -> Result<(), McpError>
    {
        let HeadingState::Alive(span) = &mut self.state else {
            return Ok(());
        };

        /*
         * 範囲より前の変更だけを両端へ同じ量だけ反映する
         */
        if edit.range.end <= span.start {
            span.start = shift_position(span.start, edit)?;
            span.end = shift_position(span.end, edit)?;
        } else if edit.range.start < span.end {
            self.state = if destructive
                && edit.range.start <= span.start
                && span.end <= edit.range.end
            {
                HeadingState::Deleted
            } else {
                HeadingState::Unresolvable
            };
        }
        Ok(())
    }
}

///
/// 変更範囲より後の位置をchecked計算で移動する。
///
/// # 引数
/// * `position` - 元位置
/// * `edit` - 適用差分
///
/// # 戻り値
/// 新位置、または内部範囲エラー。
///
fn shift_position(position: usize, edit: &TextEdit)
    -> Result<usize, McpError>
{
    position.checked_sub(edit.range.end - edit.range.start)
        .and_then(|value| value.checked_add(edit.replacement.len()))
        .ok_or_else(invalid_range)
}

///
/// 内部byte範囲の不整合を生成する。
///
/// # 戻り値
/// 固定文言の内部エラー。
///
fn invalid_range() -> McpError {
    McpError::new(McpErrorCode::InternalError, "invalid edit byte range")
}

///
/// UTF-8の半開範囲を検証する。
///
/// # 引数
/// * `source` - 本文
/// * `range` - 対象範囲
///
/// # 戻り値
/// 範囲が有効なら成功。
///
fn validate_range(source: &str, range: &Range<usize>)
    -> Result<(), McpError>
{
    if range.start > range.end || range.end > source.len()
        || !source.is_char_boundary(range.start)
        || !source.is_char_boundary(range.end)
    {
        return Err(invalid_range());
    }
    Ok(())
}

///
/// 位置追跡を必要としない単独変換にも同じ差分を適用する。
///
/// # 引数
/// * `source` - 元本文
/// * `edits` - 後方順差分
///
/// # 戻り値
/// 変換本文または範囲不整合。
///
#[cfg(test)]
pub(super) fn apply_untracked(source: &str, edits: Vec<TextEdit>)
    -> Result<String, McpError>
{
    let mut result = source.to_string();
    for edit in edits {
        validate_range(&result, &edit.range)?;
        result.replace_range(edit.range, &edit.replacement);
    }
    Ok(result)
}

// 要求内追跡をsectionsの子moduleへまとめて可読性を保つ。
impl McpService {
    ///
    /// 初期selectorへ結び付けた操作列を一度の保存前に適用する。
    ///
    /// # 引数
    /// * `source` - 整合確認済み初期本文
    /// * `operations` - 適用順の操作列
    ///
    /// # 戻り値
    /// 最終本文、または1始まりの操作番号付きエラー。
    ///
    /// # 概要
    /// bindingエラーを実行順まで遅延し、位置更新後の再解析で誤対象を防ぐ。
    ///
    pub(in crate::mcp::service) fn apply_edit_page_operations(
        &self, source: &str, operations: &[EditPageOperation],
    ) -> Result<String, McpError> {
        /*
         * 空要求を拒否し、初期対象へのbindingを保存する
         */
        if operations.is_empty() {
            return Err(McpError::new(McpErrorCode::InvalidInput,
                "operation must not be empty"));
        }
        let initial = self.parse_markdown_toc_sections(source)?;
        let bindings = operations.iter().map(|operation| {
            let selector = match operation {
                EditPageOperation::ReplaceSection { section, .. }
                | EditPageOperation::DeleteSection { section } => section,
                EditPageOperation::InsertSection { anchor, .. } => anchor,
                EditPageOperation::ReplaceText { .. } => return None,
            };
            Some(self.resolve_section_selector(&initial, selector.clone())
                .map(|heading| heading.ordinal as usize - 1))
        }).collect::<Vec<_>>();
        let mut tracked = initial.iter().map(TrackedHeading::new)
            .collect::<Vec<_>>();
        let mut current = source.to_string();

        /*
         * 操作の意味検証と差分適用を配列順に行う
         */
        for (index, operation) in operations.iter().enumerate() {
            let result = (|| {
                let identity = bindings[index].clone().transpose()?;
                self.apply_bound_operation(&mut current, &mut tracked,
                    operation, identity)
            })();
            result.map_err(|error: McpError| McpError::new(error.code(),
                format!("operation {} failed: {}", index + 1,
                    error.message())))?;
        }
        Ok(current)
    }

    ///
    /// 一操作の現在範囲を解決し、差分を追跡へ反映する。
    ///
    /// # 引数
    /// * `source` - 現在本文
    /// * `tracked` - 初期identityの追跡表
    /// * `operation` - 操作
    /// * `identity` - 初期binding
    ///
    /// # 戻り値
    /// 更新成功または既存操作エラー。
    ///
    /// # 概要
    /// 現在の親子範囲を再解析で求め、title変更の対象だけを明示保持する。
    ///
    fn apply_bound_operation(
        &self, source: &mut String, tracked: &mut [TrackedHeading],
        operation: &EditPageOperation, identity: Option<usize>,
    ) -> Result<(), McpError> {
        /*
         * 元identityの現在対象を解決して後方順差分を生成する
         */
        let sections = self.parse_markdown_toc_sections(source)?;
        let target = identity.map(|id| current_heading(&tracked[id],
            &sections)).transpose()?;
        let edits = match operation {
            EditPageOperation::ReplaceSection {
                title, content, include_subsection, ..
            } => self.replace_section_edits(source, &sections,
                target.as_ref().ok_or_else(invalid_range)?,
                title.as_deref(), content,
                include_subsection.unwrap_or(false))?,
            EditPageOperation::InsertSection { placement, content, .. } => {
                vec![self.insert_section_edit(source,
                    target.as_ref().ok_or_else(invalid_range)?,
                    *placement, content)]
            }
            EditPageOperation::DeleteSection { .. } => {
                let target = target.as_ref().ok_or_else(invalid_range)?;
                vec![TextEdit::new(
                    target.heading_start..target.content_end, String::new(),
                )]
            }
            EditPageOperation::ReplaceText { old_text, new_text,
                occurrence } => {
                if old_text.is_empty() {
                    return Err(McpError::new(McpErrorCode::InvalidInput,
                        "old_text must not be empty"));
                }
                if !source.contains(old_text) {
                    return Err(McpError::new(McpErrorCode::NotFound,
                        format!("text not found: {}", old_text)));
                }
                self.replace_text_edits(source, old_text, new_text, *occurrence)
            }
        };

        /*
         * 本文差分を先に適用し、対象見出しだけの明示保持を記録する
         */
        let preserve = if matches!(operation,
            EditPageOperation::ReplaceSection { .. })
        { identity } else { None };
        let mut preserved_start = None;
        for (edit_index, edit) in edits.iter().enumerate() {
            validate_range(source, &edit.range)?;
            if source[edit.range.clone()] == edit.replacement {
                continue;
            }
            for (id, heading) in tracked.iter_mut().enumerate() {
                if preserve == Some(id) && edit_index == 1 {
                    preserved_start = Some(edit.range.start);
                    continue;
                }
                let destructive = !matches!(operation,
                    EditPageOperation::ReplaceText { .. });
                heading.update(edit, destructive)?;
            }
            source.replace_range(edit.range.clone(), &edit.replacement);
        }

        /*
         * 変更後の構文に一致する元見出しだけをAliveとして残す
         */
        let parsed = self.parse_markdown_toc_sections(source)?;
        for (id, heading) in tracked.iter_mut().enumerate() {
            if !matches!(heading.state, HeadingState::Alive(_)) {
                continue;
            }
            if preserve == Some(id) && let Some(start) = preserved_start {
                heading.state = match parsed.iter().find(|item|
                    item.heading_start == start && item.level == heading.level)
                {
                    Some(item) => HeadingState::Alive(
                        item.heading_start..item.content_start),
                    None => HeadingState::Unresolvable,
                };
            } else if current_heading(heading, &parsed).is_err() {
                heading.state = HeadingState::Unresolvable;
            }
        }
        Ok(())
    }
}

///
/// 初期対象の現在の解析見出しを返す。
///
/// # 引数
/// * `heading` - 元対象の状態
/// * `sections` - 現在の解析見出し
///
/// # 戻り値
/// 現在対象、削除not_found、または追跡不能invalid_input。
///
fn current_heading(heading: &TrackedHeading, sections: &[ParsedHeading])
    -> Result<ParsedHeading, McpError>
{
    match &heading.state {
        HeadingState::Deleted => Err(McpError::new(
            McpErrorCode::NotFound, "original section was deleted")),
        HeadingState::Alive(span) => sections.iter().find(|item|
            item.heading_start == span.start && item.content_start == span.end
                && item.level == heading.level)
            .cloned().ok_or_else(unresolvable_heading),
        HeadingState::Unresolvable => Err(unresolvable_heading()),
    }
}

///
/// 元見出しの追跡不能エラーを返す。
///
/// # 戻り値
/// 誤対象への適用を防ぐ入力エラー。
///
fn unresolvable_heading() -> McpError {
    McpError::new(McpErrorCode::InvalidInput,
        "original section identity is no longer resolvable")
}
