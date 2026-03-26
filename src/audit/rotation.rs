/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 固定サイズローテーションの骨格を定義するモジュール
//!

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

const ACTIVE_LOG_FILE_NAME: &str = "audit.current.jsonl";

///
/// ローテーション設定の骨格
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuditRotationPolicy {
    /// ローテーション閾値サイズ
    rotate_size: u64,
}

impl AuditRotationPolicy {
    ///
    /// ローテーション設定の生成
    ///
    /// # 引数
    /// * `rotate_size` - ローテーション閾値サイズ(バイト)
    ///
    /// # 戻り値
    /// 生成したローテーション設定を返す。
    ///
    pub(crate) fn new(rotate_size: u64) -> Self {
        Self { rotate_size }
    }

    ///
    /// ローテーション閾値サイズへのアクセサ
    ///
    /// # 戻り値
    /// ローテーション閾値サイズ(バイト)を返す。
    ///
    pub(crate) fn rotate_size(&self) -> u64 {
        self.rotate_size
    }
}

///
/// アクティブ監査ログファイルのパスを返す
///
/// # 引数
/// * `output_dir` - 監査ログ出力ディレクトリ
///
/// # 戻り値
/// アクティブ監査ログファイルのパスを返す。
///
pub(crate) fn active_log_path(output_dir: &Path) -> PathBuf {
    output_dir.join(ACTIVE_LOG_FILE_NAME)
}

///
/// ローテーション要否を判定する
///
/// # 引数
/// * `policy` - ローテーション設定
/// * `current_size` - 現在のアクティブファイルサイズ(バイト)
/// * `incoming_size` - 今回追加するレコードサイズ(バイト)
///
/// # 戻り値
/// 書込前にローテーションが必要な場合は `true` を返す。
///
pub(crate) fn should_rotate(
    policy: &AuditRotationPolicy,
    current_size: u64,
    incoming_size: u64,
) -> bool {
    current_size > 0
        && current_size.saturating_add(incoming_size) > policy.rotate_size()
}

///
/// アクティブファイルをローテーション済みファイルへ切り替える
///
/// # 引数
/// * `output_dir` - 監査ログ出力ディレクトリ
/// * `rotated_at` - ローテーション確定時刻
///
/// # 戻り値
/// ローテーション済みファイルを生成した場合は、そのパスを返す。
///
pub(crate) fn rotate_active_file(
    output_dir: &Path,
    rotated_at: DateTime<Utc>,
) -> Result<Option<PathBuf>> {
    /*
     * アクティブファイル不在時は何もしない
     */
    let active_path = active_log_path(output_dir);
    if !active_path.exists() {
        return Ok(None);
    }

    /*
     * 出力先ファイル名の決定
     */
    let rotated_path = next_rotated_log_path(output_dir, rotated_at)?;

    /*
     * アクティブファイルのリネーム
     */
    fs::rename(&active_path, &rotated_path).with_context(|| {
        format!(
            "rename audit log failed: {} -> {}",
            active_path.display(),
            rotated_path.display()
        )
    })?;

    Ok(Some(rotated_path))
}

///
/// 次のローテーション済みファイルパスを返す
///
/// # 引数
/// * `output_dir` - 監査ログ出力ディレクトリ
/// * `rotated_at` - ローテーション確定時刻
///
/// # 戻り値
/// 未使用のローテーション済みファイルパスを返す。
///
fn next_rotated_log_path(
    output_dir: &Path,
    rotated_at: DateTime<Utc>,
) -> Result<PathBuf> {
    let timestamp = rotated_at.format("%Y%m%dT%H%M%SZ");

    for sequence in 1..=u32::MAX {
        let path = output_dir.join(format!(
            "audit-{}-{sequence:06}.jsonl",
            timestamp
        ));
        if !path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!("audit rotation sequence exhausted")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use super::*;

    ///
    /// ローテーション済みファイル名が規則どおり生成されることを確認する。
    ///
    /// 注記:
    /// アクティブファイルを作成してからローテーションを実行する。
    ///
    #[test]
    fn rotate_active_file_renames_current_log() {
        let dir = tempdir().expect("tempdir failed");
        let active = active_log_path(dir.path());
        fs::write(&active, b"{}\n").expect("write active failed");

        let rotated = rotate_active_file(
            dir.path(),
            Utc.with_ymd_and_hms(2026, 3, 30, 12, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("rotate failed")
        .expect("rotated path missing");

        assert!(!active.exists());
        assert_eq!(
            rotated.file_name().and_then(|value| value.to_str()),
            Some("audit-20260330T120000Z-000001.jsonl"),
        );
    }
}
