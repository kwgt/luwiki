/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 状態snapshotと比較値の実データによる回帰試験
//!

use super::*;
use redb::ReadableTable;
use crate::auth::AuthUser;
use crate::database::types::{
    BearerScopeSet, PathPrefixSet, UserAttribute, UserAttributeSet,
};
use crate::mcp::service::page_state::{read_state, valid_state_token};
use tempfile::{TempDir, tempdir};

///
/// 隔離DBと認証済み主体を準備する
///
/// # 引数
/// * `scope` - DBに保存するscope
///
/// # 戻り値
/// 作業領域、DB、入口認証文脈。
///
fn fixture(scope: BearerScope) -> (TempDir, DatabaseManager, AuthContext) {
    let dir = tempdir().unwrap();
    let db = DatabaseManager::open(
        dir.path().join("db"), dir.path().join("assets"),
    ).unwrap();
    db.add_user("alice", "password", None).unwrap();
    db.create_page("/p", "alice", "# old".into()).unwrap();
    let (_, token) = db.create_bearer_token(
        "alice", BearerScopeSet::from_iter([scope]),
        PathPrefixSet::from_iter(["/p"]), chrono::Duration::hours(1), None,
    ).unwrap();
    let auth = AuthContext::new_with_actor(
        AuthUser::new("alice".into()), token.scopes(), token.path_prefixes(),
        UserAttributeSet::new(), Some(token.token_id()), None, token.user_id(),
    );
    (dir, db, auth)
}

///
/// 通常更新・amend・置換・renameを識別する
///
/// # 注記
/// 実DBへ保存し、revision据置でもtokenが変わり内部IDが出ないことを確認。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_versions_and_replacement() {
    /* 同一本文のamendも識別する。 */
    let (_dir, db, auth) = fixture(BearerScope::Read);
    let id = db.get_page_id_by_path("/p").unwrap().unwrap();
    let old = read_state(&db, &auth, "/p").unwrap().unwrap();
    assert!(valid_state_token(&old.state_token));
    assert!(!old.state_token.contains(&id.to_string()));
    db.put_page(&id, "alice", "# old".into(), true).unwrap();
    let amended = read_state(&db, &auth, "/p").unwrap().unwrap();
    assert_eq!(old.revision, amended.revision);
    assert_ne!(old.state_token, amended.state_token);
    assert_eq!(old.state_token[3..67], amended.state_token[3..67]);

    /* revision増分、消失、別ページの同じpathへの作成を確認する。 */
    db.put_page(&id, "alice", "# new".into(), false).unwrap();
    assert_eq!(read_state(&db, &auth, "/p").unwrap().unwrap().revision, 2);
    db.rename_page("/p", "/moved").unwrap();
    assert!(read_state(&db, &auth, "/p").unwrap().is_none());
    db.create_page("/p", "alice", "# old".into()).unwrap();
    let new = read_state(&db, &auth, "/p").unwrap().unwrap();
    assert_ne!(old.state_token[3..67], new.state_token[3..67]);
    assert_eq!(new.revision, 1);
    let json = serde_json::to_value(&new).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 3);
}

///
/// 旧sourceのinstance_id欠落を読み取りだけで扱う
///
/// # 注記
/// 保存形式からinstance_idを除去したfixtureを再読し、amendで復帰する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_legacy_instance_fallback() {
    let (_dir, db, auth) = fixture(BearerScope::Read);
    let id = db.get_page_id_by_path("/p").unwrap().unwrap();
    /* 隔離DBに旧形式sourceを作り、readによる移行がないことを確認する。 */
    let txn = db.db.begin_write().unwrap();
    {
        let mut table = txn.open_table(PAGE_SOURCE_TABLE).unwrap();
        let source = table.get((id.clone(), 1)).unwrap().unwrap().value();
        let mut value = serde_json::to_value(source).unwrap();
        value.as_object_mut().unwrap().remove("instance_id");
        let source: PageSource = serde_json::from_value(value).unwrap();
        table.insert((id.clone(), 1), source).unwrap();
    }
    txn.commit().unwrap();
    let old = read_state(&db, &auth, "/p").unwrap().unwrap();
    assert!(old.instance_id.is_none());
    assert_eq!(old.state_token,
        read_state(&db, &auth, "/p").unwrap().unwrap().state_token);
    assert!(db.get_page_source(&id, 1).unwrap().unwrap()
        .instance_id().is_none());
    db.put_page(&id, "alice", "# old".into(), true).unwrap();
    let new = read_state(&db, &auth, "/p").unwrap().unwrap();
    assert!(new.instance_id.is_some());
    assert_ne!(old.state_token, new.state_token);
}

///
/// 同じread snapshotが権限とsourceの世代を維持する
///
/// # 注記
/// snapshot確定後にwrite commitし、古い読取と新しい読取を比較する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_snapshot_keeps_authorization_and_source_together() {
    let (_dir, db, auth) = fixture(BearerScope::Read);
    let snapshot = db.db.begin_read().unwrap();
    let old = db.mcp_state_in_txn(&snapshot, &auth, "/p").unwrap().unwrap();
    db.put_page(&old.page_id, "alice", "# changed".into(), false).unwrap();
    db.revoke_bearer_token_by_id(auth.token_id().unwrap()).unwrap();
    let still_old = db.mcp_state_in_txn(&snapshot, &auth, "/p")
        .unwrap().unwrap();
    assert_eq!(still_old.revision, old.revision);
    assert_eq!(still_old.source.source(), "# old");
    let error = db.read_mcp_page_state(&auth, "/p").err().unwrap();
    assert_eq!(error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Authentication));
}

///
/// 入口で保持した古いscope・prefixを信用しない
///
/// # 注記
/// DBのscope不足と、認証後prefix変更、ReadOnlyの参照を確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_rechecks_scopes_prefixes_and_read_only() {
    let (_dir, db, auth) = fixture(BearerScope::Update);
    let error = db.read_mcp_page_state(&auth, "/p").err().unwrap();
    assert_eq!(error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Scope));
    let (_dir2, db2, auth2) = fixture(BearerScope::Write);
    /* 属性をReadOnlyにしてもreadは利用可能。 */
    let txn = db2.db.begin_write().unwrap();
    {
        let mut users = txn.open_table(USER_INFO_TABLE).unwrap();
        let id = auth2.actor_id().unwrap().clone();
        let mut user = users.get(id.clone()).unwrap().unwrap().value();
        user.set_attributes(UserAttributeSet::from_iter([
            UserAttribute::ReadOnly,
        ]));
        users.insert(id, user).unwrap();
    }
    txn.commit().unwrap();
    assert!(read_state(&db2, &auth2, "/p").unwrap().is_some());
    let error = db2.read_mcp_page_state(&auth2, "/private").err().unwrap();
    assert_eq!(error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Prefix));
    db2.remove_path_prefix_from_bearer_token(auth2.token_id().unwrap(), "/p")
        .unwrap();
    /* 空prefixは制約なしとなるため、別のprefixを追加して現在権限を制限する。 */
    db2.add_path_prefix_to_bearer_token(auth2.token_id().unwrap(), "/other")
        .unwrap();
    let error = db2.read_mcp_page_state(&auth2, "/p").err().unwrap();
    assert_eq!(error.downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Prefix));
}

///
/// 再確認がTTLを延長せず、期限とユーザ削除を検知する
///
/// # 注記
/// 同じ主体の管理情報を更新して、古い認証文脈で再読する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_expiry_and_deleted_user_do_not_extend_ttl() {
    let (_dir, db, auth) = fixture(BearerScope::Read);
    let before = db.get_bearer_token_info_by_id(auth.token_id().unwrap())
        .unwrap().unwrap();
    read_state(&db, &auth, "/p").unwrap();
    assert_eq!(before.expire_at(), db.get_bearer_token_info_by_id(
        auth.token_id().unwrap()).unwrap().unwrap().expire_at());
    /* 期限切れを同じtokenの保存値へ反映する。 */
    let txn = db.db.begin_write().unwrap();
    {
        let hash = db.get_bearer_token_hash_by_id(auth.token_id().unwrap())
            .unwrap().unwrap();
        let mut table = txn.open_table(BEARER_TOKEN_TABLE).unwrap();
        let mut token = table.get(hash.clone()).unwrap().unwrap().value();
        let expired = Local::now() - chrono::Duration::seconds(1);
        token.overwrite_timestamps_for_test(expired, expired, expired);
        table.insert(hash, token).unwrap();
    }
    txn.commit().unwrap();
    assert_eq!(db.read_mcp_page_state(&auth, "/p").err().unwrap()
        .downcast_ref::<StateAccessDenied>(),
        Some(&StateAccessDenied::Authentication));
    let (_dir2, db2, auth2) = fixture(BearerScope::Read);
    db2.delete_user("alice").unwrap();
    assert!(read_state(&db2, &auth2, "/p").is_err());
}

///
/// 削除・draft・不整合とtokenの入力境界を確認する
///
/// # 注記
/// 取得不能と内部破損を区別し、digestの長さ・文字・版を検証する。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_unavailable_corrupt_source_and_token_validation() {
    let (_dir, db, auth) = fixture(BearerScope::Read);
    let id = db.get_page_id_by_path("/p").unwrap().unwrap();
    let good = read_state(&db, &auth, "/p").unwrap().unwrap().state_token;
    assert!(valid_state_token(&good));
    for invalid in ["".to_string(), good[1..].to_string(),
        good.replacen("v1:", "v2:", 1), good.replace(':', "/"),
        format!("{}g", &good[..131]), "あ".repeat(44)]
    {
        assert!(!valid_state_token(&invalid));
    }
    /* source欠落は取得不能として隠さず内部失敗にする。 */
    let txn = db.db.begin_write().unwrap();
    txn.open_table(PAGE_SOURCE_TABLE).unwrap().remove((id.clone(), 1)).unwrap();
    txn.commit().unwrap();
    assert!(read_state(&db, &auth, "/p").is_err());
    let (_dir2, db2, auth2) = fixture(BearerScope::Read);
    let id = db2.get_page_id_by_path("/p").unwrap().unwrap();
    db2.delete_page_by_id(&id).unwrap();
    assert!(read_state(&db2, &auth2, "/p").unwrap().is_none());
    db2.create_draft_page("/p", "alice").unwrap();
    assert!(read_state(&db2, &auth2, "/p").unwrap().is_none());
}

///
/// 正本commit後の派生同期失敗も最新状態として観測する
///
/// # 注記
/// 候補同期失敗を注入し、保存が戻らずtokenが更新されることを確認。
///
/// # 戻り値
/// なし。
///
#[test]
fn page_state_post_commit_failure_keeps_observable_change() {
    let (_dir, db, auth) = fixture(BearerScope::Read);
    let before = read_state(&db, &auth, "/p").unwrap().unwrap();
    let id = db.get_page_id_by_path("/p").unwrap().unwrap();
    db.set_resource_candidate_sync_failure_for_test(true);
    assert!(db.put_page(&id, "alice", "# committed".into(), false).is_err());
    let after = read_state(&db, &auth, "/p").unwrap().unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert_ne!(after.state_token, before.state_token);
    db.set_resource_candidate_sync_failure_for_test(false);
    db.delete_page_by_id_hard(&id).unwrap();
    assert!(read_state(&db, &auth, "/p").unwrap().is_none());
}
