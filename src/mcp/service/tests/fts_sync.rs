/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2026 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP保存と全文索引の同期・失敗境界を検証する。
//!

use super::*;

///
/// 索引設定を接続したサービスと隔離DBを生成する。
///
/// # 戻り値
/// 一時領域、DB、認証、索引設定、サービス。
///
fn fixture() -> (tempfile::TempDir, DatabaseManager, AuthContext,
    FtsIndexConfig, McpService)
{
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open(&dir.path().join("wiki.redb"),
        &dir.path().join("assets")).unwrap();
    db.add_user("writer", "pass", None).unwrap();
    let auth = AuthContext::new(AuthUser::new("writer".into()),
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/fts"]), None);
    let config = FtsIndexConfig::new(dir.path().join("index"));
    let service = McpService::new().with_fts_config(config.clone());
    (dir, db, auth, config, service)
}

///
/// 指定語の本文検索が返すrevisionを取得する。
///
/// # 引数
/// * `config` - 索引設定
/// * `query` - 検索式
/// * `history` - 全revision対象か
///
/// # 戻り値
/// 検索結果のrevision一覧。
///
fn revisions(config: &FtsIndexConfig, query: &str, history: bool) -> Vec<u64> {
    fts::search_index(config, FtsSearchTarget::Body, query, false, history)
        .unwrap().iter().map(|hit| hit.revision()).collect()
}

///
/// 保存5操作・履歴・amend・再帰renameの索引一致を確認する。
///
/// # 概要
/// 正本保存だけで検索結果が最新になり、履歴/amendの重複がないことを確認する。
///
/// # 注記
/// create→update→複数edit→同一/別ユーザappend→親子renameを実行する。
///
/// # 戻り値
/// なし。
///
#[test]
fn fts_sync_mutations_history_amend_and_recursive_rename() {
    /* create/updateで最新と履歴を分ける。 */
    let (_dir, db, auth, config, service) = fixture();
    let created = service.create_page(&auth, &db, "/fts/page",
        "# Entry\n\noldterm\n").unwrap();
    assert_eq!(revisions(&config, "oldterm", false), vec![1]);
    let updated = service.update_page(&auth, &db, "/fts/page",
        "# Entry\n\nupdateterm\n").unwrap();
    assert_eq!(updated.revision(), 2);
    assert!(revisions(&config, "oldterm", false).is_empty());
    assert_eq!(revisions(&config, "oldterm", true), vec![1]);

    /* 複数編集後は一revisionだけ増える。 */
    let request = EditPageRequest::new("/fts/page".into(),
        updated.revision(), updated.instance_id().into(), vec![
            EditPageOperation::ReplaceText { old_text: "updateterm".into(),
                new_text: "editterm".into(), occurrence: None },
            EditPageOperation::ReplaceText { old_text: "Entry".into(),
                new_text: "Edited".into(), occurrence: None },
        ]);
    let edited = service.edit_page(&auth, &db, &request).unwrap();
    assert_eq!(edited.revision(), 3);
    assert_eq!(revisions(&config, "editterm", false), vec![3]);
    assert!(revisions(&config, "updateterm", false).is_empty());

    /* amendは同じrevisionを置換し、別記述者の追記は新revisionになる。 */
    let appended = service.append_page(&auth, &db, "/fts/page",
        "\nappendterm\n").unwrap();
    assert!(appended.amended());
    assert_eq!(appended.revision(), 3);
    assert_eq!(revisions(&config, "editterm", true), vec![3]);
    db.add_user("other", "pass", None).unwrap();
    let other = AuthContext::new(AuthUser::new("other".into()),
        BearerScopeSet::from_iter([BearerScope::Write]),
        PathPrefixSet::from_iter(["/fts"]), None);
    let next = service.append_page(&other, &db, "/fts/page",
        "\notherterm\n").unwrap();
    assert!(!next.amended());
    assert_eq!(revisions(&config, "otherterm", false), vec![4]);

    /* 再帰移動で子ページも最新revisionへ同期する。 */
    service.create_page(&auth, &db, "/fts/page/child",
        "# Child\n\nchildterm\n").unwrap();
    service.rename_page(&auth, &db, created.path(), "/fts/moved").unwrap();
    let child = service.get_page(&auth, &db, "/fts/moved/child", None)
        .unwrap();
    assert_eq!(revisions(&config, "childterm", false),
        vec![child.revision()]);
    let found = service.search_pages(&auth, &db, &config, "childterm",
        &[FtsSearchTarget::Body], None, None).unwrap();
    assert_eq!(found.items()[0].path(), "/fts/moved/child");
}

///
/// 保存前の拒否で索引を作らず、正本も変更しないことを確認する。
///
/// # 注記
/// 空追記・prefix拒否・古い編集条件を未索引の既存ページへ送る。
///
/// # 戻り値
/// なし。
///
#[test]
fn fts_sync_denied_and_invalid_mutations_do_not_create_index() {
    /* 索引を持たない正本を準備する。 */
    let (_dir, db, auth, config, service) = fixture();
    db.create_page("/fts/page", "writer", "unchanged".into()).unwrap();
    assert!(!config.index_path().exists());

    /* 拒否/空入力/競合はFTSへ到達しない。 */
    assert_eq!(service.create_page(&auth, &db, "/outside", "denied")
        .unwrap_err().code(), McpErrorCode::Forbidden);
    assert_eq!(service.append_page(&auth, &db, "/fts/page", "")
        .unwrap_err().code(), McpErrorCode::InvalidInput);
    let request = EditPageRequest::new("/fts/page".into(), 99,
        "obsolete".into(), vec![EditPageOperation::ReplaceText {
            old_text: "unchanged".into(), new_text: "changed".into(),
            occurrence: None,
        }]);
    assert_eq!(service.edit_page(&auth, &db, &request)
        .unwrap_err().code(), McpErrorCode::NotLatestRevision);
    assert!(!config.index_path().exists());
    assert_eq!(service.get_page(&auth, &db, "/fts/page", None)
        .unwrap().content(), "unchanged");
}

///
/// 索引失敗の固定エラーと保存済み正本を確認する。
///
/// # 注記
/// 索引の親を通常ファイルで塞ぎ、保存後失敗でも本文/pathを漏らさない。
///
/// # 戻り値
/// なし。
///
#[test]
fn fts_sync_failure_preserves_committed_source_and_fixed_message() {
    let (dir, db, auth, _config, service) = fixture();
    let blocker = dir.path().join("blocked");
    fs::write(&blocker, b"blocker").unwrap();
    let service = service.with_fts_config(
        FtsIndexConfig::new(blocker.join("index")),
    );
    let error = service.create_page(&auth, &db, "/fts/saved",
        "PRIVATE_SOURCE_SENTINEL").unwrap_err();
    assert_eq!(error.code(), McpErrorCode::InternalError);
    assert_eq!(error.message(),
        "page was saved but full-text index update failed");
    let saved = service.get_page(&auth, &db, "/fts/saved", None).unwrap();
    assert_eq!(saved.content(), "PRIVATE_SOURCE_SENTINEL");
    assert_eq!(saved.revision(), 1);
    assert_eq!(fs::read(blocker).unwrap(), b"blocker");
}
