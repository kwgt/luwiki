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
use redb::Value;
use serde::Serialize;

use super::DatabaseManager;
use super::init::init_database;
use super::link_refs::build_link_refs;
use super::schema::{
    PAGE_INDEX_TABLE, PAGE_PATH_TABLE, ROOT_PAGE_PATH, SANDBOX_PAGE_PATH,
    SANDBOX_SAMPLE_CODE_FILE_NAME, SANDBOX_SAMPLE_CSV_FILE_NAME,
};
use super::types::{AssetId, PageId, PageIndex, PageSource, RenameInfo, UserId};
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
        .create_page("/tree", "user", "# tree".to_string())
        .expect("create page failed");
    manager
        .create_asset(&page_id, "note.txt", "text/plain", "user", b"tree")
        .expect("create asset failed");
    let (draft_id, _) = manager
        .create_draft_page("/tree/draft", "user")
        .expect("create draft failed");

    let read_set = manager
        .collect_export_read_set("/tree", true)
        .expect("collect export read set failed");
    assert_eq!(read_set.pages.len(), 1);
    assert_eq!(read_set.revisions.len(), 1);
    assert_eq!(read_set.assets.len(), 1);
    assert_eq!(read_set.users.len(), 1);
    assert_eq!(read_set.draft_page_ids, vec![draft_id.clone()]);

    let mut lock_page_ids = vec![page_id.clone()];
    lock_page_ids.extend(read_set.draft_page_ids.clone());
    manager
        .delete_for_migrate_export(
            "/tree",
            &[MigrateExportPageSnapshot {
                page_id: page_id.clone(),
                path: "/tree".to_string(),
                latest: 1,
            }],
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
    });
    bundle.pages.push(ExportPage {
        id: page_id.clone(),
        path: "imported".to_string(),
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
