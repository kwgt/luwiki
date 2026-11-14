/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 出力時のauthority専用マクロを展開するモジュール
//!

use std::borrow::Cow;

/// 正式記法。通常マクロparserで正規化しない。
const AUTHORITY_MACRO: &str = "{{mcp:authority}}";

///
/// 元sourceの正式記法だけを一回展開する
///
/// # 概要
/// code内も対象とし、直前の奇数個のバックスラッシュで抑止する。
/// 入力や生成結果を変更・再走査せず、非展開時は借用を返す。
///
/// # 引数
/// * `source` - 展開元の本文
/// * `authority` - run設定で解決・検証済みのauthority
///
/// # 戻り値
/// 展開後の文字列、または未変更の借用文字列。
///
pub(crate) fn expand_authority_macro<'a>(
    source: &'a str,
    authority: &str,
) -> Cow<'a, str> {
    let mut output: Option<String> = None;
    let mut search = 0;
    let mut copied = 0;

    /*
     * 元のsourceの一致だけを左から走査する
     */
    while let Some(relative) = source[search..].find(AUTHORITY_MACRO) {
        let start = search + relative;
        let end = start + AUTHORITY_MACRO.len();
        let escapes = source[..start].bytes().rev()
            .take_while(|byte| *byte == b'\\').count();
        if escapes % 2 == 0 {
            let buffer = output.get_or_insert_with(|| {
                String::with_capacity(source.len())
            });
            buffer.push_str(&source[copied..start]);
            buffer.push_str(authority);
            copied = end;
        }
        search = end;
    }

    /*
     * 未処理末尾を保持し、変更時だけ所有文字列を返す
     */
    match output {
        Some(mut output) => {
            output.push_str(&source[copied..]);
            Cow::Owned(output)
        }
        None => Cow::Borrowed(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    ///
    /// RustとTypeScriptの共通fixtureを照合する
    ///
    /// # 概要
    /// 正式/未知形、code、escape、Unicodeと改行の期待値を確認する。
    ///
    /// # 注記
    /// authority未取得はフロント専用ケースとして原文を保持する。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[test]
    fn authority_macro_shared_fixture() {
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/authority_macro_cases.json",
        )).unwrap();
        /*
         * 共通の入出力と未変更時の借用を確認する
         */
        for case in cases {
            let source = case["source"].as_str().unwrap();
            let result = match case["authority"].as_str() {
                Some(authority) => expand_authority_macro(source, authority),
                None => Cow::Borrowed(source),
            };
            assert_eq!(result, case["expected"].as_str().unwrap(),
                "case {}", case["name"]);
            if result == source {
                assert!(matches!(result, Cow::Borrowed(_)));
            }
        }
    }
}
