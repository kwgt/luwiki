/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 一括編集の正規保存・競合・commit後失敗を検証する。
//!

use super::*;

///
/// 同期失敗も隔離できる編集fixtureを生成する。
///
/// # 引数
/// * `source` - 初期本文
///
/// # 戻り値
/// 一時領域、DB、対象ID、認証文脈。
///
fn fixture(source: &str)
    -> (tempfile::TempDir, DatabaseManager, PageId, AuthContext)
{
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open(&dir.path().join("wiki.redb"),
        &dir.path().join("assets")).unwrap();
    db.add_user("user", "pass", None).unwrap();
    let page = db.create_page("/batch/page", "user", source.into()).unwrap();
    let auth = AuthContext::new(AuthUser::new("user".into()),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/batch"]), None);
    (dir, db, page, auth)
}

///
/// 最新版の競合トークンから一括要求を生成する。
///
/// # 引数
/// * `db` - DB
/// * `page` - 対象ID
/// * `operations` - 操作列
///
/// # 戻り値
/// 最新版を参照する要求。
///
fn request(db: &DatabaseManager, page: &PageId,
    operations: Vec<EditPageOperation>) -> EditPageRequest
{
    let revision = db.get_page_index_by_id(page).unwrap().unwrap().latest();
    let source = db.get_page_source(page, revision).unwrap().unwrap();
    EditPageRequest::new("/batch/page".into(), revision,
        source.instance_id().unwrap().to_string(), operations)
}

///
/// 文字列置換を作る。
///
/// # 引数
/// * `old` - 検索文字列
/// * `new` - 置換文字列
///
/// # 戻り値
/// 一致の最初を置換する操作。
///
fn replace(old: &str, new: &str) -> EditPageOperation {
    EditPageOperation::ReplaceText { old_text: old.into(), new_text: new.into(),
        occurrence: None }
}

///
/// T02/T15: 複数変換の結果を一回保存し、同じ本文でもrevisionを増やす。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 元へ戻す2操作と空Vec防御を同じ保存入口で確認する。
///
#[test]
fn batch_saves_once_and_empty_service_input_does_not_save() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let (_dir, db, page, auth) = fixture("original");
    let service = McpService::new();
    let empty = request(&db, &page, vec![]);
    assert_eq!(service.edit_page(&auth, &db, &empty).unwrap_err().code(),
        McpErrorCode::InvalidInput);
    assert_eq!(db.get_page_index_by_id(&page).unwrap().unwrap().latest(), 1);
    let input = request(&db, &page, vec![
        replace("original", "middle"), replace("middle", "original"),
    ]);
    let result = service.edit_page(&auth, &db, &input).unwrap();
    assert_eq!(result.revision(), 2);
    assert_eq!(result.summary(), "page edited (2 operations)");
    assert_ne!(result.instance_id(), input.instance_id());
    assert_eq!(db.get_page_source(&page, 2).unwrap().unwrap().source(),
        "original");
}

///
/// T12/T15: 途中失敗と最終front matter不正を保存しない。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 成功した先頭操作も含め、本文・revision・instance_idを比較する。
///
#[test]
fn batch_intermediate_and_final_validation_failures_are_atomic() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let (_dir, db, page, auth) = fixture("original");
    let service = McpService::new();
    let input = request(&db, &page, vec![
        replace("original", "middle"), replace("missing", "wrong"),
    ]);
    let error = service.edit_page(&auth, &db, &input).unwrap_err();
    assert!(error.message().starts_with("operation 2 failed:"));
    let invalid = request(&db, &page, vec![replace("original",
        "---\nmcp: []\n---\nbroken")]);
    assert!(service.edit_page(&auth, &db, &invalid).is_err());
    let source = db.get_page_source(&page, 1).unwrap().unwrap();
    assert_eq!(source.source(), "original");
    assert_eq!(source.instance_id().unwrap().to_string(), input.instance_id());
    assert_eq!(db.get_page_index_by_id(&page).unwrap().unwrap().latest(), 1);
}

///
/// T15: 最終primitive名の競合を一括編集でも正本保存前に拒否する。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 他ページのprompt名を最終本文に指定し、元本文の保持を確認する。
///
#[test]
fn batch_final_primitive_conflict_does_not_save() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let (_dir, db, page, auth) = fixture("original");
    let prompt = concat!("---\nmcp:\n  primitive: prompt\n",
        "  name: duplicate\n  description: test\n---\nbody");
    db.create_page("/batch/other", "user", prompt.into()).unwrap();
    let input = request(&db, &page, vec![replace("original", prompt)]);
    assert!(McpService::new().edit_page(&auth, &db, &input).is_err());
    assert_eq!(db.get_page_index_by_id(&page).unwrap().unwrap().latest(), 1);
    assert_eq!(db.get_page_source(&page, 1).unwrap().unwrap().source(),
        "original");
}

///
/// T14: 変換後から保存前への通常更新とamendをDB内のCASが拒否する。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 同じ期待トークンで最終保存helperへ進め、他者の最新値を保持する。
///
#[test]
fn batch_save_rechecks_revision_and_instance_after_intervening_write() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    for amend in [false, true] {
        let (_dir, db, page, auth) = fixture("original");
        let service = McpService::new();
        let input = request(&db, &page, vec![replace("original", "batch")]);
        let resolved = service.resolve_page_by_path(&db, input.path()).unwrap();
        let updated = service.apply_edit_page_operations("original",
            input.operation()).unwrap();
        db.put_page(&page, "user", "intervening".into(), amend).unwrap();
        let error = service.save_updated_page(&auth, &db, &resolved, "user",
            updated, Some((input.revision(), input.instance_id())))
            .unwrap_err();
        let expected = if amend { McpErrorCode::InstanceIdNotMatch }
            else { McpErrorCode::NotLatestRevision };
        assert_eq!(error.code(), expected);
        let latest = db.get_page_index_by_id(&page).unwrap().unwrap().latest();
        assert_eq!(db.get_page_source(&page, latest).unwrap().unwrap().source(),
            "intervening");
    }
}

///
/// T16: commit後の候補同期失敗でも正本と確定revisionは保存済み。
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 初期状態から操作列を実行し、対象・保存結果・失敗分類を照合する。
///
/// # 注記
/// 既存の隔離faultを使い、公開エラーと再取得結果を比較する。
///
#[test]
fn batch_post_commit_failure_preserves_saved_source() {
    /*
     * 初期条件と操作列から期待結果を照合する
     */
    let (_dir, db, page, auth) = fixture("original");
    let service = McpService::new();
    let input = request(&db, &page, vec![replace("original", "saved")]);
    db.set_resource_candidate_sync_failure_for_test(true);
    let error = service.edit_page(&auth, &db, &input).unwrap_err();
    assert_eq!(error.code(), McpErrorCode::InternalError);
    assert!(!error.message().starts_with("operation "));
    let source = db.get_page_source(&page, 2).unwrap().unwrap();
    assert_eq!(source.source(), "saved");
    assert_ne!(source.instance_id().unwrap().to_string(), input.instance_id());
    db.set_resource_candidate_sync_failure_for_test(false);
}
