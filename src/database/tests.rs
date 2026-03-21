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
use redb::Value;
use serde::Serialize;

use super::DatabaseManager;
use super::init::init_database;
use super::link_refs::build_link_refs;
use super::schema::{
    BEARER_TOKEN_ID_TABLE,
    BEARER_TOKEN_TABLE,
    PAGE_INDEX_TABLE, PAGE_PATH_TABLE, ROOT_PAGE_PATH, SANDBOX_PAGE_PATH,
    SANDBOX_SAMPLE_CODE_FILE_NAME, SANDBOX_SAMPLE_CSV_FILE_NAME,
};
use super::types::{
    AssetId,
    BearerScope,
    BearerScopeSet,
    BearerTokenInfo,
    BearerTokenPlaintext,
    PageId,
    PageIndex,
    PageSource,
    RenameInfo,
    TokenHash,
    UserId,
};
use super::manager::bearer_tokens::VerifyBearerTokenFailureReason;
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
/// BearerScope が文字列表現と相互変換できることを
/// 確認する。
///
/// # 注記
/// `read` / `write` の表示、パース、JSON変換を
/// 検証する。
///
#[test]
fn bearer_scope_converts_to_and_from_strings() {
    /*
     * 文字列表現を検証する
     */
    assert_eq!(BearerScope::Read.as_str(), "read");
    assert_eq!(BearerScope::Write.as_str(), "write");
    assert_eq!(BearerScope::Read.to_string(), "read");
    assert_eq!(BearerScope::Write.to_string(), "write");

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
        serde_json::from_str::<BearerScope>("\"read\"")
            .expect("deserialize read failed"),
        BearerScope::Read,
    );
    assert_eq!(
        serde_json::from_str::<BearerScope>("\"write\"")
            .expect("deserialize write failed"),
        BearerScope::Write,
    );
}

///
/// BearerScopeSet が重複排除と包含判定を正しく
/// 行うことを確認する。
///
/// # 注記
/// 空集合、`read` のみ、`write` のみ、全スコープ
/// の各ケースで `read` / `write` の包含関係を検証する。
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

    /*
     * `write` のみ保持する集合では `read` / `write`
     * の両要求を満たすことを検証する
     */
    let write_set = BearerScopeSet::from_iter([BearerScope::Write]);
    assert!(!write_set.contains(BearerScope::Read));
    assert!(write_set.contains(BearerScope::Write));
    assert!(write_set.allows(BearerScope::Read));
    assert!(write_set.allows(BearerScope::Write));
    assert_eq!(
        write_set.iter().copied().collect::<Vec<_>>(),
        vec![BearerScope::Write],
    );

    /*
     * 全スコープ相当集合では両要求を満たすことを検証する
     */
    let all_set = BearerScopeSet::all();
    assert_eq!(all_set.len(), 2);
    assert!(all_set.contains(BearerScope::Read));
    assert!(all_set.contains(BearerScope::Write));
    assert!(all_set.allows(BearerScope::Read));
    assert!(all_set.allows(BearerScope::Write));
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
        ttl,
        Some("cli token".to_string()),
    );

    assert_eq!(info.user_id(), user_id);
    assert_eq!(info.scopes(), scopes);
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
            chrono::Duration::hours(12),
            Some("alice token".to_string()),
        )
        .expect("create alice bearer token failed");
    let (bob_plaintext, bob_info) = manager
        .create_bearer_token(
            "bob",
            BearerScopeSet::from_iter([BearerScope::Write]),
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
