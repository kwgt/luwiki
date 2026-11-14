/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! テキストアセット入力と正規保存の検証
//!

use std::borrow::Cow;

use super::*;

///
/// MIME・charset・排他とUTF-8 byte容量を検証する
/// # 概要
/// MIME・charset・排他とUTF-8 byte容量を検証する
///
///
/// # 注記
/// textの借用、空文字、Unicode境界とBase64の既存許可を確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn text_asset_validation_and_byte_boundaries() {
    let service = McpService::new();

    /*
     * MIMEとcharsetの正常・不正を確認する
     */
    for mime in ["text/plain", "Text/Markdown; charset=\"UTF-8\"",
        "text/csv; header=present", "text/plain; charset=utf-8"]
    {
        let result = service.asset_input_bytes(mime, Some(""), None).unwrap();
        assert!(matches!(result, Cow::Borrowed(_)));
        assert!(result.is_empty());
    }
    for mime in ["image/png", "application/json", "text/*", "text/plain+*",
        "text/plain; charset=utf8", "text/plain; charset=shift_jis",
        "text/plain; charset=utf-8; charset=utf-8",
        " text/plain", "text/plain ", "text/plain\r\n", "broken"]
    {
        assert_eq!(service.asset_input_bytes(mime, Some("secret"), None)
            .unwrap_err().code(), McpErrorCode::InvalidInput);
    }
    for (text, base64) in [(None, None), (Some(""), Some("")),
        (Some("private-sentinel"), Some("invalid"))]
    {
        let error = service.asset_input_bytes("text/plain", text, base64)
            .unwrap_err();
        assert_eq!(error.message(),
            "exactly one of text or data_base64 is required");
    }
    assert_eq!(service.asset_input_bytes("arbitrary", None, Some("QQ=="))
        .unwrap().as_ref(), b"A");

    /*
     * ASCIIと日本語で上限直前・一致・超過を確認する
     */
    for unicode in [false, true] {
        let mut text = if unicode {
            "あ".repeat(3_495_253)
        } else {
            "A".repeat(MAX_MCP_ASSET_DATA_SIZE as usize - 1)
        };
        for extra in 0..3 {
            let result = service.asset_input_bytes(
                "text/plain", Some(&text), None,
            );
            if extra < 2 {
                assert_eq!(result.unwrap().as_ref(), text.as_bytes());
            } else {
                assert_eq!(result.unwrap_err().message(),
                    "asset data too large");
            }
            text.push('A');
        }
    }
}

///
/// 直接入力の保存bytes・MIME・空file・同名と認可優先を検証する
/// # 概要
/// 直接入力の保存bytes・MIME・空file・同名と認可優先を検証する
///
///
/// # 注記
/// 共通保存を通り、拒否時のfile数と既存bytesが変わらないことを確認。
///
/// # 戻り値
/// なし。
///
#[test]
fn text_asset_storage_and_rejection_boundaries() {
    let (base, manager) = open_test_manager();
    manager.add_user("alice", "pass", None).unwrap();
    let page = manager.create_page("/mcp/assets", "alice", "# page".into())
        .unwrap();
    let auth = AuthContext::new(AuthUser::new("alice".into()),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/mcp"]), None);
    let service = McpService::new();

    /*
     * Unicode・BOM・改行・空文字をそのまま正規保存する
     */
    for (name, mime, text) in [
        ("plain.txt", "text/plain", "hello"),
        ("test.md", "Text/Markdown; charset=\"UTF-8\"",
            "\u{feff} 日本語 e\u{301} 😀\r\n\n\r\0 "),
        ("test.csv", "text/csv; header=present", "a,b\n1,2\n"),
        ("empty.txt", "text/plain", ""),
    ] {
        let result = service.upload_page_asset(&auth, &manager,
            "/mcp/assets", name, mime, Some(text), None).unwrap();
        let id = AssetId::from_string(result.asset().asset_id()).unwrap();
        assert_eq!(manager.read_asset_data(&id).unwrap(), text.as_bytes());
        assert_eq!(result.asset().mime_type(), mime);
        assert_eq!(result.asset().size(), text.len() as u64);
        assert_eq!(result.summary(), "asset uploaded");
    }

    /*
     * 同名拒否・削除済み同名再作成と認可を確認する
     */
    let files = count_test_files(&base.join("assets"));
    let conflict = service.upload_page_asset(&auth, &manager,
        "/mcp/assets", "plain.txt", "text/plain", Some("changed"), None)
        .unwrap_err();
    assert_eq!(conflict.code(), McpErrorCode::Conflict);
    assert_eq!(count_test_files(&base.join("assets")), files);
    let old = manager.get_asset_id_by_page_file(&page, "plain.txt")
        .unwrap().unwrap();
    manager.delete_asset(&old).unwrap();
    let result = service.upload_page_asset(&auth, &manager,
        "/mcp/assets", "plain.txt", "text/plain", Some("new"), None).unwrap();
    assert_ne!(result.asset().asset_id(), old.to_string());
    let read = AuthContext::new(AuthUser::new("alice".into()),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/mcp"]), None);
    let error = service.upload_page_asset(&read, &manager,
        "/mcp/assets", "denied", "image/png", Some("secret"), Some("bad"))
        .unwrap_err();
    assert_eq!(error.code(), McpErrorCode::Forbidden);
    assert_eq!(manager.read_asset_data(&old).unwrap(), b"hello");
    fs::remove_dir_all(base).unwrap();
}

///
/// text入力の保存失敗・lockとエラー優先度を検証する
/// # 概要
/// text入力の保存失敗・lockとエラー優先度を検証する
///
///
/// # 注記
/// 登録/commit試行前/回収失敗hookと、有効lock前の入力検証を確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn text_asset_failure_cleanup_and_lock_priority() {
    /*
     * 保存失敗でもlookupを公開せず、回収可能なfileを残さない
     */
    for fault in [1, 2, 3] {
        let (base, manager) = open_test_manager();
        manager.add_user("alice", "pass", None).unwrap();
        let page = manager.create_page("/mcp/page", "alice", "# x".into())
            .unwrap();
        let auth = AuthContext::new(AuthUser::new("alice".into()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]), None);
        manager.set_asset_upload_failure_for_test(fault);
        let error = McpService::new().upload_page_asset(&auth, &manager,
            "/mcp/page", "fault.txt", "text/plain",
            Some("PRIVATE_TEXT_SENTINEL"), None).unwrap_err();
        assert_eq!(error.code(), McpErrorCode::InternalError);
        assert_eq!(error.message(), "asset create failed");
        assert!(manager.get_asset_id_by_page_file(&page, "fault.txt")
            .unwrap().is_none());
        assert_eq!(count_test_files(&base.join("assets")), 0);
        manager.set_asset_upload_failure_for_test(0);
        fs::remove_dir_all(base).unwrap();
    }

    /*
     * lockは入力検証後にtxnで拒否する
     */
    let (base, manager) = open_test_manager();
    manager.add_user("alice", "pass", None).unwrap();
    let page = manager.create_page("/mcp/page", "alice", "# x".into())
        .unwrap();
    manager.acquire_page_lock(&page, "alice").unwrap();
    let auth = AuthContext::new(AuthUser::new("alice".into()),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/mcp"]), None);
    let service = McpService::new();
    let locked = service.upload_page_asset(&auth, &manager,
        "/mcp/page", "lock.txt", "text/plain", Some("hello"), None)
        .unwrap_err();
    assert_eq!(locked.message(), "page is locked");
    let invalid = service.upload_page_asset(&auth, &manager,
        "/mcp/page", "lock.txt", "image/png", Some("hello"), None)
        .unwrap_err();
    assert_eq!(invalid.code(), McpErrorCode::InvalidInput);
    let large = "A".repeat(MAX_MCP_ASSET_DATA_SIZE as usize + 1);
    let invalid = service.upload_page_asset(&auth, &manager,
        "/mcp/page", "lock.txt", "text/plain", Some(&large), None)
        .unwrap_err();
    assert_eq!(invalid.message(), "asset data too large");
    assert_eq!(count_test_files(&base.join("assets")), 0);
    fs::remove_dir_all(base).unwrap();
}

///
/// text経路でも認可・ページ状態の拒否が本文検証より先になる
/// # 概要
/// text経路でも認可・ページ状態の拒否が本文検証より先になる
///
///
/// # 注記
/// prefix/ReadOnly/不存在/削除/draftとfile_name/MIMEの優先を確認する。
///
/// # 戻り値
/// なし。
///
#[test]
fn text_asset_authorization_and_page_states() {
    let (base, manager) = open_test_manager();
    manager.add_user("alice", "pass", None).unwrap();
    manager.create_page("/mcp/page", "alice", "# x".into()).unwrap();
    manager.create_draft_page("/mcp/draft", "alice").unwrap();
    let deleted = manager.create_page("/mcp/deleted", "alice", "x".into())
        .unwrap();
    manager.delete_page_by_id(&deleted).unwrap();
    let service = McpService::new();
    let auth = AuthContext::new(AuthUser::new("alice".into()),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/mcp"]), None);

    /*
     * ページ状態が不正ならMIMEや排他エラーを返さない
     */
    for (path, code) in [("/mcp/missing", McpErrorCode::NotFound),
        ("/mcp/deleted", McpErrorCode::NotFound),
        ("/mcp/draft", McpErrorCode::Conflict),
        ("/outside", McpErrorCode::Forbidden)]
    {
        let error = service.upload_page_asset(&auth, &manager, path,
            "test", "image/png", Some("secret"), Some("bad")).unwrap_err();
        assert_eq!(error.code(), code);
    }
    let read_only = AuthContext::new_with_attributes(
        AuthUser::new("alice".into()),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/mcp"]),
        UserAttributeSet::from_iter([UserAttribute::ReadOnly]), None, None,
    );
    assert_eq!(service.upload_page_asset(&read_only, &manager,
        "/mcp/page", "test", "text/plain", Some("x"), None)
        .unwrap_err().code(), McpErrorCode::Forbidden);

    /*
     * 共通入力は本文排他より先に検証する
     */
    for (file, mime, message) in [("a/b", "text/plain", "file_name is invalid"),
        ("a", "", "mime_type is invalid")]
    {
        let error = service.upload_page_asset(&auth, &manager,
            "/mcp/page", file, mime, Some("x"), Some("QQ==")).unwrap_err();
        assert_eq!(error.message(), message);
    }
    assert_eq!(count_test_files(&base.join("assets")), 0);
    fs::remove_dir_all(base).unwrap();
}
