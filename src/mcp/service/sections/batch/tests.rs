/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 初期見出し追跡の複合編集を検証する。
//!

use super::*;
use crate::mcp::service::{
    EditPageInsertSectionPlacement as Placement,
    EditPageReplaceTextOccurrence as Occurrence,
    SectionSelector,
};

///
/// 初期ID selectorを生成する。
///
/// # 引数
/// * `value` - 初期ID
///
/// # 戻り値
/// ID selector。
///
fn id(value: &str) -> SectionSelector {
    SectionSelector::ById(value.to_string())
}

///
/// 本文置換操作を生成する。
///
/// # 引数
/// * `section` - 初期selector
/// * `content` - 新本文
///
/// # 戻り値
/// 子節を保持する置換。
///
fn replace(section: SectionSelector, content: &str) -> EditPageOperation {
    EditPageOperation::ReplaceSection {
        section, title: None, content: content.to_string(),
        include_subsection: None,
    }
}

///
/// 文字列置換操作を生成する。
///
/// # 引数
/// * `old` - 検索文字列
/// * `new` - 新文字列
///
/// # 戻り値
/// 最初の一致を置換する操作。
///
fn text(old: &str, new: &str) -> EditPageOperation {
    EditPageOperation::ReplaceText {
        old_text: old.to_string(), new_text: new.to_string(), occurrence: None,
    }
}

///
/// 挿入操作を生成する。
///
/// # 引数
/// * `anchor` - 初期selector
/// * `placement` - 前後
/// * `content` - 挿入本文
///
/// # 戻り値
/// 挿入操作。
///
fn insert(anchor: SectionSelector, placement: Placement, content: &str)
    -> EditPageOperation
{
    EditPageOperation::InsertSection {
        anchor, placement, content: content.to_string(),
    }
}

///
/// T02/T04: 挿入後も初期IDと一意titleは元対象を指す。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 先行挿入で採番とtitle一意性を変えても元Bだけを更新する。
///
#[test]
fn initial_identity_survives_insertion_and_duplicate_title() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    for selector in [id("s-002"), SectionSelector::ByTitle("B".into())] {
        let result = service.apply_edit_page_operations(
            "# A\na\n# B\nb\n", &[
                insert(id("s-001"), Placement::Before, "# B\nnew\n"),
                replace(selector, "changed"),
            ],
        ).unwrap();
        assert_eq!(result, "# B\nnew\n# A\na\n# B\nchanged\n");
    }
}

///
/// T03: title変更後も初期selectorを保持し、新titleを初期解決しない。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 同じ元対象へ二度適用し、新title指定だけは2操作目で拒否する。
///
#[test]
fn explicit_title_change_preserves_original_binding() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    let rename = EditPageOperation::ReplaceSection {
        section: id("s-001"), title: Some("Z".into()),
        content: "one".into(), include_subsection: None,
    };
    let result = service.apply_edit_page_operations("# A\na\n", &[
        rename.clone(), replace(SectionSelector::ByTitle("A".into()), "two"),
    ]).unwrap();
    assert_eq!(result, "# Z\ntwo\n");
    let error = service.apply_edit_page_operations("# A\na\n", &[
        rename, replace(SectionSelector::ByTitle("Z".into()), "two"),
    ]).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::NotFound);
    assert!(error.message().starts_with("operation 2 failed:"));
}

///
/// T04/T12: 初期曖昧性と後続selectorエラーを操作順で扱う。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 前の操作の失敗を後続bindingエラーが追い越さない。
///
#[test]
fn binding_errors_are_delayed_until_their_operation() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    let ambiguous = replace(SectionSelector::ByTitle("B".into()), "new");
    let error = service.apply_edit_page_operations("# B\na\n# B\nb\n", &[
        text("missing", "x"), ambiguous.clone(),
    ]).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::NotFound);
    assert!(error.message().starts_with("operation 1 failed:"));
    let error = service.apply_edit_page_operations("# B\na\n# B\nb\n", &[
        EditPageOperation::DeleteSection { section: id("s-001") }, ambiguous,
    ]).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::InvalidInput);
}

///
/// T05/T06: 現在の親範囲と実際に除去した子だけを削除状態にする。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 本文のみ置換と子節込み置換の後続子参照を比較する。
///
#[test]
fn parent_replacement_preserves_or_deletes_original_children() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    let source = "# P\np\n## C\nc\n# B\nb\n";
    for include in [false, true] {
        let result = service.apply_edit_page_operations(source, &[
            EditPageOperation::ReplaceSection {
                section: id("s-001"), title: None, content: "parent".into(),
                include_subsection: Some(include),
            },
            replace(id("s-002"), "child"),
        ]);
        if include {
            assert_eq!(result.unwrap_err().code(), McpErrorCode::NotFound);
        } else {
            assert_eq!(result.unwrap(), "# P\nparent\n## C\nchild\n# B\nb\n");
        }
    }
    let result = service.apply_edit_page_operations("# P\n## C\nc\n", &[
        EditPageOperation::ReplaceSection {
            section: id("s-001"), title: None, content: "## C\nc".into(),
            include_subsection: Some(true),
        }, replace(id("s-002"), "kept"),
    ]).unwrap();
    assert_eq!(result, "# P\n## C\nkept\n");
}

///
/// T05/T11: 新設節を含む現在階層から削除範囲を決定する。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 元の子だけを削除する場合と親を削除する場合を比較する。
///
#[test]
fn deletion_uses_current_parent_and_sibling_ranges() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    let source = "# P\np\n## C\nc\n# B\nb\n";
    for (target, expected) in [
        ("s-002", "# P\np\n## X\nx\n# B\nb\n"),
        ("s-001", "# B\nb\n"),
    ] {
        let result = service.apply_edit_page_operations(source, &[
            insert(id("s-002"), Placement::Before, "## X\nx\n"),
            EditPageOperation::DeleteSection { section: id(target) },
        ]).unwrap();
        assert_eq!(result, expected);
    }
    let result = service.apply_edit_page_operations("# A\na\n# B\nb\n", &[
        insert(id("s-001"), Placement::After, "# X\nx\n"),
        EditPageOperation::DeleteSection { section: id("s-001") },
    ]).unwrap();
    assert_eq!(result, "# X\nx\n# B\nb\n");
    let error = service.apply_edit_page_operations(source, &[
        EditPageOperation::DeleteSection { section: id("s-001") },
        replace(id("s-002"), "wrong"),
    ]).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::NotFound);
}

///
/// T07/T08: 新設本文は文字列置換でき、allは開始時の一致だけを使う。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 新設節のsection参照拒否も確認する。
///
#[test]
fn inserted_text_is_editable_without_new_heading_binding() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    let insert_x = insert(id("s-001"), Placement::Before, "# X\na a\n");
    let result = service.apply_edit_page_operations("# A\nb\n", &[
        insert_x.clone(), EditPageOperation::ReplaceText {
            old_text: "a".into(), new_text: "aa".into(),
            occurrence: Some(Occurrence::All),
        },
    ]).unwrap();
    assert_eq!(result, "# X\naa aa\n# A\nb\n");
    let error = service.apply_edit_page_operations("# A\nb\n", &[
        insert_x, replace(SectionSelector::ByTitle("X".into()), "wrong"),
    ]).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::NotFound);
}

///
/// T09/T10: 見出し行変更と解析文脈消失は元対象を復活させない。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 同一文字列では保持し、フェンスで隠れた対象は拒否する。
///
#[test]
fn rewritten_or_hidden_heading_is_unresolvable() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    for first in [text("# A", "# Z"), text("intro", "```\nintro")] {
        let error = service.apply_edit_page_operations("intro\n# A\na\n", &[
            first, replace(id("s-001"), "wrong"),
        ]).unwrap_err();
        assert_eq!(error.code(), McpErrorCode::InvalidInput);
    }
    let result = service.apply_edit_page_operations("# A\na\n", &[
        text("# A", "# A"), replace(id("s-001"), "ok"),
    ]).unwrap();
    assert_eq!(result, "# A\nok\n");
    let error = service.apply_edit_page_operations("intro\n# A\na\n", &[
        text("intro", "```\nintro"), text("```\nintro", "intro"),
        replace(id("s-001"), "wrong"),
    ]).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::InvalidInput);
}

///
/// T05/T10: 削除後の同じ本文・同じ位置の新設節へ再結合しない。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 元Aを削除し同名Aを挿入しても初期identityはDeletedを保つ。
///
#[test]
fn deleted_identity_does_not_rebind_to_recreated_heading() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let error = McpService::new().apply_edit_page_operations(
        "# A\na\n# B\nb\n", &[
            EditPageOperation::DeleteSection { section: id("s-001") },
            insert(id("s-002"), Placement::Before, "# A\na\n"),
            replace(id("s-001"), "wrong"),
        ],
    ).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::NotFound);
}

///
/// T11: 同じanchorの境界affinityと連続挿入順を確認する。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 初期IDを二度参照しbeforeとafterを比較する。
///
#[test]
fn repeated_insertions_keep_original_anchor_and_current_end() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    for (placement, expected) in [
        (Placement::Before, "# X\n# Y\n# A\n# B\n"),
        (Placement::After, "# A\n# Y\n# X\n# B\n"),
    ] {
        let result = service.apply_edit_page_operations("# A\n# B\n", &[
            insert(id("s-001"), placement, "# X\n"),
            insert(id("s-001"), placement, "# Y\n"),
        ]).unwrap();
        assert_eq!(result, expected);
    }
}

///
/// T11: 日本語byte範囲・Setextと末尾改行なしの対象を保持する。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// Setextのtitle変更後に初期対象と後続節へ操作する。
///
#[test]
fn utf8_setext_and_missing_final_newline_keep_identity() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let service = McpService::new();
    let result = service.apply_edit_page_operations(
        "概要\n====\n本文\n# 次\n末尾", &[
            EditPageOperation::ReplaceSection {
                section: id("s-001"), title: Some("説明".into()),
                content: "新".into(), include_subsection: None,
            },
            replace(id("s-001"), "再"), replace(id("s-002"), "最後"),
        ],
    ).unwrap();
    assert!(result.starts_with("# 説明\n再\n# 次\n"));
    assert!(result.ends_with("最後"));
    let result = service.apply_edit_page_operations(
        "# **概要** `code`\n本文\n# 次\n末尾", &[
            replace(id("s-001"), "新"),
            replace(SectionSelector::ByTitle("概要 code".into()), "再"),
        ],
    ).unwrap();
    assert!(result.starts_with("# 概要 code\n再\n"));
}

///
/// T11: 大きいUTF-8本文と複数操作でも本文と追跡対象を保つ。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 256KiB超の本文に32回の同一文字列置換後、元節を参照する。
///
#[test]
fn representative_large_source_and_operation_array() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let source = format!("# 概要\n{}\n# 次\n本文\n", "日本語".repeat(32768));
    let mut operations = vec![text("日本語", "日本語"); 32];
    operations.push(replace(id("s-002"), "完了"));
    let result = McpService::new().apply_edit_page_operations(
        &source, &operations,
    ).unwrap();
    assert!(result.ends_with("# 次\n完了\n"));
    assert_eq!(result.matches("日本語").count(), 32768);
}

///
/// byte境界とchecked位置計算の失敗を保存前に検出する。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// UTF-8中間byteと桁溢れを内部エラーとして確認する。
///
#[test]
fn invalid_byte_boundaries_and_overflow_are_rejected() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    assert_eq!(apply_untracked("あ", vec![TextEdit::new(
        1..2, "x".into(),
    )]).unwrap_err().code(), McpErrorCode::InternalError);
    assert!(shift_position(usize::MAX,
        &TextEdit::new(0..0, "x".into())).is_err());
}
