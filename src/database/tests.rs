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
    PathPrefixSet,
    RenameInfo,
    TokenId,
    TokenHash,
    UserAttribute,
    UserAttributeSet,
    UserId,
    UserInfo,
};
use super::manager::bearer_tokens::VerifyBearerTokenFailureReason;
use super::manager::pages_write::AppendPageRequest;
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
        attributes: UserAttributeSet::new(),
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
