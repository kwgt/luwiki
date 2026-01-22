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

use redb::Database;

use super::DatabaseManager;
use super::init::init_database;
use super::link_refs::build_link_refs;
use super::schema::{PAGE_INDEX_TABLE, PAGE_PATH_TABLE, ROOT_PAGE_PATH};
use super::types::{PageId, PageIndex};

///
/// Wikiリンク抽出処理が期待通りに動作することを確認する。
///
/// # 注記
/// テスト用データベースを作成し、固定ソースからリンク参照を抽出して検証する。
///
#[test]
fn build_link_refs_extracts_wiki_links() {
    let (base_dir, db_path) = prepare_test_dirs();
    let mut db = Database::create(&db_path).expect("create db failed");
    init_database(&mut db).expect("init db failed");

    let txn = db.begin_write().expect("begin write failed");
    {
        let mut path_table = txn.open_table(PAGE_PATH_TABLE)
            .expect("open table failed");
        let mut index_table = txn.open_table(PAGE_INDEX_TABLE)
            .expect("open table failed");
        let id_root = PageId::new();
        let id_page = PageId::new();
        path_table.insert(
            "/a".to_string(),
            id_root.clone(),
        ).expect("insert /a failed");
        path_table.insert(
            "/a/b".to_string(),
            id_page.clone(),
        ).expect("insert /a/b failed");
        index_table.insert(
            id_root.clone(),
            PageIndex::new_page(id_root.clone(), "/a".to_string()),
        ).expect("insert /a index failed");
        index_table.insert(
            id_page.clone(),
            PageIndex::new_page(id_page.clone(), "/a/b".to_string()),
        ).expect("insert /a/b index failed");
    }

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
/// テスト用ディレクトリとDBファイルパスのタプルを返す。
///
fn prepare_test_dirs() -> (PathBuf, PathBuf) {
    let base = Path::new("tests").join("tmp").join(unique_suffix());
    fs::create_dir_all(&base).expect("create test dir failed");
    let db_path = base.join("database.redb");
    (base, db_path)
}

///
/// ルートページの初期化が正しく行われることを確認する。
///
/// # 注記
/// ユーザ登録後にルートページ初期化を実行し、ページの存在を検証する。
///
#[test]
fn ensure_default_root_creates_root_page() {
    let (base_dir, db_path) = prepare_test_dirs();
    let asset_path = base_dir.join("assets");

    let manager = DatabaseManager::open(&db_path, &asset_path)
        .expect("open manager failed");
    manager.add_user("user", "pass", None)
        .expect("add user failed");
    manager.ensure_default_root("user")
        .expect("ensure root failed");

    let exists = manager.get_page_id_by_path(ROOT_PAGE_PATH)
        .expect("page exists failed")
        .is_some();
    assert!(exists);

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
