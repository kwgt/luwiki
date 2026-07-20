/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! database モジュールのテストをまとめたモジュール
//!

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use redb::Database;
use redb::ReadableDatabase;
use redb::ReadableTable;
use redb::ReadableTableMetadata;
use redb::TableHandle;
use redb::TypeName;
use redb::Value;
use serde::Serialize;

use super::DatabaseManager;
use super::init::init_database;
use super::link_refs::build_link_refs;
use super::schema::{
    BEARER_TOKEN_ID_TABLE,
    BEARER_TOKEN_TABLE,
    MCP_PRIMITIVE_NAME_TABLE,
    MCP_PRIMITIVE_NAME_STATE_TABLE,
    PAGE_INDEX_TABLE, PAGE_PATH_TABLE, ROOT_PAGE_PATH, SANDBOX_PAGE_PATH,
    SANDBOX_SAMPLE_CODE_FILE_NAME, SANDBOX_SAMPLE_CSV_FILE_NAME,
    PROMPT_CANDIDATE_TABLE,
    RESOURCE_CANDIDATE_TABLE,
    RESOURCE_URI_INDEX_STATE_TABLE,
    RESOURCE_URI_INDEX_TABLE,
    TEMPLATE_CANDIDATE_TABLE,
};
use super::types::{
    AssetId,
    BearerScope,
    BearerScopeSet,
    BearerTokenInfo,
    BearerTokenPlaintext,
    McpPrimitiveKind,
    McpPrimitiveNameKey,
    PageId,
    PageIndex,
    PageSource,
    PathPrefixSet,
    PromptArgumentEntry,
    PromptCandidateEntry,
    ResourceCandidateEntry,
    RenameInfo,
    TemplateCandidateEntry,
    TokenId,
    TokenHash,
    UserAttribute,
    UserAttributeSet,
    UserId,
    UserInfo,
};
use super::manager::bearer_tokens::VerifyBearerTokenFailureReason;
use super::manager::pages_write::AppendPageRequest;
use super::{
    ResourceListEntry,
    ResourceListSource,
};
use super::resource_list::{
    builtin_resource_list_entries,
    merge_resource_list_entries,
    page_resource_list_entry,
};
use crate::export_import::MigrateExportPageSnapshot;
use crate::export_import::model::{
    ExportAsset,
    ExportBundle,
    ExportPage,
    ExportRevision,
    ExportType,
    ExportUser,
    ManifestContext,
};

///
/// Wikiリンク抽出処理が期待通りに動作することを
/// 確認する。
///
/// # 注記
/// テスト用データベースを作成し、固定ソースから
/// リンク参照を抽出して検証する。
///
#[test]
fn build_link_refs_extracts_wiki_links() {
    /*
     * テスト用データベースを初期化する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    init_database(&mut db).expect("init db failed");

    /*
     * ページパスとインデックスを登録する
     */
    let txn = db.begin_write().expect("begin write failed");
    {
        let mut path_table = txn
            .open_table(PAGE_PATH_TABLE)
            .expect("open table failed");
        let mut index_table = txn
            .open_table(PAGE_INDEX_TABLE)
            .expect("open table failed");
        let id_root = PageId::new();
        let id_page = PageId::new();
        path_table
            .insert("/a".to_string(), id_root.clone())
            .expect("insert /a failed");
        path_table
            .insert("/a/b".to_string(), id_page.clone())
            .expect("insert /a/b failed");
        index_table
            .insert(
                id_root.clone(),
                PageIndex::new_page(id_root.clone(), "/a".to_string()),
            )
            .expect("insert /a index failed");
        index_table
            .insert(
                id_page.clone(),
                PageIndex::new_page(id_page.clone(), "/a/b".to_string()),
            )
            .expect("insert /a/b index failed");
    }

    /*
     * リンク参照を抽出して結果を検証する
     */
    let source = concat!(
        "[abs](/a/b) ",
        "[child](child) ",
        "[cur](.) ",
        "[parent](..) ",
        "![img](/img/only) ",
        "[ext](https://example.com) ",
        "[mail](mailto:info@example.com)",
    );

    let refs = build_link_refs(&txn, "/a/b", source)
        .expect("build_link_refs failed");

    assert!(matches!(refs.get("/a/b"), Some(Some(_))));
    assert!(matches!(refs.get("/a"), Some(Some(_))));
    assert!(matches!(refs.get("/a/b/child"), Some(None)));
    assert!(!refs.contains_key("/img/only"));
    assert!(!refs.contains_key("https://example.com"));
    assert!(!refs.contains_key("mailto:info@example.com"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// init_database がテンプレート候補テーブルを作成することを確認する。
///
#[test]
fn init_database_creates_template_candidate_table() {
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");

    init_database(&mut db).expect("init db failed");

    let txn = db.begin_read().expect("begin read failed");
    let _ = txn
        .open_table(TEMPLATE_CANDIDATE_TABLE)
        .expect("open template candidate table failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// TemplateCandidateEntry が MessagePack 経由で往復できることを確認する。
///
#[test]
fn template_candidate_entry_round_trips_via_redb_value() {
    let entry = TemplateCandidateEntry::new(
        "議事録".to_string(),
        Some("定例会議".to_string()),
        Some(true),
        crate::database::types::TemplateCandidateSource::FrontMatter,
    );
    let bytes = <TemplateCandidateEntry as Value>::as_bytes(&entry);
    let restored =
        <TemplateCandidateEntry as Value>::from_bytes(bytes.as_slice());

    assert_eq!(restored, entry);
    assert_eq!(restored.name(), "議事録");
    assert_eq!(restored.description(), Some("定例会議"));
    assert_eq!(restored.macro_expand(), Some(true));
    assert_eq!(
        restored.source(),
        &crate::database::types::TemplateCandidateSource::FrontMatter,
    );
}

///
/// init_database がresource候補テーブルを作成することを確認する。
///
#[test]
fn init_database_creates_resource_candidate_table() {
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");

    init_database(&mut db).expect("init db failed");

    let txn = db.begin_read().expect("begin read failed");
    let _ = txn
        .open_table(RESOURCE_CANDIDATE_TABLE)
        .expect("open resource candidate table failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// ResourceCandidateEntry が MessagePack 経由で
/// 往復できることを確認する。
///
/// # 注記
/// resource本文は候補派生データに保存しない。
///
#[test]
fn resource_candidate_entry_round_trips_via_redb_value() {
    let entry = ResourceCandidateEntry::new(
        "/docs/spec".to_string(),
        "spec".to_string(),
        "resource description".to_string(),
        Some("text/markdown".to_string()),
    );
    let bytes = <ResourceCandidateEntry as Value>::as_bytes(&entry);
    let restored =
        <ResourceCandidateEntry as Value>::from_bytes(bytes.as_slice());

    assert_eq!(restored, entry);
    assert_eq!(restored.resource_path(), "/docs/spec");
    assert_eq!(restored.name(), "spec");
    assert_eq!(restored.description(), "resource description");
    assert_eq!(restored.mime_type(), Some("text/markdown"));
    assert_eq!(
        <ResourceCandidateEntry as Value>::type_name(),
        TypeName::new("ResourceCandidateEntry"),
    );
    assert_eq!(<ResourceCandidateEntry as Value>::fixed_width(), None);
}

///
/// resource候補一覧が最新ページ状態と合流することを確認する。
///
/// # 注記
/// rename、soft delete、undelete、既定MIME typeを検証する。
///
#[test]
fn list_resource_candidates_merges_latest_page_state() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/original-list",
            "tester",
            resource_source(Some("/docs/list"), "list"),
        )
        .expect("create resource page failed");
    manager
        .insert_resource_candidate_for_test(
            &page_id,
            &ResourceCandidateEntry::new(
                "/docs/list".to_string(),
                "list".to_string(),
                "description".to_string(),
                None,
            ),
        )
        .expect("insert resource candidate failed");

    manager
        .rename_page("/resources/original-list", "/resources/renamed-list")
        .expect("rename resource page failed");
    let entries = manager
        .list_resource_candidates()
        .expect("list renamed resource candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), page_id);
    assert_eq!(entries[0].current_path(), "/resources/renamed-list");
    assert_eq!(entries[0].resource_path(), "/docs/list");
    assert_eq!(entries[0].name(), "list");
    assert_eq!(entries[0].description(), "description");
    assert_eq!(entries[0].mime_type(), "text/markdown");

    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete resource page failed");
    assert!(
        manager
            .list_resource_candidates()
            .expect("list deleted resource candidates failed")
            .is_empty()
    );

    manager
        .undelete_page_by_id(&page_id, "/resources/restored-list", false)
        .expect("undelete resource page failed");
    let entries = manager
        .list_resource_candidates()
        .expect("list restored resource candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), page_id);
    assert_eq!(entries[0].current_path(), "/resources/restored-list");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource候補同期が最新ソースをinsertおよびupdateすることを確認する。
///
/// # 注記
/// resourceページを作成して同期した後、最新ソースを更新して再同期する。
///
#[test]
fn sync_resource_candidate_for_page_inserts_and_updates_latest_source() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/sync",
            "tester",
            resource_source(Some("/docs/sync-first"), "sync-first"),
        )
        .expect("create resource page failed");

    let inserted = manager
        .sync_resource_candidate_for_page(&page_id)
        .expect("sync resource candidate failed")
        .expect("resource candidate missing");
    assert_eq!(inserted.resource_path(), "/docs/sync-first");
    assert_eq!(inserted.name(), "sync-first");

    manager
        .put_page(
            &page_id,
            "tester",
            resource_source(Some("/docs/sync-second"), "sync-second"),
            false,
        )
        .expect("put resource page failed");
    let updated = manager
        .sync_resource_candidate_for_page(&page_id)
        .expect("resync resource candidate failed")
        .expect("updated resource candidate missing");

    assert_eq!(updated.resource_path(), "/docs/sync-second");
    assert_eq!(updated.name(), "sync-second");
    assert_eq!(updated.description(), "resource description");
    assert_eq!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get resource candidate failed"),
        Some(updated),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource指定解除後の同期が既存候補を除去することを確認する。
///
/// # 注記
/// resource候補を同期した後、通常ページへ更新して再同期する。
///
#[test]
fn sync_resource_candidate_for_page_removes_disabled_resource() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/removable-candidate",
            "tester",
            resource_source(Some("/docs/removable-candidate"), "removable"),
        )
        .expect("create resource page failed");
    manager
        .sync_resource_candidate_for_page(&page_id)
        .expect("sync resource candidate failed");

    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("put normal page failed");
    let synced = manager
        .sync_resource_candidate_for_page(&page_id)
        .expect("resync resource candidate failed");

    assert!(synced.is_none());
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get resource candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 通常ページ同期時にresource候補テーブルへ登録しないことを確認する。
///
#[test]
fn sync_resource_candidate_for_page_keeps_normal_page_absent() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/normal-candidate",
            "tester",
            "# 通常ページ\n本文".to_string(),
        )
        .expect("create normal page failed");

    let synced = manager
        .sync_resource_candidate_for_page(&page_id)
        .expect("sync normal page failed");
    assert!(synced.is_none());
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get resource candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// create / put 保存後にresource候補同期が自動反映されることを確認する。
///
#[test]
fn create_and_put_auto_sync_resource_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/auto-sync",
            "tester",
            resource_source(Some("/docs/auto-sync"), "auto-sync"),
        )
        .expect("create resource page failed");

    let created = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get created resource candidate failed")
        .expect("created resource candidate missing");
    assert_eq!(created.resource_path(), "/docs/auto-sync");
    assert_eq!(created.name(), "auto-sync");

    manager
        .put_page(
            &page_id,
            "tester",
            resource_source(Some("/docs/auto-updated"), "auto-updated"),
            false,
        )
        .expect("put resource page failed");
    let updated = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get updated resource candidate failed")
        .expect("updated resource candidate missing");
    assert_eq!(updated.resource_path(), "/docs/auto-updated");
    assert_eq!(updated.name(), "auto-updated");

    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("put normal page failed");
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get removed resource candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// append 保存後にresource候補同期が自動反映されることを確認する。
///
#[test]
fn append_auto_syncs_resource_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/append-auto",
            "tester",
            "# 通常ページ\n本文".to_string(),
        )
        .expect("create normal page failed");
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get initial resource candidate failed")
            .is_none()
    );
    let request = AppendPageRequest::new(
        page_id.clone(),
        "tester".to_string(),
        resource_source(Some("/docs/append-auto"), "append-auto"),
        1,
        false,
    );

    let result = manager
        .append_page_by_id(&request)
        .expect("append resource page failed");

    assert_eq!(result.revision(), 2);
    assert!(!result.amended());
    let candidate = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get appended resource candidate failed")
        .expect("appended resource candidate missing");
    assert_eq!(candidate.resource_path(), "/docs/append-auto");
    assert_eq!(candidate.name(), "append-auto");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// amend保存後にresource候補同期が自動反映されることを確認する。
///
#[test]
fn append_amend_auto_syncs_resource_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/amend-auto",
            "tester",
            "# 通常ページ\n本文".to_string(),
        )
        .expect("create normal page failed");
    let request = AppendPageRequest::new(
        page_id.clone(),
        "tester".to_string(),
        resource_source(Some("/docs/amend-auto"), "amend-auto"),
        1,
        true,
    );

    let result = manager
        .append_page_by_id(&request)
        .expect("amend resource page failed");

    assert_eq!(result.revision(), 1);
    assert!(result.amended());
    assert!(
        !manager
            .has_page_source_for_test(&page_id, 2)
            .expect("page source lookup failed")
    );
    let candidate = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get amended resource candidate failed")
        .expect("amended resource candidate missing");
    assert_eq!(candidate.resource_path(), "/docs/amend-auto");
    assert_eq!(candidate.name(), "amend-auto");
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/amend-auto"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// rollback 保存後にresource候補同期が自動反映されることを確認する。
///
#[test]
fn rollback_auto_syncs_resource_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/rollback-auto",
            "tester",
            resource_source(
                Some("/docs/rollback-original"),
                "rollback-original",
            ),
        )
        .expect("create resource page failed");
    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("put normal page failed");
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get removed resource candidate failed")
            .is_none()
    );

    manager
        .rollback_page_source_only(&page_id, 1)
        .expect("rollback resource page failed");

    let candidate = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get rollback resource candidate failed")
        .expect("rollback resource candidate missing");
    assert_eq!(candidate.resource_path(), "/docs/rollback-original");
    assert_eq!(candidate.name(), "rollback-original");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// rename・soft delete・undeleteでresource公開情報が
/// 最新ページ状態へ追従することを確認する。
///
#[test]
fn resource_candidate_list_follows_rename_delete_and_undelete_state() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/state-original",
            "tester",
            resource_source(Some("/docs/state"), "state"),
        )
        .expect("create resource page failed");

    manager
        .rename_page("/resources/state-original", "/resources/state-renamed")
        .expect("rename resource page failed");
    let entries = manager
        .list_resource_candidates()
        .expect("list renamed resource candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), page_id);
    assert_eq!(entries[0].current_path(), "/resources/state-renamed");
    assert_eq!(entries[0].resource_path(), "/docs/state");

    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete resource page failed");
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get soft deleted resource candidate failed")
            .is_some()
    );
    assert!(
        manager
            .list_resource_candidates()
            .expect("list soft deleted resource candidates failed")
            .is_empty()
    );

    manager
        .undelete_page_by_id(&page_id, "/resources/state-restored", false)
        .expect("undelete resource page failed");
    let entries = manager
        .list_resource_candidates()
        .expect("list restored resource candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), page_id);
    assert_eq!(entries[0].current_path(), "/resources/state-restored");
    assert_eq!(entries[0].resource_path(), "/docs/state");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// hard delete後にresource URI索引とresource候補が削除されることを確認する。
///
#[test]
fn hard_delete_removes_resource_uri_and_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/hard-delete-candidate",
            "tester",
            resource_source(Some("/docs/hard-delete-candidate"), "hard"),
        )
        .expect("create resource page failed");
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get resource candidate before hard delete failed")
            .is_some()
    );

    manager
        .delete_page_by_id_hard(&page_id)
        .expect("hard delete resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(
            &manager,
            "/docs/hard-delete-candidate",
        ),
        None,
    );
    assert!(
        manager
            .get_resource_candidate_by_page_id(&page_id)
            .expect("get resource candidate after hard delete failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// recursive hard delete後に配下resourceのURI索引と候補が削除されることを確認する。
///
#[test]
fn recursive_hard_delete_removes_resource_uris_and_candidates() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let parent_id = manager
        .create_page(
            "/resources/hard-tree",
            "tester",
            resource_source(Some("/docs/hard-tree"), "tree"),
        )
        .expect("create parent resource page failed");
    let child_id = manager
        .create_page(
            "/resources/hard-tree/child",
            "tester",
            resource_source(Some("/docs/hard-tree-child"), "child"),
        )
        .expect("create child resource page failed");

    manager
        .delete_pages_recursive_by_id(&parent_id, true)
        .expect("recursive hard delete failed");

    assert_eq!(resource_uri_owner_for_test(&manager, "/docs/hard-tree"), None);
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/hard-tree-child"),
        None,
    );
    assert!(
        manager
            .get_resource_candidate_by_page_id(&parent_id)
            .expect("get parent resource candidate failed")
            .is_none()
    );
    assert!(
        manager
            .get_resource_candidate_by_page_id(&child_id)
            .expect("get child resource candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource候補同期失敗後もページ正本とURI索引が
/// 保存済み状態を維持することを確認する。
///
/// # 注記
/// テスト専用の失敗注入により、正本commit後の
/// resource候補同期だけを失敗させる。
///
#[test]
fn resource_candidate_sync_failure_preserves_saved_page_state() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/sync-failure",
            "tester",
            resource_source(Some("/docs/sync-before"), "before"),
        )
        .expect("create resource page failed");
    let before_candidate = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get resource candidate before failure failed")
        .expect("resource candidate before failure missing");
    assert_eq!(before_candidate.resource_path(), "/docs/sync-before");

    manager.set_resource_candidate_sync_failure_for_test(true);
    let error = manager
        .put_page(
            &page_id,
            "tester",
            resource_source(Some("/docs/sync-after"), "after"),
            false,
        )
        .expect_err("resource candidate sync must fail");
    manager.set_resource_candidate_sync_failure_for_test(false);
    assert!(
        error
            .to_string()
            .contains("resource candidate sync failure for test")
    );

    /*
     * 正本保存とURI索引更新はcommit済みであることを確認する
     */
    let page_source = manager
        .get_page_source(&page_id, 2)
        .expect("get committed resource page source failed")
        .expect("committed resource page source missing");
    assert!(page_source.source().contains("resource_path: /docs/sync-after"));
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/sync-before"),
        None,
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/sync-after"),
        Some(page_id.clone()),
    );

    /*
     * 候補テーブルだけが古い状態に残ることを確認する
     */
    let stale_candidate = manager
        .get_resource_candidate_by_page_id(&page_id)
        .expect("get stale resource candidate failed")
        .expect("stale resource candidate missing");
    assert_eq!(stale_candidate.resource_path(), "/docs/sync-before");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource派生データを最新ページソースから
/// 再構成できることを確認する。
///
/// # 注記
/// 古い候補とURI索引を投入後に再構成し、
/// 最新ソース由来の内容へ置換されることを検証する。
///
#[test]
fn rebuild_resource_candidates_recreates_entries_from_latest_sources() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    /*
     * 状態が異なるresourceページと古い派生データを準備する
     */
    let visible_id = manager
        .create_page(
            "/resources/visible",
            "tester",
            resource_source(Some("/docs/visible"), "visible"),
        )
        .expect("create visible resource failed");
    let deleted_id = manager
        .create_page(
            "/resources/deleted",
            "tester",
            resource_source(Some("/docs/deleted"), "deleted"),
        )
        .expect("create deleted resource failed");
    manager
        .delete_page_by_id(&deleted_id)
        .expect("soft delete resource failed");
    let draft_id = manager
        .create_draft_page("/resources/draft", "tester")
        .expect("create draft failed")
        .0;
    let normal_id = manager
        .create_page("/normal", "tester", "通常本文".to_string())
        .expect("create normal page failed");

    manager
        .remove_resource_candidate_by_page_id(&visible_id)
        .expect("remove visible candidate failed");
    manager
        .remove_resource_candidate_by_page_id(&deleted_id)
        .expect("remove deleted candidate failed");
    manager
        .insert_resource_candidate_for_test(
            &normal_id,
            &ResourceCandidateEntry::new(
                "/docs/stale".to_string(),
                "stale".to_string(),
                "stale description".to_string(),
                None,
            ),
        )
        .expect("insert stale candidate failed");
    manager
        .set_resource_uri_owner_for_test("/docs/visible", None)
        .expect("remove visible resource URI failed");
    manager
        .set_resource_uri_owner_for_test("/docs/deleted", None)
        .expect("remove deleted resource URI failed");
    manager
        .set_resource_uri_owner_for_test("/docs/stale", Some(&normal_id))
        .expect("insert stale resource URI failed");
    assert!(!manager
        .is_resource_uri_index_ready()
        .expect("get readiness before rebuild failed"));

    /*
     * 再構成結果と古い派生データの除去を確認する
     */
    let count = manager
        .rebuild_resource_candidates()
        .expect("rebuild resource candidates failed");
    assert_eq!(count, 2);

    let visible_candidate = manager
        .get_resource_candidate_by_page_id(&visible_id)
        .expect("get visible candidate failed")
        .expect("visible candidate missing");
    assert_eq!(visible_candidate.resource_path(), "/docs/visible");
    assert_eq!(visible_candidate.name(), "visible");
    let deleted_candidate = manager
        .get_resource_candidate_by_page_id(&deleted_id)
        .expect("get deleted candidate failed")
        .expect("deleted candidate missing");
    assert_eq!(deleted_candidate.resource_path(), "/docs/deleted");

    for (page_id, label) in [
        (&draft_id, "draft"),
        (&normal_id, "normal"),
    ] {
        assert!(
            manager
                .get_resource_candidate_by_page_id(page_id)
                .unwrap_or_else(|_| panic!("get {} candidate failed", label))
                .is_none(),
            "{} candidate was rebuilt",
            label,
        );
    }
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/visible"),
        Some(visible_id.clone()),
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/deleted"),
        Some(deleted_id),
    );
    assert_eq!(resource_uri_owner_for_test(&manager, "/docs/stale"), None);
    assert!(manager
        .is_resource_uri_index_ready()
        .expect("get readiness after rebuild failed"));

    /*
     * soft delete済みresourceが公開一覧から
     * 除外されることを確認する
     */
    let entries = manager
        .list_resource_candidates()
        .expect("list resource candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), visible_id);
    assert_eq!(entries[0].resource_path(), "/docs/visible");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource_path重複時にresource派生データと
/// ページ正本を維持することを確認する。
///
/// # 注記
/// latest sourceへ重複resource_pathを直接投入し、
/// 再構成失敗前後の状態を比較する。
///
#[test]
fn rebuild_resource_candidates_preserves_data_on_duplicate_resource_path() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let first_id = manager
        .create_page(
            "/resources/first",
            "tester",
            resource_source(Some("/docs/first"), "first"),
        )
        .expect("create first resource failed");
    let second_id = manager
        .create_page(
            "/resources/second",
            "tester",
            resource_source(Some("/docs/second"), "second"),
        )
        .expect("create second resource failed");
    manager
        .rebuild_resource_candidates()
        .expect("initial rebuild resource candidates failed");
    let duplicate_source = resource_source(Some("/docs/first"), "duplicate");
    manager
        .replace_latest_page_source_for_resource_rebuild_test(
            &second_id,
            duplicate_source.clone(),
        )
        .expect("inject duplicate source failed");
    let first_candidate = manager
        .get_resource_candidate_by_page_id(&first_id)
        .expect("get first candidate before rebuild failed");
    let second_candidate = manager
        .get_resource_candidate_by_page_id(&second_id)
        .expect("get second candidate before rebuild failed");

    /*
     * 重複エラーが呼び出し元へ返ることを確認する
     */
    let error = manager
        .rebuild_resource_candidates()
        .expect_err("rebuild must reject duplicate resource_path");
    assert!(
        error
            .to_string()
            .contains("resource URI already exists: resource_path=/docs/first")
    );

    /*
     * 候補、URI索引、readinessが更新されていないことを確認する
     */
    assert_eq!(
        manager
            .get_resource_candidate_by_page_id(&first_id)
            .expect("get first candidate after rebuild failed"),
        first_candidate,
    );
    assert_eq!(
        manager
            .get_resource_candidate_by_page_id(&second_id)
            .expect("get second candidate after rebuild failed"),
        second_candidate,
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/first"),
        Some(first_id),
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/second"),
        Some(second_id),
    );
    assert!(manager
        .is_resource_uri_index_ready()
        .expect("get readiness after failed rebuild failed"));

    /*
     * 重複状態のページ正本が
     * 変更されていないことを確認する
     */
    let state = manager
        .get_current_page_state_by_path("/resources/second")
        .expect("get second page state failed")
        .expect("second page state missing");
    assert_eq!(
        state
            .latest_source()
            .expect("second latest source missing")
            .source(),
        duplicate_source,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// all派生データ再構成がresource候補も件数に含めて復元することを
/// 確認する。
///
#[test]
fn rebuild_all_derived_data_rebuilds_resource_candidates() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    let template_id = manager
        .create_page(
            "/templates/all-api-template",
            "tester",
            concat!(
                "---\n",
                "wiki:\n",
                "  template:\n",
                "    name: all-api-template\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create template page failed");
    let prompt_id = manager
        .create_page(
            "/prompts/all-api",
            "tester",
            prompt_source("all-api-prompt", "all api prompt"),
        )
        .expect("create prompt page failed");
    let resource_page_id = manager
        .create_page(
            "/resources/all-api",
            "tester",
            resource_source(Some("/docs/all-api-resource"), "all-api-resource"),
        )
        .expect("create resource page failed");

    manager
        .remove_template_candidate_by_page_id(&template_id)
        .expect("remove template candidate failed");
    manager
        .remove_prompt_candidate_by_page_id(&prompt_id)
        .expect("remove prompt candidate failed");
    manager
        .set_mcp_primitive_name_owner_for_test(
            McpPrimitiveKind::Prompt,
            "all-api-prompt",
            None,
        )
        .expect("remove prompt name owner failed");
    manager
        .remove_resource_candidate_by_page_id(&resource_page_id)
        .expect("remove resource candidate failed");
    manager
        .set_resource_uri_owner_for_test("/docs/all-api-resource", None)
        .expect("remove resource URI owner failed");

    let counts = manager
        .rebuild_all_derived_data(None)
        .expect("rebuild all derived data failed");

    assert_eq!(counts.templates(), 1);
    assert_eq!(counts.prompts(), 1);
    assert_eq!(counts.resources(), 1);
    assert_eq!(
        manager
            .get_template_candidate_by_page_id(&template_id)
            .expect("get template candidate failed")
            .expect("template candidate missing")
            .name(),
        "all-api-template",
    );
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&prompt_id)
            .expect("get prompt candidate failed")
            .expect("prompt candidate missing")
            .name(),
        "all-api-prompt",
    );
    let resource = manager
        .get_resource_candidate_by_page_id(&resource_page_id)
        .expect("get resource candidate failed")
        .expect("resource candidate missing");
    assert_eq!(resource.resource_path(), "/docs/all-api-resource");
    assert_eq!(resource.name(), "all-api-resource");
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "all-api-prompt",
            )
            .expect("get prompt name owner failed"),
        Some(prompt_id),
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/all-api-resource"),
        Some(resource_page_id),
    );
    assert!(manager
        .is_mcp_primitive_name_index_ready()
        .expect("get prompt index readiness failed"));
    assert!(manager
        .is_resource_uri_index_ready()
        .expect("get resource URI readiness failed"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 固定組み込みresourceとページ由来resourceを
/// 同じ内部一覧へ合流できることを確認する。
///
#[test]
fn merge_resource_list_entries_combines_builtin_and_page_resources() {
    let page_id = PageId::new();
    let page_candidate = super::ResourceCandidateListEntry::new(
        page_id.clone(),
        "/resources/spec".to_string(),
        "/docs/spec".to_string(),
        "Spec".to_string(),
        "Page resource".to_string(),
        "application/json".to_string(),
    );
    let page_entry =
        page_resource_list_entry("local.test", &page_candidate);

    let entries = merge_resource_list_entries(
        builtin_resource_list_entries("local.test"),
        vec![page_entry],
    )
    .expect("merge resource list failed");
    let uris: Vec<&str> = entries.iter().map(|entry| entry.uri()).collect();

    assert_eq!(
        uris,
        vec![
            "luwiki://local.test/builtin/front-matter-spec",
            "luwiki://local.test/builtin/mcp-prompt-spec",
            "luwiki://local.test/docs/spec",
        ],
    );
    assert_eq!(entries[0].source(), ResourceListSource::Builtin);
    assert_eq!(entries[0].page_id(), None);
    assert_eq!(entries[2].source(), ResourceListSource::Page);
    assert_eq!(entries[2].page_id(), Some(page_id));
    assert_eq!(entries[2].current_path(), Some("/resources/spec"));
    assert_eq!(entries[2].name(), "Spec");
    assert_eq!(entries[2].description(), "Page resource");
    assert_eq!(entries[2].mime_type(), "application/json");
}

///
/// resource一覧合流時にURI重複を拒否することを確認する。
///
#[test]
fn merge_resource_list_entries_rejects_duplicate_uri() {
    let duplicate = ResourceListEntry::new(
        "luwiki://local.test/builtin/front-matter-spec".to_string(),
        "Duplicate".to_string(),
        "Duplicate resource".to_string(),
        "text/markdown".to_string(),
        ResourceListSource::Page,
        Some(PageId::new()),
        Some("/resources/duplicate".to_string()),
    );

    let error = merge_resource_list_entries(
        builtin_resource_list_entries("local.test"),
        vec![duplicate],
    )
    .expect_err("duplicate resource URI must fail");

    assert!(
        error
            .to_string()
            .contains("duplicate resource URI in list")
    );
}

///
/// MCP primitive名前索引キーがredb Value経由で
/// 往復できることを確認する。
///
/// # 注記
/// type name、可変長、case-sensitiveなbyte比較も検証する。
///
#[test]
fn mcp_primitive_name_key_round_trips_via_redb_value() {
    /*
     * 大文字小文字が異なるキーを直列化する
     */
    let upper = McpPrimitiveNameKey::new(
        McpPrimitiveKind::Prompt,
        "Summary".to_string(),
    );
    let lower = McpPrimitiveNameKey::new(
        McpPrimitiveKind::Prompt,
        "summary".to_string(),
    );
    let upper_bytes = <McpPrimitiveNameKey as Value>::as_bytes(&upper);
    let restored = <McpPrimitiveNameKey as Value>::from_bytes(
        upper_bytes.as_slice(),
    );
    let lower_bytes = <McpPrimitiveNameKey as Value>::as_bytes(&lower);

    /*
     * 往復結果とredb型情報を確認する
     */
    assert_eq!(restored, upper);
    assert_eq!(restored.primitive(), McpPrimitiveKind::Prompt);
    assert_eq!(restored.name(), "Summary");
    assert_eq!(
        <McpPrimitiveNameKey as Value>::type_name(),
        TypeName::new("McpPrimitiveNameKey"),
    );
    assert_eq!(<McpPrimitiveNameKey as Value>::fixed_width(), None);

    /*
     * 大文字小文字を区別するbyte比較を確認する
     */
    assert_ne!(upper_bytes, lower_bytes);
    assert_ne!(
        <McpPrimitiveNameKey as redb::Key>::compare(
            upper_bytes.as_slice(),
            lower_bytes.as_slice(),
        ),
        std::cmp::Ordering::Equal,
    );
}

///
/// DB初期化がMCP primitive名前索引テーブルを
/// 作成することを確認する。
///
/// # 注記
/// 大文字小文字が異なる名前を別キーとして
/// insert・getする。
///
#[test]
fn init_database_creates_mcp_primitive_name_table() {
    /*
     * DBと大文字小文字が異なるキーを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    init_database(&mut db).expect("init db failed");
    let upper_key = McpPrimitiveNameKey::new(
        McpPrimitiveKind::Prompt,
        "Summary".to_string(),
    );
    let lower_key = McpPrimitiveNameKey::new(
        McpPrimitiveKind::Prompt,
        "summary".to_string(),
    );
    let upper_id = PageId::new();
    let lower_id = PageId::new();

    /*
     * 名前索引テーブルへ2件を登録する
     */
    let write_txn = db.begin_write().expect("begin write failed");
    {
        let mut table = write_txn
            .open_table(MCP_PRIMITIVE_NAME_TABLE)
            .expect("open primitive name table failed");
        table
            .insert(upper_key.clone(), upper_id.clone())
            .expect("insert upper key failed");
        table
            .insert(lower_key.clone(), lower_id.clone())
            .expect("insert lower key failed");
    }
    write_txn.commit().expect("commit failed");

    /*
     * 各キーから対応するページIDを逆引きする
     */
    let read_txn = db.begin_read().expect("begin read failed");
    let table = read_txn
        .open_table(MCP_PRIMITIVE_NAME_TABLE)
        .expect("open primitive name table failed");
    assert_eq!(
        table
            .get(upper_key)
            .expect("get upper key failed")
            .expect("upper key missing")
            .value(),
        upper_id,
    );
    assert_eq!(
        table
            .get(lower_key)
            .expect("get lower key failed")
            .expect("lower key missing")
            .value(),
        lower_id,
    );
    assert_eq!(table.len().expect("get table length failed"), 2);
    drop(table);
    drop(read_txn);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// DB初期化でresource URI逆引き索引テーブルを作成し、
/// resource_pathをcase-sensitiveに扱えることを確認する。
///
#[test]
fn init_database_creates_resource_uri_index_table() {
    /*
     * DBと大文字小文字が異なるresource_pathを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    init_database(&mut db).expect("init db failed");
    let upper_resource_path = "Resource".to_string();
    let lower_resource_path = "resource".to_string();
    let upper_id = PageId::new();
    let lower_id = PageId::new();

    /*
     * resource URI逆引き索引テーブルへ2件を登録する
     */
    let write_txn = db.begin_write().expect("begin write failed");
    {
        let mut table = write_txn
            .open_table(RESOURCE_URI_INDEX_TABLE)
            .expect("open resource URI index table failed");
        table
            .insert(upper_resource_path.clone(), upper_id.clone())
            .expect("insert upper resource_path failed");
        table
            .insert(lower_resource_path.clone(), lower_id.clone())
            .expect("insert lower resource_path failed");
    }
    write_txn.commit().expect("commit failed");

    /*
     * 各resource_pathから対応するページIDを逆引きする
     */
    let read_txn = db.begin_read().expect("begin read failed");
    let table = read_txn
        .open_table(RESOURCE_URI_INDEX_TABLE)
        .expect("open resource URI index table failed");
    assert_eq!(
        table
            .get(upper_resource_path)
            .expect("get upper resource_path failed")
            .expect("upper resource_path missing")
            .value(),
        upper_id,
    );
    assert_eq!(
        table
            .get(lower_resource_path)
            .expect("get lower resource_path failed")
            .expect("lower resource_path missing")
            .value(),
        lower_id,
    );
    assert_eq!(table.len().expect("get table length failed"), 2);
    drop(table);
    drop(read_txn);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// DB初期化でresource URI逆引き索引構築状態テーブルを
/// 作成するが、3.2時点ではready markerを立てないことを
/// 確認する。
///
#[test]
fn init_database_creates_resource_uri_index_state_without_marker() {
    /*
     * DBを初期化する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    init_database(&mut db).expect("init db failed");

    /*
     * 状態テーブルが存在し、ready markerが未設定であることを確認する
     */
    let read_txn = db.begin_read().expect("begin read failed");
    let table = read_txn
        .open_table(RESOURCE_URI_INDEX_STATE_TABLE)
        .expect("open resource URI index state table failed");
    assert!(table.get(0).expect("get state failed").is_none());
    drop(table);
    drop(read_txn);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resourceページ作成時に明示resource_pathを
/// URI逆引き索引へ登録することを確認する。
///
#[test]
fn create_resource_page_registers_explicit_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page(
            "/resources/spec",
            "tester",
            resource_source(Some("/docs/spec"), "spec"),
        )
        .expect("create resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/spec"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource_path省略時にcurrent path由来resource_pathを
/// URI逆引き索引へ登録することを確認する。
///
#[test]
fn create_resource_page_registers_path_derived_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page(
            "/resources/path-derived",
            "tester",
            resource_source(None, "path-derived"),
        )
        .expect("create resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/path-derived"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// resource指定解除時に、そのページが所有していた
/// URI逆引き索引を除去することを確認する。
///
#[test]
fn put_page_removes_resource_uri_when_resource_is_disabled() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/removable",
            "tester",
            resource_source(Some("/docs/removable"), "removable"),
        )
        .expect("create resource page failed");

    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("put normal page failed");

    assert_eq!(resource_uri_owner_for_test(&manager, "/docs/removable"), None);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 別ページが使用中のresource_pathを指定した作成を
/// 拒否することを確認する。
///
#[test]
fn create_resource_page_rejects_duplicate_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let first_id = manager
        .create_page(
            "/resources/first",
            "tester",
            resource_source(Some("/docs/duplicate"), "first"),
        )
        .expect("create first resource page failed");

    let err = manager
        .create_page(
            "/resources/second",
            "tester",
            resource_source(Some("/docs/duplicate"), "second"),
        )
        .expect_err("duplicate resource_path must be rejected");

    assert!(
        err.to_string()
            .contains("resource URI already exists: resource_path=/docs/duplicate")
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/duplicate"),
        Some(first_id),
    );
    assert_eq!(resource_uri_owner_for_test(&manager, "/docs/second"), None);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 同一ページが同じresource_pathを維持する更新を
/// 許可することを確認する。
///
#[test]
fn put_page_allows_same_page_to_keep_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/keep",
            "tester",
            resource_source(Some("/docs/keep"), "first"),
        )
        .expect("create resource page failed");

    manager
        .put_page(
            &page_id,
            "tester",
            resource_source(Some("/docs/keep"), "second"),
            false,
        )
        .expect("put same resource_path failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/keep"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// path由来resource_pathがcase-sensitiveに扱われることを
/// 確認する。
///
#[test]
fn path_derived_resource_uri_is_case_sensitive() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    let upper_id = manager
        .create_page(
            "/Resources/Spec",
            "tester",
            resource_source(None, "upper"),
        )
        .expect("create upper resource page failed");
    let lower_id = manager
        .create_page(
            "/resources/spec",
            "tester",
            resource_source(None, "lower"),
        )
        .expect("create lower resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/Resources/Spec"),
        Some(upper_id),
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/spec"),
        Some(lower_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// root path由来resource_pathの不正を拒否することを確認する。
///
#[test]
fn create_resource_page_rejects_invalid_path_derived_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    let err = manager
        .create_page(
            "/",
            "tester",
            resource_source(None, "root"),
        )
        .expect_err("invalid path-derived resource_path must be rejected");

    assert!(
        err.to_string()
            .contains("mcp.resource_path must not end with /")
    );
    assert_eq!(resource_uri_owner_for_test(&manager, "/pages/"), None);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// path由来resource_pathを持つページのrenameで
/// URI逆引き索引が旧pathから新pathへ更新されることを確認する。
///
#[test]
fn rename_path_derived_resource_page_updates_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/original",
            "tester",
            resource_source(None, "path resource"),
        )
        .expect("create resource page failed");

    manager
        .rename_page("/resources/original", "/resources/renamed")
        .expect("rename resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/original"),
        None,
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/renamed"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 明示resource_pathを持つページのrenameでは
/// URI逆引き索引のkeyが維持されることを確認する。
///
#[test]
fn rename_explicit_resource_page_keeps_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/original-explicit",
            "tester",
            resource_source(Some("/docs/stable"), "explicit resource"),
        )
        .expect("create resource page failed");

    manager
        .rename_page(
            "/resources/original-explicit",
            "/resources/renamed-explicit",
        )
        .expect("rename resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/stable"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// path由来resource_pathのrename先が別ページの予約と
/// 衝突する場合にrenameを拒否することを確認する。
///
#[test]
fn rename_path_derived_resource_page_rejects_resource_uri_conflict() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/source",
            "tester",
            resource_source(None, "source"),
        )
        .expect("create source resource page failed");
    let owner_id = manager
        .create_page(
            "/resources/conflict",
            "tester",
            resource_source(None, "owner"),
        )
        .expect("create owner resource page failed");
    manager
        .delete_page_by_id(&owner_id)
        .expect("soft delete owner failed");

    let error = manager
        .rename_page("/resources/source", "/resources/conflict")
        .expect_err("conflicting rename must fail");

    assert!(
        error
            .to_string()
            .contains("resource URI already exists: resource_path=/pages/resources/conflict")
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/source"),
        Some(page_id),
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/conflict"),
        Some(owner_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// soft delete中はresource URI予約を維持し、
/// hard delete後は解放することを確認する。
///
#[test]
fn hard_delete_releases_resource_uri_after_soft_delete_keeps_it() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/reserved",
            "tester",
            resource_source(Some("/docs/reserved"), "reserved"),
        )
        .expect("create resource page failed");

    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete failed");
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/reserved"),
        Some(page_id.clone()),
    );
    assert!(
        manager
            .create_page(
                "/resources/conflict-after-soft-delete",
                "tester",
                resource_source(Some("/docs/reserved"), "conflict"),
            )
            .is_err()
    );

    manager
        .delete_page_by_id_hard(&page_id)
        .expect("hard delete failed");
    assert_eq!(resource_uri_owner_for_test(&manager, "/docs/reserved"), None);
    manager
        .create_page(
            "/resources/reused-after-hard-delete",
            "tester",
            resource_source(Some("/docs/reserved"), "reused"),
        )
        .expect("reuse released resource_path failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// recursive hard deleteで配下ページのresource URIも
/// 解放されることを確認する。
///
#[test]
fn recursive_hard_delete_releases_child_resource_uris() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let parent_id = manager
        .create_page(
            "/resources/tree",
            "tester",
            resource_source(Some("/docs/tree"), "tree"),
        )
        .expect("create parent resource page failed");
    manager
        .create_page(
            "/resources/tree/child",
            "tester",
            resource_source(Some("/docs/tree-child"), "child"),
        )
        .expect("create child resource page failed");

    manager
        .delete_pages_recursive_by_id(&parent_id, true)
        .expect("recursive hard delete failed");

    assert_eq!(resource_uri_owner_for_test(&manager, "/docs/tree"), None);
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/tree-child"),
        None,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// undelete時に保持していたURI予約を確認し、
/// 復帰先path由来resource_pathへ同期することを確認する。
///
#[test]
fn undelete_path_derived_resource_page_updates_resource_uri() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/deleted-path",
            "tester",
            resource_source(None, "deleted path"),
        )
        .expect("create resource page failed");

    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete failed");
    manager
        .undelete_page_by_id(&page_id, "/resources/restored-path", false)
        .expect("undelete resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/deleted-path"),
        None,
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/pages/resources/restored-path"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// undelete時に保持していたURI予約が対象ページ自身を
/// 指していない場合に復帰を拒否することを確認する。
///
#[test]
fn undelete_resource_page_rejects_resource_uri_owner_mismatch() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/deleted-explicit",
            "tester",
            resource_source(Some("/docs/deleted-explicit"), "deleted"),
        )
        .expect("create deleted resource page failed");
    let other_id = manager
        .create_page(
            "/resources/other-explicit",
            "tester",
            resource_source(Some("/docs/other-explicit"), "other"),
        )
        .expect("create other resource page failed");

    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete failed");
    manager
        .set_resource_uri_owner_for_test(
            "/docs/deleted-explicit",
            Some(&other_id),
        )
        .expect("corrupt resource URI owner failed");
    let error = manager
        .undelete_page_by_id(&page_id, "/resources/restored-explicit", false)
        .expect_err("undelete must reject mismatched owner");

    assert!(
        error
            .to_string()
            .contains("resource URI already exists: resource_path=/docs/deleted-explicit")
    );
    assert!(manager
        .get_page_id_by_path("/resources/restored-explicit")
        .expect("resolve restored path failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// rollback先のresource_pathが別ページと衝突する場合に
/// rollbackを拒否し、latest revisionを維持することを確認する。
///
#[test]
fn rollback_rejects_resource_uri_conflict_atomically() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/rollback-target",
            "tester",
            resource_source(Some("/docs/rollback-old"), "old"),
        )
        .expect("create rollback target failed");
    manager
        .put_page(
            &page_id,
            "tester",
            resource_source(Some("/docs/rollback-new"), "new"),
            false,
        )
        .expect("put rollback target failed");
    manager
        .create_page(
            "/resources/rollback-owner",
            "tester",
            resource_source(Some("/docs/rollback-old"), "owner"),
        )
        .expect("create rollback owner failed");

    let error = manager
        .rollback_page_source_only(&page_id, 1)
        .expect_err("conflicting rollback must fail");

    assert!(
        error
            .to_string()
            .contains("resource URI already exists: resource_path=/docs/rollback-old")
    );
    let index = manager
        .get_page_index_by_id(&page_id)
        .expect("get target index failed")
        .expect("target index missing");
    assert_eq!(index.latest(), 2);
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/rollback-new"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// amendで別ページのresource_pathへ変更しようとした場合に
/// 保存を拒否し、revisionを増やさないことを確認する。
///
#[test]
fn append_amend_rejects_resource_uri_conflict_atomically() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/resources/amend-target",
            "tester",
            resource_source(Some("/docs/amend-target"), "target"),
        )
        .expect("create amend target failed");
    manager
        .create_page(
            "/resources/amend-owner",
            "tester",
            resource_source(Some("/docs/amend-owner"), "owner"),
        )
        .expect("create amend owner failed");
    let request = AppendPageRequest::new(
        page_id.clone(),
        "tester".to_string(),
        resource_source(Some("/docs/amend-owner"), "conflict"),
        1,
        true,
    );

    let error = manager
        .append_page_by_id(&request)
        .expect_err("conflicting amend must fail");

    assert!(
        error
            .to_string()
            .contains("resource URI already exists: resource_path=/docs/amend-owner")
    );
    let index = manager
        .get_page_index_by_id(&page_id)
        .expect("get target index failed")
        .expect("target index missing");
    assert_eq!(index.latest(), 1);
    assert!(
        !manager
            .has_page_source_for_test(&page_id, 2)
            .expect("page source lookup failed")
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/amend-target"),
        Some(page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt用primitive名前索引とresource URI逆引き索引が
/// 別責務として扱われることを確認する。
///
#[test]
fn prompt_name_and_resource_uri_indexes_are_independent() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    manager
        .create_page(
            "/prompts/shared",
            "tester",
            prompt_source("shared", "prompt"),
        )
        .expect("create prompt page failed");
    let resource_page_id = manager
        .create_page(
            "/resources/shared",
            "tester",
            resource_source(Some("/shared"), "shared"),
        )
        .expect("create resource page failed");

    assert_eq!(
        resource_uri_owner_for_test(&manager, "/shared"),
        Some(resource_page_id),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// DB初期化が既存ページからprimitive名前索引を
/// 原子的に構築することを確認する。
///
#[test]
fn init_database_builds_primitive_names_from_existing_pages() {
    /*
     * 名前索引を持たない旧DB相当を準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    let visible_id = PageId::new();
    let deleted_id = PageId::new();
    seed_legacy_page(
        &db,
        &visible_id,
        "/visible",
        prompt_source("visible", "description"),
        false,
    );
    seed_legacy_page(
        &db,
        &deleted_id,
        "/deleted",
        prompt_source("deleted", "description"),
        true,
    );
    seed_legacy_page(
        &db,
        &PageId::new(),
        "/normal",
        "# normal".to_string(),
        false,
    );

    /*
     * 初期構築と再実行を行う
     */
    init_database(&mut db).expect("initialize legacy db failed");
    init_database(&mut db).expect("reinitialize db failed");

    /*
     * 名前索引と構築済みマーカーを確認する
     */
    let txn = db.begin_read().expect("begin read failed");
    let names = txn
        .open_table(MCP_PRIMITIVE_NAME_TABLE)
        .expect("open names failed");
    assert_eq!(names.len().expect("get names length failed"), 2);
    for (name, expected) in [
        ("visible", visible_id),
        ("deleted", deleted_id),
    ] {
        let key = McpPrimitiveNameKey::new(
            McpPrimitiveKind::Prompt,
            name.to_string(),
        );
        assert_eq!(
            names
                .get(key)
                .expect("get name failed")
                .expect("name missing")
                .value(),
            expected,
        );
    }
    let state = txn
        .open_table(MCP_PRIMITIVE_NAME_STATE_TABLE)
        .expect("open state failed");
    assert_eq!(
        state.get(0).expect("get state failed").map(|v| v.value()),
        Some(1),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// DB初期化が既存の重複primitive名を拒否し、
/// 部分構築しないことを確認する。
///
#[test]
fn init_database_rejects_duplicate_primitive_names_atomically() {
    /*
     * 同名promptを持つ旧DB相当を準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    let first_id = PageId::new();
    let second_id = PageId::new();
    seed_legacy_page(
        &db,
        &first_id,
        "/first",
        prompt_source("duplicate", "first"),
        false,
    );
    seed_legacy_page(
        &db,
        &second_id,
        "/second",
        prompt_source("duplicate", "second"),
        false,
    );

    /*
     * 初期構築が重複エラーで失敗することを確認する
     */
    let error = init_database(&mut db)
        .expect_err("duplicate initialization must fail");
    let message = format!("{:#}", error);
    assert!(message.contains("primitive=prompt"));
    assert!(message.contains("name=duplicate"));
    assert!(message.contains(&first_id.to_string()));
    assert!(message.contains(&second_id.to_string()));

    /*
     * 名前索引と状態テーブルが部分commitされないことを確認する
     */
    let txn = db.begin_read().expect("begin read failed");
    let tables: Vec<String> = txn
        .list_tables()
        .expect("list tables failed")
        .map(|table| table.name().to_string())
        .collect();
    assert!(!tables.iter().any(|name| {
        name == "mcp_primitive_name_table"
            || name == "mcp_primitive_name_state_table"
    }));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// primitive名前索引readinessが対応済みマーカーを
/// 必須とすることを確認する。
///
#[test]
fn primitive_name_index_readiness_requires_supported_marker() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");

    assert!(
        manager
            .is_mcp_primitive_name_index_ready()
            .expect("read initial readiness failed")
    );
    manager
        .set_mcp_primitive_name_state_for_test(None)
        .expect("remove readiness marker failed");
    assert!(
        !manager
            .is_mcp_primitive_name_index_ready()
            .expect("read missing readiness failed")
    );
    manager
        .set_mcp_primitive_name_state_for_test(Some(2))
        .expect("set unsupported marker failed");
    assert!(
        !manager
            .is_mcp_primitive_name_index_ready()
            .expect("read unsupported readiness failed")
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt名逆引きが候補テーブルに依存せず
/// 最新ページソースを返すことを確認する。
///
/// # 注記
/// prompt候補を除去した後に名前で正本を取得し、
/// soft delete後は非公開になることを検証する。
///
#[test]
fn get_prompt_source_by_name_uses_name_index_and_latest_source() {
    /*
     * prompt正本を作成して候補だけを除去する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt-source",
            "tester",
            prompt_source("source-prompt", "description"),
        )
        .expect("create prompt failed");
    manager
        .remove_prompt_candidate_by_page_id(&page_id)
        .expect("remove prompt candidate failed");

    /*
     * 名前索引とlatest sourceだけで取得できることを確認する
     */
    let entry = manager
        .get_prompt_source_by_name("source-prompt")
        .expect("get prompt source failed")
        .expect("prompt source missing");
    assert_eq!(entry.revision(), 1);
    assert_eq!(
        entry.source(),
        prompt_source("source-prompt", "description"),
    );
    assert!(manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get prompt candidate failed")
        .is_none());

    /*
     * soft delete後は名前予約を維持しても非公開にする
     */
    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete prompt failed");
    assert!(manager
        .get_prompt_source_by_name("source-prompt")
        .expect("get deleted prompt failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt候補同期が最新ソースをinsertおよびupdateすることを確認する。
///
/// # 注記
/// promptページを作成して同期した後、最新ソースを更新して再同期する。
///
#[test]
fn sync_prompt_candidate_for_page_inserts_and_updates_latest_source() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt",
            "tester",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: first\n",
                "  description: first description\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create page failed");

    let inserted = manager
        .sync_prompt_candidate_for_page(&page_id)
        .expect("sync prompt candidate failed")
        .expect("prompt candidate missing");
    assert_eq!(inserted.name(), "first");

    manager
        .put_page(
            &page_id,
            "tester",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: second\n",
                "  description: second description\n",
                "---\n",
                "更新本文",
            )
            .to_string(),
            false,
        )
        .expect("put page failed");
    let updated = manager
        .sync_prompt_candidate_for_page(&page_id)
        .expect("resync prompt candidate failed")
        .expect("updated prompt candidate missing");

    assert_eq!(updated.name(), "second");
    assert_eq!(updated.description(), "second description");
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get prompt candidate failed"),
        Some(updated),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt指定解除後の同期が既存候補を除去することを確認する。
///
/// # 注記
/// prompt候補を同期した後、通常ページへ更新して再同期する。
///
#[test]
fn sync_prompt_candidate_for_page_removes_disabled_prompt() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt",
            "tester",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: prompt\n",
                "  description: description\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create page failed");
    manager
        .sync_prompt_candidate_for_page(&page_id)
        .expect("sync prompt candidate failed");

    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("put page failed");
    let synced = manager
        .sync_prompt_candidate_for_page(&page_id)
        .expect("resync prompt candidate failed");

    assert!(synced.is_none());
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get prompt candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 候補対象外のページIDからprompt候補を生成しないことを確認する。
///
/// # 注記
/// 存在しないページIDとdraftページを順に同期する。
///
#[test]
fn sync_prompt_candidate_for_page_ignores_missing_and_draft_pages() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");

    assert!(
        manager
            .sync_prompt_candidate_for_page(&PageId::new())
            .expect("sync missing page failed")
            .is_none()
    );

    let draft_id = manager
        .create_draft_page("/draft-prompt", "tester")
        .expect("create draft failed")
        .0;

    assert!(
        manager
            .sync_prompt_candidate_for_page(&draft_id)
            .expect("sync draft failed")
            .is_none()
    );
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&draft_id)
            .expect("get draft candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// createとputがprompt候補を自動同期することを確認する。
///
/// # 注記
/// createで登録し、putで更新およびprompt指定解除を
/// 順に検証する。
///
#[test]
fn create_and_put_auto_sync_prompt_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt",
            "tester",
            prompt_source("first", "first description"),
        )
        .expect("create page failed");

    let created = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get created candidate failed")
        .expect("created candidate missing");
    assert_eq!(created.name(), "first");

    manager
        .put_page(
            &page_id,
            "tester",
            prompt_source("second", "second description"),
            false,
        )
        .expect("put prompt page failed");
    let updated = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get updated candidate failed")
        .expect("updated candidate missing");
    assert_eq!(updated.name(), "second");
    assert_eq!(updated.description(), "second description");

    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("disable prompt failed");
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get disabled candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// appendがprompt候補を自動同期することを確認する。
///
/// # 注記
/// 通常ページへprompt定義を含む完成ソースを
/// append保存する。
///
#[test]
fn append_auto_syncs_prompt_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt",
            "tester",
            "# 通常ページ".to_string(),
        )
        .expect("create page failed");
    let request = AppendPageRequest::new(
        page_id.clone(),
        "tester".to_string(),
        prompt_source("appended", "append description"),
        1,
        false,
    );

    manager
        .append_page_by_id(&request)
        .expect("append page failed");

    let candidate = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get appended candidate failed")
        .expect("appended candidate missing");
    assert_eq!(candidate.name(), "appended");
    assert_eq!(candidate.description(), "append description");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// rollbackがprompt候補を自動同期することを確認する。
///
/// # 注記
/// prompt定義を解除した後、定義を持つrevisionへrollbackする。
///
#[test]
fn rollback_auto_syncs_prompt_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt",
            "tester",
            prompt_source("restored", "rollback description"),
        )
        .expect("create page failed");
    manager
        .put_page(
            &page_id,
            "tester",
            "# 通常ページ\n本文".to_string(),
            false,
        )
        .expect("disable prompt failed");
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get disabled candidate failed")
            .is_none()
    );

    manager
        .rollback_page_source_only(&page_id, 1)
        .expect("rollback page failed");

    let candidate = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get restored candidate failed")
        .expect("restored candidate missing");
    assert_eq!(candidate.name(), "restored");
    assert_eq!(candidate.description(), "rollback description");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 単一ページのhard deleteがprompt候補を
/// 除去することを確認する。
///
/// # 注記
/// promptページを作成し、hard delete前後の
/// 候補状態を検証する。
///
#[test]
fn delete_page_by_id_hard_removes_prompt_candidate() {
    /*
     * promptページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompt",
            "tester",
            prompt_source("prompt", "description"),
        )
        .expect("create page failed");

    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get candidate before delete failed")
            .is_some()
    );

    /*
     * hard delete後の候補除去を確認する
     */
    manager
        .delete_page_by_id_hard(&page_id)
        .expect("hard delete failed");

    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get candidate after delete failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 再帰削除がhard delete時だけprompt候補を
/// 除去することを確認する。
///
/// # 注記
/// soft delete対象とhard delete対象を分けて
/// 候補の維持および除去を検証する。
///
#[test]
fn recursive_hard_delete_removes_prompt_candidates() {
    /*
     * soft delete対象と親子hard delete対象を準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let soft_id = manager
        .create_page(
            "/soft-prompt",
            "tester",
            prompt_source("soft", "soft description"),
        )
        .expect("create soft-delete target failed");
    let parent_id = manager
        .create_page(
            "/prompts",
            "tester",
            prompt_source("parent", "parent description"),
        )
        .expect("create parent failed");
    let child_id = manager
        .create_page(
            "/prompts/child",
            "tester",
            prompt_source("child", "child description"),
        )
        .expect("create child failed");

    /*
     * soft deleteで候補が維持されることを確認する
     */
    manager
        .delete_pages_recursive_by_id(&soft_id, false)
        .expect("soft delete failed");
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&soft_id)
            .expect("get soft-deleted candidate failed")
            .is_some()
    );

    /*
     * 親子サブツリーのhard deleteで
     * 候補が除去されることを確認する
     */
    manager
        .delete_pages_recursive_by_id(&parent_id, true)
        .expect("hard delete failed");
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&parent_id)
            .expect("get hard-deleted parent candidate failed")
            .is_none()
    );
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&child_id)
            .expect("get hard-deleted child candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// rename・soft delete・undeleteでprompt候補を
/// 維持することを確認する。
///
/// # 注記
/// 候補属性とページ索引のpath・削除状態を
/// 各操作後に検証する。
///
#[test]
fn prompt_candidate_survives_rename_delete_and_undelete() {
    /*
     * 全属性を持つpromptページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompts/original",
            "tester",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: prompt\n",
                "  description: description\n",
                "  system: system\n",
                "  arguments:\n",
                "    - name: target\n",
                "      description: target description\n",
                "      required: true\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create page failed");
    let expected = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get initial candidate failed")
        .expect("initial candidate missing");

    /*
     * rename後の候補とcurrent pathを確認する
     */
    manager
        .rename_page("/prompts/original", "/prompts/renamed")
        .expect("rename page failed");
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get renamed candidate failed"),
        Some(expected.clone()),
    );
    let renamed_index = manager
        .get_page_index_by_id(&page_id)
        .expect("get renamed index failed")
        .expect("renamed index missing");
    assert_eq!(
        renamed_index.current_path(),
        Some("/prompts/renamed"),
    );
    assert!(!renamed_index.deleted());

    /*
     * soft delete後の候補維持と削除状態を確認する
     */
    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete failed");
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get deleted candidate failed"),
        Some(expected.clone()),
    );
    let deleted_index = manager
        .get_page_index_by_id(&page_id)
        .expect("get deleted index failed")
        .expect("deleted index missing");
    assert!(deleted_index.deleted());
    assert_eq!(
        deleted_index.last_deleted_path(),
        Some("/prompts/renamed"),
    );

    /*
     * 別pathへのundelete後の候補と公開状態を確認する
     */
    manager
        .undelete_page_by_id(&page_id, "/restored/prompt", false)
        .expect("undelete page failed");
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get restored candidate failed"),
        Some(expected),
    );
    let restored_index = manager
        .get_page_index_by_id(&page_id)
        .expect("get restored index failed")
        .expect("restored index missing");
    assert_eq!(
        restored_index.current_path(),
        Some("/restored/prompt"),
    );
    assert!(!restored_index.deleted());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt候補一覧が最新ページ状態と
/// 合流することを確認する。
///
/// # 注記
/// rename・soft delete・undeleteと通常ページ除外を
/// 一連の操作で検証する。
///
#[test]
fn list_prompt_candidates_merges_latest_page_state() {
    /*
     * promptページと通常ページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompts/original",
            "tester",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: prompt\n",
                "  description: description\n",
                "  system: system\n",
                "  arguments:\n",
                "    - name: target\n",
                "      description: target description\n",
                "      required: true\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create prompt page failed");
    manager
        .create_page(
            "/normal",
            "tester",
            "# normal".to_string(),
        )
        .expect("create normal page failed");

    /*
     * rename後の最新pathと候補属性を確認する
     */
    manager
        .rename_page("/prompts/original", "/prompts/renamed")
        .expect("rename page failed");
    let entries = manager
        .list_prompt_candidates()
        .expect("list renamed candidates failed");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.page_id(), page_id);
    assert_eq!(entry.current_path(), "/prompts/renamed");
    assert_eq!(entry.name(), "prompt");
    assert_eq!(entry.description(), "description");
    assert_eq!(entry.system(), Some("system"));
    assert_eq!(entry.arguments().len(), 1);
    assert_eq!(entry.arguments()[0].name(), "target");
    assert_eq!(entry.arguments()[0].required(), Some(true));

    /*
     * soft delete後に一覧から除外されることを確認する
     */
    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete failed");
    assert!(
        manager
            .list_prompt_candidates()
            .expect("list deleted candidates failed")
            .is_empty()
    );

    /*
     * undelete後に新しいpathで一覧へ復帰することを確認する
     */
    manager
        .undelete_page_by_id(&page_id, "/restored/prompt", false)
        .expect("undelete page failed");
    let entries = manager
        .list_prompt_candidates()
        .expect("list restored candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), page_id);
    assert_eq!(entries[0].current_path(), "/restored/prompt");
    assert_eq!(entries[0].name(), "prompt");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt候補同期が別ページの同名候補を
/// 拒否することを確認する。
///
/// # 注記
/// 自己更新、別ページ同名、大文字小文字違いを
/// 検証する。
///
#[test]
fn sync_prompt_candidate_rejects_duplicate_name() {
    /*
     * 最初のprompt候補を準備して自己同期する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let first_id = manager
        .create_page(
            "/first",
            "tester",
            prompt_source("Prompt", "first description"),
        )
        .expect("create first prompt failed");
    manager
        .sync_prompt_candidate_for_page(&first_id)
        .expect("self sync failed");

    /*
     * 別ページの同名候補が拒否されることを確認する
     */
    let error = manager
        .create_page(
            "/duplicate",
            "tester",
            prompt_source("Prompt", "duplicate description"),
        )
        .expect_err("duplicate prompt must fail");
    let message = error.to_string();
    assert!(message.contains("primitive=prompt"));
    assert!(message.contains("name=Prompt"));
    assert!(!message.contains("/first"));
    assert!(!message.contains("/duplicate"));
    let first_candidate = manager
        .get_prompt_candidate_by_page_id(&first_id)
        .expect("get first candidate failed")
        .expect("first candidate missing");
    assert_eq!(first_candidate.description(), "first description");

    /*
     * 大文字小文字が異なる名前は
     * 許可されることを確認する
     */
    manager
        .create_page(
            "/case-distinct",
            "tester",
            prompt_source("prompt", "case description"),
        )
        .expect("create case-distinct prompt failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt候補一覧が同名候補の不整合を
/// エラーとして扱うことを確認する。
///
/// # 注記
/// テスト限定helperで異なるページに同名候補を投入する。
///
#[test]
fn list_prompt_candidates_rejects_duplicate_name() {
    /*
     * 異なる名前を持つ2ページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let first_id = manager
        .create_page(
            "/first",
            "tester",
            prompt_source("first", "first description"),
        )
        .expect("create first prompt failed");
    let second_id = manager
        .create_page(
            "/second",
            "tester",
            prompt_source("second", "second description"),
        )
        .expect("create second prompt failed");
    let duplicate = PromptCandidateEntry::new(
        "first".to_string(),
        "duplicate description".to_string(),
        None,
        Vec::new(),
    );

    /*
     * 候補テーブルへ重複状態を作成する
     */
    manager
        .insert_prompt_candidate_for_test(&second_id, &duplicate)
        .expect("insert duplicate candidate failed");

    /*
     * 一覧取得が内部不整合を返すことを確認する
     */
    let error = manager
        .list_prompt_candidates()
        .expect_err("duplicate candidate list must fail");
    let message = error.to_string();
    assert!(message.contains("primitive=prompt"));
    assert!(message.contains("name=first"));
    assert!(!message.contains("/first"));
    assert!(!message.contains("/second"));
    assert!(!message.contains(&first_id.to_string()));
    assert!(!message.contains(&second_id.to_string()));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// create・putがprimitive名前索引を
/// 原子的に更新することを確認する。
///
/// # 注記
/// 重複拒否、名前変更、指定解除、amendを検証する。
///
#[test]
fn page_writes_update_primitive_name_index_atomically() {
    /*
     * 最初のpromptページを作成する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/first",
            "tester",
            prompt_source("first", "description"),
        )
        .expect("create first prompt failed");

    /*
     * 同名createが正本commit前に拒否されることを確認する
     */
    let error = manager
        .create_page(
            "/duplicate",
            "tester",
            prompt_source("first", "duplicate"),
        )
        .expect_err("duplicate create must fail");
    assert!(error.to_string().contains("primitive=prompt name=first"));
    assert!(
        manager
            .get_page_id_by_path("/duplicate")
            .expect("resolve duplicate path failed")
            .is_none()
    );

    /*
     * 名前変更と指定解除で
     * 旧名が解放されることを確認する
     */
    manager
        .put_page(
            &page_id,
            "tester",
            prompt_source("second", "renamed"),
            true,
        )
        .expect("amend prompt name failed");
    manager
        .create_page(
            "/old-name",
            "tester",
            prompt_source("first", "reused"),
        )
        .expect("reuse old name failed");
    manager
        .put_page(
            &page_id,
            "tester",
            "# normal".to_string(),
            false,
        )
        .expect("remove prompt designation failed");
    manager
        .create_page(
            "/released-name",
            "tester",
            prompt_source("second", "reused"),
        )
        .expect("reuse released name failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// append・rollbackがprimitive名前索引を
/// 原子的に更新することを確認する。
///
/// # 注記
/// rollback先の名前競合時にrevisionを
/// 維持することも検証する。
///
#[test]
fn append_and_rollback_update_primitive_name_index_atomically() {
    /*
     * revision 1のpromptページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/target",
            "tester",
            prompt_source("first", "revision one"),
        )
        .expect("create target failed");
    let request = AppendPageRequest::new(
        page_id.clone(),
        "tester".to_string(),
        prompt_source("second", "revision two"),
        1,
        false,
    );
    /*
     * appendで名前を変更して旧名を別ページへ割り当てる
     */
    manager
        .append_page_by_id(&request)
        .expect("append prompt failed");
    manager
        .create_page(
            "/owner",
            "tester",
            prompt_source("first", "owner"),
        )
        .expect("reuse first name failed");

    /*
     * 競合するrollbackがrevisionを変更しないことを確認する
     */
    let error = manager
        .rollback_page_source_only(&page_id, 1)
        .expect_err("conflicting rollback must fail");
    assert!(error.to_string().contains("primitive=prompt name=first"));
    let index = manager
        .get_page_index_by_id(&page_id)
        .expect("get target index failed")
        .expect("target index missing");
    assert_eq!(index.latest(), 2);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// hard deleteがprimitive名を解放し、
/// soft deleteが予約を維持することを確認する。
///
/// # 注記
/// soft delete中の重複拒否とhard delete後の再利用を検証する。
///
#[test]
fn hard_delete_releases_primitive_names() {
    /*
     * 名前予約を持つpromptページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/target",
            "tester",
            prompt_source("reserved", "description"),
        )
        .expect("create target failed");
    /*
     * soft delete中は同名createが拒否されることを確認する
     */
    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete failed");
    assert!(
        manager
            .create_page(
                "/conflict",
                "tester",
                prompt_source("reserved", "conflict"),
            )
            .is_err()
    );
    /*
     * hard delete後に同じ名前を再利用する
     */
    manager
        .delete_page_by_id_hard(&page_id)
        .expect("hard delete failed");
    manager
        .create_page(
            "/reused",
            "tester",
            prompt_source("reserved", "reused"),
        )
        .expect("reuse hard-deleted name failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// importが重複primitive名を検出して
/// 全体をrollbackすることを確認する。
///
/// # 注記
/// 同名promptを持つ2ページのbundleを低水準投入する。
///
#[test]
fn import_rejects_duplicate_primitive_names_atomically() {
    /*
     * 同名promptを持つimport bundleを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    let user_id = UserId::new();
    let first_id = PageId::new();
    let second_id = PageId::new();
    let timestamp = Local::now();
    let mut bundle = ExportBundle::new(ManifestContext {
        export_type: ExportType::Backup,
        export_root: "/".to_string(),
        relocate_prefix: None,
    });
    bundle.users.push(ExportUser {
        id: user_id.clone(),
        username: "import-user".to_string(),
        password: "hashed".to_string(),
        salt: [9u8; 16],
        display_name: "Import User".to_string(),
        attributes: UserAttributeSet::new(),
    });
    for (page_id, path) in [
        (first_id.clone(), "first"),
        (second_id.clone(), "second"),
    ] {
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: path.to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: Some(vec![1]),
        });
        bundle.revisions.push(ExportRevision {
            page: page_id,
            revision: 1,
            timestamp: timestamp.clone(),
            user: user_id.clone(),
            rename: None,
            source: prompt_source("duplicate", "description"),
        });
    }
    bundle.sync_manifest_counts();

    /*
     * 重複検出でimport全体が失敗することを確認する
     */
    let error = manager
        .insert_import_bundle(&bundle)
        .expect_err("duplicate import must fail");
    assert!(
        error
            .to_string()
            .contains("primitive=prompt name=duplicate")
    );
    assert!(
        manager
            .get_page_id_by_path("/first")
            .expect("resolve first path failed")
            .is_none()
    );
    assert!(
        manager
            .get_page_id_by_path("/second")
            .expect("resolve second path failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// importが重複resource URIを検出して
/// 全体をrollbackすることを確認する。
///
/// # 注記
/// 同じresource_pathを持つ2ページのbundleを低水準投入する。
///
#[test]
fn import_rejects_duplicate_resource_uris_atomically() {
    /*
     * 同じresource_pathを持つimport bundleを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    let user_id = UserId::new();
    let first_id = PageId::new();
    let second_id = PageId::new();
    let timestamp = Local::now();
    let mut bundle = ExportBundle::new(ManifestContext {
        export_type: ExportType::Backup,
        export_root: "/".to_string(),
        relocate_prefix: None,
    });
    bundle.users.push(ExportUser {
        id: user_id.clone(),
        username: "import-user".to_string(),
        password: "hashed".to_string(),
        salt: [9u8; 16],
        display_name: "Import User".to_string(),
        attributes: UserAttributeSet::new(),
    });
    for (page_id, path) in [
        (first_id.clone(), "resource-first"),
        (second_id.clone(), "resource-second"),
    ] {
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: path.to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: Some(vec![1]),
        });
        bundle.revisions.push(ExportRevision {
            page: page_id,
            revision: 1,
            timestamp: timestamp.clone(),
            user: user_id.clone(),
            rename: None,
            source: resource_source(Some("/docs/import-duplicate"), path),
        });
    }
    bundle.sync_manifest_counts();

    /*
     * 重複検出でimport全体が失敗することを確認する
     */
    let error = manager
        .insert_import_bundle(&bundle)
        .expect_err("duplicate resource import must fail");
    assert!(
        error
            .to_string()
            .contains("resource URI already exists: resource_path=/docs/import-duplicate")
    );
    assert!(
        manager
            .get_page_id_by_path("/resource-first")
            .expect("resolve first path failed")
            .is_none()
    );
    assert!(
        manager
            .get_page_id_by_path("/resource-second")
            .expect("resolve second path failed")
            .is_none()
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/import-duplicate"),
        None,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt候補同期失敗後もprimitive名前索引が
/// 正本と整合することを確認する。
///
/// # 注記
/// 候補テーブルだけに同名不整合を作り、
/// 保存後同期を失敗させる。
///
#[test]
fn prompt_sync_failure_preserves_primitive_name_index() {
    /*
     * 正常な2ページと候補テーブルだけの不整合を準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open database failed");
    manager
        .add_user("tester", "pass", None)
        .expect("add user failed");
    let first_id = manager
        .create_page(
            "/first",
            "tester",
            prompt_source("first", "first description"),
        )
        .expect("create first prompt failed");
    let second_id = manager
        .create_page(
            "/second",
            "tester",
            prompt_source("second", "second description"),
        )
        .expect("create second prompt failed");
    let inconsistent = PromptCandidateEntry::new(
        "conflict".to_string(),
        "inconsistent".to_string(),
        None,
        Vec::new(),
    );
    manager
        .insert_prompt_candidate_for_test(&second_id, &inconsistent)
        .expect("insert inconsistent candidate failed");

    /*
     * 正本commit後の候補同期だけが失敗する更新を実行する
     */
    let error = manager
        .put_page(
            &first_id,
            "tester",
            prompt_source("conflict", "updated description"),
            false,
        )
        .expect_err("candidate sync must fail");
    assert!(
        error
            .to_string()
            .contains("primitive=prompt name=conflict")
    );

    /*
     * 正本と名前索引が
     * 新しい名前で整合することを確認する
     */
    let state = manager
        .get_current_page_state_by_path("/first")
        .expect("get first page state failed")
        .expect("first page state missing");
    assert!(
        state
            .latest_source()
            .expect("first latest source missing")
            .source()
            .contains("name: conflict")
    );
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "conflict",
            )
            .expect("get conflict owner failed"),
        Some(first_id.clone()),
    );
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "first",
            )
            .expect("get old owner failed"),
        None,
    );
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "second",
            )
            .expect("get second owner failed"),
        Some(second_id.clone()),
    );

    /*
     * 候補テーブルだけが同期未完了であることを確認する
     */
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&first_id)
            .expect("get first candidate failed")
            .expect("first candidate missing")
            .name(),
        "first",
    );
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&second_id)
            .expect("get second candidate failed"),
        Some(inconsistent),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用promptページソースを生成する。
///
/// # 引数
/// * `name` - prompt名
/// * `description` - prompt説明
///
/// # 戻り値
/// front matterと本文を含むページソースを返す。
///
fn prompt_source(name: &str, description: &str) -> String {
    format!(
        concat!(
            "---\n",
            "mcp:\n",
            "  primitive: prompt\n",
            "  name: {}\n",
            "  description: {}\n",
            "---\n",
            "本文",
        ),
        name,
        description,
    )
}

///
/// テスト用resourceページソースを生成する。
///
/// # 引数
/// * `resource_path` - 明示resource_path。省略時はfront matterへ出力しない。
/// * `name` - resource名
///
/// # 戻り値
/// front matterと本文を含むページソースを返す。
///
fn resource_source(resource_path: Option<&str>, name: &str) -> String {
    let resource_path_line = resource_path
        .map(|id| format!("  resource_path: {}\n", id))
        .unwrap_or_default();
    format!(
        concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "{}",
            "  name: {}\n",
            "  description: resource description\n",
            "---\n",
            "本文",
        ),
        resource_path_line,
        name,
    )
}

///
/// テスト用にresource URI逆引き索引の所有ページIDを取得する。
///
/// # 引数
/// * `manager` - DBマネージャ
/// * `resource_path` - 取得対象resource_path
///
/// # 戻り値
/// 所有ページIDが存在する場合は`Some(PageId)`を返す。
///
fn resource_uri_owner_for_test(
    manager: &DatabaseManager,
    resource_path: &str,
) -> Option<PageId> {
    manager
        .get_resource_uri_owner_for_test(resource_path)
        .expect("get resource URI owner failed")
}

///
/// M3以前相当のページ正本をDBへ直接投入する。
///
/// # 引数
/// * `db` - 投入対象DB
/// * `page_id` - ページID
/// * `path` - ページpath
/// * `source` - latest source
/// * `deleted` - soft delete状態
///
/// # 戻り値
/// なし
///
fn seed_legacy_page(
    db: &Database,
    page_id: &PageId,
    path: &str,
    source: String,
    deleted: bool,
) {
    let txn = db.begin_write().expect("begin legacy seed failed");
    {
        let mut indexes = txn
            .open_table(PAGE_INDEX_TABLE)
            .expect("open legacy indexes failed");
        let mut sources = txn
            .open_table(super::schema::PAGE_SOURCE_TABLE)
            .expect("open legacy sources failed");
        let mut index = PageIndex::new_page(
            page_id.clone(),
            path.to_string(),
        );
        if deleted {
            index.set_deleted(true);
        }
        indexes
            .insert(page_id.clone(), index)
            .expect("insert legacy index failed");
        sources
            .insert(
                (page_id.clone(), 1),
                PageSource::new(
                    source,
                    UserId::new(),
                    RenameInfo::none(),
                ),
            )
            .expect("insert legacy source failed");
    }
    txn.commit().expect("commit legacy seed failed");
}

///
/// PromptCandidateEntry が MessagePack 経由で往復できることを確認する。
///
/// # 注記
/// required 三状態と引数順序を含む候補を直列化して復元する。
///
#[test]
fn prompt_candidate_entry_round_trips_via_redb_value() {
    let entry = PromptCandidateEntry::new(
        "要約".to_string(),
        "ページを要約する".to_string(),
        Some("簡潔に回答する\n".to_string()),
        vec![
            PromptArgumentEntry::new(
                "first".to_string(),
                "最初".to_string(),
                None,
            ),
            PromptArgumentEntry::new(
                "second".to_string(),
                "次".to_string(),
                Some(false),
            ),
            PromptArgumentEntry::new(
                "third".to_string(),
                "最後".to_string(),
                Some(true),
            ),
        ],
    );
    let bytes = <PromptCandidateEntry as Value>::as_bytes(&entry);
    let restored =
        <PromptCandidateEntry as Value>::from_bytes(bytes.as_slice());

    assert_eq!(restored, entry);
    assert_eq!(
        <PromptCandidateEntry as Value>::type_name(),
        TypeName::new("PromptCandidateEntry"),
    );
    assert_eq!(<PromptCandidateEntry as Value>::fixed_width(), None);
    assert_eq!(restored.name(), "要約");
    assert_eq!(restored.description(), "ページを要約する");
    assert_eq!(restored.system(), Some("簡潔に回答する\n"));
    assert_eq!(restored.arguments()[0].name(), "first");
    assert_eq!(restored.arguments()[0].required(), None);
    assert_eq!(restored.arguments()[1].name(), "second");
    assert_eq!(restored.arguments()[1].required(), Some(false));
    assert_eq!(restored.arguments()[2].name(), "third");
    assert_eq!(restored.arguments()[2].required(), Some(true));
}

///
/// init_database がprompt候補テーブルを作成し、値を読み書きできることを確認する。
///
/// # 注記
/// 初期化直後の空状態を確認してから候補をinsert/getする。
///
#[test]
fn init_database_creates_readable_prompt_candidate_table() {
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");

    init_database(&mut db).expect("init db failed");

    let page_id = PageId::new();
    let candidate = PromptCandidateEntry::new(
        "prompt".to_string(),
        "description".to_string(),
        None,
        Vec::new(),
    );
    let read_txn = db.begin_read().expect("begin read failed");
    let table = read_txn
        .open_table(PROMPT_CANDIDATE_TABLE)
        .expect("open prompt candidate table failed");
    assert_eq!(table.len().expect("read table length failed"), 0);
    drop(table);
    drop(read_txn);

    let write_txn = db.begin_write().expect("begin write failed");
    {
        let mut table = write_txn
            .open_table(PROMPT_CANDIDATE_TABLE)
            .expect("open prompt candidate table failed");
        table
            .insert(page_id.clone(), candidate.clone())
            .expect("insert prompt candidate failed");
    }
    write_txn.commit().expect("commit failed");

    let read_txn = db.begin_read().expect("begin read failed");
    let table = read_txn
        .open_table(PROMPT_CANDIDATE_TABLE)
        .expect("open prompt candidate table failed");
    let restored = table
        .get(page_id)
        .expect("get prompt candidate failed")
        .expect("prompt candidate missing")
        .value();
    assert_eq!(restored, candidate);
    drop(table);
    drop(read_txn);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テンプレート解除時に候補テーブルから除去できることを確認する。
///
#[test]
fn sync_template_candidate_for_page_removes_entry_when_template_is_removed() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page(
            "/templates/minutes",
            "user",
            "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n---\n# 議事録".to_string(),
        )
        .expect("create page failed");

    let created = manager
        .sync_template_candidate_for_page(&page_id)
        .expect("sync after create failed")
        .expect("template candidate missing after create");
    assert_eq!(created.name(), "議事録");
    assert_eq!(created.description(), Some("定例会議"));
    assert_eq!(created.macro_expand(), Some(true));
    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after create failed")
        .is_some());

    manager
        .put_page(&page_id, "user", "# 通常ページ".to_string(), false)
        .expect("put page failed");

    let updated = manager
        .sync_template_candidate_for_page(&page_id)
        .expect("sync after put failed");
    assert!(updated.is_none());
    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after remove failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// put 保存後にテンプレート候補同期が自動反映されることを確認する。
///
#[test]
fn put_page_auto_syncs_template_candidate_removal() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page(
            "/templates/minutes",
            "user",
            "---\nwiki:\n  template:\n    name: 議事録\n---\n# 議事録".to_string(),
        )
        .expect("create page failed");

    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after create failed")
        .is_some());

    manager
        .put_page(&page_id, "user", "# 通常ページ".to_string(), false)
        .expect("put page failed");

    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after put failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 通常ページ同期時に候補テーブルへ登録しないことを確認する。
///
#[test]
fn sync_template_candidate_for_page_keeps_normal_page_absent() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page("/pages/normal", "user", "# 通常ページ".to_string())
        .expect("create page failed");

    let synced = manager
        .sync_template_candidate_for_page(&page_id)
        .expect("sync normal page failed");
    assert!(synced.is_none());
    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// リネーム・削除・復帰でテンプレート候補属性を維持できることを確認する。
///
#[test]
fn template_candidate_survives_rename_delete_and_undelete() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page(
            "/templates/minutes",
            "user",
            "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n---\n# 議事録".to_string(),
        )
        .expect("create page failed");
    manager
        .sync_template_candidate_for_page(&page_id)
        .expect("sync template candidate failed");

    manager
        .rename_page("/templates/minutes", "/templates/meeting-minutes")
        .expect("rename page failed");

    let candidate_after_rename = manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after rename failed")
        .expect("candidate missing after rename");
    assert_eq!(candidate_after_rename.name(), "議事録");
    assert_eq!(candidate_after_rename.description(), Some("定例会議"));
    assert_eq!(candidate_after_rename.macro_expand(), Some(true));

    let index_after_rename = manager
        .get_page_index_by_id(&page_id)
        .expect("get index after rename failed")
        .expect("page index missing after rename");
    assert_eq!(index_after_rename.current_path(), Some("/templates/meeting-minutes"));
    assert!(!index_after_rename.deleted());

    manager
        .delete_page_by_id(&page_id)
        .expect("soft delete page failed");

    let candidate_after_delete = manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after delete failed")
        .expect("candidate missing after delete");
    assert_eq!(candidate_after_delete.name(), "議事録");
    assert_eq!(candidate_after_delete.description(), Some("定例会議"));
    assert_eq!(candidate_after_delete.macro_expand(), Some(true));

    let index_after_delete = manager
        .get_page_index_by_id(&page_id)
        .expect("get index after delete failed")
        .expect("page index missing after delete");
    assert!(index_after_delete.deleted());
    assert_eq!(
        index_after_delete.last_deleted_path(),
        Some("/templates/meeting-minutes"),
    );

    manager
        .undelete_page_by_id(&page_id, "/templates/meeting-minutes", false)
        .expect("undelete page failed");

    let candidate_after_undelete = manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after undelete failed")
        .expect("candidate missing after undelete");
    assert_eq!(candidate_after_undelete.name(), "議事録");
    assert_eq!(candidate_after_undelete.description(), Some("定例会議"));
    assert_eq!(candidate_after_undelete.macro_expand(), Some(true));

    let index_after_undelete = manager
        .get_page_index_by_id(&page_id)
        .expect("get index after undelete failed")
        .expect("page index missing after undelete");
    assert_eq!(
        index_after_undelete.current_path(),
        Some("/templates/meeting-minutes"),
    );
    assert!(!index_after_undelete.deleted());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// ハードデリート時にテンプレート候補テーブルから除去されることを確認する。
///
#[test]
fn delete_page_by_id_hard_removes_template_candidate() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page(
            "/templates/minutes",
            "user",
            "---\nwiki:\n  template:\n    name: 議事録\n---\n# 議事録".to_string(),
        )
        .expect("create page failed");
    manager
        .sync_template_candidate_for_page(&page_id)
        .expect("sync template candidate failed");
    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate before hard delete failed")
        .is_some());

    manager
        .delete_page_by_id_hard(&page_id)
        .expect("hard delete page failed");

    assert!(manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get candidate after hard delete failed")
        .is_none());
    assert!(manager
        .get_page_index_by_id(&page_id)
        .expect("get page index after hard delete failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テンプレート候補一覧取得が current path 情報と合流できることを確認する。
///
#[test]
fn list_template_candidates_uses_derived_table_and_current_page_state() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let visible_id = manager
        .create_page(
            "/templates/visible",
            "user",
            "---\nwiki:\n  template:\n    name: 議事録B\n    description: 表示対象\n    macro_expand: true\n---\n# 議事録".to_string(),
        )
        .expect("create visible template failed");
    manager
        .sync_template_candidate_for_page(&visible_id)
        .expect("sync visible template failed");

    let deleted_id = manager
        .create_page(
            "/templates/deleted",
            "user",
            "---\nwiki:\n  template:\n    name: 議事録A\n---\n# 削除対象".to_string(),
        )
        .expect("create deleted template failed");
    manager
        .sync_template_candidate_for_page(&deleted_id)
        .expect("sync deleted template failed");
    manager
        .delete_page_by_id(&deleted_id)
        .expect("delete template failed");

    manager
        .rename_page("/templates/visible", "/moved/visible-template")
        .expect("rename visible template failed");

    let entries = manager
        .list_template_candidates()
        .expect("list template candidates failed");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), visible_id);
    assert_eq!(entries[0].current_path(), "/moved/visible-template");
    assert_eq!(entries[0].name(), "議事録B");
    assert_eq!(entries[0].description(), Some("表示対象"));
    assert_eq!(entries[0].macro_expand(), Some(true));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テンプレート候補テーブルを全ページ走査から再構成できることを確認する。
///
#[test]
fn rebuild_template_candidates_recreates_entries_from_latest_sources() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let visible_id = manager
        .create_page(
            "/outside-template-root",
            "user",
            "---\nwiki:\n  template:\n    name: ルート外テンプレート\n    description: 再構成対象\n    macro_expand: true\n---\n# visible".to_string(),
        )
        .expect("create visible template failed");
    let deleted_id = manager
        .create_page(
            "/templates/deleted",
            "user",
            "---\nwiki:\n  template:\n    name: 削除済みテンプレート\n---\n# deleted".to_string(),
        )
        .expect("create deleted template failed");
    let draft_id = manager
        .create_draft_page("/templates/draft", "user")
        .expect("create draft page failed")
        .0;
    let normal_id = manager
        .create_page("/templates/normal", "user", "# normal".to_string())
        .expect("create normal page failed");

    manager
        .delete_page_by_id(&deleted_id)
        .expect("delete template failed");

    manager
        .sync_template_candidate_for_page(&visible_id)
        .expect("sync visible template failed");
    manager
        .sync_template_candidate_for_page(&deleted_id)
        .expect("sync deleted template failed");
    manager
        .sync_template_candidate_for_page(&normal_id)
        .expect("sync normal page failed");

    manager
        .remove_template_candidate_by_page_id(&visible_id)
        .expect("remove visible candidate failed");
    manager
        .remove_template_candidate_by_page_id(&deleted_id)
        .expect("remove deleted candidate failed");
    let rebuilt_count = manager
        .rebuild_template_candidates()
        .expect("rebuild template candidates failed");
    assert_eq!(rebuilt_count, 2);

    let visible_candidate = manager
        .get_template_candidate_by_page_id(&visible_id)
        .expect("get visible candidate failed")
        .expect("visible candidate missing after rebuild");
    assert_eq!(visible_candidate.name(), "ルート外テンプレート");
    assert_eq!(visible_candidate.description(), Some("再構成対象"));
    assert_eq!(visible_candidate.macro_expand(), Some(true));

    let deleted_candidate = manager
        .get_template_candidate_by_page_id(&deleted_id)
        .expect("get deleted candidate failed")
        .expect("deleted candidate missing after rebuild");
    assert_eq!(deleted_candidate.name(), "削除済みテンプレート");

    assert!(manager
        .get_template_candidate_by_page_id(&draft_id)
        .expect("get draft candidate failed")
        .is_none());
    assert!(manager
        .get_template_candidate_by_page_id(&normal_id)
        .expect("get normal candidate failed")
        .is_none());

    let visible_entries = manager
        .list_template_candidates()
        .expect("list template candidates failed");
    assert_eq!(visible_entries.len(), 1);
    assert_eq!(visible_entries[0].page_id(), visible_id);
    assert_eq!(visible_entries[0].current_path(), "/outside-template-root");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt派生データを最新ページソースから
/// 再構成できることを確認する。
///
/// # 注記
/// 古い候補と名前索引を投入後に再構成し、
/// 最新ソース由来の内容へ置換されることを検証する。
///
#[test]
fn rebuild_prompt_candidates_recreates_entries_from_latest_sources() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * 再構成元ページと古い派生データを準備する
     */
    let prompt_id = manager
        .create_page(
            "/prompts/summarize",
            "user",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: summarize\n",
                "  description: 要約する\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create prompt page failed");
    let normal_id = manager
        .create_page("/normal", "user", "通常本文".to_string())
        .expect("create normal page failed");
    manager
        .remove_prompt_candidate_by_page_id(&prompt_id)
        .expect("remove prompt candidate failed");
    manager
        .insert_prompt_candidate_for_test(
            &normal_id,
            &PromptCandidateEntry::new(
                "stale".to_string(),
                "stale".to_string(),
                None,
                Vec::new(),
            ),
        )
        .expect("insert stale candidate failed");

    manager
        .set_mcp_primitive_name_owner_for_test(
            McpPrimitiveKind::Prompt,
            "summarize",
            None,
        )
        .expect("remove prompt name failed");
    manager
        .set_mcp_primitive_name_owner_for_test(
            McpPrimitiveKind::Prompt,
            "stale",
            Some(&normal_id),
        )
        .expect("insert stale name failed");

    /*
     * 再構成結果と古い派生データの除去を確認する
     */
    let count = manager
        .rebuild_prompt_candidates()
        .expect("rebuild prompt candidates failed");
    assert_eq!(count, 1);
    let candidate = manager
        .get_prompt_candidate_by_page_id(&prompt_id)
        .expect("get prompt candidate failed")
        .expect("rebuilt prompt candidate missing");
    assert_eq!(candidate.name(), "summarize");
    assert_eq!(candidate.description(), "要約する");
    assert!(manager
        .get_prompt_candidate_by_page_id(&normal_id)
        .expect("get stale candidate failed")
        .is_none());
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "summarize",
            )
            .expect("get prompt name owner failed"),
        Some(prompt_id),
    );
    assert!(manager
        .get_mcp_primitive_name_owner_for_test(
            McpPrimitiveKind::Prompt,
            "stale",
        )
        .expect("get stale name owner failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt派生データ再構成が冪等であることを確認する。
///
/// # 注記
/// 同じ正本状態で再構成を2回実行し、
/// 件数と候補および名前索引の所有者を比較する。
///
#[test]
fn rebuild_prompt_candidates_is_idempotent() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/prompts/idempotent",
            "user",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: idempotent\n",
                "  description: 冪等性確認\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create prompt page failed");

    /*
     * 同一正本に対する2回の再構成結果を比較する
     */
    let first_count = manager
        .rebuild_prompt_candidates()
        .expect("first rebuild failed");
    let first_candidate = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get first candidate failed");
    let second_count = manager
        .rebuild_prompt_candidates()
        .expect("second rebuild failed");
    let second_candidate = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get second candidate failed");
    let owner = manager
        .get_mcp_primitive_name_owner_for_test(
            McpPrimitiveKind::Prompt,
            "idempotent",
        )
        .expect("get prompt name owner failed");

    assert_eq!(first_count, 1);
    assert_eq!(second_count, 1);
    assert_eq!(second_candidate, first_candidate);
    assert_eq!(owner, Some(page_id));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt再構成がページ状態と用途のポリシーを
/// 適用することを確認する。
///
/// # 注記
/// 通常prompt、soft delete、draft、通常ページ、
/// template、resourceを同時に再構成して検証する。
///
#[test]
fn rebuild_prompt_candidates_applies_page_state_and_kind_policy() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * 状態と用途が異なるページを準備する
     */
    let visible_id = manager
        .create_page(
            "/prompts/visible",
            "user",
            prompt_source("visible", "公開prompt"),
        )
        .expect("create visible prompt failed");
    let deleted_id = manager
        .create_page(
            "/prompts/deleted",
            "user",
            prompt_source("deleted", "削除済みprompt"),
        )
        .expect("create deleted prompt failed");
    manager
        .delete_page_by_id(&deleted_id)
        .expect("soft delete prompt failed");
    let draft_id = manager
        .create_draft_page("/prompts/draft", "user")
        .expect("create draft failed")
        .0;
    let normal_id = manager
        .create_page("/normal", "user", "通常本文".to_string())
        .expect("create normal page failed");
    let template_id = manager
        .create_page(
            "/templates/page",
            "user",
            concat!(
                "---\n",
                "wiki:\n",
                "  template:\n",
                "    name: template\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create template page failed");
    let resource_page_id = manager
        .create_page(
            "/resources/spec",
            "user",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: resource\n",
                "  name: spec\n",
                "  description: 仕様\n",
                "---\n",
                "本文",
            )
            .to_string(),
        )
        .expect("create resource page failed");

    /*
     * prompt候補を欠損させて全件再構成する
     */
    manager
        .remove_prompt_candidate_by_page_id(&visible_id)
        .expect("remove visible candidate failed");
    manager
        .remove_prompt_candidate_by_page_id(&deleted_id)
        .expect("remove deleted candidate failed");
    let count = manager
        .rebuild_prompt_candidates()
        .expect("rebuild prompt candidates failed");

    /*
     * 候補と名前予約の状態別ポリシーを確認する
     */
    assert_eq!(count, 2);
    assert!(manager
        .get_prompt_candidate_by_page_id(&visible_id)
        .expect("get visible candidate failed")
        .is_some());
    assert!(manager
        .get_prompt_candidate_by_page_id(&deleted_id)
        .expect("get deleted candidate failed")
        .is_some());
    for (page_id, label) in [
        (&draft_id, "draft"),
        (&normal_id, "normal"),
        (&template_id, "template"),
        (&resource_page_id, "resource"),
    ] {
        assert!(
            manager
                .get_prompt_candidate_by_page_id(page_id)
                .unwrap_or_else(|_| panic!("get {} candidate failed", label))
                .is_none(),
            "{} candidate was rebuilt",
            label,
        );
    }
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "deleted",
            )
            .expect("get deleted name owner failed"),
        Some(deleted_id),
    );

    /*
     * soft delete済みpromptが
     * 公開一覧から除外されることを確認する
     */
    let entries = manager
        .list_prompt_candidates()
        .expect("list prompt candidates failed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].page_id(), visible_id);
    assert_eq!(entries[0].name(), "visible");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 保存後同期と全件再構成が同等のprompt候補へ
/// 射影することを確認する。
///
/// # 注記
/// 全prompt属性を含む同じ最新ソースから生成した
/// 差分同期結果と全件再構成結果を比較する。
///
#[test]
fn sync_and_rebuild_prompt_candidates_use_equivalent_projection() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * 保存後同期で全属性を持つ候補を生成する
     */
    let page_id = manager
        .create_page(
            "/prompts/projection",
            "user",
            concat!(
                "---\n",
                "mcp:\n",
                "  primitive: prompt\n",
                "  name: projection\n",
                "  description: 射影比較\n",
                "  system: |\n",
                "    system first\n",
                "    system second\n",
                "  arguments:\n",
                "    - name: first\n",
                "      description: 第一引数\n",
                "    - name: second\n",
                "      description: 第二引数\n",
                "      required: false\n",
                "    - name: third\n",
                "      description: 第三引数\n",
                "      required: true\n",
                "---\n",
                "本文 {{@first}} {{@second}} {{@third}}",
            )
            .to_string(),
        )
        .expect("create prompt page failed");
    let synced = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get synced candidate failed")
        .expect("synced candidate missing");

    /*
     * 候補を除去して同じ最新ソースから全件再構成する
     */
    manager
        .remove_prompt_candidate_by_page_id(&page_id)
        .expect("remove synced candidate failed");
    let count = manager
        .rebuild_prompt_candidates()
        .expect("rebuild prompt candidates failed");
    let rebuilt = manager
        .get_prompt_candidate_by_page_id(&page_id)
        .expect("get rebuilt candidate failed")
        .expect("rebuilt candidate missing");

    /*
     * 共通射影で全属性と引数順序が
     * 一致することを確認する
     */
    assert_eq!(count, 1);
    assert_eq!(rebuilt, synced);
    assert_eq!(rebuilt.name(), "projection");
    assert_eq!(rebuilt.description(), "射影比較");
    assert_eq!(
        rebuilt.system(),
        Some("system first\nsystem second\n"),
    );
    assert_eq!(rebuilt.arguments()[0].name(), "first");
    assert_eq!(rebuilt.arguments()[0].required(), None);
    assert_eq!(rebuilt.arguments()[1].name(), "second");
    assert_eq!(rebuilt.arguments()[1].required(), Some(false));
    assert_eq!(rebuilt.arguments()[2].name(), "third");
    assert_eq!(rebuilt.arguments()[2].required(), Some(true));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// front matter解析失敗時にprompt派生データと
/// ページ正本を維持することを確認する。
///
/// # 注記
/// latest sourceへ不正front matterを直接投入し、
/// 再構成失敗前後の状態を比較する。
///
#[test]
fn rebuild_prompt_candidates_preserves_data_on_parse_error() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let stable_id = manager
        .create_page(
            "/prompts/stable",
            "user",
            prompt_source("stable", "stable description"),
        )
        .expect("create stable prompt failed");
    let invalid_id = manager
        .create_page("/invalid", "user", "正常本文".to_string())
        .expect("create invalid target failed");
    let invalid_source = concat!(
        "---\n",
        "mcp: [\n",
        "---\n",
        "本文",
    )
    .to_string();
    manager
        .replace_latest_page_source_for_prompt_rebuild_test(
            &invalid_id,
            invalid_source.clone(),
        )
        .expect("inject invalid source failed");
    let candidate_before = manager
        .get_prompt_candidate_by_page_id(&stable_id)
        .expect("get candidate before rebuild failed");
    let owner_before = manager
        .get_mcp_primitive_name_owner_for_test(
            McpPrimitiveKind::Prompt,
            "stable",
        )
        .expect("get owner before rebuild failed");

    /*
     * 解析失敗が呼び出し元へ返ることを確認する
     */
    let error = manager
        .rebuild_prompt_candidates()
        .expect_err("rebuild must reject invalid front matter");
    assert!(!error.to_string().is_empty());

    /*
     * 派生データと状態マーカーが
     * 維持されることを確認する
     */
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&stable_id)
            .expect("get candidate after rebuild failed"),
        candidate_before,
    );
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "stable",
            )
            .expect("get owner after rebuild failed"),
        owner_before,
    );
    assert!(manager
        .is_mcp_primitive_name_index_ready()
        .expect("get readiness after rebuild failed"));

    /*
     * ページ正本が変更されていないことを確認する
     */
    let state = manager
        .get_current_page_state_by_path("/invalid")
        .expect("get invalid page state failed")
        .expect("invalid page state missing");
    assert_eq!(
        state
            .latest_source()
            .expect("invalid latest source missing")
            .source(),
        invalid_source,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// prompt名重複時にprompt派生データと
/// ページ正本を維持することを確認する。
///
/// # 注記
/// latest sourceへ重複名を直接投入し、
/// 再構成失敗前後の状態を比較する。
///
#[test]
fn rebuild_prompt_candidates_preserves_data_on_duplicate_name() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let first_id = manager
        .create_page(
            "/prompts/first",
            "user",
            prompt_source("first", "first description"),
        )
        .expect("create first prompt failed");
    let second_id = manager
        .create_page(
            "/prompts/second",
            "user",
            prompt_source("second", "second description"),
        )
        .expect("create second prompt failed");
    let duplicate_source =
        prompt_source("first", "duplicate description");
    manager
        .replace_latest_page_source_for_prompt_rebuild_test(
            &second_id,
            duplicate_source.clone(),
        )
        .expect("inject duplicate source failed");
    let first_candidate = manager
        .get_prompt_candidate_by_page_id(&first_id)
        .expect("get first candidate before rebuild failed");
    let second_candidate = manager
        .get_prompt_candidate_by_page_id(&second_id)
        .expect("get second candidate before rebuild failed");

    /*
     * 重複エラーが呼び出し元へ返ることを確認する
     */
    let error = manager
        .rebuild_prompt_candidates()
        .expect_err("rebuild must reject duplicate prompt name");
    assert!(error
        .to_string()
        .contains("primitive=prompt name=first"));

    /*
     * 候補と名前索引が更新されていないことを確認する
     */
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&first_id)
            .expect("get first candidate after rebuild failed"),
        first_candidate,
    );
    assert_eq!(
        manager
            .get_prompt_candidate_by_page_id(&second_id)
            .expect("get second candidate after rebuild failed"),
        second_candidate,
    );
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "first",
            )
            .expect("get first owner failed"),
        Some(first_id),
    );
    assert_eq!(
        manager
            .get_mcp_primitive_name_owner_for_test(
                McpPrimitiveKind::Prompt,
                "second",
            )
            .expect("get second owner failed"),
        Some(second_id),
    );
    assert!(manager
        .is_mcp_primitive_name_index_ready()
        .expect("get readiness after rebuild failed"));

    /*
     * 重複状態のページ正本が
     * 変更されていないことを確認する
     */
    let state = manager
        .get_current_page_state_by_path("/prompts/second")
        .expect("get second page state failed")
        .expect("second page state missing");
    assert_eq!(
        state
            .latest_source()
            .expect("second latest source missing")
            .source(),
        duplicate_source,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// legacy template_root 候補を再構成時に fallback 取り込みできることを確認する。
///
#[test]
fn rebuild_template_candidates_with_legacy_imports_fallback_and_prefers_front_matter() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let legacy_id = manager
        .create_page(
            "/templates/legacy-only",
            "user",
            "# legacy only".to_string(),
        )
        .expect("create legacy template failed");
    let front_matter_id = manager
        .create_page(
            "/templates/front-matter",
            "user",
            "---\nwiki:\n  template:\n    name: front matter 優先\n    description: fm\n    macro_expand: true\n---\n# fm".to_string(),
        )
        .expect("create front matter template failed");
    let outside_id = manager
        .create_page(
            "/outside/front-matter",
            "user",
            "---\nwiki:\n  template:\n    name: ルート外\n---\n# outside".to_string(),
        )
        .expect("create outside template failed");

    let rebuilt_count = manager
        .rebuild_template_candidates_with_legacy(Some("/templates"))
        .expect("rebuild with legacy failed");
    assert_eq!(rebuilt_count, 3);

    let legacy_candidate = manager
        .get_template_candidate_by_page_id(&legacy_id)
        .expect("get legacy candidate failed")
        .expect("legacy candidate missing");
    assert_eq!(legacy_candidate.name(), "legacy-only");
    assert_eq!(legacy_candidate.description(), None);
    assert_eq!(legacy_candidate.macro_expand(), None);
    assert_eq!(
        legacy_candidate.source(),
        &crate::database::types::TemplateCandidateSource::LegacyTemplateRoot,
    );

    let front_matter_candidate = manager
        .get_template_candidate_by_page_id(&front_matter_id)
        .expect("get front matter candidate failed")
        .expect("front matter candidate missing");
    assert_eq!(front_matter_candidate.name(), "front matter 優先");
    assert_eq!(front_matter_candidate.description(), Some("fm"));
    assert_eq!(front_matter_candidate.macro_expand(), Some(true));
    assert_eq!(
        front_matter_candidate.source(),
        &crate::database::types::TemplateCandidateSource::FrontMatter,
    );

    let outside_candidate = manager
        .get_template_candidate_by_page_id(&outside_id)
        .expect("get outside candidate failed")
        .expect("outside candidate missing");
    assert_eq!(outside_candidate.name(), "ルート外");
    assert_eq!(
        outside_candidate.source(),
        &crate::database::types::TemplateCandidateSource::FrontMatter,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// legacy 候補は通常同期で自動除去しないことを確認する。
///
#[test]
fn sync_template_candidate_for_page_keeps_legacy_candidate_without_front_matter() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    let page_id = manager
        .create_page("/templates/legacy-only", "user", "# legacy".to_string())
        .expect("create legacy page failed");

    manager
        .rebuild_template_candidates_with_legacy(Some("/templates"))
        .expect("rebuild with legacy failed");

    let synced = manager
        .sync_template_candidate_for_page(&page_id)
        .expect("sync page failed");
    assert!(synced.is_none());

    let candidate = manager
        .get_template_candidate_by_page_id(&page_id)
        .expect("get legacy candidate failed")
        .expect("legacy candidate missing after sync");
    assert_eq!(
        candidate.source(),
        &crate::database::types::TemplateCandidateSource::LegacyTemplateRoot,
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// BearerScope が文字列表現と相互変換できることを
/// 確認する。
///
/// # 注記
/// `read` / `write` / 分解スコープの表示、パース、JSON変換を
/// 検証する。
///
#[test]
fn bearer_scope_converts_to_and_from_strings() {
    /*
     * 文字列表現を検証する
     */
    assert_eq!(BearerScope::Read.as_str(), "read");
    assert_eq!(BearerScope::Write.as_str(), "write");
    assert_eq!(BearerScope::Create.as_str(), "create");
    assert_eq!(BearerScope::Update.as_str(), "update");
    assert_eq!(BearerScope::Append.as_str(), "append");
    assert_eq!(BearerScope::Delete.as_str(), "delete");
    assert_eq!(BearerScope::Read.to_string(), "read");
    assert_eq!(BearerScope::Write.to_string(), "write");
    assert_eq!(BearerScope::Create.to_string(), "create");
    assert_eq!(BearerScope::Update.to_string(), "update");
    assert_eq!(BearerScope::Append.to_string(), "append");
    assert_eq!(BearerScope::Delete.to_string(), "delete");

    /*
     * 文字列からの変換を検証する
     */
    assert_eq!(
        BearerScope::try_from("read").expect("parse read failed"),
        BearerScope::Read,
    );
    assert_eq!(
        BearerScope::try_from("write").expect("parse write failed"),
        BearerScope::Write,
    );
    assert_eq!(
        BearerScope::try_from("create").expect("parse create failed"),
        BearerScope::Create,
    );
    assert_eq!(
        BearerScope::try_from("update").expect("parse update failed"),
        BearerScope::Update,
    );
    assert_eq!(
        BearerScope::try_from("append").expect("parse append failed"),
        BearerScope::Append,
    );
    assert_eq!(
        BearerScope::try_from("delete").expect("parse delete failed"),
        BearerScope::Delete,
    );
    assert!(BearerScope::try_from("admin").is_err());

    /*
     * JSON表現との相互変換を検証する
     */
    assert_eq!(
        serde_json::to_string(&BearerScope::Read)
            .expect("serialize read failed"),
        "\"read\"",
    );
    assert_eq!(
        serde_json::to_string(&BearerScope::Write)
            .expect("serialize write failed"),
        "\"write\"",
    );
    assert_eq!(
        serde_json::to_string(&BearerScope::Create)
            .expect("serialize create failed"),
        "\"create\"",
    );
    assert_eq!(
        serde_json::from_str::<BearerScope>("\"read\"")
            .expect("deserialize read failed"),
        BearerScope::Read,
    );
    assert_eq!(
        serde_json::from_str::<BearerScope>("\"write\"")
            .expect("deserialize write failed"),
        BearerScope::Write,
    );
    assert_eq!(
        serde_json::from_str::<BearerScope>("\"append\"")
            .expect("deserialize append failed"),
        BearerScope::Append,
    );
}

///
/// BearerScopeSet が重複排除と包含判定を正しく
/// 行うことを確認する。
///
/// # 注記
/// 空集合、`read` のみ、`write` のみ、分解スコープ、
/// 全スコープの各ケースで包含関係を検証する。
///
#[test]
fn bearer_scope_set_deduplicates_and_allows_required_scope() {
    /*
     * 空集合ではどの要求スコープも満たさないことを検証する
     */
    let empty_set = BearerScopeSet::new();
    assert!(empty_set.is_empty());
    assert!(!empty_set.allows(BearerScope::Read));
    assert!(!empty_set.allows(BearerScope::Write));
    assert!(!empty_set.allows(BearerScope::Create));
    assert!(!empty_set.allows(BearerScope::Update));
    assert!(!empty_set.allows(BearerScope::Append));
    assert!(!empty_set.allows(BearerScope::Delete));

    /*
     * `read` のみ保持する集合の重複排除と包含判定を検証する
     */
    let mut read_set = BearerScopeSet::new();
    assert!(read_set.insert(BearerScope::Read));
    assert!(!read_set.insert(BearerScope::Read));
    assert_eq!(read_set.len(), 1);
    assert!(read_set.contains(BearerScope::Read));
    assert!(!read_set.contains(BearerScope::Write));
    assert!(read_set.allows(BearerScope::Read));
    assert!(!read_set.allows(BearerScope::Write));
    assert!(!read_set.allows(BearerScope::Create));

    /*
     * `write` のみ保持する集合では `read` / `write`
     * の両要求を満たすことを検証する
     */
    let write_set = BearerScopeSet::from_iter([BearerScope::Write]);
    assert!(!write_set.contains(BearerScope::Read));
    assert!(write_set.contains(BearerScope::Write));
    assert!(write_set.allows(BearerScope::Read));
    assert!(write_set.allows(BearerScope::Write));
    assert!(write_set.allows(BearerScope::Create));
    assert!(write_set.allows(BearerScope::Update));
    assert!(write_set.allows(BearerScope::Append));
    assert!(write_set.allows(BearerScope::Delete));
    assert_eq!(
        write_set.iter().copied().collect::<Vec<_>>(),
        vec![BearerScope::Write],
    );

    /*
     * 分解スコープでは個別要求だけを満たすことを検証する
     */
    let append_set = BearerScopeSet::from_iter([BearerScope::Append]);
    assert!(!append_set.allows(BearerScope::Read));
    assert!(!append_set.allows(BearerScope::Update));
    assert!(append_set.allows(BearerScope::Append));

    /*
     * 全スコープ相当集合では両要求を満たすことを検証する
     */
    let all_set = BearerScopeSet::all();
    assert_eq!(all_set.len(), 6);
    assert!(all_set.contains(BearerScope::Read));
    assert!(all_set.contains(BearerScope::Write));
    assert!(all_set.contains(BearerScope::Create));
    assert!(all_set.contains(BearerScope::Update));
    assert!(all_set.contains(BearerScope::Append));
    assert!(all_set.contains(BearerScope::Delete));
    assert!(all_set.allows(BearerScope::Read));
    assert!(all_set.allows(BearerScope::Write));
    assert!(all_set.allows(BearerScope::Create));
    assert!(all_set.allows(BearerScope::Update));
    assert!(all_set.allows(BearerScope::Append));
    assert!(all_set.allows(BearerScope::Delete));
}

///
/// BearerTokenInfo が設計どおりの管理項目を保持し、
/// 更新できることを確認する。
///
/// # 注記
/// 生成時の初期値、TTL延長、失効更新を検証する。
///
#[test]
fn bearer_token_info_stores_and_updates_management_fields() {
    /*
     * 初期生成内容を検証する
     */
    let user_id = UserId::new();
    let scopes = BearerScopeSet::from_iter([BearerScope::Write]);
    let ttl = chrono::Duration::days(30);
    let mut info = BearerTokenInfo::new(
        user_id.clone(),
        scopes.clone(),
        PathPrefixSet::new(),
        ttl,
        Some("cli token".to_string()),
    );

    assert_eq!(info.user_id(), user_id);
    assert_eq!(info.scopes(), scopes);
    assert!(info.path_prefixes().is_empty());
    assert_eq!(info.ttl(), ttl);
    assert_eq!(info.name(), Some("cli token".to_string()));
    assert!(!info.revoked());
    assert_eq!(info.updated_at(), info.created_at());
    assert_eq!(info.expire_at(), info.created_at() + ttl);

    /*
     * TTL延長更新を検証する
     */
    let renew_at = info.created_at() + chrono::Duration::days(20);
    info.extend_expire_at(renew_at);
    assert_eq!(info.updated_at(), renew_at);
    assert_eq!(info.expire_at(), renew_at + ttl);

    /*
     * 失効更新を検証する
     */
    let revoked_at = renew_at + chrono::Duration::minutes(1);
    info.revoke(revoked_at);
    assert!(info.revoked());
    assert_eq!(info.updated_at(), revoked_at);
    assert!(!info.token_id().to_string().is_empty());
}

///
/// UserAttribute と UserAttributeSet が文字列表現と
/// 集合操作を扱えることを確認する。
///
/// # 注記
/// `NoBasicAuth` と `ReadOnly` の表示、パース、JSON変換、
/// 集合包含判定を検証する。
///
#[test]
fn user_attribute_and_set_work_as_expected() {
    /*
     * 文字列表現とパースを検証する
     */
    assert_eq!(UserAttribute::NoBasicAuth.as_str(), "NoBasicAuth");
    assert_eq!(UserAttribute::NoBasicAuth.to_string(), "NoBasicAuth");
    assert_eq!(
        UserAttribute::try_from("NoBasicAuth")
            .expect("parse NoBasicAuth failed"),
        UserAttribute::NoBasicAuth,
    );
    assert_eq!(UserAttribute::ReadOnly.as_str(), "ReadOnly");
    assert_eq!(UserAttribute::ReadOnly.to_string(), "ReadOnly");
    assert_eq!(
        UserAttribute::try_from("ReadOnly")
            .expect("parse ReadOnly failed"),
        UserAttribute::ReadOnly,
    );
    assert!(UserAttribute::try_from("Unknown").is_err());

    /*
     * JSON表現を検証する
     */
    assert_eq!(
        serde_json::to_string(&UserAttribute::NoBasicAuth)
            .expect("serialize NoBasicAuth failed"),
        "\"NoBasicAuth\"",
    );
    assert_eq!(
        serde_json::from_str::<UserAttribute>("\"NoBasicAuth\"")
            .expect("deserialize NoBasicAuth failed"),
        UserAttribute::NoBasicAuth,
    );
    assert_eq!(
        serde_json::to_string(&UserAttribute::ReadOnly)
            .expect("serialize ReadOnly failed"),
        "\"ReadOnly\"",
    );
    assert_eq!(
        serde_json::from_str::<UserAttribute>("\"ReadOnly\"")
            .expect("deserialize ReadOnly failed"),
        UserAttribute::ReadOnly,
    );

    /*
     * 集合操作を検証する
     */
    let mut attributes = UserAttributeSet::new();
    assert!(attributes.is_empty());
    assert!(attributes.insert(UserAttribute::NoBasicAuth));
    assert!(!attributes.insert(UserAttribute::NoBasicAuth));
    assert!(attributes.contains(UserAttribute::NoBasicAuth));
    assert!(attributes.insert(UserAttribute::ReadOnly));
    assert!(!attributes.insert(UserAttribute::ReadOnly));
    assert!(attributes.contains(UserAttribute::ReadOnly));
}

///
/// UserInfo が属性集合を保持し、`NoBasicAuth` から
/// Basic認証許可判定と write 許可判定を導出できることを確認する。
///
/// # 注記
/// 通常ユーザと `NoBasicAuth` / `ReadOnly` 属性付きユーザの
/// `allows_basic_auth()` と `allows_write()` を検証する。
///
#[test]
fn user_info_tracks_attributes_and_basic_auth_permission() {
    /*
     * 通常ユーザでは Basic認証と write が許可されることを検証する
     */
    let plain_user = UserInfo::new("user", "password", None);
    assert!(plain_user.attributes().is_empty());
    assert!(plain_user.allows_basic_auth());
    assert!(plain_user.allows_write());

    /*
     * `NoBasicAuth` 属性付きユーザでは Basic認証だけが拒否されることを検証する
     */
    let no_basic_auth_user = UserInfo::new_for_test(
        UserId::new(),
        Local::now(),
        "user2",
        "User 2",
        UserAttributeSet::from_iter([UserAttribute::NoBasicAuth]),
    );
    assert!(no_basic_auth_user
        .attributes()
        .contains(UserAttribute::NoBasicAuth));
    assert!(!no_basic_auth_user.allows_basic_auth());
    assert!(no_basic_auth_user.allows_write());

    /*
     * `ReadOnly` 属性付きユーザでは write だけが拒否されることを検証する
     */
    let read_only_user = UserInfo::new_for_test(
        UserId::new(),
        Local::now(),
        "user3",
        "User 3",
        UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
    );
    assert!(read_only_user
        .attributes()
        .contains(UserAttribute::ReadOnly));
    assert!(read_only_user.allows_basic_auth());
    assert!(!read_only_user.allows_write());
}

///
/// 旧形式の BearerTokenInfo から `path_prefixes` 欠落を
/// 読み取り互換で吸収できることを確認する。
///
/// # 注記
/// `path_prefixes` を持たない旧形式を named field の
/// MessagePack として復元し、空集合扱いになることを検証する。
///
#[test]
fn bearer_token_info_deserialize_reads_legacy_without_path_prefixes() {
    #[derive(Serialize)]
    struct LegacyBearerTokenInfo {
        token_id: TokenId,
        user_id: UserId,
        scopes: BearerScopeSet,
        created_at: DateTime<Local>,
        updated_at: DateTime<Local>,
        ttl: chrono::Duration,
        expire_at: DateTime<Local>,
        revoked: bool,
        name: Option<String>,
    }

    /*
     * `path_prefixes` を持たない旧形式を組み立てる
     */
    let now = Local::now();
    let legacy = LegacyBearerTokenInfo {
        token_id: TokenId::new(),
        user_id: UserId::new(),
        scopes: BearerScopeSet::from_iter([
            BearerScope::Read,
            BearerScope::Write,
        ]),
        created_at: now,
        updated_at: now,
        ttl: chrono::Duration::days(30),
        expire_at: now + chrono::Duration::days(30),
        revoked: false,
        name: Some("legacy token".to_string()),
    };

    /*
     * 旧形式 MessagePack から復元できることを検証する
     */
    let bytes = rmp_serde::to_vec_named(&legacy)
        .expect("serialize legacy bearer token failed");
    let info = rmp_serde::from_slice::<BearerTokenInfo>(&bytes)
        .expect("deserialize legacy bearer token failed");

    assert_eq!(info.token_id(), legacy.token_id);
    assert_eq!(info.user_id(), legacy.user_id);
    assert_eq!(info.scopes(), legacy.scopes);
    assert!(info.path_prefixes().is_empty());
    assert_eq!(info.created_at(), legacy.created_at);
    assert_eq!(info.updated_at(), legacy.updated_at);
    assert_eq!(info.ttl(), legacy.ttl);
    assert_eq!(info.expire_at(), legacy.expire_at);
    assert_eq!(info.revoked(), legacy.revoked);
    assert_eq!(info.name(), legacy.name);
}

///
/// 旧形式の UserInfo から `attributes` 欠落を
/// 読み取り互換で吸収できることを確認する。
///
/// # 注記
/// `attributes` を持たない旧形式を named field の
/// MessagePack として復元し、空集合扱いになることを検証する。
///
#[test]
fn user_info_deserialize_reads_legacy_without_attributes() {
    #[derive(Serialize)]
    struct LegacyUserInfo {
        id: UserId,
        username: String,
        password: String,
        salt: [u8; 16],
        display_name: String,
        timestamp: DateTime<Local>,
    }

    /*
     * `attributes` を持たない旧形式を組み立てる
     */
    let legacy = LegacyUserInfo {
        id: UserId::new(),
        username: "legacy-user".to_string(),
        password: "hashed-password".to_string(),
        salt: [7u8; 16],
        display_name: "Legacy User".to_string(),
        timestamp: Local::now(),
    };

    /*
     * 旧形式 MessagePack から復元できることを検証する
     */
    let bytes = rmp_serde::to_vec_named(&legacy)
        .expect("serialize legacy user info failed");
    let info = rmp_serde::from_slice::<UserInfo>(&bytes)
        .expect("deserialize legacy user info failed");

    assert_eq!(info.id(), legacy.id);
    assert_eq!(info.username(), legacy.username);
    assert_eq!(info.password(), legacy.password);
    assert_eq!(info.salt(), legacy.salt);
    assert_eq!(info.display_name(), legacy.display_name);
    assert!(info.attributes().is_empty());
    assert!(info.allows_basic_auth());
    assert_eq!(info.timestamp(), legacy.timestamp);
}

///
/// TokenHash が固定長の照合キーとして扱えることを
/// 確認する。
///
/// # 注記
/// SHA-256 算出、固定長バイト表現、JSON 変換を
/// 検証する。
///
#[test]
fn token_hash_is_fixed_width_sha256_value() {
    /*
     * 同一入力で同一ハッシュになることを検証する
     */
    let hash1 = TokenHash::from_token("token-plain-text");
    let hash2 = TokenHash::from_token("token-plain-text");
    let hash3 = TokenHash::from_token("another-token");
    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);

    /*
     * 固定長表現を検証する
     */
    let bytes = hash1.to_bytes();
    assert_eq!(bytes.len(), 32);
    assert_eq!(TokenHash::fixed_width(), Some(32));
    assert_eq!(TokenHash::from_bytes(&bytes), hash1);

    /*
     * JSON表現との相互変換を検証する
     */
    let json = serde_json::to_string(&hash1)
        .expect("serialize token hash failed");
    assert_eq!(json.len(), 66);
    assert_eq!(
        serde_json::from_str::<TokenHash>(&json)
            .expect("deserialize token hash failed"),
        hash1,
    );
}

///
/// BearerTokenPlaintext が既定では伏字化され、
/// 明示アクセス時だけ平文へ到達できることを確認する。
///
#[test]
fn bearer_token_plaintext_redacts_display_and_debug() {
    let plaintext = BearerTokenPlaintext::new("token-plain-text");

    assert_eq!(plaintext.expose(), "token-plain-text");
    assert_eq!(plaintext.to_string(), "[redacted bearer token]");
    assert_eq!(
        format!("{:?}", plaintext),
        "[redacted bearer token]",
    );
}

///
/// Bearerトークン平文生成と照合用ハッシュ生成が
/// 基本要件を満たすことを確認する。
///
/// # 注記
/// 平文の非空性、Base64文字集合、同一平文の同一ハッシュ、
/// 異なる平文の異なるハッシュを検証する。
///
#[test]
fn bearer_token_plaintext_generation_and_hashing_work() {
    /*
     * テスト用マネージャを生成する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");

    /*
     * Bearerトークン平文生成を検証する
     */
    let plaintext = manager.generate_bearer_token_plaintext();
    let raw = plaintext.expose();
    assert!(!raw.is_empty());
    assert_eq!(raw.len(), 44);
    assert!(raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '/' || ch == '='));

    /*
     * 照合用ハッシュ生成を検証する
     */
    let same_plaintext = BearerTokenPlaintext::new(raw.to_string());
    let different_plaintext = BearerTokenPlaintext::new("different-token");
    let hash1 = DatabaseManager::calculate_bearer_token_hash(&plaintext);
    let hash2 = DatabaseManager::calculate_bearer_token_hash(&same_plaintext);
    let hash3 = DatabaseManager::calculate_bearer_token_hash(&different_plaintext);
    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// DB初期化時に Bearer関連テーブルが作成されることを
/// 確認する。
///
/// # 注記
/// 初期化直後に Bearer 主テーブルと変換テーブルが
/// 開けることを検証する。
///
#[test]
fn init_database_creates_bearer_tables() {
    /*
     * テスト用データベースを初期化する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    init_database(&mut db).expect("init db failed");

    /*
     * Bearer関連テーブルが開けることを検証する
     */
    let txn = db.begin_read().expect("begin read failed");
    let _ = txn
        .open_table(BEARER_TOKEN_TABLE)
        .expect("open bearer token table failed");
    let _ = txn
        .open_table(BEARER_TOKEN_ID_TABLE)
        .expect("open bearer token id table failed");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// Bearerトークン作成時に主テーブルと token_id 変換テーブルへ
/// 整合した内容が登録されることを確認する。
///
/// # 注記
/// 作成直後の `token_id -> token_hash -> BearerTokenInfo` の到達性と、
/// 管理API経由で取得した情報との一致を検証する。
///
#[test]
fn db_bearer_token_create_registers_consistent_main_and_lookup_tables() {
    /*
     * テスト用データベースと対象ユーザを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * Bearerトークンを作成する
     */
    let scopes = BearerScopeSet::from_iter([BearerScope::Write]);
    let ttl = chrono::Duration::hours(12);
    let (plaintext, created_info) = manager
        .create_bearer_token(
            "user",
            scopes.clone(),
            PathPrefixSet::new(),
            ttl,
            Some("db consistency".to_string()),
        )
        .expect("create bearer token failed");
    let token_id = created_info.token_id();
    let expected_hash =
        DatabaseManager::calculate_bearer_token_hash(&plaintext);

    /*
     * 管理API経由で token_id から主テーブル内容へ到達できることを検証する
     */
    let resolved_hash = manager
        .get_bearer_token_hash_by_id(&token_id)
        .expect("get bearer token hash by id failed")
        .expect("bearer token hash not found");
    assert_eq!(resolved_hash, expected_hash);

    let resolved_info = manager
        .get_bearer_token_info_by_id(&token_id)
        .expect("get bearer token info by id failed")
        .expect("bearer token info not found");
    assert_eq!(resolved_info.token_id(), created_info.token_id());
    assert_eq!(resolved_info.user_id(), created_info.user_id());
    assert_eq!(resolved_info.scopes(), created_info.scopes());
    assert_eq!(resolved_info.ttl(), created_info.ttl());
    assert_eq!(resolved_info.name(), created_info.name());
    assert_eq!(resolved_info.created_at(), created_info.created_at());
    assert_eq!(resolved_info.updated_at(), created_info.updated_at());
    assert_eq!(resolved_info.expire_at(), created_info.expire_at());
    assert_eq!(resolved_info.revoked(), created_info.revoked());

    /*
     * 直接テーブル参照でも同一トークンへ到達できることを検証する
     */
    drop(manager);
    let db = Database::create(&db_path).expect("reopen db failed");
    let txn = db.begin_read().expect("begin read failed");
    let token_id_table = txn
        .open_table(BEARER_TOKEN_ID_TABLE)
        .expect("open bearer token id table failed");
    let token_table = txn
        .open_table(BEARER_TOKEN_TABLE)
        .expect("open bearer token table failed");

    let table_hash = token_id_table
        .get(token_id.clone())
        .expect("get token hash from token id table failed")
        .expect("token id table entry not found")
        .value();
    assert_eq!(table_hash, expected_hash);

    let stored_info = token_table
        .get(table_hash)
        .expect("get token info from bearer token table failed")
        .expect("bearer token table entry not found")
        .value();
    assert_eq!(stored_info.token_id(), created_info.token_id());
    assert_eq!(stored_info.user_id(), created_info.user_id());
    assert_eq!(stored_info.scopes(), scopes);
    assert_eq!(stored_info.ttl(), ttl);
    assert_eq!(stored_info.name(), Some("db consistency".to_string()));
    assert_eq!(stored_info.created_at(), created_info.created_at());
    assert_eq!(stored_info.updated_at(), created_info.updated_at());
    assert_eq!(stored_info.expire_at(), created_info.expire_at());
    assert_eq!(stored_info.revoked(), created_info.revoked());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// Bearerトークン失効時に `revoked` と `updated_at` が
/// 正しく更新されることを確認する。
///
/// # 注記
/// 有効トークンを単体失効し、状態遷移と主要管理項目の維持を検証する。
///
#[test]
fn db_bearer_token_revoke_updates_revoked_and_updated_at() {
    /*
     * テスト用データベースと対象ユーザを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * 有効な Bearerトークンを作成する
     */
    let scopes = BearerScopeSet::from_iter([BearerScope::Write]);
    let ttl = chrono::Duration::hours(12);
    let (_, created_info) = manager
        .create_bearer_token(
            "user",
            scopes.clone(),
            PathPrefixSet::new(),
            ttl,
            Some("revoke target".to_string()),
        )
        .expect("create bearer token failed");
    let token_id = created_info.token_id();
    let created_updated_at = created_info.updated_at();
    assert!(!created_info.revoked());

    /*
     * Bearerトークンを失効する
     */
    let revoke_result = manager
        .revoke_bearer_token_by_id(&token_id)
        .expect("revoke bearer token failed");
    assert_eq!(revoke_result.updated_count(), 1);
    assert_eq!(revoke_result.warning_count(), 0);

    /*
     * 失効後の状態遷移を検証する
     */
    let revoked_info = manager
        .get_bearer_token_info_by_id(&token_id)
        .expect("get bearer token info by id failed")
        .expect("revoked bearer token info not found");
    assert_eq!(revoked_info.token_id(), created_info.token_id());
    assert_eq!(revoked_info.user_id(), created_info.user_id());
    assert_eq!(revoked_info.scopes(), scopes);
    assert_eq!(revoked_info.ttl(), ttl);
    assert_eq!(revoked_info.name(), Some("revoke target".to_string()));
    assert_eq!(revoked_info.created_at(), created_info.created_at());
    assert_eq!(revoked_info.expire_at(), created_info.expire_at());
    assert!(revoked_info.revoked());
    assert!(revoked_info.updated_at() > created_updated_at);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// Bearerトークン削除時に主テーブルと token_id 変換テーブルの
/// 両方から整合して削除されることを確認する。
///
/// # 注記
/// 単体削除後に管理APIと直接テーブル参照の双方で残存がないことを検証する。
///
#[test]
fn db_bearer_token_purge_removes_main_and_lookup_tables_consistently() {
    /*
     * テスト用データベースと対象ユーザを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * Bearerトークンを作成する
     */
    let (_, created_info) = manager
        .create_bearer_token(
            "user",
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            chrono::Duration::hours(12),
            Some("purge target".to_string()),
        )
        .expect("create bearer token failed");
    let token_id = created_info.token_id();

    assert!(manager
        .get_bearer_token_hash_by_id(&token_id)
        .expect("get bearer token hash by id failed")
        .is_some());
    assert!(manager
        .get_bearer_token_info_by_id(&token_id)
        .expect("get bearer token info by id failed")
        .is_some());

    /*
     * Bearerトークンを削除する
     */
    let deleted_count = manager
        .purge_bearer_token_by_id(&token_id)
        .expect("purge bearer token failed");
    assert_eq!(deleted_count, 1);

    /*
     * 管理API経由で残存がないことを検証する
     */
    assert!(manager
        .get_bearer_token_hash_by_id(&token_id)
        .expect("get bearer token hash by id after purge failed")
        .is_none());
    assert!(manager
        .get_bearer_token_info_by_id(&token_id)
        .expect("get bearer token info by id after purge failed")
        .is_none());

    /*
     * 直接テーブル参照でも残存がないことを検証する
     */
    drop(manager);
    let db = Database::create(&db_path).expect("reopen db failed");
    let txn = db.begin_read().expect("begin read failed");
    let token_id_table = txn
        .open_table(BEARER_TOKEN_ID_TABLE)
        .expect("open bearer token id table failed");
    let token_table = txn
        .open_table(BEARER_TOKEN_TABLE)
        .expect("open bearer token table failed");

    assert!(token_id_table
        .get(token_id.clone())
        .expect("get token hash from token id table after purge failed")
        .is_none());

    let main_table_count = token_table
        .iter()
        .expect("iterate bearer token table failed")
        .count();
    assert_eq!(main_table_count, 0);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// ユーザ削除時に関連する Bearerトークンが一覧と照合から
/// 消えることを確認する。
///
/// # 注記
/// 削除対象ユーザのトークンのみが連動削除され、他ユーザの
/// トークンは残ることを検証する。
///
#[test]
fn db_bearer_token_delete_user_removes_related_tokens_from_list_and_verify() {
    /*
     * テスト用データベースと複数ユーザを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("alice", "pass", None)
        .expect("add alice failed");
    manager
        .add_user("bob", "pass", None)
        .expect("add bob failed");

    /*
     * 各ユーザに Bearerトークンを発行する
     */
    let (alice_plaintext, alice_info) = manager
        .create_bearer_token(
            "alice",
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            chrono::Duration::hours(12),
            Some("alice token".to_string()),
        )
        .expect("create alice bearer token failed");
    let (bob_plaintext, bob_info) = manager
        .create_bearer_token(
            "bob",
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::new(),
            chrono::Duration::hours(12),
            Some("bob token".to_string()),
        )
        .expect("create bob bearer token failed");

    let listed_before = manager
        .list_bearer_tokens()
        .expect("list bearer tokens before delete failed");
    assert_eq!(listed_before.len(), 2);

    /*
     * 対象ユーザを削除する
     */
    manager
        .delete_user("alice")
        .expect("delete user failed");

    /*
     * 一覧から対象ユーザのトークンだけが消えることを検証する
     */
    let listed_after = manager
        .list_bearer_tokens()
        .expect("list bearer tokens after delete failed");
    assert_eq!(listed_after.len(), 1);
    assert_eq!(listed_after[0].token_id(), bob_info.token_id());
    assert_eq!(listed_after[0].user_id(), bob_info.user_id());

    assert!(manager
        .get_bearer_token_hash_by_id(&alice_info.token_id())
        .expect("get alice token hash after delete failed")
        .is_none());
    assert!(manager
        .get_bearer_token_info_by_id(&alice_info.token_id())
        .expect("get alice token info after delete failed")
        .is_none());

    assert!(manager
        .get_bearer_token_hash_by_id(&bob_info.token_id())
        .expect("get bob token hash after delete failed")
        .is_some());

    /*
     * 照合結果からも対象ユーザのトークンが消えることを検証する
     */
    let alice_verify = manager
        .verify_bearer_token(&alice_plaintext)
        .expect("verify alice token after delete failed");
    assert!(matches!(
        alice_verify,
        Err(VerifyBearerTokenFailureReason::Unissued)
    ));

    let bob_verify = manager
        .verify_bearer_token(&bob_plaintext)
        .expect("verify bob token after delete failed")
        .expect("bob token should still be valid");
    assert_eq!(bob_verify.token_info().token_id(), bob_info.token_id());
    assert_eq!(bob_verify.user_info().id(), bob_info.user_id());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用の一時ディレクトリとDBパスを生成する。
///
/// # 戻り値
/// テスト用ディレクトリとDBファイルパスの
/// タプルを返す。
///
fn prepare_test_dirs() -> (PathBuf, PathBuf) {
    let base = Path::new("tests").join("tmp").join(unique_suffix());
    fs::create_dir_all(&base).expect("create test dir failed");
    let db_path = base.join("database.redb");
    (base, db_path)
}

///
/// ルートページの初期化が正しく行われることを
/// 確認する。
///
/// # 注記
/// ユーザ登録後にルートページ初期化を実行し、
/// ページの存在を検証する。
///
#[test]
fn ensure_default_root_creates_root_page() {
    /*
     * データベースを開き初期化処理を実行する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");

    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    manager
        .ensure_default_root("user")
        .expect("ensure root failed");

    /*
     * 生成されたページとアセットの存在を検証する
     */
    let exists = manager
        .get_page_id_by_path(ROOT_PAGE_PATH)
        .expect("page exists failed")
        .is_some();
    assert!(exists);

    let sandbox_page_id = manager
        .get_page_id_by_path(SANDBOX_PAGE_PATH)
        .expect("sandbox page resolve failed")
        .expect("sandbox page not found");
    let code_asset_exists = manager
        .get_asset_id_by_page_file(
            &sandbox_page_id,
            SANDBOX_SAMPLE_CODE_FILE_NAME,
        )
        .expect("sandbox code asset lookup failed")
        .is_some();
    assert!(code_asset_exists);
    let csv_asset_exists = manager
        .get_asset_id_by_page_file(
            &sandbox_page_id,
            SANDBOX_SAMPLE_CSV_FILE_NAME,
        )
        .expect("sandbox csv asset lookup failed")
        .is_some();
    assert!(csv_asset_exists);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// current path から現在ページ状態を解決できることを
/// 確認する。
///
/// # 注記
/// 通常ページと draft ページを作成し、
/// `get_current_page_state_by_path` の戻り値を検証する。
///
#[test]
fn current_page_state_can_be_resolved_by_path() {
    /*
     * テスト用データベースと対象ユーザを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * 通常ページと draft ページを作成する
     */
    let page_id = manager
        .create_page("/mcp/page", "user", "# page".to_string())
        .expect("create page failed");
    let (draft_page_id, _) = manager
        .create_draft_page("/mcp/draft", "user")
        .expect("create draft page failed");

    /*
     * 通常ページの current path 解決結果を検証する
     */
    let page_state = manager
        .get_current_page_state_by_path("/mcp/page")
        .expect("resolve page state failed")
        .expect("page state missing");
    assert_eq!(page_state.page_id(), page_id);
    assert_eq!(page_state.current_path(), "/mcp/page");
    assert_eq!(page_state.latest_revision(), Some(1));
    assert_eq!(
        page_state
            .latest_source()
            .expect("latest source missing")
            .source(),
        "# page",
    );
    assert!(!page_state.page_index().is_draft());

    /*
     * draft ページの current path 解決結果を検証する
     */
    let draft_state = manager
        .get_current_page_state_by_path("/mcp/draft")
        .expect("resolve draft state failed")
        .expect("draft state missing");
    assert_eq!(draft_state.page_id(), draft_page_id);
    assert_eq!(draft_state.current_path(), "/mcp/draft");
    assert_eq!(draft_state.latest_revision(), None);
    assert!(draft_state.latest_source().is_none());
    assert!(draft_state.page_index().is_draft());

    /*
     * 未存在 path は解決できないことを検証する
     */
    assert!(manager
        .get_current_page_state_by_path("/mcp/missing")
        .expect("resolve missing path failed")
        .is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 複数ページIDの current path 情報を一括取得できることを
/// 確認する。
///
/// # 注記
/// 通常ページと draft ページを作成し、
/// `get_current_page_paths_by_ids` の戻り値を検証する。
///
#[test]
fn current_page_paths_can_be_resolved_by_ids() {
    /*
     * テスト用データベースと対象ユーザを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");

    /*
     * 通常ページと draft ページを作成する
     */
    let page_id = manager
        .create_page("/mcp/page2", "user", "# page2".to_string())
        .expect("create page failed");
    let (draft_page_id, _) = manager
        .create_draft_page("/mcp/draft2", "user")
        .expect("create draft page failed");

    /*
     * 一括解決結果を検証する
     */
    let paths = manager
        .get_current_page_paths_by_ids(&[
            page_id.clone(),
            draft_page_id.clone(),
            PageId::new(),
        ])
        .expect("resolve current paths failed");
    let page_info = paths.get(&page_id).expect("page path info missing");
    assert_eq!(page_info.current_path(), "/mcp/page2");
    assert!(!page_info.deleted());
    assert!(!page_info.draft());

    let draft_info = paths
        .get(&draft_page_id)
        .expect("draft path info missing");
    assert_eq!(draft_info.current_path(), "/mcp/draft2");
    assert!(!draft_info.deleted());
    assert!(draft_info.draft());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// `append_page_by_id` が前提 revision 一致時に
/// 新規 revision を追加できることを確認する。
///
/// # 注記
/// 初回 revision を持つページへ `allow_amend = false` で保存し、
/// 結果 revision と本文を検証する。
///
#[test]
fn append_page_by_id_adds_new_revision_when_latest_matches() {
    /*
     * テスト用データベースと対象ページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page("/mcp/append", "user", "# base".to_string())
        .expect("create page failed");

    /*
     * `append` 用 compare-and-write を実行する
     */
    let request = AppendPageRequest::new(
        page_id.clone(),
        "user".to_string(),
        "# base\nappended".to_string(),
        1,
        false,
    );
    let result = manager
        .append_page_by_id(&request)
        .expect("append page failed");

    /*
     * 新規 revision 追加結果を検証する
     */
    assert_eq!(result.revision(), 2);
    assert!(!result.amended());

    let page_state = manager
        .get_current_page_state_by_path("/mcp/append")
        .expect("resolve page failed")
        .expect("page state missing");
    assert_eq!(page_state.latest_revision(), Some(2));
    assert_eq!(
        page_state
            .latest_source()
            .expect("latest source missing")
            .source(),
        "# base\nappended",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// `append_page_by_id` が同一ユーザかつ amend 許可時に
/// 最新 revision を更新できることを確認する。
///
/// # 注記
/// `allow_amend = true` で保存し、
/// revision が増えずに本文だけ更新されることを検証する。
///
#[test]
fn append_page_by_id_amends_latest_revision_for_same_user() {
    /*
     * テスト用データベースと対象ページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page("/mcp/amend", "user", "# base".to_string())
        .expect("create page failed");

    /*
     * 同一ユーザ amend を実行する
     */
    let request = AppendPageRequest::new(
        page_id.clone(),
        "user".to_string(),
        "# base\namended".to_string(),
        1,
        true,
    );
    let result = manager
        .append_page_by_id(&request)
        .expect("append page failed");

    /*
     * amend 結果を検証する
     */
    assert_eq!(result.revision(), 1);
    assert!(result.amended());
    assert!(
        !manager
            .has_page_source_for_test(&page_id, 2)
            .expect("page source lookup failed")
    );

    let page_state = manager
        .get_current_page_state_by_path("/mcp/amend")
        .expect("resolve page failed")
        .expect("page state missing");
    assert_eq!(page_state.latest_revision(), Some(1));
    assert_eq!(
        page_state
            .latest_source()
            .expect("latest source missing")
            .source(),
        "# base\namended",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// `append_page_by_id` が最新 revision 不一致を
/// 競合として拒否することを確認する。
///
/// # 注記
/// 保存前に別更新で revision を進め、
/// `expected_latest_revision` が古い要求を失敗させる。
///
#[test]
fn append_page_by_id_rejects_revision_conflict() {
    /*
     * テスト用データベースと対象ページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page("/mcp/conflict", "user", "# base".to_string())
        .expect("create page failed");

    /*
     * 先行更新で latest revision を進める
     */
    manager
        .put_page(&page_id, "user", "# base\nv2".to_string(), false)
        .expect("put page failed");

    /*
     * 古い revision 前提の保存が拒否されることを検証する
     */
    let request = AppendPageRequest::new(
        page_id,
        "user".to_string(),
        "# base\nstale".to_string(),
        1,
        false,
    );
    let err = manager
        .append_page_by_id(&request)
        .expect_err("append page should fail");
    assert!(matches!(
        err.downcast_ref::<super::schema::DbError>(),
        Some(super::schema::DbError::RevisionConflict),
    ));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// `append` 競合確認 API が最新 revision・更新者・ロック状態を
/// 返せることを確認する。
///
/// # 注記
/// 通常ページへロックを取得し、
/// `get_append_conflict_state_by_id` の戻り値を検証する。
///
#[test]
fn append_conflict_state_reports_latest_revision_user_and_lock() {
    /*
     * テスト用データベースと対象ページを準備する
     */
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager
        .add_user("user", "pass", None)
        .expect("add user failed");
    let page_id = manager
        .create_page("/mcp/state", "user", "# base".to_string())
        .expect("create page failed");
    manager
        .acquire_page_lock(&page_id, "user")
        .expect("acquire lock failed");

    /*
     * 競合確認結果を検証する
     */
    let conflict_state = manager
        .get_append_conflict_state_by_id(&page_id)
        .expect("get conflict state failed")
        .expect("conflict state missing");
    let page_state = manager
        .get_current_page_state_by_path("/mcp/state")
        .expect("resolve page failed")
        .expect("page state missing");

    assert_eq!(conflict_state.latest_revision(), Some(1));
    assert_eq!(
        conflict_state.latest_user_id(),
        Some(
            page_state
                .latest_source()
                .expect("latest source missing")
                .user(),
        ),
    );
    assert!(conflict_state.locked());
    assert!(!conflict_state.draft());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// 旧形式の MessagePack から rename なしを読み込めることを確認する。
///
#[test]
fn page_source_deserialize_reads_legacy_without_rename() {
    /*
     * 旧形式データを生成して読み込む
     */
    let legacy = LegacyPageSource {
        revision: 3,
        instance_id: None,
        timestamp: Local::now(),
        user: UserId::new(),
        rename: None,
        source: "# test".to_string(),
    };
    let bytes = rmp_serde::to_vec_named(&legacy)
        .expect("serialize legacy page source failed");
    let page_source = <PageSource as Value>::from_bytes(&bytes);

    /*
     * 新形式への変換結果を検証する
     */
    assert_eq!(page_source.revision(), 3);
    assert!(!page_source.rename().is_active());
    assert!(!page_source.rename().is_removed_by_migrate());
}

///
/// 旧形式の MessagePack から rename ありを読み込めることを確認する。
///
#[test]
fn page_source_deserialize_reads_legacy_with_rename() {
    /*
     * 旧形式データを生成して読み込む
     */
    let mut link_refs = std::collections::BTreeMap::new();
    link_refs.insert("/dest".to_string(), Some(PageId::new()));
    let legacy = LegacyPageSource {
        revision: 4,
        instance_id: None,
        timestamp: Local::now(),
        user: UserId::new(),
        rename: Some(LegacyRenameInfo {
            from: Some("/src".to_string()),
            to: "/dest".to_string(),
            link_refs: link_refs.clone(),
        }),
        source: "# rename".to_string(),
    };
    let bytes = rmp_serde::to_vec_named(&legacy)
        .expect("serialize legacy page source failed");
    let page_source = <PageSource as Value>::from_bytes(&bytes);

    /*
     * 新形式への変換結果を検証する
     */
    let rename = page_source.rename();
    assert!(rename.is_active());
    assert_eq!(rename.from().as_deref(), Some("/src"));
    assert_eq!(rename.to().as_deref(), Some("/dest"));
    assert_eq!(
        rename
            .link_refs()
            .expect("active rename must have link refs")
            .len(),
        link_refs.len()
    );
}

///
/// 新形式で失効 rename を保持できることを確認する。
///
#[test]
fn page_source_new_revision_keeps_removed_by_migrate() {
    let page_source = PageSource::new_revision(
        5,
        "# migrate".to_string(),
        UserId::new(),
        RenameInfo::removed_by_migrate(),
    );

    assert!(page_source.rename().is_removed_by_migrate());
    assert!(!page_source.rename().is_active());
}

///
/// export/import 用の低水準 DB API が期待通りに動作することを確認する。
///
#[test]
fn export_import_low_level_db_api_works() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");

    manager
        .add_user("user", "pass", Some("User"))
        .expect("add user failed");
    let page_id = manager
        .create_page(
            "/tree",
            "user",
            prompt_source("migrate", "migrate description"),
        )
        .expect("create page failed");
    manager
        .create_asset(&page_id, "note.txt", "text/plain", "user", b"tree")
        .expect("create asset failed");
    let (draft_id, _) = manager
        .create_draft_page("/tree/draft", "user")
        .expect("create draft failed");
    let resource_page_id = manager
        .create_page(
            "/tree/resource",
            "user",
            resource_source(Some("/docs/migrate-resource"), "migrate"),
        )
        .expect("create resource page failed");

    let read_set = manager
        .collect_export_read_set("/tree", true)
        .expect("collect export read set failed");
    assert_eq!(read_set.pages.len(), 2);
    assert_eq!(read_set.revisions.len(), 2);
    assert_eq!(read_set.assets.len(), 1);
    assert_eq!(read_set.users.len(), 1);
    assert_eq!(read_set.draft_page_ids, vec![draft_id.clone()]);

    let mut lock_page_ids = vec![page_id.clone()];
    lock_page_ids.extend(read_set.draft_page_ids.clone());
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get prompt candidate before migrate failed")
            .is_some()
    );
    manager
        .delete_for_migrate_export(
            "/tree",
            &[
                MigrateExportPageSnapshot {
                    page_id: page_id.clone(),
                    path: "/tree".to_string(),
                    latest: 1,
                },
                MigrateExportPageSnapshot {
                    page_id: resource_page_id.clone(),
                    path: "/tree/resource".to_string(),
                    latest: 1,
                },
            ],
            &read_set.draft_page_ids,
            &lock_page_ids,
        )
        .expect("delete for migrate export failed");

    assert!(
        manager
            .get_page_id_by_path("/tree")
            .expect("get page id failed")
            .is_none()
    );
    assert!(
        manager
            .get_page_id_by_path("/tree/draft")
            .expect("get draft id failed")
            .is_none()
    );
    assert!(
        manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get migrated prompt candidate failed")
            .is_none()
    );
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/migrate-resource"),
        None,
    );
    assert!(
        manager
            .get_resource_candidate_by_page_id(&resource_page_id)
            .expect("get migrated resource candidate failed")
            .is_none()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// import 用の低水準投入 API と staged asset API が期待通りに動作することを確認する。
///
#[test]
fn import_bundle_and_asset_staging_api_work() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");
    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");

    let user_id = UserId::new();
    let page_id = PageId::new();
    let resource_page_id = PageId::new();
    let asset_id = AssetId::new();
    let timestamp = Local::now();
    let staged_path = manager
        .stage_asset_blob(&asset_id, b"hello")
        .expect("stage asset failed");
    assert!(staged_path.exists());
    manager
        .commit_staged_asset_blob(&staged_path, &asset_id)
        .expect("commit staged asset failed");
    assert!(!staged_path.exists());

    let discard_asset_id = AssetId::new();
    let discard_path = manager
        .stage_asset_blob(&discard_asset_id, b"discard")
        .expect("stage discard asset failed");
    manager
        .discard_staged_asset_blob(&discard_path)
        .expect("discard staged asset failed");
    assert!(!discard_path.exists());

    let mut bundle = ExportBundle::new(ManifestContext {
        export_type: ExportType::Backup,
        export_root: "/".to_string(),
        relocate_prefix: None,
    });
    bundle.users.push(ExportUser {
        id: user_id.clone(),
        username: "import-user".to_string(),
        password: "hashed".to_string(),
        salt: [7u8; 16],
        display_name: "Import User".to_string(),
        attributes: UserAttributeSet::new(),
    });
    bundle.pages.push(ExportPage {
        id: page_id.clone(),
        path: "imported".to_string(),
        latest: 1,
        earliest: 1,
        rename_revisions: Some(vec![1]),
    });
    bundle.pages.push(ExportPage {
        id: resource_page_id.clone(),
        path: "imported-resource".to_string(),
        latest: 1,
        earliest: 1,
        rename_revisions: Some(vec![1]),
    });
    bundle.revisions.push(ExportRevision {
        page: page_id.clone(),
        revision: 1,
        timestamp,
        user: user_id.clone(),
        rename: None,
        source: "# imported".to_string(),
    });
    bundle.revisions.push(ExportRevision {
        page: resource_page_id.clone(),
        revision: 1,
        timestamp,
        user: user_id.clone(),
        rename: None,
        source: resource_source(Some("/docs/imported-resource"), "imported"),
    });
    bundle.assets.push(ExportAsset {
        id: asset_id.clone(),
        page: page_id.clone(),
        file_name: "hello.txt".to_string(),
        mime: "text/plain".to_string(),
        size: 5,
        user: user_id.clone(),
        timestamp,
    });
    bundle.sync_manifest_counts();

    manager
        .insert_import_bundle(&bundle)
        .expect("insert import bundle failed");

    let imported_page_id = manager
        .get_page_id_by_path("/imported")
        .expect("resolve imported page failed")
        .expect("imported page missing");
    assert_eq!(imported_page_id, page_id);
    assert!(
        manager
            .get_user_info_by_name("import-user")
            .expect("get user failed")
            .is_some()
    );
    assert!(
        manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .is_some()
    );
    let imported_resource_page_id = manager
        .get_page_id_by_path("/imported-resource")
        .expect("resolve imported resource page failed")
        .expect("imported resource page missing");
    assert_eq!(imported_resource_page_id, resource_page_id);
    let resource_candidate = manager
        .get_resource_candidate_by_page_id(&resource_page_id)
        .expect("get imported resource candidate failed")
        .expect("imported resource candidate missing");
    assert_eq!(resource_candidate.resource_path(), "/docs/imported-resource");
    assert_eq!(resource_candidate.name(), "imported");
    assert_eq!(
        resource_uri_owner_for_test(&manager, "/docs/imported-resource"),
        Some(resource_page_id),
    );
    assert_eq!(
        manager
            .read_asset_data(&asset_id)
            .expect("read asset data failed"),
        b"hello".to_vec()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用の一意なサフィックス文字列を生成する。
///
/// # 戻り値
/// 生成したサフィックス文字列を返す。
///
fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time failed")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{}", pid, now, seq)
}

#[derive(Serialize)]
struct LegacyPageSource {
    revision: u64,
    instance_id: Option<PageId>,
    timestamp: DateTime<Local>,
    user: UserId,
    rename: Option<LegacyRenameInfo>,
    source: String,
}

#[derive(Serialize)]
struct LegacyRenameInfo {
    from: Option<String>,
    to: String,
    link_refs: std::collections::BTreeMap<String, Option<PageId>>,
}
