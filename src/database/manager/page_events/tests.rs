/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//! ブラウザ通知の実DB・認可・確定点の検証

use super::*;
use crate::auth::AuthUser;
use crate::database::types::{BearerScopeSet, UserAttributeSet};
use redb::ReadableTable;

///
/// Basic認証済みの隔離対象を作る
///
/// # 戻り値
/// 作業領域・DB・主体・資格識別値・表示ID・基準。
///
fn fixture() -> (
    tempfile::TempDir,
    DatabaseManager,
    AuthContext,
    String,
    PageId,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let db =
        DatabaseManager::open(dir.path().join("db"), dir.path().join("assets"))
            .unwrap();
    db.add_user("alice", "password", None).unwrap();
    db.create_page("/parent", "alice", "parent".into()).unwrap();
    let id = db.create_page("/parent/p", "alice", "body".into()).unwrap();
    let user = db
        .authenticate_basic_user("alice", "password")
        .unwrap()
        .unwrap();
    let auth = AuthContext::new_with_actor(
        AuthUser::new("alice".into()),
        BearerScopeSet::all(),
        PathPrefixSet::new(),
        UserAttributeSet::new(),
        None,
        None,
        user.id(),
    );
    let current = db
        .read_browser_page_state(
            &auth,
            Some(&user.credential_tag()),
            "/parent/p",
            &id,
            "",
        )
        .unwrap();
    let token = current.page_state.unwrap().state_token;
    (dir, db, auth, user.credential_tag(), id, token)
}

///
/// rollbackは起床せず、amend成功は同revisionでも通知対象になる
///
/// # 注記
/// 未確定txnを破棄した後、同じ本文のamendを確定する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_commit_and_amend() {
    let (_dir, db, auth, credential, id, token) = fixture();
    let mut receiver = db.subscribe_page_changes();
    drop(receiver.borrow_and_update());
    drop(db.db.begin_write().unwrap());
    assert!(!receiver.has_changed().unwrap());
    db.put_page(&id, "alice", "body".into(), true).unwrap();
    assert!(receiver.has_changed().unwrap());
    let state = db
        .read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        )
        .unwrap();
    assert_eq!(state.kind, "updated");
    assert_eq!(state.page_state.unwrap().revision, 1);
}

///
/// rename・hard delete・同path再作成と先祖導線を区別する
///
/// # 注記
/// 通常ページを移動・削除・再作成し、同snapshotの公開分類を比較する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_rename_delete_and_replacement() {
    let (_dir, db, auth, credential, id, token) = fixture();
    db.rename_page("/parent/p", "/moved").unwrap();
    let read = || {
        db.read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        )
        .unwrap()
    };
    assert_eq!(read().kind, "renamed");
    assert_eq!(read().navigation.unwrap().path, "/moved");
    db.delete_page_by_id_hard(&id).unwrap();
    assert_eq!(read().kind, "unavailable");
    assert_eq!(read().navigation.unwrap().path, "/parent");
    db.create_page("/parent/p", "alice", "new".into()).unwrap();
    assert_eq!(read().kind, "replaced");
}

///
/// handshake後のpassword変更で古い接続を拒否する
///
/// # 注記
/// 同じユーザIDの資格情報だけを更新し、旧識別値を再照合する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_reject_changed_credentials() {
    let (_dir, db, auth, credential, id, token) = fixture();
    let txn = db.db.begin_write().unwrap();
    {
        let mut users = txn.open_table(USER_INFO_TABLE).unwrap();
        let mut user = users
            .get(auth.actor_id().unwrap().clone())
            .unwrap()
            .unwrap()
            .value();
        user.set_password("changed-password");
        users.insert(user.id(), user).unwrap();
    }
    db.commit_page_change(txn).unwrap();
    let error = db
        .read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        )
        .err()
        .unwrap();
    assert_eq!(
        error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Authentication)
    );
}

///
/// Bearerの移動先と先祖を独立認可する
///
/// # 注記
/// /parent/p限定tokenでは親を返さず、prefix外へ移動した対象も拒否する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_prefix_preserves_navigation_privacy() {
    let (_dir, db, _, _, id, expected) = fixture();
    let (_, token) = db
        .create_bearer_token(
            "alice",
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/parent/p"]),
            chrono::Duration::hours(1),
            None,
        )
        .unwrap();
    let auth = AuthContext::new_with_actor(
        AuthUser::new("alice".into()),
        token.scopes(),
        token.path_prefixes(),
        UserAttributeSet::new(),
        Some(token.token_id()),
        None,
        token.user_id(),
    );
    db.rename_page("/parent/p", "/outside").unwrap();
    let error = db
        .read_browser_page_state(&auth, None, "/parent/p", &id, &expected)
        .err()
        .unwrap();
    assert_eq!(
        error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Prefix)
    );
    db.delete_page_by_id_hard(&id).unwrap();
    let state = db
        .read_browser_page_state(&auth, None, "/parent/p", &id, &expected)
        .unwrap();
    assert!(state.navigation.is_none());
}

///
/// 不整合を取得不能へ丸めず、認証拒否を先に確定する
///
/// # 注記
/// sourceだけを除いた隔離DBで、通常認証と不正資格識別値を比較する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_corruption_and_denial_do_not_write() {
    let (_dir, db, auth, credential, id, token) = fixture();
    /* 隔離fixtureのsourceを欠落させる。 */
    let txn = db.db.begin_write().unwrap();
    {
        let mut sources = txn.open_table(PAGE_SOURCE_TABLE).unwrap();
        sources.remove((id.clone(), 1)).unwrap();
    }
    db.commit_page_change(txn).unwrap();
    let mut receiver = db.subscribe_page_changes();
    drop(receiver.borrow_and_update());

    /* 正常認証では内部失敗、失効済み資格では認証拒否とする。 */
    let error = db
        .read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        )
        .err()
        .unwrap();
    assert!(error.downcast_ref::<StateAccessDenied>().is_none());
    let denied = db
        .read_browser_page_state(
            &auth,
            Some("wrong credential tag"),
            "/parent/p",
            &id,
            &token,
        )
        .err()
        .unwrap();
    assert_eq!(
        denied.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Authentication)
    );
    assert!(!receiver.has_changed().unwrap());
}

///
/// receiver不在と短時間の更新を現在状態へ集約する
///
/// # 注記
/// 保存後にreceiverを登録し、追加保存後の最新revisionを確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_coalesce_and_late_subscription() {
    let (_dir, db, auth, credential, id, token) = fixture();
    db.put_page(&id, "alice", "before subscribe".into(), false)
        .unwrap();
    let mut receiver = db.subscribe_page_changes();
    drop(receiver.borrow_and_update());
    for n in 0..3 {
        db.put_page(&id, "alice", format!("change {n}"), false)
            .unwrap();
    }
    assert!(receiver.has_changed().unwrap());
    let state = db
        .read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        )
        .unwrap();
    assert_eq!(state.page_state.unwrap().revision, 5);
}

///
/// commit後の同期失敗でも保存した状態を通知できる
///
/// # 注記
/// 派生同期失敗を注入し、API失敗とDB成功を区別する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_publish_before_post_processing_failure() {
    let (_dir, db, auth, credential, id, token) = fixture();
    let mut receiver = db.subscribe_page_changes();
    drop(receiver.borrow_and_update());
    db.set_resource_candidate_sync_failure_for_test(true);
    assert!(db.put_page(&id, "alice", "saved".into(), false).is_err());
    assert!(receiver.has_changed().unwrap());
    let state = db
        .read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        )
        .unwrap();
    assert_eq!(state.kind, "updated");
    assert_eq!(state.page_state.unwrap().revision, 2);
}

///
/// ReadOnlyへの変更は参照を許し、NoBasicAuthは接続を拒否する
///
/// # 注記
/// 同一IDの属性だけを変更し、保存済みcredentialで再照合する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_current_basic_attributes() {
    use crate::database::types::UserAttribute;
    let (_dir, db, auth, credential, id, token) = fixture();
    for attribute in [UserAttribute::ReadOnly, UserAttribute::NoBasicAuth] {
        let txn = db.db.begin_write().unwrap();
        {
            let mut users = txn.open_table(USER_INFO_TABLE).unwrap();
            let mut user = users
                .get(auth.actor_id().unwrap().clone())
                .unwrap()
                .unwrap()
                .value();
            user.set_attributes(UserAttributeSet::from_iter([attribute]));
            users.insert(user.id(), user).unwrap();
        }
        db.commit_page_change(txn).unwrap();
        let state = db.read_browser_page_state(
            &auth,
            Some(&credential),
            "/parent/p",
            &id,
            &token,
        );
        if attribute == UserAttribute::ReadOnly {
            assert_eq!(state.unwrap().kind, "unchanged");
        } else {
            assert_eq!(
                state.err().unwrap().downcast_ref::<StateAccessDenied>(),
                Some(&StateAccessDenied::Authentication)
            );
        }
    }
}

///
/// ブラウザ照合が現在prefix・失効・期限を確認しTTLを延長しない
///
/// # 注記
/// 同じ認証文脈を保持して管理情報を変更し、拒否分類を比較する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_current_bearer_authorization() {
    let (_dir, db, _, _, id, expected) = fixture();
    for change in ["prefix", "revoke", "expiry", "scope"] {
        /* 古い入口のread許可を信用しないことも確認する。 */
        let scopes = if change == "scope" {
            BearerScopeSet::from_iter([BearerScope::Append])
        } else {
            BearerScopeSet::from_iter([BearerScope::Read])
        };
        let (_, token) = db.create_bearer_token(
            "alice", scopes, PathPrefixSet::from_iter(["/parent"]),
            chrono::Duration::hours(1), None,
        ).unwrap();
        let auth = AuthContext::new_with_actor(
            AuthUser::new("alice".into()), BearerScopeSet::all(),
            PathPrefixSet::new(), UserAttributeSet::new(),
            Some(token.token_id()), None, token.user_id(),
        );
        let read = || db.read_browser_page_state(
            &auth, None, "/parent/p", &id, &expected,
        );
        if change != "scope" {
            assert_eq!(read().unwrap().kind, "unchanged");
            db.check_page_event_identity(&auth, None).unwrap();
            let current = db.get_bearer_token_info_by_id(
                &token.token_id(),
            ).unwrap().unwrap();
            assert_eq!(current.expire_at(), token.expire_at());
        }

        /* 管理側の変更後も古い認証文脈で再照合する。 */
        let denied = match change {
            "prefix" => {
                db.remove_path_prefix_from_bearer_token(
                    &token.token_id(), "/parent",
                ).unwrap();
                db.add_path_prefix_to_bearer_token(
                    &token.token_id(), "/other",
                ).unwrap();
                StateAccessDenied::Prefix
            }
            "revoke" => {
                db.revoke_bearer_token_by_id(&token.token_id()).unwrap();
                StateAccessDenied::Authentication
            }
            "expiry" => {
                let hash = db.get_bearer_token_hash_by_id(
                    &token.token_id(),
                ).unwrap().unwrap();
                let txn = db.db.begin_write().unwrap();
                {
                    let mut table = txn.open_table(BEARER_TOKEN_TABLE)
                        .unwrap();
                    let mut value = table.get(hash.clone()).unwrap()
                        .unwrap().value();
                    let expired = Local::now() - chrono::Duration::seconds(1);
                    value.overwrite_timestamps_for_test(
                        expired, expired, expired,
                    );
                    table.insert(hash, value).unwrap();
                }
                db.commit_page_change(txn).unwrap();
                StateAccessDenied::Authentication
            }
            _ => StateAccessDenied::Scope,
        };
        let error = read().err().unwrap();
        assert_eq!(error.downcast_ref::<StateAccessDenied>(), Some(&denied));
    }
}

///
/// 同名ユーザを再作成しても旧接続の主体を置き換えない
///
/// # 注記
/// 削除前のIDと資格識別値を残し、同名・同passwordの主体を再作成する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_basic_subject_replacement_is_denied() {
    let (_dir, db, auth, credential, id, expected) = fixture();
    db.delete_user("alice").unwrap();
    db.add_user("alice", "password", None).unwrap();
    let error = db.read_browser_page_state(
        &auth, Some(&credential), "/parent/p", &id, &expected,
    ).err().unwrap();
    assert_eq!(error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Authentication));
}

///
/// 複数の履歴・再帰操作でcommit確定の起床を取りこぼさない
///
/// # 注記
/// 各操作の前に既読印を更新し、成功時のwatchと失敗時の無起床を確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_history_and_recursive_commits() {
    let (_dir, db, _, _, id, _) = fixture();
    let mut changes = db.subscribe_page_changes();
    /* 履歴の増加・rollback・compactionの確定を確認する。 */
    drop(changes.borrow_and_update());
    db.put_page(&id, "alice", "second".into(), false).unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    db.rollback_page_source_only(&id, 1).unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    db.compact_page_source(&id, 1).unwrap();
    assert!(changes.has_changed().unwrap());

    /* 子を含むrename・削除・復元・hard deleteの確定を確認する。 */
    db.create_page("/parent/p/child", "alice", "child".into()).unwrap();
    drop(changes.borrow_and_update());
    db.rename_pages_recursive_by_id(&id, "/moved").unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    db.delete_pages_recursive_by_id(&id, false).unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    db.undelete_pages_recursive_by_id(&id, "/parent/p", true).unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    db.delete_pages_recursive_by_id(&id, true).unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    assert!(db.put_page(&id, "alice", "missing".into(), false).is_err());
    assert!(!changes.has_changed().unwrap());
}

///
/// draft作成・保存・期限cleanupを確定通知へ接続する
///
/// # 注記
/// 有効draftを保存し、別draftを期限切れにしてcleanup後の消失を確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_draft_save_and_cleanup_commits() {
    let (_dir, db, _, _, _, _) = fixture();
    let mut changes = db.subscribe_page_changes();
    drop(changes.borrow_and_update());
    let (draft, _) = db.create_draft_page("/draft", "alice").unwrap();
    assert!(changes.has_changed().unwrap());
    drop(changes.borrow_and_update());
    db.put_page(&draft, "alice", "saved draft".into(), false).unwrap();
    assert!(changes.has_changed().unwrap());

    /* cleanupによりdraftが通常取得不能になることを確認する。 */
    let (expired, _) = db.create_draft_page("/expired", "alice").unwrap();
    db.expire_page_lock_for_test(&expired).unwrap();
    drop(changes.borrow_and_update());
    db.cleanup_expired_locks().unwrap();
    assert!(changes.has_changed().unwrap());
    assert!(db.get_page_id_by_path("/expired").unwrap().is_none());
}

///
/// 不在・draft先祖を飛ばしrootで探索を終了する
///
/// # 注記
/// rootだけを通常候補として残し、root自身には先祖導線を返さない。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_events_ancestor_root_and_missing_terminal() {
    let (_dir, db, auth, credential, id, expected) = fixture();
    let root = db.get_page_id_by_path("/").unwrap().unwrap_or_else(|| {
        db.create_page("/", "alice", "root".into()).unwrap()
    });
    db.delete_page_by_id_hard(&id).unwrap();
    let parent = db.get_page_id_by_path("/parent").unwrap().unwrap();
    db.delete_page_by_id_hard(&parent).unwrap();
    db.create_draft_page("/parent", "alice").unwrap();
    let read = || db.read_browser_page_state(
        &auth, Some(&credential), "/parent/p", &id, &expected,
    ).unwrap();
    assert_eq!(read().navigation.unwrap().path, "/");
    assert!(db.delete_page_by_id_hard(&root).is_err());
    let root_state = db.read_browser_page_state(
        &auth, Some(&credential), "/", &root, &expected,
    ).unwrap();
    assert_eq!(root_state.kind, "updated");
    assert!(root_state.navigation.is_none());
}
