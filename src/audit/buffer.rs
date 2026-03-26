/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! `append` 集約用バッファの骨格を定義するモジュール
//!

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use super::model::{
    AppendAuditOutcome,
    AppendAuditKey,
    AppendAuditSummarySeed,
    AuditRecord,
};

///
/// 保留中の `append` 集約状態
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingAppendAudit {
    /// 集約キー
    pub(crate) key: AppendAuditKey,

    /// 集約開始時刻
    pub(crate) first_timestamp: DateTime<Utc>,

    /// 最終受理時刻
    pub(crate) last_timestamp: DateTime<Utc>,

    /// 集約した append 件数
    pub(crate) append_count: u64,

    /// 初回観測時の revision
    pub(crate) revision: Option<u64>,

    /// summary 生成用の中間情報
    pub(crate) summary_seed: AppendAuditSummarySeed,

    /// 不変部分のテンプレートレコード
    pub(crate) record_template: AuditRecord,
}

///
/// 保留中 `append` 集約の管理マップ
///
pub(crate) type PendingAppendMap =
    BTreeMap<AppendAuditKey, PendingAppendAudit>;

/// `append` 集約の時間窓(秒)
const APPEND_AUDIT_WINDOW_SECS: i64 = 60;

///
/// `append` 集約バッファの骨格
///
#[derive(Debug, Default)]
pub(crate) struct AppendAuditBuffer {
    /// 保留中の `append` 集約
    pending: PendingAppendMap,
}

impl AppendAuditBuffer {
    ///
    /// `append` 集約バッファの生成
    ///
    /// # 戻り値
    /// 生成した `append` 集約バッファを返す。
    ///
    pub(crate) fn new() -> Self {
        Self::default()
    }

    ///
    /// 保留中集約マップへのアクセサ
    ///
    /// # 戻り値
    /// 保留中集約マップへの参照を返す。
    ///
    pub(crate) fn pending<'a>(&'a self) -> &'a PendingAppendMap {
        &self.pending
    }

    ///
    /// `append` 成功イベントを保留し、期限切れ集約を返す
    ///
    /// # 引数
    /// * `record` - 受理した `append` 成功レコード
    /// * `outcome` - 集約用の内部結果分類
    ///
    /// # 戻り値
    /// 今回の受理契機で確定した監査レコード群を返す。
    ///
    pub(crate) fn push_append(
        &mut self,
        record: AuditRecord,
        outcome: AppendAuditOutcome,
    ) -> Vec<AuditRecord> {
        /*
         * 期限切れ集約の確定
         */
        let flushed = self.collect_expired(record.timestamp);

        /*
         * 受理レコードの集約
         */
        let Some(key) = record.append_audit_key() else {
            return flushed;
        };

        if let Some(pending) = self.pending.get_mut(&key) {
            pending.absorb(&record, outcome);
            return flushed;
        }

        let pending = PendingAppendAudit::new(record, key, outcome);
        self.pending.insert(pending.key.clone(), pending);

        flushed
    }

    ///
    /// 指定時刻までに期限切れとなった集約を確定する
    ///
    /// # 引数
    /// * `now` - 期限切れ判定基準時刻
    ///
    /// # 戻り値
    /// 確定した監査レコード群を返す。
    ///
    pub(crate) fn collect_expired(
        &mut self,
        now: DateTime<Utc>,
    ) -> Vec<AuditRecord> {
        /*
         * 期限切れキーの抽出
         */
        let expired_keys = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.is_expired_at(now))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();

        /*
         * 期限切れ集約の確定
         */
        self.take_records(&expired_keys, now)
    }

    ///
    /// 保留中の全 `append` 集約を確定する
    ///
    /// # 引数
    /// * `now` - 確定時刻
    ///
    /// # 戻り値
    /// 確定した監査レコード群を返す。
    ///
    pub(crate) fn flush_all(
        &mut self,
        now: DateTime<Utc>,
    ) -> Vec<AuditRecord> {
        let keys = self.pending.keys().cloned().collect::<Vec<_>>();
        self.take_records(&keys, now)
    }

    ///
    /// 保留状態の有無を返す
    ///
    /// # 戻り値
    /// 保留中集約が存在する場合は `true` を返す。
    ///
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    ///
    /// 指定キー群に対応する保留集約を確定する
    ///
    /// # 引数
    /// * `keys` - 確定対象キー
    /// * `now` - 確定時刻
    ///
    /// # 戻り値
    /// 確定した監査レコード群を返す。
    ///
    fn take_records(
        &mut self,
        keys: &[AppendAuditKey],
        now: DateTime<Utc>,
    ) -> Vec<AuditRecord> {
        keys.iter()
            .filter_map(|key| self.pending.remove(key))
            .map(|pending| pending.build_record(now))
            .collect()
    }
}

impl PendingAppendAudit {
    ///
    /// 保留中 `append` 集約状態を生成する
    ///
    /// # 引数
    /// * `record` - 初回受理レコード
    /// * `key` - 集約キー
    /// * `outcome` - 集約用の内部結果分類
    ///
    /// # 戻り値
    /// 生成した保留状態を返す。
    ///
    fn new(
        record: AuditRecord,
        key: AppendAuditKey,
        outcome: AppendAuditOutcome,
    ) -> Self {
        let mut summary_seed = AppendAuditSummarySeed::default();
        summary_seed.record(outcome, record.summary.as_deref());

        Self {
            key,
            first_timestamp: record.timestamp,
            last_timestamp: record.timestamp,
            append_count: 1,
            revision: record.revision,
            summary_seed,
            record_template: record,
        }
    }

    ///
    /// 追加 `append` 成功レコードを集約へ取り込む
    ///
    /// # 引数
    /// * `record` - 追加受理レコード
    /// * `outcome` - 集約用の内部結果分類
    ///
    /// # 戻り値
    /// なし
    ///
    fn absorb(
        &mut self,
        record: &AuditRecord,
        outcome: AppendAuditOutcome,
    ) {
        self.last_timestamp = record.timestamp;
        self.append_count += 1;
        self.summary_seed
            .record(outcome, record.summary.as_deref());
    }

    ///
    /// 指定時刻時点で期限切れかを返す
    ///
    /// # 引数
    /// * `now` - 判定時刻
    ///
    /// # 戻り値
    /// 時間窓を超過している場合は `true` を返す。
    ///
    fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        (now - self.last_timestamp).num_seconds() >= APPEND_AUDIT_WINDOW_SECS
    }

    ///
    /// 保留状態から確定済み監査レコードを生成する
    ///
    /// # 引数
    /// * `now` - 確定時刻
    ///
    /// # 戻り値
    /// 生成した監査レコードを返す。
    ///
    fn build_record(&self, now: DateTime<Utc>) -> AuditRecord {
        let mut record = self.record_template.clone();
        record.timestamp = now;
        record.summary = Some(self.build_summary());
        record.revision = self.revision;

        record
    }

    ///
    /// 集約済み `append` 向け summary を生成する
    ///
    /// # 戻り値
    /// 集約件数を含む summary を返す。
    ///
    fn build_summary(&self) -> String {
        let mut summary = format!(
            "page appended {} times (amended={}, new_revision={})",
            self.append_count,
            self.summary_seed.amend_count,
            self.summary_seed.new_revision_count,
        );

        if let Some(last_summary) = self.summary_seed.last_summary.as_deref() {
            summary.push_str(": ");
            summary.push_str(last_summary);
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;
    use crate::audit::model::{AuditOperation, AuditResult};
    use crate::database::types::{TokenId, UserId};

    fn append_record(
        timestamp_offset_secs: i64,
        summary: &str,
        revision: u64,
    ) -> AuditRecord {
        AuditRecord::new(
            AuditOperation::Append,
            UserId::from_string("01J00000000000000000000000")
                .expect("user id"),
            Some(
                TokenId::from_string("01J00000000000000000000001")
                    .expect("token id"),
            ),
            None,
            Some("/audit/page".to_string()),
            AuditResult::Success,
            Utc::now() + Duration::seconds(timestamp_offset_secs),
            Some(summary.to_string()),
            Some(revision),
        )
    }

    ///
    /// 同一キーの `append` が 1 件へ集約されることを確認する。
    ///
    /// 注記:
    /// 2 件投入後に明示 flush を実行し、集約 summary を検証する。
    ///
    #[test]
    fn push_append_merges_same_key_records() {
        let mut buffer = AppendAuditBuffer::new();
        let first = append_record(0, "page appended", 2);
        let second = append_record(1, "page appended (amended)", 2);

        assert!(buffer
            .push_append(first, AppendAuditOutcome::NewRevision)
            .is_empty());
        assert!(buffer
            .push_append(second, AppendAuditOutcome::Amended)
            .is_empty());

        let flushed = buffer.flush_all(Utc::now() + Duration::seconds(2));
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].revision, Some(2));
        assert!(flushed[0]
            .summary
            .as_deref()
            .expect("summary")
            .contains("page appended 2 times"));
    }

    ///
    /// 時間窓を超えた保留集約だけが確定することを確認する。
    ///
    /// 注記:
    /// 初回投入から 61 秒後に期限切れ回収を呼び出す。
    ///
    #[test]
    fn collect_expired_returns_window_expired_records() {
        let mut buffer = AppendAuditBuffer::new();
        let first = append_record(0, "page appended", 4);
        let now = first.timestamp + Duration::seconds(61);

        buffer.push_append(first, AppendAuditOutcome::NewRevision);
        let flushed = buffer.collect_expired(now);

        assert_eq!(flushed.len(), 1);
        assert!(!buffer.has_pending());
    }
}
