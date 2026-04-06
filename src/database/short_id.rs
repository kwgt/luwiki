/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! ページ短縮IDの変換処理をまとめたモジュール
//!

use crate::database::types::{PageId, ShortIdDecodeError};

///
/// ページIDから短縮IDを生成する
///
/// # 引数
/// * `page_id` - 変換対象のページID
///
/// # 戻り値
/// `page_id` に対応する短縮ID文字列を返す。
///
#[allow(dead_code)]
pub(crate) fn encode_page_short_id(page_id: &PageId) -> String {
    page_id.to_base62_fixed()
}

///
/// 短縮IDからページIDを復元する
///
/// # 引数
/// * `short_id` - 復元対象の短縮ID文字列
///
/// # 戻り値
/// 復元に成功した場合は対応するページIDを返す。
/// 入力不正時は呼び出し側がHTTP 404へ写像できる専用エラーを返す。
///
#[allow(dead_code)]
pub(crate) fn decode_page_short_id(
    short_id: &str,
) -> Result<PageId, ShortIdDecodeError> {
    PageId::from_base62_fixed(short_id)
}

#[cfg(test)]
mod tests {
    use super::{decode_page_short_id, encode_page_short_id};
    use crate::database::types::{PageId, ShortIdDecodeError};

    ///
    /// base62 固定長変換が可逆であり大小文字を保持することを確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test database::short_id::tests::encode_page_short_id_is_fixed_width_and_roundtrips -- --test-threads=1`
    /// で実行する。
    ///
    #[test]
    fn encode_page_short_id_is_fixed_width_and_roundtrips() {
        let page_id = PageId::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("page id parse failed");

        let short_id = encode_page_short_id(&page_id);

        assert_eq!(short_id.len(), 22);
        assert_eq!(
            decode_page_short_id(&short_id).expect("short id decode failed"),
            page_id,
        );
    }

    ///
    /// base62 の大文字小文字が別値として扱われることを確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test database::short_id::tests::decode_page_short_id_distinguishes_letter_case -- --test-threads=1`
    /// で実行する。
    ///
    #[test]
    fn decode_page_short_id_distinguishes_letter_case() {
        let upper = "000000000000000000000A";
        let lower = "000000000000000000000a";

        let upper_page_id =
            decode_page_short_id(upper).expect("upper short id decode failed");
        let lower_page_id =
            decode_page_short_id(lower).expect("lower short id decode failed");

        assert_ne!(upper_page_id, lower_page_id);
        assert_eq!(encode_page_short_id(&upper_page_id), upper);
        assert_eq!(encode_page_short_id(&lower_page_id), lower);
    }

    ///
    /// 短縮ID復元時の入力不正を確認する。
    ///
    /// # 戻り値
    /// テスト結果を返す。
    ///
    /// # 注記
    /// `cargo test database::short_id::tests::decode_page_short_id_rejects_invalid_inputs -- --test-threads=1`
    /// で実行する。
    ///
    #[test]
    fn decode_page_short_id_rejects_invalid_inputs() {
        assert_eq!(
            decode_page_short_id("short"),
            Err(ShortIdDecodeError::InvalidLength),
        );
        assert_eq!(
            decode_page_short_id("000000000000000000000_"),
            Err(ShortIdDecodeError::InvalidCharacter),
        );
        assert_eq!(
            decode_page_short_id("zzzzzzzzzzzzzzzzzzzzzz"),
            Err(ShortIdDecodeError::Overflow),
        );
    }
}
