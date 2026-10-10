/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! Wikiパスの入力検証、解決および境界比較を共有するモジュール
//!

use anyhow::{Result, bail};

///
/// 検証済みの正規化された絶対Wikiパス
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedWikiPath {
    /// 認可とDB操作へ同じ値として渡すパス
    value: String,
}

impl NormalizedWikiPath {
    ///
    /// 通常入力を検証してdotセグメントを解決する
    ///
    /// # 引数
    /// * `raw_path` - 入力形式の解釈済みパス
    ///
    /// # 戻り値
    /// 正規化値、または入力不正を返す。
    ///
    /// # 概要
    /// 入力表記を検証してから解決し、正規形を再検証する。
    ///
    pub(crate) fn parse(raw_path: &str) -> Result<Self> {
        validate_raw_path(raw_path, false)?;
        Self::from_stored(&resolve_dot_segments(raw_path)?)
    }

    ///
    /// 保存値を変換せず正規形として検証する
    ///
    /// # 引数
    /// * `path` - 保存済みの絶対パス
    ///
    /// # 戻り値
    /// 正規化値、または不正な保存値を返す。
    ///
    pub(crate) fn from_stored(path: &str) -> Result<Self> {
        validate_raw_path(path, false)?;
        if path.split('/').any(|part| part == "." || part == "..") {
            bail!("path must be normalized");
        }
        Ok(Self { value: path.to_string() })
    }

    ///
    /// 正規化パスへの参照を取得する
    ///
    /// # 戻り値
    /// 正規化済みの文字列を返す。
    ///
    pub(crate) fn as_str(&self) -> &str {
        &self.value
    }

    ///
    /// 正規化パスを文字列へ取り出す
    ///
    /// # 戻り値
    /// 所有権を移した正規化文字列を返す。
    ///
    pub(crate) fn into_string(self) -> String {
        self.value
    }
}

///
/// REST移動・復元先の末尾記法を保持する入力
///
// MS3でREST入口へ接続する基盤として先行実装する。
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct DestinationPathInput {
    /// dot解決後の配置先
    base: NormalizedWikiPath,
    /// 元のページ名を配置先へ付加する指定
    reuse_basename: bool,
}

// MS2では解決契約を検証し、RESTの実呼び出しはMS3で接続する。
#[allow(dead_code)]
impl DestinationPathInput {
    ///
    /// REST専用の配置先入力を検証する
    ///
    /// # 引数
    /// * `raw_path` - 末尾スラッシュ記法を含む入力
    ///
    /// # 戻り値
    /// 配置先入力、または入力不正を返す。
    ///
    /// # 概要
    /// 末尾指定の意図を保持し、配置先のdotだけを解決する。
    ///
    pub(crate) fn parse(raw_path: &str) -> Result<Self> {
        /*
         * 縮約前に禁止表記を検証する
         */
        validate_raw_path(raw_path, true)?;
        let reuse_basename = raw_path.ends_with('/');
        let base = if raw_path == "/" {
            raw_path
        } else {
            raw_path.strip_suffix('/').unwrap_or(raw_path)
        };

        /*
         * 末尾指定の意図と解決済み配置先を保持する
         */
        Ok(Self {
            base: NormalizedWikiPath::parse(base)?,
            reuse_basename,
        })
    }

    ///
    /// 元パスから最終配置先を一度だけ算出する
    ///
    /// # 引数
    /// * `source` - 同一snapshotまたはtxnから取得した元パス
    ///
    /// # 戻り値
    /// 最終配置先、またはページ名のない元パスのエラーを返す。
    ///
    /// # 概要
    /// 元のページ名をrootでも一度だけ付加して最終値を返す。
    ///
    pub(crate) fn resolve(
        &self,
        source: &NormalizedWikiPath,
    ) -> Result<NormalizedWikiPath> {
        /*
         * 明示した最終配置先はそのまま利用する
         */
        if !self.reuse_basename {
            return Ok(self.base.clone());
        }
        let name = source.as_str().rsplit('/').next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| anyhow::anyhow!("root has no reusable name"))?;

        /*
         * rootでも区切りを重複させずページ名を付加する
         */
        let destination = if self.base.as_str() == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", self.base.as_str(), name)
        };
        NormalizedWikiPath::from_stored(&destination)
    }
}

///
/// raw入力の禁止表記を解決前に検証する
///
/// # 引数
/// * `path` - 入力パス
/// * `allow_trailing` - REST配置先だけの末尾記法許可
///
/// # 戻り値
/// 検証成功、または入力不正を返す。
///
/// # 概要
/// 不正入力を縮約やtrimで消さず、raw段階で拒否する。
///
fn validate_raw_path(path: &str, allow_trailing: bool) -> Result<()> {
    /*
     * 絶対パスと禁止文字を検証する
     */
    if !path.starts_with('/') {
        bail!("path must be absolute");
    }
    if path.chars().any(|ch| {
        ch == '\\' || ch.is_whitespace() || ch.is_control()
    }) {
        bail!("path contains invalid character");
    }

    /*
     * 縮約すると見えなくなるスラッシュ表記を拒否する
     */
    if path.contains("//") {
        bail!("path must not contain consecutive slashes");
    }
    if !allow_trailing && path != "/" && path.ends_with('/') {
        bail!("path must not have a trailing slash");
    }
    Ok(())
}

///
/// 絶対パスのdotセグメントを解決する
///
/// # 引数
/// * `path` - raw検証済みの絶対パス
///
/// # 戻り値
/// 解決後パス、またはルートを越える入力のエラーを返す。
///
/// # 概要
/// dotをroot基準に解決し、rootより上への遡及を拒否する。
///
fn resolve_dot_segments(path: &str) -> Result<String> {
    /*
     * セグメントをroot基準のstackとして解決する
     */
    let mut parts = Vec::new();
    for part in path.split('/').skip(1) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    bail!("path must not traverse above root");
                }
            }
            _ => parts.push(part),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

///
/// パス境界を考慮してprefixと比較する
///
/// # 引数
/// * `path` - 対象の保存表記。不正規形は一致させない
/// * `prefix` - 変換しない保存済みprefix。不正規形は一致させない
///
/// # 戻り値
/// 完全一致または配下ならtrueを返す。
///
pub(crate) fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    if validate_raw_path(path, false).is_err()
        || path.split('/').any(|part| part == "." || part == "..")
        || validate_raw_path(prefix, false).is_err()
        || prefix.split('/').any(|part| part == "." || part == "..")
    {
        return false;
    }
    prefix == "/" || path == prefix || path.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::{
        DestinationPathInput, NormalizedWikiPath, path_matches_prefix,
    };

    ///
    /// raw禁止表記がdot解決やtrimで消されないことを検証する
    ///
    /// # 注記
    /// 末尾/・連続/・空白・制御文字・相対入力を渡し、拒否を確認する。
    ///
    /// # 概要
    /// 認可前に入力規則を満たすことを、異なる表記で検証する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn rejects_invalid_raw_paths_before_resolution() {
        /*
         * 禁止入力が変換されず拒否されることを確認する
         */
        for path in [
            "", "docs/a", "/docs/a/", "//", "/docs//a", "/docs/./",
            "/docs/../bad name", " /docs", "/docs ", "/docs/　a",
            "/docs/\ta", "/docs/\na", "/docs/\0a", "/docs/\\a",
        ] {
            assert!(NormalizedWikiPath::parse(path).is_err(), "{path:?}");
        }
    }

    ///
    /// dot解決とroot越え拒否、decode非実施を検証する
    ///
    /// # 注記
    /// dot入力の最終値とroot越えエラー、%文字列の保持を確認する。
    ///
    /// # 概要
    /// 認可前に入力規則を満たすことを、異なる表記で検証する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn resolves_dots_without_decoding_or_root_escape() {
        /*
         * 正規化後のpathとJSON由来の%表記を確認する
         */
        for (raw, expected) in [
            ("/", "/"), ("/docs/./a", "/docs/a"),
            ("/docs/../a", "/a"), ("/docs/..", "/"),
            ("/docs/%2E%2E/a", "/docs/%2E%2E/a"),
        ] {
            let path = NormalizedWikiPath::parse(raw).unwrap();
            assert_eq!(path.as_str(), expected);
        }
        assert!(NormalizedWikiPath::parse("/../../a").is_err());
    }

    ///
    /// 保存パスを自動変換しないことを検証する
    ///
    /// # 注記
    /// 入力として解決可能なdotも保存値では拒否する。
    ///
    /// # 概要
    /// 認可前に入力規則を満たすことを、異なる表記で検証する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn stored_paths_are_validated_without_conversion() {
        assert!(NormalizedWikiPath::from_stored("/docs/../a").is_err());
        assert!(NormalizedWikiPath::from_stored("/docs/").is_err());
        let path = NormalizedWikiPath::from_stored("/docs/a").unwrap();
        assert_eq!(path.into_string(), "/docs/a");
    }

    ///
    /// REST配置先のrootと末尾記法を検証する
    ///
    /// # 注記
    /// root・dot・明示配置先から単一basename付加後の値を確認する。
    ///
    /// # 概要
    /// 認可前に入力規則を満たすことを、異なる表記で検証する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn destinations_reuse_basename_only_when_requested() {
        /*
         * 配置先記法を解決して末尾や二重区切りがないことを確認する
         */
        let source = NormalizedWikiPath::from_stored("/docs/a").unwrap();
        for (raw, expected) in [
            ("/", "/a"), ("/archive/", "/archive/a"),
            ("/archive/../", "/a"), ("/archive/../b", "/b"),
        ] {
            let input = DestinationPathInput::parse(raw).unwrap();
            assert_eq!(input.resolve(&source).unwrap().as_str(), expected);
        }
        assert!(DestinationPathInput::parse("/archive//").is_err());
        assert!(DestinationPathInput::parse("/../").is_err());
        let root = NormalizedWikiPath::from_stored("/").unwrap();
        assert!(DestinationPathInput::parse("/").unwrap()
            .resolve(&root).is_err());
    }
    ///
    /// 境界一致と不正規prefixの拒否を検証する
    ///
    /// # 注記
    /// root/完全一致/配下を許可し、空prefixや非正規表記で広がらない。
    ///
    /// # 概要
    /// 認可前に入力規則を満たすことを、異なる表記で検証する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn boundaries_reject_noncanonical_prefixes() {
        assert!(path_matches_prefix("/docs/a", "/"));
        assert!(path_matches_prefix("/docs", "/docs"));
        assert!(path_matches_prefix("/docs/a", "/docs"));
        assert!(!path_matches_prefix("/docs2", "/docs"));
        for prefix in ["", "docs", "/docs/", "/docs/..", "//", "/d a"] {
            assert!(!path_matches_prefix("/docs/a", prefix));
        }
        for path in ["", "/docs/../a", "/docs/", "//", "/docs/a b"] {
            assert!(!path_matches_prefix(path, "/docs"));
            assert!(!path_matches_prefix(path, "/"));
        }
    }

}
