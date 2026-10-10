/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 監査イベント投入入口の骨格を定義するモジュール
//!

use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;

use super::buffer::AppendAuditBuffer;
use super::model::{
    AppendAuditOutcome,
    AuditOperation,
    AuditRecord,
    AuditResult,
};
use super::writer::AuditWriter;

///
/// 固定診断と非公開の元writer原因を保持する停止失敗
///
#[derive(Debug)]
pub(crate) struct AuditShutdownFailure {
    /// 今回のappend write元error
    pub(crate) append: Option<anyhow::Error>,
    /// 今回のwriter flush元error
    pub(crate) flush: Option<anyhow::Error>,
    /// 前回のwrite失敗によって再送しなかったか
    pub(crate) previous_append_failure: bool,
}

impl std::fmt::Display for AuditShutdownFailure {
    ///
    /// 元error本文を転記せず固定診断を返す
    ///
    /// # 引数
    /// * formatter - 診断出力先
    ///
    /// # 戻り値
    /// 固定診断の書込結果。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>)
        -> std::fmt::Result
    {
        let append = self.append.is_some() || self.previous_append_failure;
        formatter.write_str(match (append, self.flush.is_some()) {
            (true, true) =>
                "audit append write and writer shutdown flush failed",
            (true, false) => "audit append shutdown write failed",
            (false, true) => "audit writer shutdown flush failed",
            (false, false) => "audit shutdown failed",
        })
    }
}

impl std::error::Error for AuditShutdownFailure {}

///
/// 監査イベント投入入口の骨格
///
#[derive(Debug)]
pub(crate) struct AuditSink {
    /// `append` 集約バッファ
    buffer: AppendAuditBuffer,

    /// JSONL writer
    writer: AuditWriter,

    /// 停止flushのwrite失敗後に同snapshotを自動再投入しない印
    shutdown_write_failed: bool,
}

impl AuditSink {
    ///
    /// 監査イベント投入入口の生成
    ///
    /// # 引数
    /// * `buffer` - `append` 集約バッファ
    /// * `writer` - JSONL writer
    ///
    /// # 戻り値
    /// 生成した監査イベント投入入口を返す。
    ///
    pub(crate) fn new(
        buffer: AppendAuditBuffer,
        writer: AuditWriter,
    ) -> Self {
        Self { buffer, writer, shutdown_write_failed: false }
    }

    ///
    /// 監査イベントを投入する骨格関数
    ///
    /// # 引数
    /// * `record` - 投入する監査レコード
    ///
    /// # 戻り値
    /// 現時点では即時書込を呼び出して成功を返す。
    ///
    pub(crate) fn record(&mut self, record: AuditRecord) -> Result<()> {
        /*
         * `append` 成功は集約し、それ以外は即時書込とする
         */
        if let Some(outcome) = infer_append_outcome(&record) {
            let flushed = self.buffer.push_append(record, outcome);
            return self.write_records(&flushed);
        }

        /*
         * 非 `append` イベントでも期限切れ集約は先に確定する
         */
        let mut flushed = self.buffer.collect_expired(record.timestamp);
        flushed.push(record);

        self.write_records(&flushed)
    }

    ///
    /// `append` 成功イベントを明示的に集約投入する
    ///
    /// # 引数
    /// * `record` - 集約対象監査レコード
    /// * `outcome` - 集約用の内部結果分類
    ///
    /// # 戻り値
    /// 今回確定した監査レコードの書込結果を返す。
    ///
    pub(crate) fn record_append(
        &mut self,
        record: AuditRecord,
        outcome: AppendAuditOutcome,
    ) -> Result<()> {
        let flushed = self.buffer.push_append(record, outcome);
        self.write_records(&flushed)
    }

    ///
    /// 監査ログの明示 flush
    ///
    /// # 戻り値
    /// 停止書込とwriter flushの失敗を固定診断で返す。
    ///
    /// # 概要
    /// 通常recordの責務を変えず明示停止snapshotの再投入だけを防ぐ。
    ///
    pub(crate) fn flush(&mut self) -> Result<()> {
        /*
         * 明示停止中のsnapshotは非破壊に確定し成功keyだけ退役する
         */
        let previous_append_failure = self.shutdown_write_failed;
        let mut append_error = None;
        if !previous_append_failure {
            let snapshot = self.buffer.shutdown_snapshot(Utc::now());
            for (key, record) in snapshot {
                if let Err(error) = self.writer.write_record(&record) {
                    append_error = Some(error);
                    self.shutdown_write_failed = true;
                    break;
                }
                self.buffer.retire_shutdown_group(&key);
            }
        }

        /*
         * append書込失敗でもwriter最終flushを必ず試行する
         */
        let flush_error = self.writer.flush().err();
        if append_error.is_none() && flush_error.is_none()
            && !previous_append_failure
        {
            Ok(())
        } else {
            Err(anyhow::Error::new(AuditShutdownFailure {
                append: append_error, flush: flush_error,
                previous_append_failure,
            }))
        }
    }

    ///
    /// 保持処理を同sink write lock内で実行する小入口
    ///
    /// # 引数
    /// * `policy` - 既存保持設定
    /// * `now` - 期限判定時刻
    ///
    /// # 戻り値
    /// 個別削除結果、又はdir列挙等の失敗。
    ///
    /// # 概要
    /// rotation/request投入と同じwriter状態を同期して観測する。
    ///
    pub(crate) fn maintenance(
        &mut self,
        policy: &super::retention::AuditRetentionPolicy,
        now: DateTime<Utc>,
    ) -> Result<super::retention::RetentionSweepResult> {
        /*
         * active名温存と既存命名規則を同lock内で再利用する
         */
        let plan = super::retention::build_retention_plan(
            self.writer.output_dir(), policy, now,
        )?;
        Ok(super::retention::execute_retention_plan(plan))
    }

    ///
    /// 終了writer障害を製品外fixtureへ設定する
    ///
    /// # 引数
    /// * write_at - n件目失敗
    /// * partial - 部分writeの有無
    /// * flush - 最終flushの失敗
    ///
    /// # 戻り値
    /// なし。
    ///
    #[cfg(test)]
    pub(crate) fn inject_shutdown_failure(
        &mut self, write_at: Option<usize>, partial: bool, flush: bool,
    ) {
        self.writer.inject_shutdown_failure(write_at, partial, flush);
    }

    ///
    /// 終了fixtureのwriter試行件数を取得する
    ///
    /// # 戻り値
    /// write/flush件数。
    ///
    #[cfg(test)]
    pub(crate) fn shutdown_attempts(&self) -> (usize, usize) {
        self.writer.shutdown_attempts()
    }

    ///
    /// 保留中 `append` の有無を返す
    ///
    /// # 戻り値
    /// 保留中集約が存在する場合は `true` を返す。
    ///
    pub(crate) fn has_pending_appends(&self) -> bool {
        self.buffer.has_pending()
    }

    ///
    /// 複数監査レコードを順に書き込む
    ///
    /// # 引数
    /// * `records` - 書き込む監査レコード群
    ///
    /// # 戻り値
    /// すべての書込が成功した場合は `Ok(())` を返す。
    ///
    fn write_records(
        &mut self,
        records: &[AuditRecord],
    ) -> Result<()> {
        for record in records {
            self.writer.write_record(record)?;
        }

        Ok(())
    }
}

///
/// `append` 集約に利用する内部結果分類を推定する
///
/// # 引数
/// * `record` - 判定対象監査レコード
///
/// # 戻り値
/// `append` 成功で summary から判定できる場合は内部結果分類を返す。
///
fn infer_append_outcome(record: &AuditRecord) -> Option<AppendAuditOutcome> {
    if record.operation != AuditOperation::Append
        || record.result != AuditResult::Success
    {
        return None;
    }

    match record.summary.as_deref() {
        Some(summary) if summary.contains("(amended)") => {
            Some(AppendAuditOutcome::Amended)
        }
        Some(_) | None => Some(AppendAuditOutcome::NewRevision),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;
    use crate::audit::rotation::active_log_path;
    use crate::audit::writer::{AuditWriter, AuditWriterConfig};
    use crate::database::types::{TokenId, UserId};

    fn test_append_record(summary: &str) -> AuditRecord {
        AuditRecord::new(
            AuditOperation::Append,
            UserId::new(),
            Some(TokenId::new()),
            None,
            Some("/audit/page".to_string()),
            AuditResult::Success,
            Utc::now(),
            Some(summary.to_string()),
            Some(2),
        )
    }

    ///
    /// `flush()` が保留中 `append` 集約を JSONL へ確定出力することを
    /// 確認する。
    ///
    /// # 注記
    /// 1 件の `append` 成功を保留させた後に flush し、
    /// ファイル出力と保留解消の双方を検証する。
    ///
    #[test]
    fn flush_writes_pending_append_aggregation() {
        let dir = tempdir().expect("tempdir failed");
        let writer = AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().to_path_buf(),
            rotation_policy: super::super::rotation::AuditRotationPolicy::new(
                1024,
            ),
        });
        let mut sink = AuditSink::new(AppendAuditBuffer::new(), writer);

        sink.record(test_append_record("page appended"))
            .expect("record append failed");
        assert!(sink.has_pending_appends());

        sink.flush().expect("flush failed");

        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active log failed");
        assert!(body.contains("\"operation\":\"append\""));
        assert!(body.contains("\"result\":\"success\""));
        assert!(body.contains("\"target_path\":\"/audit/page\""));
        assert!(body.contains("\"summary\":\""));
        assert!(!sink.has_pending_appends());
    }

    ///
    /// 停止partial-write失敗後に未試行groupを保持し再送しない
    ///
    /// # 概要
    /// n件目の部分byte障害と最終flush試行、snapshot固定を実証する。
    ///
    /// # 注記
    /// write成功keyだけ退役し通常recordを永久停止しない。
    ///
    #[test]
    fn shutdown_partial_write_keeps_groups_and_never_replays_snapshot() {
        /*
         * 同一主体の3groupとnew/amend集約を準備する
         */
        let dir = tempdir().unwrap();
        let writer = AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().into(),
            rotation_policy: super::super::rotation::AuditRotationPolicy::new(
                1024 * 1024,
            ),
        });
        let mut sink = AuditSink::new(AppendAuditBuffer::new(), writer);
        let template = test_append_record("page appended");
        for path in ["/a", "/b", "/c"] {
            let mut record = template.clone();
            record.target_path = Some(path.into());
            sink.record(record.clone()).unwrap();
            if path == "/a" {
                record.summary = Some("page appended (amended)".into());
                sink.record(record).unwrap();
            }
        }
        let fixed = sink.buffer.shutdown_snapshot(Utc::now());
        assert_eq!(fixed.len(), 3);
        sink.writer.inject_shutdown_failure(Some(2), true, false);

        /*
         * 2件目失敗でも最後のwriter flushを必ず試みる
         */
        assert!(sink.flush().is_err());
        assert_eq!(sink.writer.shutdown_attempts(), (2, 1));
        assert_eq!(sink.buffer.pending().len(), 2);
        let remaining = sink.buffer.shutdown_snapshot(
            Utc::now() + chrono::Duration::hours(1),
        );
        assert_eq!(remaining, fixed[1..].to_vec());
        assert!(sink.flush().is_err());
        assert_eq!(sink.writer.shutdown_attempts(), (2, 2));
        assert_eq!(sink.buffer.pending().len(), 2);

        /*
         * 通常recordは永続停止しない。停止snapshotの再投入はしない
         */
        let mut ordinary = template.clone();
        ordinary.operation = AuditOperation::Get;
        sink.record(ordinary).unwrap();
        assert_eq!(sink.writer.shutdown_attempts().0, 3);
        let bytes = fs::read(active_log_path(dir.path())).unwrap();
        let partial = [b'{', b'"', b'o', b'p', b'e', b'r', b'a'];
        assert!(bytes.ends_with(&partial));
    }

    ///
    /// 最終flush失敗で成功groupをbufferへ戻さない
    ///
    /// # 概要
    /// writer受理済みlineの再appendを禁止し終了Errを維持する。
    ///
    /// # 注記
    /// 全group退役後のflush障害とwrite/flush両障害を比較する。
    ///
    #[test]
    fn shutdown_final_flush_does_not_restore_successful_groups() {
        /*
         * writer受理後の最終flushだけを失敗させる
         */
        let dir = tempdir().unwrap();
        let writer = AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().into(),
            rotation_policy: super::super::rotation::AuditRotationPolicy::new(
                1024 * 1024,
            ),
        });
        let mut sink = AuditSink::new(AppendAuditBuffer::new(), writer);
        sink.record(test_append_record("page appended")).unwrap();
        sink.writer.inject_shutdown_failure(None, false, true);
        assert!(sink.flush().is_err());
        assert!(!sink.has_pending_appends());
        assert_eq!(sink.writer.shutdown_attempts(), (1, 1));
        assert!(sink.flush().is_err());
        assert_eq!(sink.writer.shutdown_attempts(), (1, 2));

        /*
         * write/flush両障害を固定診断へ保持する
         */
        let mut sink = AuditSink::new(AppendAuditBuffer::new(),
            AuditWriter::new(AuditWriterConfig {
                output_dir: dir.path().join("both"),
                rotation_policy:
                    super::super::rotation::AuditRotationPolicy::new(
                    1024 * 1024,
                ),
            }));
        sink.record(test_append_record("page appended")).unwrap();
        sink.writer.inject_shutdown_failure(Some(1), false, true);
        let error = sink.flush().unwrap_err();
        assert_eq!(sink.writer.shutdown_attempts(), (1, 1));
        assert!(sink.has_pending_appends());
        assert_eq!(error.to_string(),
            "audit append write and writer shutdown flush failed");
    }

    ///
    /// 既存size rotationとREST/MCP append集約を混在させる
    ///
    /// # 概要
    /// 共通writerでREST即時lineとMCP停止集約lineを出力する。
    ///
    /// # 注記
    /// rotated/active全lineを読み、操作と集約を照合する。
    ///
    #[test]
    fn mixed_rest_append_shutdown_reuses_size_rotation() {
        /*
         * 小閾値で既存writerのrotationだけを再利用する
         */
        let dir = tempdir().unwrap();
        let writer = AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().into(),
            rotation_policy: super::super::rotation::AuditRotationPolicy::new(
                100,
            ),
        });
        let mut sink = AuditSink::new(AppendAuditBuffer::new(), writer);
        let append = test_append_record("page appended");
        sink.record(append.clone()).unwrap();
        let mut amended = append.clone();
        amended.summary = Some("page appended (amended)".into());
        sink.record(amended).unwrap();
        let mut rest = append;
        rest.operation = AuditOperation::Rest(
            super::super::model::RestAuditOperation::Hello,
        );
        rest.target_path = None;
        sink.record(rest).unwrap();
        sink.flush().unwrap();

        /*
         * 即時RESTと一集約appendが同じwriter系へ出る
         */
        let mut rows = Vec::<AuditRecord>::new();
        for entry in fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|value| value == "jsonl") {
                for line in fs::read_to_string(path).unwrap().lines() {
                    rows.push(serde_json::from_str(line).unwrap());
                }
            }
        }
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row|
            row.operation.as_str() == "GET /api/hello"));
        let append = rows.iter().find(|row|
            row.operation == AuditOperation::Append).unwrap();
        assert!(append.summary.as_deref().unwrap().contains("2"));
        assert!(!sink.has_pending_appends());
    }

}
