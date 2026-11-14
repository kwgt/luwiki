/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 状態snapshotの公開変換と比較値を定義する
//!

use sha2::{Digest, Sha256};

use crate::database::types::{PageId, PageSource};

///
/// domainと長さ付き入力からSHA-256を生成する
///
/// # 引数
/// * `domain` - 用途識別
/// * `parts` - 区切りを含む任意bytes
///
/// # 戻り値
/// 小文字hexのdigest。
///
pub(crate) fn digest(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in std::iter::once(domain).chain(parts.iter().copied()) {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

///
/// 不透明な基準tokenの構文を検証する
///
/// # 引数
/// * `value` - 外部入力
///
/// # 戻り値
/// v1と二つの小文字hex digestならtrue。
///
pub(crate) fn valid_state_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 132
        && bytes.starts_with(b"v1:")
        && bytes[67] == b':'
        && bytes[3..67]
            .iter()
            .chain(&bytes[68..])
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

///
/// 同じsourceの入力から不透明な比較tokenを作る
///
/// # 引数
/// * `path` - 正規化済みpath
/// * `page_id` - 内部ID
/// * `revision` - 取得したrevision
/// * `source` - 同じsnapshotのsource
///
/// # 戻り値
/// OP1と互換の比較token。
///
pub(crate) fn state_token(
    path: &str,
    page_id: &PageId,
    revision: u64,
    source: &PageSource,
) -> String {
    /* 識別部と状態部のdomainを分ける。 */
    let id = page_id.to_string();
    let identity = digest(
        b"luwiki:page-state:identity:v1",
        &[path.as_bytes(), id.as_bytes()],
    );
    let instance = source.instance_id().map(|id| id.to_string());
    let body;
    let (kind, content) = match &instance {
        Some(id) => (b"instance".as_slice(), id.as_bytes()),
        None => {
            body = source.source();
            (b"source".as_slice(), body.as_bytes())
        }
    };
    let state = digest(
        b"luwiki:page-state:source:v1",
        &[
            path.as_bytes(),
            id.as_bytes(),
            &revision.to_be_bytes(),
            kind,
            content,
        ],
    );
    format!("v1:{identity}:{state}")
}
