/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 監査イベント投入入口の骨格を定義するモジュール
//!

use anyhow::Result;
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
/// 監査イベント投入入口の骨格
///
#[derive(Debug)]
pub(crate) struct AuditSink {
    /// `append` 集約バッファ
    buffer: AppendAuditBuffer,

    /// JSONL writer
    writer: AuditWriter,
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
        Self { buffer, writer }
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
    /// 現時点では writer の flush 結果を返す。
    ///
    pub(crate) fn flush(&mut self) -> Result<()> {
        /*
         * 保留中 `append` を先に確定する
         */
        let flushed = self.buffer.flush_all(Utc::now());
        self.write_records(&flushed)?;

        /*
         * writer 自身の flush
         */
        self.writer.flush()
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
}
