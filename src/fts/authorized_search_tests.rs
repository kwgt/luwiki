/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 認可FTS queryのscore保持と収集時cutoffを検証する
//!

use super::{FtsDocument, FtsIndexConfig, FtsIndexManager, FtsSearchTarget};
use crate::database::types::PageId;
use tantivy::doc;
use tempfile::tempdir;

///
/// 作成順から独立したPageIDを生成する
///
/// # 引数
/// * `index` - 一意番号
///
/// # 戻り値
/// 保存/比較できる固定PageID。
///
fn id(index: u64) -> PageId {
    PageId::from_string(&format!("{:026}", index)).unwrap()
}

///
/// 全同score候補を逆作成順にしても収集時100件を安定化する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離fixtureの許可/非許可・状態・順序を比較し、公開契約を確認する。
///
/// # 注記
/// 120許可IDと複数revisionを順/逆順にindex化し、各targetのcutoffを比較する。
///
#[test]
fn authorized_top100_is_stable_across_creation_order() {
    let dir = tempdir().unwrap();
    let config = FtsIndexConfig::new(dir.path().join("fts"));
    let allowed = (1..=120).map(id).collect::<Vec<_>>();
    for reverse in [false, true] {
        /*
         * 同scoreかつ複数revisionの文書を順/逆順で作る
         */
        let manager = FtsIndexManager::create(&config).unwrap();
        let mut docs = Vec::new();
        for page_id in &allowed {
            for revision in [1, 2] {
                docs.push(FtsDocument::new(
                    page_id.clone(),
                    revision,
                    false,
                    revision == 2,
                    "needle".into(),
                    "needle".into(),
                    "needle".into(),
                    "needle".into(),
                ));
            }
        }
        if reverse {
            docs.reverse();
        }
        manager.rebuild(&docs).unwrap();
        for target in [
            FtsSearchTarget::Headings,
            FtsSearchTarget::Body,
            FtsSearchTarget::Code,
            FtsSearchTarget::FrontMatter,
        ] {
            let results = manager
                .search_with_page_ids(
                    target,
                    "needle",
                    false,
                    true,
                    Some(&allowed),
                )
                .unwrap();
            let expected = allowed[..50]
                .iter()
                .flat_map(|page_id| {
                    [1, 2].map(|revision| (page_id.clone(), revision))
                })
                .collect::<Vec<_>>();
            let actual = results
                .iter()
                .map(|hit| (hit.page_id(), hit.revision()))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }
}

///
/// score0許可filterは同flagsの旧queryのscoreを変更しない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離fixtureの許可/非許可・状態・順序を比較し、公開契約を確認する。
///
/// # 注記
/// 既存deleted/latest条件のscore寄与を含め全flag組合せで比較する。
///
#[test]
fn authorized_filter_preserves_original_scores() {
    let dir = tempdir().unwrap();
    let config = FtsIndexConfig::new(dir.path().join("fts"));
    let manager = FtsIndexManager::create(&config).unwrap();
    let mut docs = Vec::new();
    for index in 1..=8 {
        for revision in [1, 2] {
            docs.push(FtsDocument::new(
                id(index),
                revision,
                index % 2 == 0,
                revision == 2,
                "needle".into(),
                format!("needle {}", "padding ".repeat(index as usize)),
                "needle".into(),
                "needle".into(),
            ));
        }
    }
    manager.rebuild(&docs).unwrap();
    let allowed = [id(1), id(2), id(5), id(8)];
    /*
     * 公開scoreは旧queryの各候補scoreとbit単位で一致する
     */
    for target in [
        FtsSearchTarget::Headings,
        FtsSearchTarget::Body,
        FtsSearchTarget::Code,
        FtsSearchTarget::FrontMatter,
    ] {
        for with_deleted in [false, true] {
            for all_revision in [false, true] {
                let original = manager
                    .search(target, "needle", with_deleted, all_revision)
                    .unwrap();
                let filtered = manager
                    .search_with_page_ids(
                        target,
                        "needle",
                        with_deleted,
                        all_revision,
                        Some(&allowed),
                    )
                    .unwrap();
                for hit in filtered {
                    assert!(allowed.contains(&hit.page_id()));
                    let old = original
                        .iter()
                        .find(|old| {
                            old.page_id() == hit.page_id()
                                && old.revision() == hit.revision()
                        })
                        .unwrap();
                    assert_eq!(hit.score().to_bits(), old.score().to_bits());
                }
            }
        }
    }
}

///
/// 一致docの保存キー読取失敗をTopDocs直後に返す
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離fixtureの許可/非許可・状態・順序を比較し、公開契約を確認する。
///
/// # 注記
/// 範囲外の不正docはcallbackへ入らず、許可docの不正は公開前に失敗する。
///
#[test]
fn stored_key_failure_is_reported_before_returning_hits() {
    let dir = tempdir().unwrap();
    let config = FtsIndexConfig::new(dir.path().join("fts"));
    let manager = FtsIndexManager::create(&config).unwrap();
    manager
        .rebuild(&[FtsDocument::new(
            id(1),
            1,
            false,
            true,
            "needle".into(),
            "needle".into(),
            "needle".into(),
            "needle".into(),
        )])
        .unwrap();
    /*
     * indexed page_idはあるが保存revisionのないdocを追加する
     */
    let mut writer = manager.index.writer(50_000_000).unwrap();
    writer
        .add_document(doc!(
            manager.schema.page_id => id(2).to_string(),
            manager.schema.deleted => false,
            manager.schema.is_latest => true,
            manager.schema.body => "needle",
        ))
        .unwrap();
    writer.commit().unwrap();
    drop(writer);
    let allowed = manager
        .search_with_page_ids(
            FtsSearchTarget::Body,
            "needle",
            false,
            false,
            Some(&[id(1)]),
        )
        .unwrap();
    assert_eq!(allowed.len(), 1);
    let err = manager
        .search_with_page_ids(
            FtsSearchTarget::Body,
            "needle",
            false,
            false,
            Some(&[id(2)]),
        )
        .unwrap_err();
    assert!(err.to_string().contains("stable search key read failed"));
}
