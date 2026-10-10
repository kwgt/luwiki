/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 認可付きTopDocsのscoreを保ち、収集時の同score順を安定させる
//!

use std::cmp::Ordering;
use std::sync::{Arc, Mutex};

use tantivy::collector::{ScoreSegmentTweaker, ScoreTweaker};
use tantivy::schema::{Field, TantivyDocument, Value};
use tantivy::store::StoreReader;
use tantivy::{DocId, Score, SegmentReader};

///
/// 大きいscore、小さいpage ID/revisionを優先する比較キー
///
#[derive(Clone, Debug)]
pub(super) struct StableScore {
    /// 変更しない公開score
    pub(super) score: f32,
    /// 同score時のID順
    page_id: String,
    /// 同ID時のrevision順
    revision: u64,
}

impl PartialEq for StableScore {
    ///
    /// 比較キー全体の等価性を判定する
    ///
    /// # 引数
    /// * `other` - 比較対象
    ///
    /// # 戻り値
    /// 全順序で同順位ならtrueを返す。
    ///
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

impl PartialOrd for StableScore {
    ///
    /// TopDocsの優先順位を比較する
    ///
    /// # 引数
    /// * `other` - 比較対象
    ///
    /// # 戻り値
    /// score降順、ID/revision昇順を選ぶための比較結果。
    ///
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(
            self.score
                .total_cmp(&other.score)
                .then_with(|| other.page_id.cmp(&self.page_id))
                .then_with(|| other.revision.cmp(&self.revision)),
        )
    }
}

///
/// segment readerと比較キー生成を接続する
///
pub(super) struct StableScoreTweaker {
    /// page_id保存field
    pub(super) page_id: Field,
    /// revision保存field
    pub(super) revision: Field,
    /// score callbackで返せない読取失敗を公開前に引き継ぐ
    pub(super) failure: Arc<Mutex<Option<String>>>,
}

impl ScoreTweaker<StableScore> for StableScoreTweaker {
    type Child = StableSegmentTweaker;

    ///
    /// 対象segmentの保存値readerを取得する
    ///
    /// # 引数
    /// * `reader` - 検索対象segment
    ///
    /// # 戻り値
    /// キー生成器、または保存値readerの初期化エラー。
    ///
    fn segment_tweaker(
        &self,
        reader: &SegmentReader,
    ) -> tantivy::Result<Self::Child> {
        Ok(StableSegmentTweaker {
            store: reader.get_store_reader(1)?,
            page_id: self.page_id,
            revision: self.revision,
            failure: self.failure.clone(),
        })
    }
}

///
/// query一致docの保存ID/revisionだけを同score比較に使う
///
pub(super) struct StableSegmentTweaker {
    /// 同じsegmentの保存doc reader
    store: StoreReader,
    /// page_id保存field
    page_id: Field,
    /// revision保存field
    revision: Field,
    /// query単位の読取エラー引継ぎ
    failure: Arc<Mutex<Option<String>>>,
}

impl ScoreSegmentTweaker<StableScore> for StableSegmentTweaker {
    ///
    /// 原scoreと保存ID/revisionから比較キーを生成する
    ///
    /// # 引数
    /// * `doc` - query一致文書ID
    /// * `score` - 既存queryのscore
    ///
    /// # 戻り値
    /// 原scoreを保持する比較キー。読取失敗はlatchへ引き継ぐ。
    ///
    /// # 概要
    /// callbackの読取失敗は検索完了直後に判定し、代替キーを公開しない。
    ///
    fn score(&mut self, doc: DocId, score: Score) -> StableScore {
        /*
         * queryで許可された一致docのキーを読み取る
         */
        let result = self
            .store
            .get::<TantivyDocument>(doc)
            .map_err(|err| err.to_string())
            .and_then(|document| {
                let page_id = document
                    .get_first(self.page_id)
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| "search page_id missing".to_string())?;
                let revision = document
                    .get_first(self.revision)
                    .and_then(|value| value.as_u64())
                    .ok_or_else(|| "search revision missing".to_string())?;
                Ok(StableScore {
                    score,
                    page_id: page_id.to_string(),
                    revision,
                })
            });
        match result {
            Ok(key) => key,
            Err(err) => {
                /*
                 * TopDocs完了直後に失敗を返し、代替キーを公開しない
                 */
                let mut failure = self
                    .failure
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if failure.is_none() {
                    *failure = Some(err);
                }
                StableScore {
                    score,
                    page_id: String::new(),
                    revision: 0,
                }
            }
        }
    }
}
