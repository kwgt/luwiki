/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 認可付き更新・asset拒否無更新・同対象保証を隔離DBで検証する
//!

use std::fs;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Barrier};

use chrono::{Duration, Local};
use tempfile::{TempDir, tempdir};

use super::DatabaseManager;
use super::authorized_assets::{
    AssetPageSelector, AssetReadMode, AssetSelector,
};
use super::authorized_writes::PageDestination;
use super::mutation::{MutationAccess, MutationLock, MutationOutcome};
use crate::auth::PageAccessPolicy;
use crate::database::schema::{
    DELETED_PAGE_PATH_TABLE, LOCK_INFO_TABLE, PAGE_PATH_TABLE,
};
use crate::database::types::{LockInfo, PathPrefixSet};
use crate::database::{DbError, PathPrefixDenied};
use crate::wiki_path::DestinationPathInput;

///
/// fixtureごとにDB/assetsを隔離する
///
/// # 戻り値
/// 一時領域と初期化したDB。
///
fn fixture() -> (TempDir, DatabaseManager) {
    let dir = tempdir().unwrap();
    let db =
        DatabaseManager::open(dir.path().join("db"), dir.path().join("assets"))
            .unwrap();
    db.add_user("user", "pass", None).unwrap();
    (dir, db)
}

///
/// 既存unlocked条件の認可入力を作る
///
/// # 引数
/// * `prefixes` - 許可prefix
///
/// # 戻り値
/// 通常ページの認可/lock条件。
///
fn unlocked(prefixes: &PathPrefixSet) -> MutationAccess<'_> {
    MutationAccess {
        policy: PageAccessPolicy::new(prefixes),
        lock: MutationLock::Unlocked,
        allow_draft: false,
        allow_deleted: false,
        cleanup_draft: false,
        expected_version: None,
    }
}

///
/// source保存とlock解放が同txnでlatestを維持する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// denyではdraft/lock/assetを保全し、許可時は保存後indexのtokenだけを外す。
///
#[test]
fn source_atomic_save_preserves_revision_and_denied_draft() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let (page, lock) = db.create_draft_page("/allow/draft", "user").unwrap();
    let asset = db
        .create_asset(
            &page,
            "blob",
            "application/octet-stream",
            "user",
            b"data",
        )
        .unwrap();
    let denied_prefix = PathPrefixSet::from_iter(["/other"]);
    let denied = MutationAccess {
        policy: PageAccessPolicy::new(&denied_prefix),
        lock: MutationLock::Token {
            user_name: "user",
            token: Ok(Some(&lock.token())),
            validate_unlocked: false,
            invalid_token_reason: "lock token invalid",
        },
        allow_draft: true,
        allow_deleted: false,
        cleanup_draft: true,
        expected_version: None,
    };
    assert!(
        db.put_page_authorized(&page, "user", "first".into(), false, &denied)
            .err()
            .unwrap()
            .downcast_ref::<PathPrefixDenied>()
            .is_some()
    );
    assert!(db.get_page_index_by_id(&page).unwrap().unwrap().is_draft());
    assert_eq!(fs::read(db.asset_file_path(&asset)).unwrap(), b"data");
    let allowed_prefix = PathPrefixSet::from_iter(["/allow"]);
    let token = lock.token();
    let allowed = MutationAccess {
        policy: PageAccessPolicy::new(&allowed_prefix),
        lock: MutationLock::Token {
            user_name: "user",
            token: Ok(Some(&token)),
            validate_unlocked: false,
            invalid_token_reason: "lock token invalid",
        },
        allow_draft: true,
        allow_deleted: false,
        cleanup_draft: true,
        expected_version: None,
    };
    let saved = db
        .put_page_authorized(&page, "user", "first".into(), false, &allowed)
        .unwrap();
    assert!(saved.outcome.committed);
    assert_eq!(saved.outcome.revision, Some(1));
    assert!(
        db.get_page_index_by_id(&page)
            .unwrap()
            .unwrap()
            .lock_token()
            .is_none()
    );
    let mut allowed = unlocked(&allowed_prefix);
    allowed.allow_draft = true;
    let lock = db.acquire_page_lock(&page, "user").unwrap();
    let token = lock.token();
    allowed.lock = MutationLock::Token {
        user_name: "user",
        token: Ok(Some(&token)),
        validate_unlocked: false,
        invalid_token_reason: "lock token invalid",
    };
    let saved = db
        .put_page_authorized(&page, "user", "second".into(), false, &allowed)
        .unwrap();
    assert_eq!(saved.outcome.revision, Some(2));
    let index = db.get_page_index_by_id(&page).unwrap().unwrap();
    assert_eq!(index.latest(), 2);
    assert!(index.lock_token().is_none());
}

///
/// 元/先認可と再帰境界は無更新を保ち同txnの元basenameを使う
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// 元のみ/先のみdenyと子lock失敗を照合後、raw root/末尾配置先へ移動する。
///
#[test]
fn recursive_move_guards_both_origins_and_partial_changes() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let root = db
        .create_page("/allow/tree", "user", "root".into())
        .unwrap();
    let child = db
        .create_page("/allow/tree/child", "user", "child".into())
        .unwrap();
    let boundary = db
        .create_page("/allow/tree2", "user", "boundary".into())
        .unwrap();
    for prefixes in [
        PathPrefixSet::from_iter(["/allow"]),
        PathPrefixSet::from_iter(["/dest"]),
    ] {
        let input = DestinationPathInput::parse("/dest/").unwrap();
        let err = db
            .move_page_authorized(
                &root,
                PageDestination::Rest(&input),
                false,
                true,
                &unlocked(&prefixes),
                true,
            )
            .err()
            .unwrap();
        assert!(err.downcast_ref::<PathPrefixDenied>().is_some());
        assert_eq!(
            db.get_page_index_by_id(&root).unwrap().unwrap().path(),
            "/allow/tree"
        );
        assert_eq!(
            db.get_page_index_by_id(&child).unwrap().unwrap().path(),
            "/allow/tree/child"
        );
    }
    let prefixes = PathPrefixSet::from_iter(["/allow", "/dest"]);
    let input = DestinationPathInput::parse("/dest/./").unwrap();
    let lock = db.acquire_page_lock(&child, "user").unwrap();
    assert!(
        db.move_page_authorized(
            &root,
            PageDestination::Rest(&input),
            false,
            true,
            &unlocked(&prefixes),
            true
        )
        .is_err()
    );
    assert_eq!(db.get_page_index_by_id(&root).unwrap().unwrap().latest(), 1);
    db.release_page_lock(&child, "user", &lock.token()).unwrap();
    let moved = db
        .move_page_authorized(
            &root,
            PageDestination::Rest(&input),
            false,
            true,
            &unlocked(&prefixes),
            true,
        )
        .unwrap();
    assert_eq!(moved.outcome.path, "/dest/tree");
    assert_eq!(moved.outcome.affected_page_ids.len(), 2);
    assert_eq!(
        db.get_page_index_by_id(&child).unwrap().unwrap().path(),
        "/dest/tree/child"
    );
    assert_eq!(
        db.get_page_index_by_id(&boundary).unwrap().unwrap().path(),
        "/allow/tree2"
    );
    let root_input = DestinationPathInput::parse("/").unwrap();
    let all = PathPrefixSet::new();
    db.move_page_authorized(
        &root,
        PageDestination::Rest(&root_input),
        false,
        true,
        &unlocked(&all),
        true,
    )
    .unwrap();
    assert_eq!(
        db.get_page_index_by_id(&root).unwrap().unwrap().path(),
        "/tree"
    );
    assert_eq!(
        db.get_page_index_by_id(&child).unwrap().unwrap().path(),
        "/tree/child"
    );
}

///
/// upload拒否/登録/commit失敗はFS/DBを保全する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// cfg(test)の局所fault段階で新file回収を確認し、正常登録へ進む。
///
#[test]
fn upload_guard_recovers_new_files_and_denied_upload_has_no_fs() {
    /*
     * 隔離DBとfile領域を準備する
     */
    let (dir, db) = fixture();
    let page = db
        .create_page("/allow/page", "user", "body".into())
        .unwrap();
    let deny = PathPrefixSet::from_iter(["/other"]);
    assert!(
        db.create_asset_authorized(
            AssetPageSelector::Id(&page),
            "file.txt",
            "text/plain",
            "user",
            b"data",
            &unlocked(&deny)
        )
        .is_err()
    );
    assert!(!dir.path().join("assets").exists());
    let all = PathPrefixSet::new();
    for fault in [1, 2] {
        db.fail_asset_upload_for_test.store(fault, Ordering::SeqCst);
        let err = db
            .create_asset_authorized(
                AssetPageSelector::Id(&page),
                "file.txt",
                "text/plain",
                "user",
                b"data",
                &unlocked(&all),
            )
            .err()
            .unwrap();
        if fault == 2 {
            let outcome = err.downcast_ref::<MutationOutcome>().unwrap();
            assert!(!outcome.committed && !outcome.post_processing_failure);
            assert_eq!(outcome.path, "/allow/page");
            assert!(outcome.asset_id.is_some());
        }
        assert!(db.list_page_assets(&page).unwrap().is_empty());
        assert!(asset_files(dir.path().join("assets")).is_empty());
    }
    db.fail_asset_upload_for_test.store(0, Ordering::SeqCst);
    let created = db
        .create_asset_authorized(
            AssetPageSelector::Id(&page),
            "file.txt",
            "text/plain",
            "user",
            b"data",
            &unlocked(&all),
        )
        .unwrap();
    assert!(created.outcome.committed);
    let read = db
        .read_asset_authorized(
            AssetSelector::Id(&created.value.info.id()),
            &PageAccessPolicy::new(&all),
            AssetReadMode::Data { max_bytes: None },
        )
        .unwrap();
    assert_eq!(read.data.unwrap(), b"data");
}

///
/// 再帰的にfileだけを収集する
///
/// # 引数
/// * `root` - 隔離asset領域
///
/// # 戻り値
/// file一覧。
///
fn asset_files(root: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if !root.exists() {
        return files;
    }
    for item in fs::read_dir(root).unwrap() {
        let path = item.unwrap().path();
        if path.is_dir() {
            files.extend(asset_files(path));
        } else {
            files.push(path);
        }
    }
    files
}

///
/// 認可後expired draft cleanupのcommitとasset404を区別する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// 保存時刻を直接期限切れにしてsleepに依存しない。
///
#[test]
fn expired_draft_asset_delete_commits_cleanup_with_asset_not_found() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let (page, lock) = db.create_draft_page("/allow/draft", "user").unwrap();
    let asset = db
        .create_asset(&page, "x", "text/plain", "user", b"x")
        .unwrap();
    let mut value = serde_json::to_value(&lock).unwrap();
    value["expire"] =
        serde_json::json!((Local::now() - Duration::seconds(1)).to_rfc3339());
    let expired: LockInfo = serde_json::from_value(value).unwrap();
    let txn = db.db.begin_write().unwrap();
    txn.open_table(LOCK_INFO_TABLE)
        .unwrap()
        .insert(lock.token(), expired)
        .unwrap();
    txn.commit().unwrap();
    let all = PathPrefixSet::new();
    let access = MutationAccess {
        policy: PageAccessPolicy::new(&all),
        lock: MutationLock::Token {
            user_name: "user",
            token: Ok(None),
            validate_unlocked: false,
            invalid_token_reason: "lock token invalid",
        },
        allow_draft: true,
        allow_deleted: false,
        cleanup_draft: true,
        expected_version: None,
    };
    let err = db
        .delete_asset_authorized(AssetSelector::Id(&asset), &access)
        .err()
        .unwrap();
    assert!(matches!(
        err.downcast_ref::<DbError>(),
        Some(DbError::AssetNotFound)
    ));
    let outcome = err.downcast_ref::<MutationOutcome>().unwrap();
    assert!(
        outcome.committed
            && outcome.cleanup_only
            && !outcome.post_processing_failure
    );
    assert!(db.get_page_index_by_id(&page).unwrap().is_none());
    assert!(db.get_asset_info_by_id(&asset).unwrap().is_none());
    assert!(!db.asset_file_path(&asset).exists());
}

///
/// 最新再認可は並行writer後の別path/IDを追従して更新しない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// barrierでrenameと旧pathへの別ID作成を同期し、期待IDを固定する。
///
#[test]
fn fixed_id_writer_reauthorizes_after_parallel_rename_and_path_reuse() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let page = db
        .create_page("/allow/page", "user", "original".into())
        .unwrap();
    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(2));
    let writer = {
        let db = db.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            db.rename_page("/allow/page", "/outside/page").unwrap();
            let other = db
                .create_page("/allow/page", "user", "other".into())
                .unwrap();
            barrier.wait();
            other
        })
    };
    barrier.wait();
    barrier.wait();
    let other = writer.join().unwrap();
    let allow = PathPrefixSet::from_iter(["/allow"]);
    assert!(
        db.put_page_authorized(
            &page,
            "user",
            "bad".into(),
            false,
            &unlocked(&allow)
        )
        .err()
        .unwrap()
        .downcast_ref::<PathPrefixDenied>()
        .is_some()
    );
    assert_eq!(
        db.get_page_source(&other, 1).unwrap().unwrap().source(),
        "other"
    );
    let all = PathPrefixSet::new();
    let saved = db
        .put_page_authorized(
            &page,
            "user",
            "new".into(),
            false,
            &unlocked(&all),
        )
        .unwrap();
    assert_eq!(saved.outcome.path, "/outside/page");
    assert_eq!(
        db.get_page_source(&other, 1).unwrap().unwrap().source(),
        "other"
    );
}

///
/// zombie両状態・owner欠損・embedded ID不一致を区別する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// metadata変更だけのfixtureでFSを別IDへ追跡しないことを確認する。
///
#[test]
fn asset_snapshot_rejects_zombie_owner_and_embedded_id_inconsistency() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let page = db
        .create_page("/allow/page", "user", "body".into())
        .unwrap();
    let other = db
        .create_page("/outside/page", "user", "other".into())
        .unwrap();
    let all = PathPrefixSet::new();
    for deleted in [false, true] {
        let asset = db
            .create_asset(
                &page,
                if deleted { "z1" } else { "z0" },
                "text/plain",
                "user",
                b"z",
            )
            .unwrap();
        db.force_update_asset_info_for_test(&asset, |info| {
            info.clear_page_id();
            info.set_deleted(deleted);
        })
        .unwrap();
        let err = db
            .read_asset_authorized(
                AssetSelector::Id(&asset),
                &PageAccessPolicy::new(&all),
                AssetReadMode::Metadata,
            )
            .err()
            .unwrap();
        assert!(matches!(
            err.downcast_ref::<DbError>(),
            Some(DbError::AssetDeleted)
        ));
        let before =
            db.get_asset_info_by_id(&asset).unwrap().unwrap().deleted();
        assert!(
            db.delete_asset_authorized(
                AssetSelector::Id(&asset),
                &unlocked(&all)
            )
            .is_err()
        );
        assert_eq!(
            db.get_asset_info_by_id(&asset).unwrap().unwrap().deleted(),
            before
        );
    }
    let asset = db
        .create_asset(&page, "corrupt", "text/plain", "user", b"a")
        .unwrap();
    let other_asset = db
        .create_asset(&other, "other", "text/plain", "user", b"secret")
        .unwrap();
    let other_info = db.get_asset_info_by_id(&other_asset).unwrap().unwrap();
    db.force_update_asset_info_for_test(&asset, |info| *info = other_info)
        .unwrap();
    let err = db
        .read_asset_authorized(
            AssetSelector::Id(&asset),
            &PageAccessPolicy::new(&all),
            AssetReadMode::Data { max_bytes: None },
        )
        .err()
        .unwrap();
    assert!(err.to_string().contains("ID mismatch"));
    let missing = db
        .create_asset(&page, "missing-owner", "text/plain", "user", b"m")
        .unwrap();
    let fake = crate::database::types::PageId::new();
    db.force_update_asset_info_for_test(&missing, |info| {
        info.set_page_id(fake)
    })
    .unwrap();
    let err = db
        .read_asset_authorized(
            AssetSelector::Id(&missing),
            &PageAccessPolicy::new(&all),
            AssetReadMode::Metadata,
        )
        .err()
        .unwrap();
    assert!(err.downcast_ref::<DbError>().is_none());
    assert!(err.to_string().contains("owner index missing"));
}

///
/// zombie移動の履歴groupから他owner metadataを返さない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// 旧groupを保持する既存move/restoreの実操作で候補を照合する。
///
#[test]
fn historical_group_after_zombie_move_is_not_current_owner() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let old = db.create_page("/allow/old", "user", "old".into()).unwrap();
    let new = db
        .create_page("/outside/new", "user", "new".into())
        .unwrap();
    let asset = db
        .create_asset(&old, "history.txt", "text/plain", "user", b"secret")
        .unwrap();
    db.delete_page_by_id(&old).unwrap();
    db.move_asset(&asset, &new, true).unwrap();
    db.undelete_asset(&asset, Some("moved.txt")).unwrap();
    db.undelete_page_by_id(&old, "/allow/old", true).unwrap();
    let allowed = PathPrefixSet::from_iter(["/allow"]);
    let (_, assets) = db
        .list_assets_authorized(
            AssetPageSelector::Id(&old),
            &PageAccessPolicy::new(&allowed),
        )
        .unwrap();
    assert!(assets.is_empty());
    let err = db
        .read_asset_authorized(
            AssetSelector::File {
                page: AssetPageSelector::Id(&old),
                file_name: "moved.txt",
            },
            &PageAccessPolicy::new(&allowed),
            AssetReadMode::Metadata,
        )
        .err()
        .unwrap();
    assert!(matches!(
        err.downcast_ref::<DbError>(),
        Some(DbError::AssetNotFound)
    ));
}

///
/// DB commit後の派生失敗は元errとcommit対象を保持する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離した状態で要求結果と保存済みデータを照合する。
///
/// # 注記
/// 既存resource同期faultで保存済みsource/lockと型付きcontextを確認する。
///
#[test]
fn post_commit_sync_failure_keeps_saved_target_and_revision() {
    /*
     * 隔離DBと既存状態を準備する
     */
    let (_dir, db) = fixture();
    let page = db.create_page("/allow/page", "user", "old".into()).unwrap();
    db.set_resource_candidate_sync_failure_for_test(true);
    let all = PathPrefixSet::new();
    let err = db
        .put_page_authorized(
            &page,
            "user",
            "new".into(),
            false,
            &unlocked(&all),
        )
        .err()
        .unwrap();
    let outcome = err.downcast_ref::<MutationOutcome>().unwrap();
    assert!(outcome.committed && outcome.post_processing_failure);
    assert_eq!(outcome.path, "/allow/page");
    assert_eq!(outcome.revision, Some(2));
    assert_eq!(
        db.get_page_source(&page, 2).unwrap().unwrap().source(),
        "new"
    );
}

///
/// 壊れた配下path行は範囲外の最新IDを更新しない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 移動/削除/復元の全候補照合が本文/lock/asset/FSの変更前に拒否する。
///
/// # 注記
/// 配下path行に別pathのIDを注入し、3操作の拒否後に保存状態を比較する。
///
#[test]
fn recursive_path_index_mismatch_preserves_outside_target() {
    for operation in [0, 1, 2] {
        /*
         * 隔離DBと既存状態を準備する
         */
        let (_dir, db) = fixture();
        let root = db
            .create_page("/allow/tree", "user", "root".into())
            .unwrap();
        let outside = db
            .create_page("/outside/secret", "user", "secret".into())
            .unwrap();
        let asset = db
            .create_asset(&outside, "data", "text/plain", "user", b"secret")
            .unwrap();
        if operation == 2 {
            db.delete_page_by_id(&root).unwrap();
            db.delete_page_by_id(&outside).unwrap();
        }
        let lock = if operation != 2 {
            Some(db.acquire_page_lock(&outside, "user").unwrap())
        } else {
            None
        };
        let before = db.get_page_index_by_id(&outside).unwrap().unwrap();
        /*
         * 最新pathと一致しない配下行を隔離DBへ注入する
         */
        let txn = db.db.begin_write().unwrap();
        if operation == 2 {
            txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)
                .unwrap()
                .insert("/allow/tree/foreign".to_string(), outside.clone())
                .unwrap();
        } else {
            txn.open_table(PAGE_PATH_TABLE)
                .unwrap()
                .insert("/allow/tree/foreign".to_string(), outside.clone())
                .unwrap();
        }
        txn.commit().unwrap();
        let prefixes = PathPrefixSet::from_iter(["/allow", "/dest"]);
        let mut access = unlocked(&prefixes);
        access.allow_deleted = operation == 2;
        let destination = DestinationPathInput::parse("/dest/tree").unwrap();
        let err = if operation == 1 {
            db.delete_page_authorized(&root, true, "user", &access)
                .err()
                .unwrap()
        } else {
            db.move_page_authorized(
                &root,
                PageDestination::Rest(&destination),
                operation == 2,
                true,
                &access,
                true,
            )
            .err()
            .unwrap()
        };
        assert!(
            format!("{:#}", err).contains("recursive target index mismatch")
        );
        let after = db.get_page_index_by_id(&outside).unwrap().unwrap();
        assert_eq!(after.path(), before.path());
        assert_eq!(after.latest(), before.latest());
        assert_eq!(after.deleted(), before.deleted());
        assert_eq!(after.lock_token(), before.lock_token());
        assert_eq!(
            db.get_page_source(&outside, 1).unwrap().unwrap().source(),
            "secret"
        );
        assert_eq!(fs::read(db.asset_file_path(&asset)).unwrap(), b"secret");
        if let Some(lock) = lock {
            assert_eq!(
                db.get_page_lock_info(&outside).unwrap().unwrap().token(),
                lock.token()
            );
        }
    }
}

///
/// 非再帰/再帰の移動/復元は元先どちらの拒否でも部分更新しない
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 8ケースを実操作分岐で比較し、本文/状態/lock/asset/fileを保全する。
///
/// # 注記
/// 片側だけを許可し、配置先特例を解決した元/先のpolicyと保存状態を照合する。
///
#[test]
fn move_restore_denied_origin_matrix_preserves_all_state() {
    for recursive in [false, true] {
        for restore in [false, true] {
            for deny_source in [false, true] {
                /*
                 * 各ケースの通常/削除状態と既存asset/lockを隔離して準備する
                 */
                let (_dir, db) = fixture();
                let root = db
                    .create_page("/source/tree", "user", "root".into())
                    .unwrap();
                let child = db
                    .create_page("/source/tree/child", "user", "child".into())
                    .unwrap();
                let asset = db
                    .create_asset(
                        &root,
                        "data.txt",
                        "text/plain",
                        "user",
                        b"secret",
                    )
                    .unwrap();
                if restore {
                    db.delete_pages_recursive_by_id(&root, false).unwrap();
                } else {
                    db.acquire_page_lock(&root, "user").unwrap();
                    db.acquire_page_lock(&child, "user").unwrap();
                }
                let indexes: Vec<_> = [&root, &child]
                    .into_iter()
                    .map(|id| {
                        serde_json::to_value(
                            db.get_page_index_by_id(id).unwrap().unwrap(),
                        )
                        .unwrap()
                    })
                    .collect();
                let sources: Vec<_> = [&root, &child]
                    .into_iter()
                    .map(|id| {
                        serde_json::to_value(
                            db.get_page_source(id, 1).unwrap().unwrap(),
                        )
                        .unwrap()
                    })
                    .collect();
                let info = serde_json::to_value(
                    db.get_asset_info_by_id(&asset).unwrap().unwrap(),
                )
                .unwrap();
                let locks: Vec<_> = [&root, &child]
                    .into_iter()
                    .map(|id| {
                        serde_json::to_value(db.get_page_lock_info(id).unwrap())
                            .unwrap()
                    })
                    .collect();
                /*
                 * 一方だけ許可するpolicyとREST末尾特例の最終配置先を照合する
                 */
                let prefixes = PathPrefixSet::from_iter([if deny_source {
                    "/destination"
                } else {
                    "/source"
                }]);
                let policy = PageAccessPolicy::new(&prefixes);
                let input =
                    DestinationPathInput::parse("/destination/./").unwrap();
                let source = crate::wiki_path::NormalizedWikiPath::from_stored(
                    "/source/tree",
                )
                .unwrap();
                let target = input.resolve(&source).unwrap();
                assert_eq!(target.as_str(), "/destination/tree");
                assert_eq!(policy.allows(source.as_str()), !deny_source);
                assert_eq!(policy.allows(target.as_str()), deny_source);
                let mut access = unlocked(&prefixes);
                access.allow_deleted = restore;
                let err = db
                    .move_page_authorized(
                        &root,
                        PageDestination::Rest(&input),
                        restore,
                        recursive,
                        &access,
                        true,
                    )
                    .err()
                    .unwrap();
                assert!(err.downcast_ref::<PathPrefixDenied>().is_some());
                /*
                 * 認可拒否後も起点/配下/metadata/lock/fileが同じであることを確認する
                 */
                for (i, id) in [&root, &child].into_iter().enumerate() {
                    assert_eq!(
                        serde_json::to_value(
                            db.get_page_index_by_id(id).unwrap().unwrap()
                        )
                        .unwrap(),
                        indexes[i]
                    );
                    assert_eq!(
                        serde_json::to_value(
                            db.get_page_source(id, 1).unwrap().unwrap()
                        )
                        .unwrap(),
                        sources[i]
                    );
                    assert_eq!(
                        serde_json::to_value(
                            db.get_page_lock_info(id).unwrap()
                        )
                        .unwrap(),
                        locks[i]
                    );
                }
                assert_eq!(
                    serde_json::to_value(
                        db.get_asset_info_by_id(&asset).unwrap().unwrap()
                    )
                    .unwrap(),
                    info
                );
                assert_eq!(
                    fs::read(db.asset_file_path(&asset)).unwrap(),
                    b"secret"
                );
                assert!(
                    db.get_page_id_by_path("/destination/tree")
                        .unwrap()
                        .is_none()
                );
                assert!(
                    db.get_page_id_by_path("/destination/tree/child")
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}
