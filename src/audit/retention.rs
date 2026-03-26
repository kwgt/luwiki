/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 保持期間超過ログ削除の骨格を定義するモジュール
//!

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use tracing::{debug, warn};

const ACTIVE_LOG_FILE_NAME: &str = "audit.current.jsonl";

///
/// 保持削除設定の骨格
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuditRetentionPolicy {
    /// 保持期間
    retention: Duration,
}

impl AuditRetentionPolicy {
    ///
    /// 保持削除設定の生成
    ///
    /// # 引数
    /// * `retention` - 監査ログ保持期間
    ///
    /// # 戻り値
    /// 生成した保持削除設定を返す。
    ///
    pub(crate) fn new(retention: Duration) -> Self {
        Self { retention }
    }

    ///
    /// 保持期間へのアクセサ
    ///
    /// # 戻り値
    /// 監査ログ保持期間を返す。
    ///
    pub(crate) fn retention(&self) -> Duration {
        self.retention
    }

    ///
    /// 保持期限の閾値時刻を返す
    ///
    /// # 引数
    /// * `now` - 基準時刻
    ///
    /// # 戻り値
    /// 削除判定に利用する閾値時刻を返す。
    ///
    pub(crate) fn cutoff_at(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - self.retention
    }
}

///
/// 削除候補ファイル列挙結果の骨格
///
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RetentionSweepPlan {
    /// 削除候補ファイル
    pub(crate) delete_candidates: Vec<PathBuf>,

    /// 規則外などで温存したファイル
    pub(crate) skipped_files: Vec<PathBuf>,
}

///
/// 保持削除実行結果
///
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RetentionSweepResult {
    /// 削除したファイル
    pub(crate) deleted_files: Vec<PathBuf>,

    /// 削除失敗したファイル
    pub(crate) failed_files: Vec<PathBuf>,

    /// 規則外などで温存したファイル
    pub(crate) skipped_files: Vec<PathBuf>,
}

///
/// 保持削除計画を生成する
///
/// # 引数
/// * `output_dir` - 監査ログ出力ディレクトリ
/// * `policy` - 保持削除設定
/// * `now` - 基準時刻
///
/// # 戻り値
/// 列挙した削除計画を返す。
///
pub(crate) fn build_retention_plan(
    output_dir: &Path,
    policy: &AuditRetentionPolicy,
    now: DateTime<Utc>,
) -> Result<RetentionSweepPlan> {
    /*
     * 監査ログディレクトリの列挙
     */
    let entries = fs::read_dir(output_dir).with_context(|| {
        format!(
            "read audit output dir failed: {}",
            output_dir.display()
        )
    })?;
    let cutoff_at = policy.cutoff_at(now);
    let mut plan = RetentionSweepPlan::default();

    /*
     * 削除対象と温存対象の振り分け
     */
    for entry in entries {
        let entry = entry.context("read audit dir entry failed")?;
        let path = entry.path();
        let file_type = entry.file_type().with_context(|| {
            format!("read audit entry type failed: {}", path.display())
        })?;
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            plan.skipped_files.push(path);
            continue;
        };

        if file_name == ACTIVE_LOG_FILE_NAME || !file_type.is_file() {
            continue;
        }

        match parse_rotated_timestamp(file_name) {
            Some(rotated_at) if rotated_at < cutoff_at => {
                plan.delete_candidates.push(path);
            }
            Some(_) => {}
            None => {
                warn!(
                    path = %path.display(),
                    "skip audit retention for unmanaged file"
                );
                plan.skipped_files.push(path);
            }
        }
    }

    Ok(plan)
}

///
/// 保持削除を実行する
///
/// # 引数
/// * `plan` - 実行する削除計画
///
/// # 戻り値
/// 削除結果を返す。
///
pub(crate) fn execute_retention_plan(
    plan: RetentionSweepPlan,
) -> RetentionSweepResult {
    let mut result = RetentionSweepResult {
        deleted_files: Vec::new(),
        failed_files: Vec::new(),
        skipped_files: plan.skipped_files,
    };

    /*
     * 削除対象を順次処理
     */
    for path in plan.delete_candidates {
        match fs::remove_file(&path) {
            Ok(()) => {
                debug!(path = %path.display(), "deleted audit log by retention");
                result.deleted_files.push(path);
            }
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "delete audit log by retention failed"
                );
                result.failed_files.push(path);
            }
        }
    }

    result
}

///
/// ローテーション済みファイル名から UTC 時刻を復元する
///
/// # 引数
/// * `file_name` - 判定対象ファイル名
///
/// # 戻り値
/// 命名規則に一致する場合はローテーション時刻を返す。
///
fn parse_rotated_timestamp(file_name: &str) -> Option<DateTime<Utc>> {
    let body = file_name
        .strip_prefix("audit-")?
        .strip_suffix(".jsonl")?;
    let (timestamp, sequence) = body.rsplit_once('-')?;
    if sequence.len() != 6 || !sequence.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let naive =
        NaiveDateTime::parse_from_str(timestamp, "%Y%m%dT%H%M%SZ").ok()?;

    Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{Duration, TimeZone, Utc};
    use tempfile::tempdir;

    use super::*;

    ///
    /// アクティブファイルを除外し、期限超過ファイルだけを削除候補に含めることを確認する。
    ///
    /// 注記:
    /// 規則外ファイルも配置し、`skipped_files` へ振り分けられることを確認する。
    ///
    #[test]
    fn build_retention_plan_selects_only_expired_rotated_files() {
        let dir = tempdir().expect("tempdir failed");
        fs::write(dir.path().join("audit.current.jsonl"), b"{}\n")
            .expect("write current failed");
        fs::write(
            dir.path().join("audit-20260101T000000Z-000001.jsonl"),
            b"old\n",
        )
        .expect("write old failed");
        fs::write(
            dir.path().join("audit-20260330T000000Z-000001.jsonl"),
            b"new\n",
        )
        .expect("write new failed");
        fs::write(dir.path().join("notes.txt"), b"skip\n")
            .expect("write skip failed");

        let plan = build_retention_plan(
            dir.path(),
            &AuditRetentionPolicy::new(Duration::days(30)),
            Utc.with_ymd_and_hms(2026, 3, 30, 12, 0, 0)
                .single()
                .expect("now"),
        )
        .expect("build plan failed");

        assert_eq!(plan.delete_candidates.len(), 1);
        assert!(plan.delete_candidates[0]
            .ends_with("audit-20260101T000000Z-000001.jsonl"));
        assert_eq!(plan.skipped_files.len(), 1);
    }

    ///
    /// 削除実行が成功候補だけを削除結果へ積むことを確認する。
    ///
    /// 注記:
    /// 一時ディレクトリ配下の削除可能ファイルを 1 件与える。
    ///
    #[test]
    fn execute_retention_plan_deletes_candidates() {
        let dir = tempdir().expect("tempdir failed");
        let target = dir.path().join("audit-20260101T000000Z-000001.jsonl");
        fs::write(&target, b"old\n").expect("write target failed");

        let result = execute_retention_plan(RetentionSweepPlan {
            delete_candidates: vec![target.clone()],
            skipped_files: vec![],
        });

        assert_eq!(result.deleted_files, vec![target.clone()]);
        assert!(result.failed_files.is_empty());
        assert!(!target.exists());
    }
}
