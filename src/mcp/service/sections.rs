/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPサービスのsections処理を定義するモジュール
//!

mod batch;

use batch::TextEdit;

use super::{
    EditPageInsertSectionPlacement,
    EditPageOperation,
    EditPageReplaceTextOccurrence,
    Event,
    HeadingLevel,
    McpError,
    McpErrorCode,
    McpService,
    ParsedHeading,
    Parser,
    SectionSelector,
    Tag,
    TagEnd,
};

// 可読性確保のため、業務別のimplを子モジュールへ分離する。
impl McpService {
    ///
    /// Markdown 本文から TOC / section編集用 section 一覧を構築する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    ///
    /// # 戻り値
    /// 解析した section 一覧を返す。
    ///
    pub(super) fn parse_markdown_toc_sections(
        &self,
        source: &str,
    ) -> Result<Vec<ParsedHeading>, McpError> {
        /*
         * 見出しイベントの収集
         */
        let mut raw_headings = Vec::new();
        let mut current_heading: Option<(u32, usize, String)> = None;
        let parser = Parser::new(source).into_offset_iter();

        for (event, range) in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    current_heading = Some((
                        heading_level_to_u32(level),
                        range.start,
                        String::new(),
                    ));
                }
                Event::End(TagEnd::Heading(_)) => {
                    let Some((level, start, title)) =
                        current_heading.take()
                    else {
                        return Err(McpError::new(
                            McpErrorCode::InternalError,
                            "heading parser state is inconsistent",
                        ));
                    };
                    raw_headings.push((level, start, range.end, title));
                }
                Event::Text(text) | Event::Code(text) => {
                    if let Some((_, _, title)) = current_heading.as_mut() {
                        title.push_str(&text);
                    }
                }
                _ => {}
            }
        }

        /*
         * section メタ情報の構築
         */
        let mut parsed = Vec::new();
        let mut stack: Vec<(u32, String)> = Vec::new();
        for (index, (level, _, heading_end, title)) in
            raw_headings.iter().enumerate()
        {
            while stack
                .last()
                .is_some_and(|(parent_level, _)| *parent_level >= *level)
            {
                stack.pop();
            }

            let id = format!("s-{:03}", index + 1);
            let parent_id = stack.last().map(|(_, id)| id.clone());
            let content_end = raw_headings
                .iter()
                .skip(index + 1)
                .find(|(next_level, _, _, _)| *next_level <= *level)
                .map(|(_, start, _, _)| *start)
                .unwrap_or(source.len());

            parsed.push(ParsedHeading {
                id: id.clone(),
                title: title.trim().to_string(),
                level: *level,
                ordinal: (index + 1) as u32,
                parent_id,
                heading_start: raw_headings[index].1,
                content_start: *heading_end,
                content_end,
            });
            stack.push((*level, id));
        }

        Ok(parsed)
    }

    ///
    /// section 本文を抽出する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `section` - 対象 section
    ///
    /// # 戻り値
    /// section 本文を返す。
    ///
    pub(super) fn extract_section_content(
        &self,
        source: &str,
        section: &ParsedHeading,
    ) -> String {
        source[section.content_start..section.content_end]
            .trim_start_matches(['\r', '\n'])
            .to_string()
    }

    ///
    /// ATX 見出し行なら見出しレベルを返す
    ///
    /// # 引数
    /// * `line` - 判定対象行
    ///
    /// # 戻り値
    /// ATX 見出し行なら見出しレベルを返す。
    ///
    fn atx_heading_line_level(&self, line: &str) -> Option<u32> {
        let trimmed = line.trim_start();
        let level = trimmed.chars().take_while(|ch| *ch == '#').count();
        if level == 0 || level > 6 {
            return None;
        }

        match trimmed.chars().nth(level) {
            None => Some(level as u32),
            Some(ch) if ch.is_whitespace() => Some(level as u32),
            Some(_) => None,
        }
    }

    ///
    /// `replace_section.title` を検証する
    ///
    /// # 引数
    /// * `title` - 入力 title
    ///
    /// # 戻り値
    /// trim 済み title を返す。
    ///
    fn validate_replace_section_title(
        &self,
        title: &str,
    ) -> Result<String, McpError> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "replace_section.title must not be empty",
            ));
        }
        if trimmed.chars().any(char::is_control) {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "replace_section.title must not contain control characters",
            ));
        }
        if self.atx_heading_line_level(trimmed).is_some() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "replace_section.title must not be a Markdown heading line",
            ));
        }

        Ok(trimmed.to_string())
    }

    ///
    /// `replace_section.content` を検証する
    ///
    /// # 引数
    /// * `content` - 入力 content
    /// * `section` - 対象 section
    /// * `effective_title` - 置換後に有効となる見出し文字列
    /// * `include_subsection` - サブセクションを含めるかどうか
    ///
    /// # 戻り値
    /// 検証成功時は `Ok(())` を返す。
    ///
    fn validate_replace_section_content(
        &self,
        content: &str,
        section: &ParsedHeading,
        effective_title: &str,
        include_subsection: bool,
    ) -> Result<(), McpError> {
        let heading_line =
            format!("{} {}", "#".repeat(section.level as usize), effective_title);
        if content
            .lines()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| line.trim() == heading_line)
        {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "replace_section.content must not include the target heading line",
            ));
        }

        for line in content.lines() {
            let Some(level) = self.atx_heading_line_level(line) else {
                continue;
            };

            if !include_subsection {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    "replace_section.content must not include Markdown heading lines",
                ));
            }
            if level <= section.level {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    "replace_section.content must not include same or higher level headings",
                ));
            }
        }

        Ok(())
    }

    ///
    /// `replace_section` の本文置換終了位置を返す
    ///
    /// # 引数
    /// * `sections` - 解析済み section 一覧
    /// * `section` - 対象 section
    /// * `include_subsection` - サブセクションを含めるかどうか
    ///
    /// # 戻り値
    /// 本文置換終了 byte offset を返す。
    ///
    fn resolve_replace_section_body_end(
        &self,
        sections: &[ParsedHeading],
        section: &ParsedHeading,
        include_subsection: bool,
    ) -> usize {
        if include_subsection {
            return section.content_end;
        }

        sections
            .iter()
            .find(|candidate| {
                candidate.heading_start >= section.content_start
                    && candidate.heading_start < section.content_end
                    && candidate.level > section.level
            })
            .map(|candidate| candidate.heading_start)
            .unwrap_or(section.content_end)
    }

    ///
    /// section 本文を置き換える
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `sections` - 解析済み section 一覧
    /// * `section` - 置き換え対象 section
    /// * `new_title` - 置き換え後見出し文字列
    /// * `new_content` - 置き換え後本文
    /// * `include_subsection` - サブセクションを含めるかどうか
    ///
    /// # 戻り値
    /// 対象見出し行を保持し、本文部分だけを差し替えた Markdown 本文を返す。
    ///
    #[cfg(test)]
    pub(super) fn replace_section_content(
        &self,
        source: &str,
        sections: &[ParsedHeading],
        section: &ParsedHeading,
        new_title: Option<&str>,
        new_content: &str,
        include_subsection: bool,
    ) -> Result<String, McpError> {
        let edits = self.replace_section_edits(
            source, sections, section, new_title, new_content,
            include_subsection,
        )?;
        batch::apply_untracked(source, edits)
    }

    ///
    /// 見出しと本文の置換差分を後方から生成する。
    ///
    /// # 引数
    /// * `source` - 現在本文
    /// * `sections` - 現在見出し
    /// * `section` - 対象
    /// * `new_title` - 任意の見出し変更
    /// * `new_content` - 新本文
    /// * `include_subsection` - 子節も置換するか
    ///
    /// # 戻り値
    /// 本文、見出しの順の差分。
    ///
    /// # 概要
    /// 既存の空行保持と見出し検証を位置追跡でも共有する。
    ///
    fn replace_section_edits(
        &self,
        source: &str,
        sections: &[ParsedHeading],
        section: &ParsedHeading,
        new_title: Option<&str>,
        new_content: &str,
        include_subsection: bool,
    ) -> Result<Vec<TextEdit>, McpError> {
        /*
         * 既存の見出し・本文制約を検証する
         */
        let effective_title = match new_title {
            Some(title) => self.validate_replace_section_title(title)?,
            None => section.title.clone(),
        };
        self.validate_replace_section_content(
            new_content,
            section,
            &effective_title,
            include_subsection,
        )?;

        let body_end = self.resolve_replace_section_body_end(
            sections,
            section,
            include_subsection,
        );
        let heading_line_end = source[section.heading_start..]
            .find('\n')
            .map(|offset| section.heading_start + offset)
            .unwrap_or(section.content_start);
        let heading_line_break = &source[heading_line_end..section.content_start];
        let target_body = &source[section.content_start..body_end];

        /*
         * 従来の前後空行を保持して本文差分を組み立てる
         */
        let leading_end = target_body
            .char_indices()
            .find(|(_, ch)| !matches!(*ch, '\r' | '\n'))
            .map(|(index, _)| index)
            .unwrap_or(target_body.len());
        let trailing_start = target_body
            .char_indices()
            .rev()
            .find(|(_, ch)| !matches!(*ch, '\r' | '\n'))
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);
        let leading_breaks = &target_body[..leading_end];
        let trailing_breaks = if trailing_start < leading_end {
            ""
        } else {
            &target_body[trailing_start..]
        };
        let heading_line =
            format!("{} {}", "#".repeat(section.level as usize), effective_title);

        /*
         * 後方の本文を先に変更して見出し差分の座標を保つ
         */
        Ok(vec![
            TextEdit::new(
                section.content_start..body_end,
                format!("{leading_breaks}{new_content}{trailing_breaks}"),
            ),
            TextEdit::new(
                section.heading_start..section.content_start,
                format!("{heading_line}{heading_line_break}"),
            ),
        ])
    }

    ///
    /// section を挿入する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `anchor` - 挿入位置の基準 section
    /// * `placement` - anchor に対する前後指定
    /// * `new_section` - 挿入する完全なセクション本文
    ///
    /// # 戻り値
    /// 指定位置へ section を挿入した Markdown 本文を返す。
    ///
    #[cfg(test)]
    pub(super) fn insert_section_content(
        &self,
        source: &str,
        anchor: &ParsedHeading,
        placement: EditPageInsertSectionPlacement,
        new_section: &str,
    ) -> String {
        let edit = self.insert_section_edit(source, anchor, placement,
            new_section);
        format!("{}{}{}", &source[..edit.range.start], edit.replacement,
            &source[edit.range.end..])
    }

    ///
    /// 挿入位置と既存の改行補完を差分として返す。
    ///
    /// # 引数
    /// * `source` - 現在本文
    /// * `anchor` - 基準見出し
    /// * `placement` - 前後指定
    /// * `new_section` - 挿入本文
    ///
    /// # 戻り値
    /// ゼロ幅の挿入差分。
    ///
    fn insert_section_edit(
        &self, source: &str, anchor: &ParsedHeading,
        placement: EditPageInsertSectionPlacement, new_section: &str,
    ) -> TextEdit {
        let at = match placement {
            EditPageInsertSectionPlacement::Before => anchor.heading_start,
            EditPageInsertSectionPlacement::After => anchor.content_end,
        };
        let mut replacement = new_section.to_string();
        if !new_section.ends_with('\n') && !source[at..].starts_with('\n') {
            replacement.push('\n');
        }
        TextEdit::new(at..at, replacement)
    }

    ///
    /// section を削除する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `section` - 削除対象 section
    ///
    /// # 戻り値
    /// 対象 section を削除した Markdown 本文を返す。
    ///
    /// # 注記
    /// 削除範囲は見出し行を含む section 全体とし、子見出し配下もまとめて削除する。
    ///
    #[cfg(test)]
    pub(super) fn delete_section_content(
        &self,
        source: &str,
        section: &ParsedHeading,
    ) -> String {
        let edit = TextEdit::new(
            section.heading_start..section.content_end, String::new(),
        );
        format!("{}{}", &source[..edit.range.start], &source[edit.range.end..])
    }

    ///
    /// テキスト置換を適用する
    ///
    /// # 引数
    /// * `source` - Markdown 本文
    /// * `old_text` - 置換対象文字列
    /// * `new_text` - 置換後文字列
    /// * `occurrence` - 複数一致時の対象指定
    ///
    /// # 戻り値
    /// 置換後の Markdown 本文を返す。
    ///
    /// # 注記
    /// `occurrence = None` は `First` と同じ意味で扱う。
    ///
    #[cfg(test)]
    pub(super) fn replace_text_content(
        &self,
        source: &str,
        old_text: &str,
        new_text: &str,
        occurrence: Option<EditPageReplaceTextOccurrence>,
    ) -> String {
        let mut result = source.to_string();
        for edit in self.replace_text_edits(source, old_text, new_text,
            occurrence)
        {
            result.replace_range(edit.range, &edit.replacement);
        }
        result
    }

    ///
    /// 操作開始時の文字列一致を後方順の差分へ変換する。
    ///
    /// # 引数
    /// * `source` - 現在本文
    /// * `old_text` - 検索文字列
    /// * `new_text` - 置換文字列
    /// * `occurrence` - firstまたはall
    ///
    /// # 戻り値
    /// 座標を変えずに適用できる後方順差分。
    ///
    /// # 概要
    /// 置換開始時の一致だけを列挙し、挿入した文字列を再探索しない。
    ///
    fn replace_text_edits(
        &self, source: &str, old_text: &str, new_text: &str,
        occurrence: Option<EditPageReplaceTextOccurrence>,
    ) -> Vec<TextEdit> {
        /*
         * first/allの一致を列挙して座標が壊れない後方順にする
         */
        let count = match occurrence.unwrap_or(
            EditPageReplaceTextOccurrence::First,
        ) {
            EditPageReplaceTextOccurrence::First => 1,
            EditPageReplaceTextOccurrence::All => usize::MAX,
        };
        let mut edits = source.match_indices(old_text).take(count)
            .map(|(start, value)| TextEdit::new(
                start..start + value.len(), new_text.to_string(),
            ))
            .collect::<Vec<_>>();
        edits.reverse();
        edits
    }

    ///
    /// `edit_page` の単一 operation を本文へ適用する
    ///
    /// # 引数
    /// * `source` - 変換対象の Markdown 本文
    /// * `operation` - 適用する編集操作
    ///
    /// # 戻り値
    /// 変換後の Markdown 本文を返す。
    ///
    /// # 注記
    /// operation 共通の失敗分類は本 helper で固定する。
    /// - selector 解決失敗は `resolve_section_selector()` の分類をそのまま使う
    /// - `replace_text.old_text` が空文字なら `invalid_input`
    /// - `replace_text` で一致箇所が 0 件なら `not_found`
    ///
    #[cfg(test)]
    pub(super) fn apply_edit_page_operation(
        &self,
        source: &str,
        operation: &EditPageOperation,
    ) -> Result<String, McpError> {
        self.apply_edit_page_operations(source, std::slice::from_ref(operation))
    }

    ///
    /// section selector を解決する
    ///
    /// # 引数
    /// * `sections` - 解析済み section 一覧
    /// * `selector` - section selector
    ///
    /// # 戻り値
    /// 解決した section を返す。
    ///
    /// # 注記
    /// 本 helper は `get_page_section` だけでなく、`edit_page` における
    /// `replace_section` / `insert_section` / `delete_section` の selector 解決でも
    /// 共通利用する前提とする。
    /// `ById` と `ByTitle` の両方をここで扱い、`ByTitle` の空文字拒否、
    /// 見出し未存在時の `not_found`、同名複数時の `invalid_input` も
    /// `edit_page` 側で再利用する。
    /// したがって selector 解決失敗の分類は以下で固定する。
    /// - `ById` 未存在: `not_found`
    /// - `ByTitle` 未存在: `not_found`
    /// - `ByTitle` 空文字: `invalid_input`
    /// - `ByTitle` 複数一致: `invalid_input`
    ///
    pub(super) fn resolve_section_selector(
        &self,
        sections: &[ParsedHeading],
        selector: SectionSelector,
    ) -> Result<ParsedHeading, McpError> {
        match selector {
            SectionSelector::ById(id) => sections
                .iter()
                .find(|section| section.id == id)
                .cloned()
                .ok_or_else(|| {
                    McpError::new(
                        McpErrorCode::NotFound,
                        format!("section not found: {}", id),
                    )
                }),
            SectionSelector::ByTitle(title) => {
                let title = title.trim();
                if title.is_empty() {
                    return Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        "section title must not be empty",
                    ));
                }

                let matches = sections
                    .iter()
                    .filter(|section| section.title == title)
                    .cloned()
                    .collect::<Vec<_>>();
                match matches.len() {
                    0 => Err(McpError::new(
                        McpErrorCode::NotFound,
                        format!("section not found: {}", title),
                    )),
                    1 => Ok(matches[0].clone()),
                    /*
                     * 同名見出しが複数ある場合は selector として一意に解決できないため、
                     * `get_page_section` / `edit_page` の双方で `invalid_input` とする。
                     */
                    _ => Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        format!("section title is ambiguous: {}", title),
                    )),
                }
            }
        }
    }
}

///
/// 見出しレベルを数値へ変換する
///
/// # 引数
/// * `level` - pulldown-cmark の見出しレベル
///
/// # 戻り値
/// 数値化した見出しレベルを返す。
///
fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

