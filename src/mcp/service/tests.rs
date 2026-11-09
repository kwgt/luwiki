/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP service tests
//!

    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::auth::{AuthContext, AuthUser};
    use crate::database::DatabaseManager;
    use crate::database::types::{
        BearerScopeSet,
        PathPrefixSet,
        UserAttribute,
        UserAttributeSet,
    };
    use crate::fts::{FtsDocument, FtsIndexConfig, extract_markdown_sections};

    use super::*;

    ///
    /// MCP サービス層テスト用の DB を生成する
    ///
    /// # 戻り値
    /// `(一時ディレクトリ, DatabaseManager)` を返す。
    ///
    fn open_test_manager() -> (PathBuf, DatabaseManager) {
        let base_dir = std::env::temp_dir()
            .join(format!("luwiki-mcp-service-{}", ulid::Ulid::new()));
        let asset_dir = base_dir.join("assets");
        let db_path = base_dir.join("test.redb");

        fs::create_dir_all(&asset_dir).expect("create asset dir failed");

        let manager = DatabaseManager::open(&db_path, &asset_dir)
            .expect("open manager failed");

        (base_dir, manager)
    }

    ///
    /// テストディレクトリ配下のファイル数を数える
    ///
    /// # 引数
    /// * `path` - 集計対象ディレクトリ
    ///
    /// # 戻り値
    /// 子ディレクトリを含むファイル数を返す。
    ///
    fn count_test_files(path: &Path) -> usize {
        fs::read_dir(path)
            .expect("read test directory failed")
            .map(|entry| {
                let child = entry.expect("read test entry failed").path();
                if child.is_dir() {
                    count_test_files(&child)
                } else {
                    1
                }
            })
            .sum()
    }

    ///
    /// MCP サービス層テスト用の FTS 設定を生成する
    ///
    /// # 引数
    /// * `base_dir` - テスト基底ディレクトリ
    ///
    /// # 戻り値
    /// FTS 設定を返す。
    ///
    fn open_test_fts_config(base_dir: &PathBuf) -> FtsIndexConfig {
        FtsIndexConfig::new(base_dir.join("fts"))
    }

    ///
    /// 指定ページの FTS 文書を再構築する
    ///
    /// # 引数
    /// * `manager` - データベースマネージャ
    /// * `fts_config` - FTS 設定
    /// * `path` - 対象 current path
    ///
    /// # 戻り値
    /// なし
    ///
    fn rebuild_test_fts_page(
        manager: &DatabaseManager,
        fts_config: &FtsIndexConfig,
        path: &str,
    ) {
        let page_id = manager
            .get_page_id_by_path(path)
            .expect("resolve page id failed")
            .expect("page id missing");
        fts::update_pages_index(fts_config, manager, &[page_id], false)
            .expect("update index failed");
    }

    #[test]
    fn operation_scope_follows_required_scope_table() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::new(),
            None,
        );

        assert!(service
            .ensure_operation_scope(&auth, McpOperation::GetPage)
            .is_ok());
        assert_eq!(
            McpOperation::ListPrompts.required_scope(),
            BearerScope::Read,
        );
        assert!(!McpOperation::ListPrompts.is_write());
        assert_eq!(
            McpOperation::ListResources.required_scope(),
            BearerScope::Read,
        );
        assert!(!McpOperation::ListResources.is_write());
        assert_eq!(
            McpOperation::GetPrompt.required_scope(),
            BearerScope::Read,
        );
        assert!(!McpOperation::GetPrompt.is_write());
        for operation in [
            McpOperation::ListPageAssets,
            McpOperation::ResolvePageAsset,
            McpOperation::GetPageAssetMetadata,
            McpOperation::GetPageAssetData,
        ] {
            assert_eq!(operation.required_scope(), BearerScope::Read);
            assert!(!operation.is_write());
            assert!(service.ensure_operation_scope(&auth, operation).is_ok());
        }
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::CreatePage)
            .is_ok());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::UpdatePage)
            .is_ok());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::AppendPage)
            .is_ok());
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::DeletePage)
            .is_ok());
        assert_eq!(
            McpOperation::UploadPageAsset.required_scope(),
            BearerScope::Update,
        );
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::UploadPageAsset)
            .is_ok());
        assert_eq!(
            McpOperation::DeletePageAsset.required_scope(),
            BearerScope::Delete,
        );
        assert!(service
            .ensure_operation_scope(&auth, McpOperation::DeletePageAsset)
            .is_ok());
    }

    ///
    /// 更新系アセット操作のscope行列を検証する
    ///
    /// # 注記
    /// - uploadはupdate、deleteはdeleteで許可する。
    /// - legacy writeは両操作を許可する。
    /// - read、create、appendと不一致の分解scopeは拒否する。
    ///
    #[test]
    fn update_asset_operations_honor_scope_matrix() {
        let service = McpService::new();

        /*
         * 許可scopeの検証
         */
        for (scope, operation) in [
            (BearerScope::Update, McpOperation::UploadPageAsset),
            (BearerScope::Delete, McpOperation::DeletePageAsset),
            (BearerScope::Write, McpOperation::UploadPageAsset),
            (BearerScope::Write, McpOperation::DeletePageAsset),
        ] {
            let auth = AuthContext::new(
                AuthUser::new("alice".to_string()),
                BearerScopeSet::from_iter([scope]),
                PathPrefixSet::new(),
                None,
            );

            service
                .ensure_operation_scope(&auth, operation)
                .expect("asset operation scope must be allowed");
        }

        /*
         * 不許可scopeの検証
         */
        for (scope, operation, required) in [
            (
                BearerScope::Read,
                McpOperation::UploadPageAsset,
                "required scope denied: update",
            ),
            (
                BearerScope::Create,
                McpOperation::UploadPageAsset,
                "required scope denied: update",
            ),
            (
                BearerScope::Append,
                McpOperation::UploadPageAsset,
                "required scope denied: update",
            ),
            (
                BearerScope::Delete,
                McpOperation::UploadPageAsset,
                "required scope denied: update",
            ),
            (
                BearerScope::Read,
                McpOperation::DeletePageAsset,
                "required scope denied: delete",
            ),
            (
                BearerScope::Create,
                McpOperation::DeletePageAsset,
                "required scope denied: delete",
            ),
            (
                BearerScope::Append,
                McpOperation::DeletePageAsset,
                "required scope denied: delete",
            ),
            (
                BearerScope::Update,
                McpOperation::DeletePageAsset,
                "required scope denied: delete",
            ),
        ] {
            let auth = AuthContext::new(
                AuthUser::new("alice".to_string()),
                BearerScopeSet::from_iter([scope]),
                PathPrefixSet::new(),
                None,
            );
            let err = service
                .ensure_operation_scope(&auth, operation)
                .expect_err("unmatched scope must be rejected");

            assert_eq!(err.code(), McpErrorCode::Forbidden);
            assert_eq!(err.message(), required);
        }
    }

    ///
    /// prompts/getが要求引数の必須性、名前、型を
    /// 決定済み規則で検証することを確認する。
    ///
    /// # 注記
    /// required不足、未知引数、各非string型を拒否し、
    /// optional未指定と未使用引数を許可する。
    ///
    #[test]
    fn get_prompt_validates_argument_inputs() {
        /*
         * 引数定義を持つpromptとread認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/validation",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: validate-arguments\n",
                    "  description: validate arguments\n",
                    "  arguments:\n",
                    "    - name: required\n",
                    "      description: required value\n",
                    "      required: true\n",
                    "    - name: optional\n",
                    "      description: optional value\n",
                    "    - name: unused\n",
                    "      description: unused value\n",
                    "---\n",
                    "{{@required}}/{{@optional}}",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let service = McpService::new();

        /*
         * rmcp引数mapのキー一意性を確認する
         */
        let mut unique_arguments = JsonObject::new();
        unique_arguments.insert(
            "required".to_string(),
            JsonValue::String("first".to_string()),
        );
        unique_arguments.insert(
            "required".to_string(),
            JsonValue::String("second".to_string()),
        );
        assert_eq!(unique_arguments.len(), 1);
        assert_eq!(
            unique_arguments
                .get("required")
                .and_then(JsonValue::as_str),
            Some("second"),
        );

        /*
         * required不足と未知引数を拒否する
         */
        let missing = service
            .get_prompt(
                &auth,
                &manager,
                "validate-arguments",
                None,
            )
            .expect_err("required argument must be rejected");
        assert_eq!(missing.code(), McpErrorCode::InvalidInput);
        assert_eq!(
            missing.message(),
            "required prompt argument is missing: required",
        );
        let unknown = JsonObject::from_iter([
            (
                "required".to_string(),
                JsonValue::String("value".to_string()),
            ),
            (
                "unknown".to_string(),
                JsonValue::String("secret-value".to_string()),
            ),
        ]);
        let unknown_error = service
            .get_prompt(
                &auth,
                &manager,
                "validate-arguments",
                Some(&unknown),
            )
            .expect_err("unknown argument must be rejected");
        assert_eq!(
            unknown_error.message(),
            "unknown prompt argument: unknown",
        );
        assert!(!unknown_error.message().contains("secret-value"));

        /*
         * string以外の全JSON型を拒否する
         */
        for value in [
            JsonValue::Null,
            JsonValue::Bool(true),
            JsonValue::Number(1.into()),
            JsonValue::Array(Vec::new()),
            JsonValue::Object(JsonObject::new()),
        ] {
            let arguments = JsonObject::from_iter([
                (
                    "required".to_string(),
                    JsonValue::String("value".to_string()),
                ),
                ("optional".to_string(), value),
            ]);
            let error = service
                .get_prompt(
                    &auth,
                    &manager,
                    "validate-arguments",
                    Some(&arguments),
                )
                .expect_err("non-string argument must be rejected");
            assert_eq!(error.code(), McpErrorCode::InvalidInput);
            assert_eq!(
                error.message(),
                "prompt argument must be a string: optional",
            );
        }

        /*
         * optional未指定と未使用引数を許可する
         */
        let arguments = JsonObject::from_iter([(
            "required".to_string(),
            JsonValue::String("value".to_string()),
        )]);
        let result = service
            .get_prompt(
                &auth,
                &manager,
                "validate-arguments",
                Some(&arguments),
            )
            .expect("valid arguments must succeed");
        assert_eq!(result.message(), "value/");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// prompts/getがplaceholderを一回だけ展開し、
    /// 専用エスケープ規則を適用することを確認する。
    ///
    /// # 注記
    /// system、Markdownコード、複数出現、不正形式、
    /// 挿入値内placeholderを同時に検証する。
    ///
    #[test]
    fn get_prompt_expands_placeholders_once() {
        /*
         * 展開規則を判別できるpromptを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/prompts/expansion",
                "user",
                concat!(
                    "---\n",
                    "mcp:\n",
                    "  primitive: prompt\n",
                    "  name: expand-arguments\n",
                    "  description: expand arguments\n",
                    "  system: \"System {{@value}}\"\n",
                    "  arguments:\n",
                    "    - name: value\n",
                    "      description: required value\n",
                    "      required: true\n",
                    "    - name: optional\n",
                    "      description: optional value\n",
                    "---\n",
                    "{{@value}}|{{@value}}|{{@optional}}\n",
                    "{{@@literal}}|\\{{@value}}\n",
                    "{{@ value }}|{{@value }}\n",
                    "`{{@value}}`\n",
                    "```\n{{@value}}\n```\n",
                )
                .to_string(),
            )
            .expect("create prompt failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::new(),
            None,
        );
        let inserted = "{{@optional}}/{{macro}}/{{!macro}}";
        let arguments = JsonObject::from_iter([(
            "value".to_string(),
            JsonValue::String(inserted.to_string()),
        )]);

        /*
         * systemと本文の一回展開結果を確認する
         */
        let result = McpService::new()
            .get_prompt(
                &auth,
                &manager,
                "expand-arguments",
                Some(&arguments),
            )
            .expect("expand prompt failed");
        assert_eq!(
            result.message(),
            concat!(
                "System {{@optional}}/{{macro}}/{{!macro}}\n\n",
                "{{@optional}}/{{macro}}/{{!macro}}|",
                "{{@optional}}/{{macro}}/{{!macro}}|\n",
                "{{@literal}}|\\{{@optional}}/{{macro}}/{{!macro}}\n",
                "{{@ value }}|{{@value }}\n",
                "`{{@optional}}/{{macro}}/{{!macro}}`\n",
                "```\n",
                "{{@optional}}/{{macro}}/{{!macro}}\n",
                "```\n",
            ),
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 分解済みスコープでは他操作を暗黙包含しないことを確認する。
    ///
    /// # 注記
    /// `append` と `update` を例に、スコープ不足が forbidden で
    /// 返ることを検証する。
    ///
    #[test]
    fn operation_scope_rejects_insufficient_decomposed_scope() {
        let service = McpService::new();
        let append_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::new(),
            None,
        );
        let update_only = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::new(),
            None,
        );

        /*
         * 分解済みスコープが別 write 系操作を包含しないことを検証する
         */
        let update_err = service
            .ensure_operation_scope(&append_only, McpOperation::UpdatePage)
            .expect_err("append only must not allow update");
        let append_err = service
            .ensure_operation_scope(&update_only, McpOperation::AppendPage)
            .expect_err("update only must not allow append");

        assert_eq!(update_err.code(), McpErrorCode::Forbidden);
        assert_eq!(append_err.code(), McpErrorCode::Forbidden);

        for scope in [
            BearerScope::Append,
            BearerScope::Create,
            BearerScope::Update,
            BearerScope::Delete,
        ] {
            let auth = AuthContext::new(
                AuthUser::new("alice".to_string()),
                BearerScopeSet::from_iter([scope]),
                PathPrefixSet::new(),
                None,
            );

            for operation in [
                McpOperation::ListPageAssets,
                McpOperation::ResolvePageAsset,
                McpOperation::GetPageAssetMetadata,
                McpOperation::GetPageAssetData,
            ] {
                let err = service
                    .ensure_operation_scope(&auth, operation)
                    .expect_err("decomposed write scope must not allow read");
                assert_eq!(err.code(), McpErrorCode::Forbidden);
            }
        }
    }

    ///
    /// ReadOnly 属性を持つユーザでは write 系操作だけが拒否されることを確認する。
    ///
    /// # 注記
    /// read 系操作は従来どおり成功し、write 系操作だけが
    /// `forbidden` になることを検証する。
    ///
    #[test]
    fn operation_scope_rejects_read_only_user_only_for_write_operations() {
        /*
         * read / write の既存判定を確認する
         */
        let service = McpService::new();
        let auth = AuthContext::new_with_attributes(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::new(),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );

        assert!(service
            .ensure_operation_scope(&auth, McpOperation::GetPage)
            .is_ok());

        let err = service
            .ensure_operation_scope(&auth, McpOperation::UpdatePage)
            .expect_err("read only user must not allow update");

        assert_eq!(err.code(), McpErrorCode::Forbidden);
        assert_eq!(
            err.message(),
            "read only denied: write operation is not allowed"
        );

        /*
         * 更新系アセット操作の優先順位を確認する
         * scope不足よりReadOnlyを先に判定する
         */
        let scope_less_auth = AuthContext::new_with_attributes(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::new(),
            PathPrefixSet::new(),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );
        for operation in [
            McpOperation::UploadPageAsset,
            McpOperation::DeletePageAsset,
        ] {
            assert!(operation.is_write());
            let err = service
                .ensure_operation_scope(&scope_less_auth, operation)
                .expect_err("read only must be checked before scope");

            assert_eq!(err.code(), McpErrorCode::Forbidden);
            assert_eq!(
                err.message(),
                "read only denied: write operation is not allowed"
            );
        }
    }

    ///
    /// 更新系アセット操作がReadOnlyユーザを
    /// 拒否することを検証する
    ///
    /// # 注記
    /// - 操作固有scopeとlegacy writeを持つ
    ///   認証文脈を使用する。
    /// - required scopeを満たしていても
    ///   ReadOnly拒否になることを確認する。
    ///
    #[test]
    fn update_asset_operations_reject_read_only_with_write_scopes() {
        let service = McpService::new();

        for (scope, operations) in [
            (
                BearerScope::Update,
                vec![McpOperation::UploadPageAsset],
            ),
            (
                BearerScope::Delete,
                vec![McpOperation::DeletePageAsset],
            ),
            (
                BearerScope::Write,
                vec![
                    McpOperation::UploadPageAsset,
                    McpOperation::DeletePageAsset,
                ],
            ),
        ] {
            let auth = AuthContext::new_with_attributes(
                AuthUser::new("alice".to_string()),
                BearerScopeSet::from_iter([scope]),
                PathPrefixSet::new(),
                UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
                None,
                None,
            );

            for operation in operations {
                let err = service
                    .ensure_operation_scope(&auth, operation)
                    .expect_err("read only user must be rejected");

                assert_eq!(err.code(), McpErrorCode::Forbidden);
                assert_eq!(
                    err.message(),
                    "read only denied: write operation is not allowed",
                );
            }
        }
    }

    ///
    /// ReadOnly 属性付きユーザで read 系が成功し、write 系が forbidden
    /// になることを確認する。
    ///
    /// # 注記
    /// `get_page` / `get_page_toc` / `list_pages` / `search_pages` /
    /// `get_page_section` は成功し、`create_page` / `update_page` /
    /// `rename_page` / `append_page` は `forbidden` になることを検証する。
    ///
    #[test]
    fn read_only_user_allows_mcp_read_operations_and_rejects_write_operations() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/readonly",
                "user",
                "# Title\n\nintro\n\n## Child\n\nchild text\n".to_string(),
            )
            .expect("create page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/readonly");

        let service = McpService::new();
        let auth = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/mcp"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );

        /*
         * read 系操作が成功することを検証する
         */
        let page = service
            .get_page(&auth, &manager, "/mcp/readonly", None)
            .expect("get page failed");
        assert_eq!(page.path(), "/mcp/readonly");

        let toc = service
            .get_page_toc(&auth, &manager, "/mcp/readonly", None)
            .expect("get page toc failed");
        assert_eq!(toc.sections().len(), 2);

        let list = service
            .list_pages(&auth, &manager, "/mcp", Some(10), None)
            .expect("list pages failed");
        assert_eq!(list.items().len(), 1);
        assert_eq!(list.items()[0].path(), "/mcp/readonly");

        let search = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "child",
                &[FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("search pages failed");
        assert_eq!(search.items().len(), 1);
        assert_eq!(search.items()[0].path(), "/mcp/readonly");

        let section = service
            .get_page_section(
                &auth,
                &manager,
                "/mcp/readonly",
                SectionSelector::ByTitle("Child".to_string()),
                None,
            )
            .expect("get page section failed");
        assert_eq!(section.section().title(), "Child");

        /*
         * write 系操作が forbidden になることを検証する
         */
        let create_err = service
            .create_page(&auth, &manager, "/mcp/readonly-created", "# created")
            .expect_err("read only user must not create page");
        assert_eq!(create_err.code(), McpErrorCode::Forbidden);

        let update_err = service
            .update_page(&auth, &manager, "/mcp/readonly", "# updated")
            .expect_err("read only user must not update page");
        assert_eq!(update_err.code(), McpErrorCode::Forbidden);

        let rename_err = service
            .rename_page(
                &auth,
                &manager,
                "/mcp/readonly",
                "/mcp/readonly-renamed",
            )
            .expect_err("read only user must not rename page");
        assert_eq!(rename_err.code(), McpErrorCode::Forbidden);

        let append_err = service
            .append_page(&auth, &manager, "/mcp/readonly", "\nnext")
            .expect_err("read only user must not append page");
        assert_eq!(append_err.code(), McpErrorCode::Forbidden);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn read_only_user_rejects_edit_page_as_forbidden() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/edit-readonly",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");

        let latest_source = manager
            .get_page_source(
                &manager
                    .get_page_id_by_path("/mcp/edit-readonly")
                    .expect("resolve page id failed")
                    .expect("page id not found"),
                1,
            )
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-readonly".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("read only user must not edit page");

        assert_eq!(error.code(), McpErrorCode::Forbidden);
        assert_eq!(
            error.message(),
            "read only denied: write operation is not allowed"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_succeeds_when_revision_and_instance_id_match_latest() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-success",
                "user",
                "# Title\n\nbefore body\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-success".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "before".to_string(),
                new_text: "after".to_string(),
                occurrence: None,
            },
        );

        let result = service
            .edit_page(&auth, &manager, &request)
            .expect("edit page should succeed");
        let saved_source = manager
            .get_page_source(&page_id, result.revision())
            .expect("lookup saved source failed")
            .expect("saved source missing");

        assert_eq!(result.path(), "/mcp/edit-success");
        assert_eq!(result.revision(), 2);
        assert_eq!(result.summary(), "page edited");
        assert_eq!(
            result.instance_id(),
            saved_source
                .instance_id()
                .expect("saved instance_id missing")
                .to_string()
        );
        assert!(saved_source.source().contains("after body"));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_rejects_non_latest_revision() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-revision",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");
        manager
            .put_page(&page_id, "user", "# Title\n\nnew body\n".to_string(), false)
            .expect("put page failed");
        let stale_source = manager
            .get_page_source(&page_id, 1)
            .expect("get stale source failed")
            .expect("stale source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-revision".to_string(),
            1,
            stale_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("stale revision must fail");

        assert_eq!(error.code(), McpErrorCode::NotLatestRevision);
        assert_eq!(error.message(), "revision is not latest");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_rejects_mismatched_instance_id() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/edit-instance",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-instance".to_string(),
            1,
            "instance-mismatch".to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("mismatched instance_id must fail");

        assert_eq!(error.code(), McpErrorCode::InstanceIdNotMatch);
        assert_eq!(
            error.message(),
            "instance_id does not match latest content"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_resolves_current_path_and_saves_updated_page() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-flow",
                "user",
                "# Title\n\nbefore body\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/./edit-flow".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "before".to_string(),
                new_text: "after".to_string(),
                occurrence: None,
            },
        );

        let result = service
            .edit_page(&auth, &manager, &request)
            .expect("edit page failed");
        let saved_source = manager
            .get_page_source(&page_id, result.revision())
            .expect("lookup saved source failed")
            .expect("saved source missing");

        assert_eq!(result.path(), "/mcp/edit-flow");
        assert_eq!(result.revision(), 2);
        assert_eq!(result.summary(), "page edited");
        assert_eq!(saved_source.source(), "# Title\n\nafter body\n");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_returns_conflict_when_page_is_locked() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-locked",
                "user",
                "# Title\n\nbody\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let _lock_info = manager
            .acquire_page_lock(&page_id, "user")
            .expect("acquire lock failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-locked".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "body".to_string(),
                new_text: "updated".to_string(),
                occurrence: None,
            },
        );

        let error = service
            .edit_page(&auth, &manager, &request)
            .expect_err("locked page must fail");

        assert_eq!(error.code(), McpErrorCode::Conflict);
        assert_eq!(error.message(), "page is locked");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_classifies_selector_and_operation_failures() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-failure",
                "user",
                "# Root\n\nintro\n\n## Child\n\nfirst\n\n## Child\n\nsecond\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let instance_id = latest_source
            .instance_id()
            .expect("instance_id missing")
            .to_string();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        let missing_section = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::DeleteSection {
                        section: SectionSelector::ById("s-999".to_string()),
                    },
                ),
            )
            .expect_err("missing section must fail");
        assert_eq!(missing_section.code(), McpErrorCode::NotFound);

        let ambiguous_title = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ByTitle("Child".to_string()),
                        title: None,
                        content: "updated".to_string(),
                        include_subsection: None,
                    },
                ),
            )
            .expect_err("ambiguous title must fail");
        assert_eq!(ambiguous_title.code(), McpErrorCode::InvalidInput);

        let missing_text = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::ReplaceText {
                        old_text: "missing".to_string(),
                        new_text: "updated".to_string(),
                        occurrence: None,
                    },
                ),
            )
            .expect_err("missing text must fail");
        assert_eq!(missing_text.code(), McpErrorCode::NotFound);

        let invalid_text = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-failure".to_string(),
                    1,
                    instance_id,
                    EditPageOperation::ReplaceText {
                        old_text: "".to_string(),
                        new_text: "updated".to_string(),
                        occurrence: None,
                    },
                ),
            )
            .expect_err("empty old_text must fail");
        assert_eq!(invalid_text.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_preserves_saved_content_latest_revision_and_instance_id_consistency() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-consistency",
                "user",
                "# Title\n\nalpha beta alpha\n".to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();
        let request = EditPageRequest::new(
            "/mcp/edit-consistency".to_string(),
            1,
            latest_source
                .instance_id()
                .expect("instance_id missing")
                .to_string(),
            EditPageOperation::ReplaceText {
                old_text: "alpha".to_string(),
                new_text: "gamma".to_string(),
                occurrence: Some(EditPageReplaceTextOccurrence::All),
            },
        );

        let result = service
            .edit_page(&auth, &manager, &request)
            .expect("edit page failed");
        let resolved = service
            .resolve_page_by_path(&manager, "/mcp/edit-consistency")
            .expect("resolve updated page failed");
        let latest_source = manager
            .get_page_source(&page_id, result.revision())
            .expect("get latest source failed")
            .expect("latest source missing");

        assert_eq!(resolved.latest_revision(), Some(result.revision()));
        assert_eq!(result.revision(), 2);
        assert_eq!(latest_source.source(), "# Title\n\ngamma beta gamma\n");
        assert_eq!(
            latest_source
                .instance_id()
                .expect("latest instance_id missing")
                .to_string(),
            result.instance_id()
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_applies_replace_section_extension_fields() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        let renamed_page_id = manager
            .create_page(
                "/mcp/edit-replace-title",
                "user",
                "# Root\n\nintro\n\n## Child\n\nchild text\n".to_string(),
            )
            .expect("create page failed");
        let renamed_source = manager
            .get_page_source(&renamed_page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let renamed_result = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-title".to_string(),
                    1,
                    renamed_source
                        .instance_id()
                        .expect("instance_id missing")
                        .to_string(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: Some("Renamed root".to_string()),
                        content: "updated root body".to_string(),
                        include_subsection: Some(false),
                    },
                ),
            )
            .expect("edit page should succeed");
        let saved_renamed_source = manager
            .get_page_source(&renamed_page_id, renamed_result.revision())
            .expect("get saved source failed")
            .expect("saved source missing");
        assert_eq!(
            saved_renamed_source.source(),
            "# Renamed root\n\nupdated root body\n\n## Child\n\nchild text\n"
        );

        let kept_page_id = manager
            .create_page(
                "/mcp/edit-replace-keep-subsections",
                "user",
                "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n"
                    .to_string(),
            )
            .expect("create page failed");
        let kept_source = manager
            .get_page_source(&kept_page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let kept_result = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-keep-subsections".to_string(),
                    1,
                    kept_source
                        .instance_id()
                        .expect("instance_id missing")
                        .to_string(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: None,
                        content: "updated root body".to_string(),
                        include_subsection: Some(false),
                    },
                ),
            )
            .expect("edit page should succeed");
        let saved_kept_source = manager
            .get_page_source(&kept_page_id, kept_result.revision())
            .expect("get saved source failed")
            .expect("saved source missing");
        assert_eq!(
            saved_kept_source.source(),
            "# Root\n\nupdated root body\n\n## Child\n\nchild text\n\n# Next\n\nnext\n"
        );

        let replaced_page_id = manager
            .create_page(
                "/mcp/edit-replace-subsections",
                "user",
                "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n"
                    .to_string(),
            )
            .expect("create page failed");
        let replaced_source = manager
            .get_page_source(&replaced_page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let replaced_result = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-subsections".to_string(),
                    1,
                    replaced_source
                        .instance_id()
                        .expect("instance_id missing")
                        .to_string(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: None,
                        content: "updated root body".to_string(),
                        include_subsection: Some(true),
                    },
                ),
            )
            .expect("edit page should succeed");
        let saved_replaced_source = manager
            .get_page_source(&replaced_page_id, replaced_result.revision())
            .expect("get saved source failed")
            .expect("saved source missing");
        assert_eq!(
            saved_replaced_source.source(),
            "# Root\n\nupdated root body\n\n# Next\n\nnext\n"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_validates_revision_and_instance_before_replace_section_content() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-replace-priority",
                "user",
                "# Root\n\nintro\n".to_string(),
            )
            .expect("create page failed");
        manager
            .put_page(&page_id, "user", "# Root\n\nlatest\n".to_string(), false)
            .expect("put page failed");
        let stale_source = manager
            .get_page_source(&page_id, 1)
            .expect("get stale source failed")
            .expect("stale source missing");
        let latest_source = manager
            .get_page_source(&page_id, 2)
            .expect("get latest source failed")
            .expect("latest source missing");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        let stale_revision = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-priority".to_string(),
                    1,
                    stale_source
                        .instance_id()
                        .expect("stale instance_id missing")
                        .to_string(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: None,
                        content: "# Root\n\ninvalid".to_string(),
                        include_subsection: Some(true),
                    },
                ),
            )
            .expect_err("stale revision should fail before content validation");
        assert_eq!(stale_revision.code(), McpErrorCode::NotLatestRevision);

        let mismatched_instance = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-priority".to_string(),
                    2,
                    "instance-mismatch".to_string(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: None,
                        content: "# Root\n\ninvalid".to_string(),
                        include_subsection: Some(true),
                    },
                ),
            )
            .expect_err("instance_id mismatch should fail before content validation");
        assert_eq!(mismatched_instance.code(), McpErrorCode::InstanceIdNotMatch);

        let latest_unchanged = manager
            .get_page_source(&page_id, 2)
            .expect("get latest source failed")
            .expect("latest source missing");
        assert_eq!(latest_unchanged.instance_id(), latest_source.instance_id());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn edit_page_resolves_replace_section_selector_before_content_validation() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/edit-replace-selector-priority",
                "user",
                "# Root\n\nintro\n\n## Child\n\nfirst\n\n## Child\n\nsecond\n"
                    .to_string(),
            )
            .expect("create page failed");
        let latest_source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let instance_id = latest_source
            .instance_id()
            .expect("instance_id missing")
            .to_string();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        let missing_section = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-selector-priority".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-999".to_string()),
                        title: None,
                        content: "# Missing\n\ninvalid".to_string(),
                        include_subsection: Some(true),
                    },
                ),
            )
            .expect_err("missing section should fail before content validation");
        assert_eq!(missing_section.code(), McpErrorCode::NotFound);

        let empty_title_selector = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-selector-priority".to_string(),
                    1,
                    instance_id.clone(),
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ByTitle("   ".to_string()),
                        title: None,
                        content: "# Empty\n\ninvalid".to_string(),
                        include_subsection: Some(true),
                    },
                ),
            )
            .expect_err("empty title selector should fail before content validation");
        assert_eq!(empty_title_selector.code(), McpErrorCode::InvalidInput);

        let ambiguous_title = service
            .edit_page(
                &auth,
                &manager,
                &EditPageRequest::new(
                    "/mcp/edit-replace-selector-priority".to_string(),
                    1,
                    instance_id,
                    EditPageOperation::ReplaceSection {
                        section: SectionSelector::ByTitle("Child".to_string()),
                        title: None,
                        content: "## Child\n\ninvalid".to_string(),
                        include_subsection: Some(true),
                    },
                ),
            )
            .expect_err("ambiguous title should fail before content validation");
        assert_eq!(ambiguous_title.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn path_prefix_matches_boundary_only() {
        assert!(path_matches_prefix("/docs", "/docs"));
        assert!(path_matches_prefix("/docs/a", "/docs"));
        assert!(!path_matches_prefix("/docs2", "/docs"));
    }

    ///
    /// 更新系アセット操作のpage path prefix境界を検証する
    ///
    /// # 注記
    /// - trailing slashを正規化した許可prefix内pathを受理する。
    /// - 文字列前方一致だけの範囲外pathを拒否する。
    /// - ルートprefixでは任意のpage pathを受理する。
    ///
    #[test]
    fn update_asset_operations_enforce_normalized_page_path_prefixes() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let allowed_page_id = manager
            .create_page(
                "/allowed/assets",
                "alice",
                "# allowed".to_string(),
            )
            .expect("create allowed page failed");
        manager
            .create_page(
                "/allowed2/assets",
                "alice",
                "# outside".to_string(),
            )
            .expect("create outside page failed");
        let service = McpService::new();

        /*
         * 正規化後の許可prefix内pathを検証する
         */
        for (scope, operation) in [
            (BearerScope::Update, McpOperation::UploadPageAsset),
            (BearerScope::Delete, McpOperation::DeletePageAsset),
        ] {
            let auth = AuthContext::new(
                AuthUser::new("alice".to_string()),
                BearerScopeSet::from_iter([scope]),
                PathPrefixSet::from_iter(["/allowed"]),
                None,
            );
            let resolved = service
                .resolve_authorized_page(
                    &auth,
                    &manager,
                    operation,
                    "/allowed/./assets",
                )
                .expect("allowed asset page must resolve");

            assert_eq!(resolved.normalized_path(), "/allowed/assets");
            assert_eq!(resolved.page_id(), allowed_page_id);

            let err = service
                .resolve_authorized_page(
                    &auth,
                    &manager,
                    operation,
                    "/allowed2/assets",
                )
                .expect_err("prefix boundary must be enforced");
            assert_eq!(err.code(), McpErrorCode::Forbidden);
            assert_eq!(
                err.message(),
                "path prefix denied: /allowed2/assets",
            );
        }

        /*
         * ルートprefixによる全page path許可を検証する
         */
        let root_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );
        for operation in [
            McpOperation::UploadPageAsset,
            McpOperation::DeletePageAsset,
        ] {
            service
                .resolve_authorized_page(
                    &root_auth,
                    &manager,
                    operation,
                    "/allowed2/assets",
                )
                .expect("root prefix must allow asset page");
        }

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 更新系アセット操作のprefix判定対象を検証する
    ///
    /// # 注記
    /// - page pathだけが許可prefix内となる入力を使用する。
    /// - file_name、asset_id、内部保存パスに関係なく
    ///   uploadとdeleteが成功することを確認する。
    ///
    #[test]
    fn update_asset_operations_apply_prefix_only_to_page_path() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/allowed/assets",
                "alice",
                "# allowed".to_string(),
            )
            .expect("create allowed page failed");
        let upload_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );
        let delete_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Delete]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );
        let service = McpService::new();

        /*
         * page pathだけをprefix判定してuploadする
         */
        let upload = service
            .upload_page_asset(
                &upload_auth,
                &manager,
                "/allowed/assets",
                "private-name.bin",
                "application/octet-stream",
                "cHJpdmF0ZQ==",
            )
            .expect("non-path asset fields must not deny upload");
        let asset_id = AssetId::from_string(upload.asset().asset_id())
            .expect("parse uploaded asset id failed");
        let internal_path = manager.asset_file_path(&asset_id);

        assert_eq!(upload.asset().file_name(), "private-name.bin");
        assert!(!asset_id.to_string().starts_with("/allowed"));
        assert!(!internal_path.starts_with(Path::new("/allowed")));
        assert!(internal_path.is_file());

        /*
         * asset_id解決後もpage pathだけをprefix判定してdeleteする
         */
        let deleted = service
            .delete_page_asset(
                &delete_auth,
                &manager,
                "/allowed/assets",
                "private-name.bin",
            )
            .expect("non-path asset fields must not deny delete");

        assert_eq!(deleted.asset_id(), asset_id.to_string());
        assert_eq!(deleted.path(), "/allowed/assets");
        assert_eq!(deleted.file_name(), "private-name.bin");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 更新系アセット操作の認可と
    /// ページ解決順序を検証する
    ///
    /// # 注記
    /// - 認可失敗をロック、payload、asset解決より
    ///   先に確認する。
    /// - page不存在をpayload、asset解決より先に確認する。
    ///
    #[test]
    fn update_asset_authorization_and_page_resolution_run_first() {
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let allowed_page_id = manager
            .create_page(
                "/allowed/assets",
                "alice",
                "# allowed".to_string(),
            )
            .expect("create allowed page failed");
        let outside_page_id = manager
            .create_page(
                "/outside/assets",
                "alice",
                "# outside".to_string(),
            )
            .expect("create outside page failed");
        manager
            .acquire_page_lock(&allowed_page_id, "alice")
            .expect("lock allowed page failed");
        manager
            .acquire_page_lock(&outside_page_id, "alice")
            .expect("lock outside page failed");
        let service = McpService::new();

        /*
         * scope拒否をロック、payload、asset解決より
         * 先に確認する
         */
        let read_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );
        let upload_scope_error = service
            .upload_page_asset(
                &read_auth,
                &manager,
                "/allowed/assets",
                "missing.bin",
                "application/octet-stream",
                "private:not-base64",
            )
            .expect_err("scope must be checked before upload work");
        let delete_scope_error = service
            .delete_page_asset(
                &read_auth,
                &manager,
                "/allowed/assets",
                "missing.bin",
            )
            .expect_err("scope must be checked before delete work");

        assert_eq!(upload_scope_error.code(), McpErrorCode::Forbidden);
        assert_eq!(
            upload_scope_error.message(),
            "required scope denied: update",
        );
        assert_eq!(delete_scope_error.code(), McpErrorCode::Forbidden);
        assert_eq!(
            delete_scope_error.message(),
            "required scope denied: delete",
        );

        /*
         * prefix拒否をロック、payload、asset解決より
         * 先に確認する
         */
        let write_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );
        let upload_prefix_error = service
            .upload_page_asset(
                &write_auth,
                &manager,
                "/outside/assets",
                "missing.bin",
                "application/octet-stream",
                "private:not-base64",
            )
            .expect_err("prefix must be checked before upload work");
        let delete_prefix_error = service
            .delete_page_asset(
                &write_auth,
                &manager,
                "/outside/assets",
                "missing.bin",
            )
            .expect_err("prefix must be checked before delete work");

        for err in [upload_prefix_error, delete_prefix_error] {
            assert_eq!(err.code(), McpErrorCode::Forbidden);
            assert_eq!(
                err.message(),
                "path prefix denied: /outside/assets",
            );
        }

        /*
         * page不存在をpayloadとasset解決より先に確認する
         */
        let root_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );
        let upload_page_error = service
            .upload_page_asset(
                &root_auth,
                &manager,
                "/missing/assets",
                "missing.bin",
                "application/octet-stream",
                "private:not-base64",
            )
            .expect_err("page must resolve before upload payload");
        let delete_page_error = service
            .delete_page_asset(
                &root_auth,
                &manager,
                "/missing/assets",
                "missing.bin",
            )
            .expect_err("page must resolve before delete asset");

        for err in [upload_page_error, delete_page_error] {
            assert_eq!(err.code(), McpErrorCode::NotFound);
            assert_eq!(err.message(), "page not found: /missing/assets");
        }

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// ロック中ページの更新系アセット操作を検証する
    ///
    /// # 注記
    /// - lock token入力なしのuploadとdeleteを実行する。
    /// - conflict後にDBとアセット実体が変化しないことを確認する。
    ///
    #[test]
    fn update_asset_operations_reject_locked_pages_without_mutation() {
        /*
         * ロック対象ページと既存アセットを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/assets",
                "alice",
                "# assets".to_string(),
            )
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "existing.bin",
                "application/octet-stream",
                "alice",
                b"existing",
            )
            .expect("create asset failed");
        let asset_dir = base_dir.join("assets");
        let asset_path = manager.asset_file_path(&asset_id);
        let file_count = count_test_files(&asset_dir);
        manager
            .acquire_page_lock(&page_id, "alice")
            .expect("acquire page lock failed");
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );
        let service = McpService::new();

        /*
         * ロック中のuploadとdeleteを拒否する
         */
        let upload_error = service
            .upload_page_asset(
                &auth,
                &manager,
                "/mcp/assets",
                "new.bin",
                "application/octet-stream",
                "bmV3",
            )
            .expect_err("locked page upload must fail");
        let delete_error = service
            .delete_page_asset(
                &auth,
                &manager,
                "/mcp/assets",
                "existing.bin",
            )
            .expect_err("locked page delete must fail");

        for error in [upload_error, delete_error] {
            assert_eq!(error.code(), McpErrorCode::Conflict);
            assert_eq!(error.message(), "page is locked");
        }

        /*
         * DBとアセット実体が変化していないことを検証する
         */
        let existing_lookup = manager
            .get_asset_id_by_page_file(&page_id, "existing.bin")
            .expect("lookup existing asset failed");
        let existing_info = manager
            .get_asset_info_by_id(&asset_id)
            .expect("get existing asset info failed")
            .expect("existing asset info missing");
        let stored_data = manager
            .read_asset_data(&asset_id)
            .expect("read existing asset failed");

        assert_eq!(existing_lookup, Some(asset_id));
        assert!(!existing_info.deleted());
        assert_eq!(stored_data, b"existing");
        assert!(asset_path.is_file());
        assert!(manager
            .get_asset_id_by_page_file(&page_id, "new.bin")
            .expect("lookup rejected upload failed")
            .is_none());
        assert_eq!(count_test_files(&asset_dir), file_count);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 更新系アセット操作の認可失敗後の状態を検証する
    ///
    /// # 注記
    /// - scope不足とpath prefix違反でuploadとdeleteを拒否する。
    /// - 認可失敗後にDBとアセット実体が変化しないことを確認する。
    ///
    #[test]
    fn update_asset_authorization_failures_preserve_asset_state() {
        /*
         * 認可対象ページと既存アセットを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page(
                "/mcp/assets",
                "alice",
                "# assets".to_string(),
            )
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "existing.bin",
                "application/octet-stream",
                "alice",
                b"existing",
            )
            .expect("create asset failed");
        let asset_dir = base_dir.join("assets");
        let asset_path = manager.asset_file_path(&asset_id);
        let file_count = count_test_files(&asset_dir);
        let service = McpService::new();

        /*
         * required scope不足による拒否を検証する
         */
        let read_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );
        let upload_scope_error = service
            .upload_page_asset(
                &read_auth,
                &manager,
                "/mcp/assets",
                "scope.bin",
                "application/octet-stream",
                "c2NvcGU=",
            )
            .expect_err("scope denied upload must fail");
        let delete_scope_error = service
            .delete_page_asset(
                &read_auth,
                &manager,
                "/mcp/assets",
                "existing.bin",
            )
            .expect_err("scope denied delete must fail");

        assert_eq!(upload_scope_error.code(), McpErrorCode::Forbidden);
        assert_eq!(
            upload_scope_error.message(),
            "required scope denied: update",
        );
        assert_eq!(delete_scope_error.code(), McpErrorCode::Forbidden);
        assert_eq!(
            delete_scope_error.message(),
            "required scope denied: delete",
        );

        /*
         * path prefix違反による拒否を検証する
         */
        let prefix_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/allowed"]),
            None,
        );
        let upload_prefix_error = service
            .upload_page_asset(
                &prefix_auth,
                &manager,
                "/mcp/assets",
                "prefix.bin",
                "application/octet-stream",
                "cHJlZml4",
            )
            .expect_err("prefix denied upload must fail");
        let delete_prefix_error = service
            .delete_page_asset(
                &prefix_auth,
                &manager,
                "/mcp/assets",
                "existing.bin",
            )
            .expect_err("prefix denied delete must fail");

        for error in [upload_prefix_error, delete_prefix_error] {
            assert_eq!(error.code(), McpErrorCode::Forbidden);
            assert_eq!(
                error.message(),
                "path prefix denied: /mcp/assets",
            );
        }

        /*
         * DBとアセット実体が変化していないことを検証する
         */
        let existing_lookup = manager
            .get_asset_id_by_page_file(&page_id, "existing.bin")
            .expect("lookup existing asset failed");
        let existing_info = manager
            .get_asset_info_by_id(&asset_id)
            .expect("get existing asset info failed")
            .expect("existing asset info missing");
        let stored_data = manager
            .read_asset_data(&asset_id)
            .expect("read existing asset failed");

        assert_eq!(existing_lookup, Some(asset_id));
        assert!(!existing_info.deleted());
        assert_eq!(stored_data, b"existing");
        assert!(asset_path.is_file());
        for file_name in ["scope.bin", "prefix.bin"] {
            assert!(manager
                .get_asset_id_by_page_file(&page_id, file_name)
                .expect("lookup rejected upload failed")
                .is_none());
        }
        assert_eq!(count_test_files(&asset_dir), file_count);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// MCP入力で末尾拒否とroot維持を検証する
    ///
    /// # 注記
    /// 末尾/はInvalidInput、rootは有効な同値として比較する。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn validate_and_normalize_path_rejects_trailing_slash() {
        let service = McpService::new();
        let error = service.validate_and_normalize_path("/docs/topic/")
            .expect_err("trailing slash must fail");
        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert_eq!(service.validate_and_normalize_path("/").unwrap(), "/");
    }

    ///
    /// MCP入力でdot解決とroot越え拒否を検証する
    ///
    /// # 注記
    /// 解決後パスを比較し、rootを越える入力だけを拒否する。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn validate_and_normalize_path_resolves_dot_segments() {
        let service = McpService::new();
        assert_eq!(service.validate_and_normalize_path("/docs/../private")
            .unwrap(), "/private");
        let error = service.validate_and_normalize_path("/../private")
            .expect_err("root escape must fail");
        assert_eq!(error.code(), McpErrorCode::InvalidInput);
    }

    #[test]
    fn resolve_prefix_request_denies_outside_prefix() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let err = service
            .resolve_list_prefix_request(&auth, Some("/private"))
            .expect_err("outside prefix must fail");

        assert_eq!(err.code(), McpErrorCode::Forbidden);
    }

    ///
    /// 許可候補だけを返し保存された非正規表記を許可しない
    ///
    /// # 概要
    /// prefix内でもdotや連続区切りの保存値は変換せず除外する。
    ///
    /// # 注記
    /// 正常候補と範囲外/非正規候補を混ぜ、返却したpathを比較する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn filter_authorized_paths_keeps_only_allowed_paths() {
        /*
         * 同じprefixでも保存状態に問題がある候補を区別する
         */
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let filtered = service.filter_authorized_paths(
            &auth,
            vec!["/docs", "/docs/a", "/private", "/docs2",
                "/docs/../private", "/docs//a"],
        );

        assert_eq!(filtered, vec!["/docs", "/docs/a"]);
    }

    #[test]
    fn rename_requires_both_source_and_destination_in_prefix() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        assert!(service
            .ensure_rename_authorized(&auth, "/docs/a", "/docs/b")
            .is_ok());

        let err = service
            .ensure_rename_authorized(&auth, "/docs/a", "/private/b")
            .expect_err("outside destination must fail");
        assert_eq!(err.code(), McpErrorCode::Forbidden);
    }

    #[test]
    fn search_prefix_request_uses_search_scope_and_prefix_filter() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let resolved = service
            .resolve_search_prefix_request(&auth, Some("/docs"))
            .expect("search prefix resolve failed");
        let filtered = service.filter_paths_for_prefix_request(
            &auth,
            &resolved,
            vec!["/docs", "/docs/a", "/private", "/docs2"],
        );

        assert_eq!(filtered, vec!["/docs", "/docs/a"]);
    }

    ///
    /// MCP prefixのdot解決と要求prefix認可を検証する
    ///
    /// # 注記
    /// 入力prefixを解決し、既存のfilter modeを維持する。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn resolve_prefix_request_resolves_dot_segments() {
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );

        let resolved = service
            .resolve_list_prefix_request(&auth, Some("/docs/."))
            .expect("prefix normalize failed");

        assert_eq!(resolved.requested_prefix(), Some("/docs"));
        assert_eq!(
            resolved.filter_mode(),
            &PrefixFilterMode::DescendantsOf("/docs".to_string()),
        );
    }

    #[test]
    fn resolve_page_by_path_returns_resolved_page() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/page", "user", "# page".to_string())
            .expect("create page failed");
        let service = McpService::new();

        /*
         * current path 解決結果を検証する
         */
        let resolved = service
            .resolve_page_by_path(&manager, "/mcp/./page")
            .expect("resolve page failed");

        assert_eq!(resolved.normalized_path(), "/mcp/page");
        assert_eq!(resolved.page_id(), page_id);
        assert_eq!(resolved.latest_revision(), Some(1));
        assert!(!resolved.page_index().is_draft());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_page_by_path_rejects_draft_page() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_draft_page("/mcp/draft", "user")
            .expect("create draft failed");
        let service = McpService::new();

        /*
         * draft は conflict 扱いで拒否されることを検証する
         */
        let err = service
            .resolve_page_by_path(&manager, "/mcp/draft")
            .expect_err("draft must fail");

        assert_eq!(err.code(), McpErrorCode::Conflict);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn asset_file_name_validation_rejects_path_like_names() {
        let service = McpService::new();

        for file_name in ["", "dir/file.png", "dir\\file.png"] {
            let err = service
                .validate_asset_file_name(file_name)
                .expect_err("path-like file name must fail");
            assert_eq!(err.code(), McpErrorCode::InvalidInput);
            assert_eq!(err.message(), "file_name is invalid");
        }

        service
            .validate_asset_file_name("image.png")
            .expect("plain file name must be accepted");
    }

    ///
    /// upload用MIME種別の入力境界を検証する
    ///
    /// # 注記
    /// - 空文字だけを拒否する。
    /// - 非空のMIME種別は補完や拡張子照合なしで受理する。
    ///
    #[test]
    fn upload_page_asset_mime_type_validation_uses_nonempty_value() {
        let service = McpService::new();

        /*
         * 正常なMIME種別の検証
         */
        for mime_type in ["image/png", "application/x-custom"] {
            service
                .validate_asset_mime_type(mime_type)
                .expect("nonempty MIME type must be accepted");
        }

        /*
         * 空文字拒否と固定エラーの検証
         */
        let err = service
            .validate_asset_mime_type("")
            .expect_err("empty MIME type must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "mime_type is invalid");
    }

    ///
    /// upload用base64の正常値と形式不正を検証する
    ///
    /// # 注記
    /// - 正常値を元のbytesへ復元する。
    /// - 空文字とdecode不能な入力を固定エラーで拒否する。
    /// - decode不能な入力値を公開エラーへ含めない。
    ///
    #[test]
    fn upload_page_asset_data_base64_validation_handles_input_forms() {
        let service = McpService::new();

        /*
         * 正常なbase64入力の検証
         */
        let data = service
            .decode_asset_data_base64("aW1hZ2U=")
            .expect("valid base64 must be decoded");

        assert_eq!(data, b"image");

        /*
         * 空文字とdecode不能入力の検証
         */
        for data_base64 in ["", "private:not-base64"] {
            let err = service
                .decode_asset_data_base64(data_base64)
                .expect_err("invalid base64 must fail");
            assert_eq!(err.code(), McpErrorCode::InvalidInput);
            assert_eq!(err.message(), "data_base64 is invalid");
            if !data_base64.is_empty() {
                assert!(!err.message().contains(data_base64));
            }
        }
    }

    ///
    /// upload用base64のdecode前byte長上限を検証する
    ///
    /// # 注記
    /// - 上限長の有効なbase64入力を受理する。
    /// - 上限を1byte超える入力をdecode前に
    ///   固定エラーで拒否する。
    ///
    #[test]
    fn upload_page_asset_data_base64_validation_checks_encoded_limit() {
        let service = McpService::new();
        let raw_data = vec![0_u8; MAX_MCP_ASSET_DATA_SIZE as usize];
        let at_limit = base64::engine::general_purpose::STANDARD
            .encode(&raw_data);

        /*
         * decode前上限値の検証
         */
        assert_eq!(at_limit.len(), MAX_MCP_ASSET_DATA_BASE64_SIZE);
        let decoded = service
            .decode_asset_data_base64(&at_limit)
            .expect("encoded data at limit must be accepted");
        assert_eq!(decoded.len(), MAX_MCP_ASSET_DATA_SIZE as usize);

        /*
         * decode前上限超過の検証
         */
        let over_limit = "A".repeat(MAX_MCP_ASSET_DATA_BASE64_SIZE + 1);
        let err = service
            .decode_asset_data_base64(&over_limit)
            .expect_err("encoded data over limit must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "asset data too large");
        assert!(!err.message().contains(&over_limit.len().to_string()));
    }

    ///
    /// upload用base64のdecode後raw bytes上限を検証する
    ///
    /// # 注記
    /// - raw bytesが10MiB以下の入力を受理する。
    /// - raw bytesが10MiBを1byte超える入力を
    ///   固定エラーで拒否する。
    /// - encoded長が同じでもdecode後上限を独立して評価する。
    ///
    #[test]
    fn upload_page_asset_data_base64_validation_checks_decoded_limit() {
        let service = McpService::new();
        let at_limit = vec![0_u8; MAX_MCP_ASSET_DATA_SIZE as usize];
        let over_limit =
            vec![0_u8; MAX_MCP_ASSET_DATA_SIZE as usize + 1];

        /*
         * decode後上限値の検証
         */
        let encoded_at_limit =
            base64::engine::general_purpose::STANDARD.encode(&at_limit);
        let decoded = service
            .decode_asset_data_base64(&encoded_at_limit)
            .expect("raw data at limit must be accepted");
        assert_eq!(decoded, at_limit);

        /*
         * decode後上限超過の検証
         */
        let encoded_over_limit =
            base64::engine::general_purpose::STANDARD.encode(&over_limit);
        assert_eq!(
            encoded_over_limit.len(),
            MAX_MCP_ASSET_DATA_BASE64_SIZE,
        );
        assert_eq!(encoded_over_limit.len(), encoded_at_limit.len());
        let err = service
            .decode_asset_data_base64(&encoded_over_limit)
            .expect_err("raw data over limit must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "asset data too large");
        assert!(!err.message().contains(&over_limit.len().to_string()));
    }

    ///
    /// upload成功時のDB、ファイル、公開metadataを検証する
    ///
    /// # 注記
    /// - 認証文脈のユーザーで新規アセットをuploadする。
    /// - DB情報、保存ファイル、
    ///   公開metadataの一致を確認する。
    ///
    #[test]
    fn upload_page_asset_persists_data_and_public_metadata() {
        /*
         * テスト用データベースと認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * upload成功応答を検証する
         */
        let result = service
            .upload_page_asset(
                &auth,
                &manager,
                "/mcp/./assets",
                "image.png",
                "image/png",
                "aW1hZ2U=",
            )
            .expect("upload page asset failed");
        let asset_id = AssetId::from_string(result.asset().asset_id())
            .expect("parse uploaded asset id failed");

        assert_eq!(result.summary(), "asset uploaded");
        assert_eq!(result.asset().path(), "/mcp/assets");
        assert_eq!(result.asset().file_name(), "image.png");
        assert_eq!(result.asset().mime_type(), "image/png");
        assert_eq!(result.asset().size(), 5);
        assert_eq!(result.asset().username(), "alice");
        assert!(result.asset().instance_id().is_some());

        /*
         * DB情報と保存ファイルを検証する
         */
        let lookup_id = manager
            .get_asset_id_by_page_file(&page_id, "image.png")
            .expect("lookup uploaded asset failed")
            .expect("uploaded asset id missing");
        let asset_info = manager
            .get_asset_info_by_id(&asset_id)
            .expect("get uploaded asset info failed")
            .expect("uploaded asset info missing");
        let user_id = manager
            .get_user_id_by_name("alice")
            .expect("get upload user failed")
            .expect("upload user missing");
        let stored_data = manager
            .read_asset_data(&asset_id)
            .expect("read uploaded asset failed");

        assert_eq!(lookup_id, asset_id);
        assert_eq!(asset_info.page_id().as_ref(), Some(&page_id));
        assert_eq!(asset_info.file_name(), "image.png");
        assert_eq!(asset_info.mime(), "image/png");
        assert_eq!(asset_info.size(), 5);
        assert_eq!(asset_info.user(), user_id);
        assert!(!asset_info.deleted());
        assert_eq!(stored_data, b"image");
        assert!(manager.asset_file_path(&asset_id).is_file());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// delete成功時の論理削除と公開応答を検証する
    ///
    /// # 注記
    /// - 通常アセットをservice経由で削除する。
    /// - DB削除フラグ、lookup除外、固定応答を確認する。
    ///
    #[test]
    fn delete_page_asset_logically_deletes_asset() {
        /*
         * テスト用データベースと認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "alice",
                b"image",
            )
            .expect("create asset failed");
        let delete_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Delete]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let read_auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * delete成功応答を検証する
         */
        let result = service
            .delete_page_asset(
                &delete_auth,
                &manager,
                "/mcp/./assets",
                "image.png",
            )
            .expect("delete page asset failed");

        assert_eq!(result.asset_id(), asset_id.to_string());
        assert_eq!(result.path(), "/mcp/assets");
        assert_eq!(result.file_name(), "image.png");
        assert_eq!(result.summary(), "asset deleted");

        /*
         * 論理削除と通常lookup除外を検証する
         */
        let asset_info = manager
            .get_asset_info_by_id(&asset_id)
            .expect("get deleted asset info failed")
            .expect("deleted asset info missing");
        let lookup_id = manager
            .get_asset_id_by_page_file(&page_id, "image.png")
            .expect("lookup deleted asset failed");
        let err = service
            .resolve_page_asset(
                &read_auth,
                &manager,
                "/mcp/assets",
                "image.png",
            )
            .expect_err("deleted asset must not resolve");

        assert!(asset_info.deleted());
        assert!(lookup_id.is_none());
        assert_eq!(err.code(), McpErrorCode::NotFound);
        assert_eq!(err.message(), "asset not found");
        assert!(manager.asset_file_path(&asset_id).is_file());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// upload時の同名アセット状態を検証する
    ///
    /// # 注記
    /// - 同名未削除アセットへのuploadを拒否する。
    /// - 削除後は新しいAssetIdでuploadできることを確認する。
    ///
    #[test]
    fn upload_page_asset_handles_active_and_deleted_same_name() {
        /*
         * テスト用データベースと同名アセットを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let old_asset_id = manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "alice",
                b"old",
            )
            .expect("create old asset failed");
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * 同名未削除アセットとの競合を検証する
         */
        let err = service
            .upload_page_asset(
                &auth,
                &manager,
                "/mcp/assets",
                "image.png",
                "image/png",
                "bmV3",
            )
            .expect_err("active same-name asset must conflict");

        assert_eq!(err.code(), McpErrorCode::Conflict);
        assert_eq!(err.message(), "asset already exists");

        /*
         * 削除済み同名アセットからの再作成を検証する
         */
        manager
            .delete_asset(&old_asset_id)
            .expect("delete old asset failed");
        let result = service
            .upload_page_asset(
                &auth,
                &manager,
                "/mcp/assets",
                "image.png",
                "image/png",
                "bmV3",
            )
            .expect("recreate same-name asset failed");
        let new_asset_id = AssetId::from_string(result.asset().asset_id())
            .expect("parse new asset id failed");
        let lookup_id = manager
            .get_asset_id_by_page_file(&page_id, "image.png")
            .expect("lookup recreated asset failed")
            .expect("recreated asset id missing");
        let old_info = manager
            .get_asset_info_by_id(&old_asset_id)
            .expect("get old asset info failed")
            .expect("old asset info missing");
        let new_data = manager
            .read_asset_data(&new_asset_id)
            .expect("read recreated asset failed");

        assert_ne!(new_asset_id, old_asset_id);
        assert_eq!(lookup_id, new_asset_id);
        assert!(old_info.deleted());
        assert_eq!(new_data, b"new");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// delete対象の通常解決不能状態を検証する
    ///
    /// # 注記
    /// - 不存在、削除済み、ゾンビ、
    ///   所属不一致を準備する。
    /// - file_name不一致とアセット情報欠損も
    ///   同様に拒否する。
    ///
    #[test]
    fn delete_page_asset_rejects_unresolvable_states() {
        /*
         * テスト用データベースと各アセット状態を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let other_page_id = manager
            .create_page("/mcp/other", "alice", "# other".to_string())
            .expect("create other page failed");
        let deleted_id = manager
            .create_asset(
                &page_id,
                "deleted.png",
                "image/png",
                "alice",
                b"deleted",
            )
            .expect("create deleted asset failed");
        let zombie_id = manager
            .create_asset(
                &page_id,
                "zombie.png",
                "image/png",
                "alice",
                b"zombie",
            )
            .expect("create zombie asset failed");
        let wrong_page_id = manager
            .create_asset(
                &page_id,
                "wrong-page.png",
                "image/png",
                "alice",
                b"wrong-page",
            )
            .expect("create wrong page asset failed");
        let wrong_name_id = manager
            .create_asset(
                &page_id,
                "original.png",
                "image/png",
                "alice",
                b"wrong-name",
            )
            .expect("create wrong name asset failed");
        let missing_info_id = manager
            .create_asset(
                &page_id,
                "missing-info.png",
                "image/png",
                "alice",
                b"missing-info",
            )
            .expect("create missing info asset failed");
        manager
            .delete_asset(&deleted_id)
            .expect("delete asset failed");
        manager
            .force_update_asset_info_for_test(&zombie_id, |info| {
                info.clear_page_id()
            })
            .expect("make zombie asset failed");
        manager
            .force_update_asset_info_for_test(&wrong_page_id, |info| {
                info.set_page_id(other_page_id.clone())
            })
            .expect("make wrong page asset failed");
        manager
            .force_update_asset_info_for_test(&wrong_name_id, |info| {
                info.set_file_name("renamed.png".to_string())
            })
            .expect("make wrong name asset failed");
        manager
            .force_remove_asset_info_for_test(&missing_info_id)
            .expect("remove asset info failed");
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Delete]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * 通常解決不能な全状態を拒否することを検証する
         */
        for file_name in [
            "missing.png",
            "deleted.png",
            "zombie.png",
            "wrong-page.png",
            "original.png",
            "missing-info.png",
        ] {
            let err = service
                .delete_page_asset(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("unresolvable asset must fail");
            assert_eq!(err.code(), McpErrorCode::NotFound);
            assert_eq!(err.message(), "asset not found");
            assert!(!err.message().contains(file_name));
        }

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// アセットDB失敗の固定公開エラー写像を検証する
    ///
    /// # 注記
    /// - uploadとdeleteの既知DBエラーを個別に写像する。
    /// - 未分類DB／FS相当エラーから内部詳細を除去する。
    ///
    #[test]
    fn asset_db_errors_map_to_fixed_public_errors() {
        /*
         * upload失敗の固定写像を検証する
         */
        let upload_cases = [
            (
                map_upload_asset_db_error(Error::new(DbError::PageNotFound)),
                McpErrorCode::NotFound,
                "page not found",
            ),
            (
                map_upload_asset_db_error(Error::new(DbError::UserNotFound)),
                McpErrorCode::InternalError,
                "user resolution failed",
            ),
            (
                map_upload_asset_db_error(Error::new(
                    DbError::AssetAlreadyExists,
                )),
                McpErrorCode::Conflict,
                "asset already exists",
            ),
            (
                map_upload_asset_db_error(anyhow::anyhow!(
                    "private DB/FS detail: /secret/assets/file",
                )),
                McpErrorCode::InternalError,
                "asset create failed",
            ),
        ];
        for (err, code, message) in upload_cases {
            assert_eq!(err.code(), code);
            assert_eq!(err.message(), message);
            assert!(!err.message().contains("/secret"));
        }

        /*
         * delete失敗の固定写像を検証する
         */
        let delete_cases = [
            map_delete_asset_db_error(Error::new(DbError::AssetNotFound)),
            map_delete_asset_db_error(Error::new(DbError::AssetDeleted)),
        ];
        for err in delete_cases {
            assert_eq!(err.code(), McpErrorCode::NotFound);
            assert_eq!(err.message(), "asset not found");
        }
        let internal = map_delete_asset_db_error(anyhow::anyhow!(
            "private DB/FS detail: /secret/assets/file",
        ));

        assert_eq!(internal.code(), McpErrorCode::InternalError);
        assert_eq!(internal.message(), "asset delete failed");
        assert!(!internal.message().contains("/secret"));
    }

    ///
    /// 再現可能なアセット作成失敗時のcleanupを検証する
    ///
    /// # 注記
    /// - page不存在、user不存在、同名競合を発生させる。
    /// - 失敗試行で保存ファイルが
    ///   増えないことを確認する。
    ///
    #[test]
    fn create_asset_cleans_files_after_reproducible_failures() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        let asset_dir = base_dir.join("assets");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");

        /*
         * page不存在とuser不存在のcleanupを検証する
         */
        let page_err = manager
            .create_asset(
                &PageId::new(),
                "missing-page.png",
                "image/png",
                "alice",
                b"private-page-data",
            )
            .expect_err("missing page must fail");
        assert!(matches!(
            page_err.downcast_ref::<DbError>(),
            Some(DbError::PageNotFound),
        ));
        assert_eq!(count_test_files(&asset_dir), 0);

        let page_id = manager
            .create_page("/mcp/assets", "alice", "# page".to_string())
            .expect("create page failed");
        let user_err = manager
            .create_asset(
                &page_id,
                "missing-user.png",
                "image/png",
                "missing-user",
                b"private-user-data",
            )
            .expect_err("missing user must fail");
        assert!(matches!(
            user_err.downcast_ref::<DbError>(),
            Some(DbError::UserNotFound),
        ));
        assert_eq!(count_test_files(&asset_dir), 0);

        /*
         * 同名競合時に既存ファイルだけが
         * 残ることを検証する
         */
        manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "alice",
                b"existing",
            )
            .expect("create existing asset failed");
        let file_count = count_test_files(&asset_dir);
        let conflict_err = manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "alice",
                b"private-conflict-data",
            )
            .expect_err("same-name asset must fail");

        assert!(matches!(
            conflict_err.downcast_ref::<DbError>(),
            Some(DbError::AssetAlreadyExists),
        ));
        assert_eq!(file_count, 1);
        assert_eq!(count_test_files(&asset_dir), file_count);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_page_asset_by_file_builds_public_metadata() {
        /*
         * テスト用データベースとアセットを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "user",
                b"image",
            )
            .expect("create asset failed");
        let service = McpService::new();
        let resolved = service
            .resolve_page_by_path(&manager, "/mcp/assets")
            .expect("resolve page failed");

        /*
         * DB内部情報を公開用metadataへ変換できることを検証する
         */
        let metadata = service
            .resolve_page_asset_by_file(&manager, &resolved, "image.png")
            .expect("resolve asset failed");

        assert_eq!(metadata.asset_id(), asset_id.to_string());
        assert_eq!(metadata.path(), "/mcp/assets");
        assert_eq!(metadata.file_name(), "image.png");
        assert_eq!(metadata.mime_type(), "image/png");
        assert_eq!(metadata.size(), 5);
        assert_eq!(metadata.username(), "user");
        assert!(metadata.instance_id().is_some());
        assert!(!metadata.timestamp().is_empty());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_authorized_page_applies_read_scope_and_prefix() {
        /*
         * テスト用データベースと認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        manager
            .create_page("/private/assets", "user", "# page".to_string())
            .expect("create private page failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * 許可prefix内では認可後にページ解決できることを検証する
         */
        let resolved = service
            .resolve_authorized_page(
                &auth,
                &manager,
                McpOperation::ListPageAssets,
                "/mcp/./assets",
            )
            .expect("resolve authorized page failed");
        assert_eq!(resolved.normalized_path(), "/mcp/assets");
        assert_eq!(resolved.page_id(), page_id);

        /*
         * 許可prefix外では page_id 解決前に forbidden になることを検証する
         */
        let err = service
            .resolve_authorized_page(
                &auth,
                &manager,
                McpOperation::ListPageAssets,
                "/private/assets",
            )
            .expect_err("outside prefix must fail");
        assert_eq!(err.code(), McpErrorCode::Forbidden);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// list_page_assets が空ページで空一覧を返すことを検証する
    ///
    #[test]
    fn list_page_assets_returns_empty_assets_for_page_without_assets() {
        /*
         * テスト用データベースとアセットなしページを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/empty-assets", "user", "# page".to_string())
            .expect("create page failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * ページが存在し、通常アセットが0件の場合は空一覧になることを検証する
         */
        let result = service
            .list_page_assets(&auth, &manager, "/mcp/empty-assets")
            .expect("list empty page assets failed");

        assert_eq!(result.path(), "/mcp/empty-assets");
        assert!(result.assets().is_empty());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 参照系アセットserviceメソッドの認可境界を検証する
    ///
    /// # 注記
    /// read scope、legacy write scope、path prefix 境界、
    /// ReadOnly属性、ページロック非影響を確認する。
    ///
    #[test]
    fn reference_asset_service_methods_honor_read_authorization_boundaries() {
        /*
         * テスト用データベース、通常アセット、ロック中ページを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/docs/assets", "user", "# page".to_string())
            .expect("create page failed");
        let outside_page_id = manager
            .create_page("/docs2/assets", "user", "# page".to_string())
            .expect("create outside page failed");
        let locked_page_id = manager
            .create_page("/docs/locked-assets", "user", "# page".to_string())
            .expect("create locked page failed");
        manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "user",
                b"image",
            )
            .expect("create asset failed");
        manager
            .create_asset(
                &outside_page_id,
                "outside.png",
                "image/png",
                "user",
                b"outside",
            )
            .expect("create outside asset failed");
        manager
            .create_asset(
                &locked_page_id,
                "locked.png",
                "image/png",
                "user",
                b"locked",
            )
            .expect("create locked asset failed");
        let _lock_info = manager
            .acquire_page_lock(&locked_page_id, "user")
            .expect("acquire page lock failed");
        let read_auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );
        let write_auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Write]),
            PathPrefixSet::from_iter(["/docs"]),
            None,
        );
        let read_only_auth = AuthContext::new_with_attributes(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/docs"]),
            UserAttributeSet::from_iter([UserAttribute::ReadOnly]),
            None,
            None,
        );
        let service = McpService::new();

        /*
         * read scope で参照系4メソッドが許可されることを検証する
         */
        let list_result = service
            .list_page_assets(&read_auth, &manager, "/docs/assets")
            .expect("read scope must list assets");
        assert_eq!(list_result.assets().len(), 1);

        let resolve_result = service
            .resolve_page_asset(
                &read_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .expect("read scope must resolve asset");
        assert_eq!(resolve_result.asset().file_name(), "image.png");

        let metadata_result = service
            .get_page_asset_metadata(
                &read_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .expect("read scope must get asset metadata");
        assert_eq!(metadata_result.asset().mime_type(), "image/png");

        let data_result = service
            .get_page_asset_data(
                &read_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .expect("read scope must get asset data");
        assert_eq!(data_result.data_base64(), "aW1hZ2U=");

        /*
         * legacy write scope が参照系4メソッドの read 要求を満たすことを検証する
         */
        assert!(service
            .list_page_assets(&write_auth, &manager, "/docs/assets")
            .is_ok());
        assert!(service
            .resolve_page_asset(
                &write_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .is_ok());
        assert!(service
            .get_page_asset_metadata(
                &write_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .is_ok());
        assert!(service
            .get_page_asset_data(
                &write_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .is_ok());

        /*
         * path prefix 制約は境界一致で評価されることを検証する
         */
        let outside_err = service
            .list_page_assets(&read_auth, &manager, "/docs2/assets")
            .expect_err("/docs prefix must not allow /docs2");
        assert_eq!(outside_err.code(), McpErrorCode::Forbidden);

        let normalized_result = service
            .list_page_assets(&read_auth, &manager, "/docs/./assets")
            .expect("dot segments must resolve inside prefix");
        assert_eq!(normalized_result.path(), "/docs/assets");

        /*
         * ReadOnly 属性とページロックは参照系アセットtoolを阻害しないことを検証する
         */
        let read_only_result = service
            .get_page_asset_data(
                &read_only_auth,
                &manager,
                "/docs/assets",
                "image.png",
            )
            .expect("read only user with read scope must get asset data");
        assert_eq!(read_only_result.data_base64(), "aW1hZ2U=");

        let locked_result = service
            .get_page_asset_metadata(
                &read_auth,
                &manager,
                "/docs/locked-assets",
                "locked.png",
            )
            .expect("locked page must not block reference asset metadata");
        assert_eq!(locked_result.asset().file_name(), "locked.png");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 参照系アセットserviceメソッドが不正入力を invalid_input に分類することを検証する
    ///
    #[test]
    fn reference_asset_service_methods_reject_invalid_inputs() {
        /*
         * テスト用データベースと認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * path 入力不正は invalid_input になることを検証する
         */
        let path_err = service
            .list_page_assets(&auth, &manager, "relative/path")
            .expect_err("relative path must fail");
        assert_eq!(path_err.code(), McpErrorCode::InvalidInput);

        /*
         * file_name 入力不正は各 file_name 指定toolで invalid_input になることを検証する
         */
        for file_name in ["", "dir/file.png", "dir\\file.png"] {
            let resolve_err = service
                .resolve_page_asset(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("invalid resolve file_name must fail");
            assert_eq!(resolve_err.code(), McpErrorCode::InvalidInput);
            assert_eq!(resolve_err.message(), "file_name is invalid");

            let metadata_err = service
                .get_page_asset_metadata(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("invalid metadata file_name must fail");
            assert_eq!(metadata_err.code(), McpErrorCode::InvalidInput);
            assert_eq!(metadata_err.message(), "file_name is invalid");

            let data_err = service
                .get_page_asset_data(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("invalid data file_name must fail");
            assert_eq!(data_err.code(), McpErrorCode::InvalidInput);
            assert_eq!(data_err.message(), "file_name is invalid");
        }

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 参照系アセットserviceメソッドが未存在・削除済み状態を not_found に集約することを検証する
    ///
    #[test]
    fn reference_asset_service_methods_hide_missing_and_deleted_state() {
        /*
         * テスト用データベース、通常ページ、削除済みページ、削除済みアセットを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let deleted_page_id = manager
            .create_page("/mcp/deleted-page", "user", "# deleted".to_string())
            .expect("create deleted page failed");
        let deleted_asset_id = manager
            .create_asset(
                &page_id,
                "deleted.png",
                "image/png",
                "user",
                b"deleted",
            )
            .expect("create deleted asset failed");
        let other_page_id = manager
            .create_page("/mcp/other-assets", "user", "# other".to_string())
            .expect("create other page failed");
        let zombie_asset_id = manager
            .create_asset(
                &page_id,
                "zombie.png",
                "image/png",
                "user",
                b"zombie",
            )
            .expect("create zombie asset failed");
        let wrong_page_asset_id = manager
            .create_asset(
                &page_id,
                "wrong-page.png",
                "image/png",
                "user",
                b"wrong-page",
            )
            .expect("create wrong page asset failed");
        let wrong_name_asset_id = manager
            .create_asset(
                &page_id,
                "original.png",
                "image/png",
                "user",
                b"wrong-name",
            )
            .expect("create wrong name asset failed");
        manager
            .delete_page_by_id(&deleted_page_id)
            .expect("delete page failed");
        manager
            .delete_asset(&deleted_asset_id)
            .expect("delete asset failed");
        manager
            .force_update_asset_info_for_test(
                &zombie_asset_id,
                |asset_info| asset_info.clear_page_id(),
            )
            .expect("make zombie asset failed");
        manager
            .force_update_asset_info_for_test(
                &wrong_page_asset_id,
                |asset_info| asset_info.set_page_id(other_page_id.clone()),
            )
            .expect("make wrong page asset failed");
        manager
            .force_update_asset_info_for_test(
                &wrong_name_asset_id,
                |asset_info| {
                    asset_info.set_file_name("renamed.png".to_string())
                },
            )
            .expect("make wrong name asset failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * ページ未存在と削除済みページは not_found になることを検証する
         */
        for path in ["/mcp/missing-page", "/mcp/deleted-page"] {
            let err = service
                .list_page_assets(&auth, &manager, path)
                .expect_err("missing or deleted page must fail");
            assert_eq!(err.code(), McpErrorCode::NotFound);
        }

        /*
         * asset 未存在と削除済み asset は not_found に集約されることを検証する
         */
        for file_name in ["missing.png", "deleted.png"] {
            let resolve_err = service
                .resolve_page_asset(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("missing or deleted asset must fail");
            assert_eq!(resolve_err.code(), McpErrorCode::NotFound);
            assert_eq!(resolve_err.message(), "asset not found");

            let metadata_err = service
                .get_page_asset_metadata(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("missing or deleted asset metadata must fail");
            assert_eq!(metadata_err.code(), McpErrorCode::NotFound);
            assert_eq!(metadata_err.message(), "asset not found");

            let data_err = service
                .get_page_asset_data(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("missing or deleted asset data must fail");
            assert_eq!(data_err.code(), McpErrorCode::NotFound);
            assert_eq!(data_err.message(), "asset not found");
        }

        /*
         * 内部不整合assetは一覧から除外または file_name 指定解決で not_found に集約されることを検証する
         */
        let list_result = service
            .list_page_assets(&auth, &manager, "/mcp/assets")
            .expect("list page assets failed");
        let listed_file_names: Vec<&str> = list_result
            .assets()
            .iter()
            .map(|asset| asset.file_name())
            .collect();
        assert!(!listed_file_names.contains(&"zombie.png"));
        assert!(!listed_file_names.contains(&"wrong-page.png"));

        for file_name in ["zombie.png", "wrong-page.png", "original.png"] {
            let resolve_err = service
                .resolve_page_asset(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("inconsistent asset must fail");
            assert_eq!(resolve_err.code(), McpErrorCode::NotFound);
            assert_eq!(resolve_err.message(), "asset not found");

            let metadata_err = service
                .get_page_asset_metadata(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("inconsistent asset metadata must fail");
            assert_eq!(metadata_err.code(), McpErrorCode::NotFound);
            assert_eq!(metadata_err.message(), "asset not found");

            let data_err = service
                .get_page_asset_data(
                    &auth,
                    &manager,
                    "/mcp/assets",
                    file_name,
                )
                .expect_err("inconsistent asset data must fail");
            assert_eq!(data_err.code(), McpErrorCode::NotFound);
            assert_eq!(data_err.message(), "asset not found");
        }

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 参照系アセットserviceメソッドの正常系を検証する
    ///
    /// # 注記
    /// - テスト用DBにページとアセットを作成する。
    /// - 参照系4メソッドがmetadataとbase64データを返すことを確認する。
    ///
    #[test]
    fn asset_service_methods_return_metadata_and_data() {
        /*
         * テスト用データベースと認証文脈を準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "image.png",
                "image/png",
                "user",
                b"image",
            )
            .expect("create asset failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * list_page_assets が公開metadata一覧を返すことを検証する
         */
        let list_result = service
            .list_page_assets(&auth, &manager, "/mcp/./assets")
            .expect("list page assets failed");
        let db_assets = manager
            .list_page_assets(&page_id)
            .expect("list db page assets failed");
        assert_eq!(list_result.path(), "/mcp/assets");
        assert_eq!(list_result.assets().len(), 1);
        assert_eq!(db_assets.len(), 1);
        assert_eq!(list_result.assets()[0].asset_id(), asset_id.to_string());

        /*
         * file_name指定の参照系メソッドが同じmetadataを返すことを検証する
         */
        let resolve_result = service
            .resolve_page_asset(
                &auth,
                &manager,
                "/mcp/assets",
                "image.png",
            )
            .expect("resolve page asset failed");
        assert_eq!(resolve_result.path(), "/mcp/assets");
        assert_eq!(resolve_result.asset().file_name(), "image.png");

        let metadata_result = service
            .get_page_asset_metadata(
                &auth,
                &manager,
                "/mcp/assets",
                "image.png",
            )
            .expect("get page asset metadata failed");
        assert_eq!(metadata_result.path(), "/mcp/assets");
        assert_eq!(metadata_result.asset().mime_type(), "image/png");

        let data_result = service
            .get_page_asset_data(
                &auth,
                &manager,
                "/mcp/assets",
                "image.png",
            )
            .expect("get page asset data failed");
        assert_eq!(data_result.path(), "/mcp/assets");
        assert_eq!(data_result.asset().size(), 5);
        assert_eq!(data_result.data_base64(), "aW1hZ2U=");

        /*
         * service結果が既存 DatabaseManager asset API で確認できる状態と一致することを検証する
         */
        let lookup_asset_id = manager
            .get_asset_id_by_page_file(&page_id, "image.png")
            .expect("lookup asset id failed")
            .expect("asset id missing");
        let db_asset_info = manager
            .get_asset_info_by_id(&lookup_asset_id)
            .expect("get db asset info failed")
            .expect("asset info missing");
        let db_data = manager
            .read_asset_data(&lookup_asset_id)
            .expect("read db asset data failed");

        assert_eq!(lookup_asset_id, asset_id);
        assert_eq!(db_asset_info.page_id().as_ref(), Some(&page_id));
        assert_eq!(db_asset_info.file_name(), "image.png");
        assert_eq!(db_asset_info.mime(), "image/png");
        assert_eq!(db_asset_info.size(), 5);
        assert!(!db_asset_info.deleted());
        assert!(!db_asset_info.is_zombie());
        assert_eq!(db_data, b"image");
        assert_eq!(
            metadata_result.asset().asset_id(),
            lookup_asset_id.to_string(),
        );
        assert_eq!(
            data_result.asset().instance_id(),
            db_asset_info.instance_id().map(|id| id.to_string()).as_deref(),
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 参照系アセットデータ取得のサイズ上限を検証する
    ///
    /// # 注記
    /// - 10MiBを1byte超えるテスト用アセットを作成する。
    /// - get_page_asset_data が本体返却前に conflict を返すことを確認する。
    ///
    #[test]
    fn get_page_asset_data_rejects_oversized_asset() {
        /*
         * テスト用データベースと大きなアセットを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let data = vec![0_u8; MAX_MCP_ASSET_DATA_SIZE as usize + 1];
        manager
            .create_asset(
                &page_id,
                "large.bin",
                "application/octet-stream",
                "user",
                &data,
            )
            .expect("create asset failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * 上限超過アセットは conflict になることを検証する
         */
        let err = service
            .get_page_asset_data(
                &auth,
                &manager,
                "/mcp/assets",
                "large.bin",
            )
            .expect_err("oversized asset must fail");
        assert_eq!(err.code(), McpErrorCode::Conflict);
        assert_eq!(err.message(), "asset data too large");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// get_page_asset_data が読込後のデータサイズ上限を検証することを確認する
    ///
    #[test]
    fn get_page_asset_data_rejects_oversized_data_after_read() {
        /*
         * metadata size は小さいまま、保存済みファイルだけを上限超過へ差し替える
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "large-after-read.bin",
                "application/octet-stream",
                "user",
                b"small",
            )
            .expect("create asset failed");
        fs::write(
            manager.asset_file_path(&asset_id),
            vec![0_u8; MAX_MCP_ASSET_DATA_SIZE as usize + 1],
        )
        .expect("overwrite asset file failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * 読込後に data length が上限超過なら conflict になることを検証する
         */
        let err = service
            .get_page_asset_data(
                &auth,
                &manager,
                "/mcp/assets",
                "large-after-read.bin",
            )
            .expect_err("oversized data after read must fail");

        assert_eq!(err.code(), McpErrorCode::Conflict);
        assert_eq!(err.message(), "asset data too large");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// get_page_asset_data が保存済みデータ読込失敗を固定文言の internal_error にすることを確認する
    ///
    #[test]
    fn get_page_asset_data_maps_missing_file_to_internal_error() {
        /*
         * DB metadata は残したまま保存済みアセットファイルだけを削除する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "missing-file.bin",
                "application/octet-stream",
                "user",
                b"data",
            )
            .expect("create asset failed");
        fs::remove_file(manager.asset_file_path(&asset_id))
            .expect("remove asset file failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * 内部I/O詳細を含めず固定文言の internal_error になることを検証する
         */
        let err = service
            .get_page_asset_data(
                &auth,
                &manager,
                "/mcp/assets",
                "missing-file.bin",
            )
            .expect_err("missing asset file must fail");

        assert_eq!(err.code(), McpErrorCode::InternalError);
        assert_eq!(err.message(), "asset read failed");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// list_page_assets が DB lookup 失敗を固定文言の internal_error にすることを確認する
    ///
    #[test]
    fn list_page_assets_maps_lookup_failure_to_internal_error() {
        /*
         * group参照を残したまま AssetInfo だけを削除する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/assets", "user", "# page".to_string())
            .expect("create page failed");
        let asset_id = manager
            .create_asset(
                &page_id,
                "broken.png",
                "image/png",
                "user",
                b"broken",
            )
            .expect("create asset failed");
        manager
            .force_remove_asset_info_for_test(&asset_id)
            .expect("remove asset info failed");
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );
        let service = McpService::new();

        /*
         * DB内部詳細を含めず固定文言の internal_error になることを検証する
         */
        let err = service
            .list_page_assets(&auth, &manager, "/mcp/assets")
            .expect_err("broken asset lookup must fail");

        assert_eq!(err.code(), McpErrorCode::InternalError);
        assert_eq!(err.message(), "asset lookup failed");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_page_by_path_returns_not_found_for_missing_page() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();

        /*
         * 未存在 path は not_found になることを検証する
         */
        let err = service
            .resolve_page_by_path(&manager, "/mcp/missing")
            .expect_err("missing page must fail");

        assert_eq!(err.code(), McpErrorCode::NotFound);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn get_page_and_get_page_toc_follow_read_contract() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/read",
                "user",
                "# Title\n\nintro\n\n## Child\n\nchild body\n".to_string(),
            )
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * ページ全体取得を検証する
         */
        let page = service
            .get_page(&auth, &manager, "/mcp/read", None)
            .expect("get page failed");
        assert_eq!(page.path(), "/mcp/read");
        assert_eq!(page.revision(), 1);
        assert!(page.content().contains("## Child"));

        /*
         * TOC 取得を検証する
         */
        let toc = service
            .get_page_toc(&auth, &manager, "/mcp/read", None)
            .expect("get toc failed");
        assert_eq!(toc.path(), "/mcp/read");
        assert_eq!(toc.revision(), 1);
        assert_eq!(toc.sections().len(), 2);
        assert_eq!(toc.sections()[0].id(), "s-001");
        assert_eq!(toc.sections()[0].title(), "Title");
        assert_eq!(toc.sections()[1].parent_id(), Some("s-001"));
        assert!(toc.sections()[0].section_chars() > 0);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 単一 path 操作で path prefix 制約違反を拒否することを確認する。
    ///
    /// # 注記
    /// `get_page` 実行時に許可外 path を指定し、認可失敗になることを
    /// 検証する。
    ///
    #[test]
    fn get_page_rejects_path_outside_prefix_constraint() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/private/read", "user", "# secret".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * path prefix 制約違反が forbidden になることを検証する
         */
        let err = service
            .get_page(&auth, &manager, "/private/read", None)
            .expect_err("outside path must fail");

        assert_eq!(err.code(), McpErrorCode::Forbidden);
        assert_eq!(err.message(), "path prefix denied: /private/read");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn list_pages_applies_prefix_cursor_and_limit() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/a", "user", "a".to_string())
            .expect("create page a failed");
        manager
            .create_page("/mcp/b", "user", "b".to_string())
            .expect("create page b failed");
        manager
            .create_page("/mcp/c", "user", "c".to_string())
            .expect("create page c failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * cursor と limit によるページングを検証する
         */
        let first = service
            .list_pages(&auth, &manager, "/mcp", Some(2), None)
            .expect("first list failed");
        assert_eq!(first.items().len(), 2);
        assert_eq!(first.items()[0].path(), "/mcp/a");
        assert_eq!(first.items()[1].path(), "/mcp/b");
        assert!(first.has_more());
        assert_eq!(first.next_cursor(), Some("/mcp/b"));

        let second = service
            .list_pages(&auth, &manager, "/mcp", Some(2), first.next_cursor())
            .expect("second list failed");
        assert_eq!(second.items().len(), 1);
        assert_eq!(second.items()[0].path(), "/mcp/c");
        assert!(!second.has_more());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `list_pages` が要求 prefix 外の cursor を拒否することを確認する。
    ///
    /// # 注記
    /// `cursor` に prefix 外 path を与え、`InvalidInput` を期待する。
    ///
    #[test]
    fn list_pages_rejects_cursor_outside_requested_prefix() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/a", "user", "a".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 要求 prefix 外 cursor が入力不正になることを検証する
         */
        let err = service
            .list_pages(&auth, &manager, "/mcp", Some(10), Some("/other/a"))
            .expect_err("outside cursor must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "cursor must be under requested prefix");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_merges_targets_and_filters_by_prefix() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/search",
                "user",
                "# SearchTitle\n\nkeyword in body\n".to_string(),
            )
            .expect("create search page failed");
        manager
            .create_page(
                "/other/search",
                "user",
                "# Other\n\nkeyword outside jail\n".to_string(),
            )
            .expect("create other page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/search");
        rebuild_test_fts_page(&manager, &fts_config, "/other/search");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * path jail と prefix の後段フィルタを検証する
         */
        let result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "keyword",
                &[FtsSearchTarget::Headings, FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("search pages failed");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].path(), "/mcp/search");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `search_pages` が上位 N 件へ切り詰めることを確認する。
    ///
    /// # 注記
    /// スコア差のある 2 件を作成し、`limit=1` で上位 1 件のみ返ることを
    /// 確認する。
    ///
    #[test]
    fn search_pages_applies_top_n_limit() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/high",
                "user",
                "# keyword keyword\n\nkeyword in body\nkeyword again\n"
                    .to_string(),
            )
            .expect("create high score page failed");
        manager
            .create_page(
                "/mcp/low",
                "user",
                "# keyword\n\nsingle hit\n".to_string(),
            )
            .expect("create low score page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/high");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/low");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * top-N 制約で上位 1 件だけ返ることを検証する
         */
        let result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "keyword",
                &[FtsSearchTarget::Headings, FtsSearchTarget::Body],
                Some("/mcp"),
                Some(1),
            )
            .expect("search pages failed");

        assert_eq!(result.items().len(), 1);
        assert_eq!(result.items()[0].path(), "/mcp/high");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_rejects_empty_targets() {
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        let err = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "keyword",
                &[],
                Some("/mcp"),
                Some(10),
            )
            .expect_err("empty targets must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);
        assert_eq!(err.message(), "target must not be empty");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_supports_front_matter_target() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/front-matter",
                "user",
                "---\ncustom_meta:\n  search_note: mcpfrontmattertoken\n---\nbody only\n"
                    .to_string(),
            )
            .expect("create front matter page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/front-matter");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * front_matter 指定でのみヒットすることを検証する
         */
        let front_matter_result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "mcpfrontmattertoken",
                &[FtsSearchTarget::FrontMatter],
                Some("/mcp"),
                Some(10),
            )
            .expect("front matter search pages failed");
        let body_result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "mcpfrontmattertoken",
                &[FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("body search pages failed");

        assert_eq!(front_matter_result.items().len(), 1);
        assert_eq!(front_matter_result.items()[0].path(), "/mcp/front-matter");
        assert!(body_result.items().is_empty());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn search_pages_merges_front_matter_and_body_targets() {
        /*
         * テスト用データベースと FTS を準備する
         */
        let (base_dir, manager) = open_test_manager();
        let fts_config = open_test_fts_config(&base_dir);
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/merged-front-matter",
                "user",
                "---\ncustom_meta:\n  search_note: sharedmcpkeyword\n---\nbody only\n"
                    .to_string(),
            )
            .expect("create merged front matter page failed");
        manager
            .create_page(
                "/mcp/merged-body",
                "user",
                "body sharedmcpkeyword\n".to_string(),
            )
            .expect("create merged body page failed");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/merged-front-matter");
        rebuild_test_fts_page(&manager, &fts_config, "/mcp/merged-body");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * front_matter と body の複合指定で双方のヒットを返すことを検証する
         */
        let result = service
            .search_pages(
                &auth,
                &manager,
                &fts_config,
                "sharedmcpkeyword",
                &[FtsSearchTarget::FrontMatter, FtsSearchTarget::Body],
                Some("/mcp"),
                Some(10),
            )
            .expect("merged search pages failed");

        assert_eq!(result.items().len(), 2);
        assert!(
            result
                .items()
                .iter()
                .any(|item| item.path() == "/mcp/merged-front-matter")
        );
        assert!(
            result
                .items()
                .iter()
                .any(|item| item.path() == "/mcp/merged-body")
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn get_page_section_supports_id_and_title_selector() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page(
                "/mcp/section",
                "user",
                "# Root\n\nintro\n\n## Child\n\nchild text\n".to_string(),
            )
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * ID 指定と title 指定の両方を検証する
         */
        let by_id = service
            .get_page_section(
                &auth,
                &manager,
                "/mcp/section",
                SectionSelector::ById("s-001".to_string()),
                None,
            )
            .expect("get section by id failed");
        assert_eq!(by_id.section().id(), "s-001");
        assert!(by_id.content().contains("## Child"));

        let by_title = service
            .get_page_section(
                &auth,
                &manager,
                "/mcp/section",
                SectionSelector::ByTitle("Child".to_string()),
                None,
            )
            .expect("get section by title failed");
        assert_eq!(by_title.section().title(), "Child");
        assert_eq!(by_title.content().trim(), "child text");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn resolve_section_selector_rejects_ambiguous_title_as_invalid_input() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n\n## Child\n\nfirst\n\n## Child\n\nsecond\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");

        let error = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ByTitle("Child".to_string()),
            )
            .expect_err("ambiguous title must fail");

        assert_eq!(error.code(), McpErrorCode::InvalidInput);
        assert!(
            error.message().contains("section title is ambiguous"),
            "unexpected error: {}",
            error.message(),
        );
    }

    #[test]
    fn replace_section_content_keeps_heading_and_replaces_body_range() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let target = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve section by id failed");

        let replaced = service.replace_section_content(
            source,
            &sections,
            &target,
            None,
            "replaced root body",
            true,
        )
        .expect("replace section content failed");

        assert!(replaced.starts_with("# Root\n"));
        assert!(replaced.contains("replaced root body"));
        assert!(!replaced.contains("intro"));
        assert!(!replaced.contains("## Child"));
        assert!(replaced.contains("# Next"));
    }

    #[test]
    fn replace_section_content_renames_heading_and_keeps_subsections_by_default() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";

        let replaced = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("Renamed root".to_string()),
                    content: "replaced root body".to_string(),
                    include_subsection: None,
                },
            )
            .expect("replace section should succeed");

        assert!(replaced.starts_with("# Renamed root\n"));
        assert!(replaced.contains("replaced root body"));
        assert!(!replaced.contains("intro"));
        assert!(replaced.contains("## Child\n\nchild text"));
        assert!(replaced.contains("# Next\n\nnext"));
    }

    #[test]
    fn replace_section_content_trims_title_and_allows_non_heading_marks() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";

        let trimmed_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("  Trimmed root  ".to_string()),
                    content: "updated".to_string(),
                    include_subsection: None,
                },
            )
            .expect("trimmed title should succeed");
        assert!(trimmed_title.starts_with("# Trimmed root\n"));
        assert!(!trimmed_title.starts_with("#   Trimmed root"));

        let hash_tag_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("#tag".to_string()),
                    content: "updated".to_string(),
                    include_subsection: None,
                },
            )
            .expect("hash tag title should not be rejected");
        assert!(hash_tag_title.starts_with("# #tag\n"));

        let setext_like_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("Title ===".to_string()),
                    content: "updated".to_string(),
                    include_subsection: None,
                },
            )
            .expect("setext-like title should not be rejected");
        assert!(setext_like_title.starts_with("# Title ===\n"));
    }

    #[test]
    fn replace_section_content_rejects_invalid_titles() {
        let service = McpService::new();
        let source = "# Root\n\nintro\n";

        for title in [
            "",
            "   ",
            "Root\nNext",
            "Root\rNext",
            "Root\u{0000}Next",
            "# Root",
            "## Root",
        ] {
            let error = service
                .apply_edit_page_operation(
                    source,
                    &EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: Some(title.to_string()),
                        content: "updated".to_string(),
                        include_subsection: None,
                    },
                )
                .expect_err("invalid title should fail");

            assert_eq!(error.code(), McpErrorCode::InvalidInput);
        }
    }

    #[test]
    fn replace_section_content_treats_unspecified_and_false_include_subsection_equally() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";
        let operation = |include_subsection| {
            service
                .apply_edit_page_operation(
                    source,
                    &EditPageOperation::ReplaceSection {
                        section: SectionSelector::ById("s-001".to_string()),
                        title: None,
                        content: "updated root body".to_string(),
                        include_subsection,
                    },
                )
                .expect("replace section should succeed")
        };

        let unspecified = operation(None);
        let explicit_false = operation(Some(false));

        assert_eq!(unspecified, explicit_false);
        assert!(unspecified.contains("# Root\n\nupdated root body"));
        assert!(unspecified.contains("## Child\n\nchild text"));
        assert!(unspecified.contains("# Next\n\nnext"));
    }

    #[test]
    fn replace_section_content_can_replace_subsections_when_requested() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";

        let replaced = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "replaced root body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect("replace section should succeed");

        assert!(replaced.starts_with("# Root\n"));
        assert!(replaced.contains("replaced root body"));
        assert!(!replaced.contains("intro"));
        assert!(!replaced.contains("## Child"));
        assert!(replaced.contains("# Next\n\nnext"));
    }

    #[test]
    fn replace_section_content_handles_sections_without_subsections() {
        let service = McpService::new();
        let source = "# Root\n\nroot body\n\n# Next\n\nnext body\n";

        let replaced_false = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "updated root body".to_string(),
                    include_subsection: Some(false),
                },
            )
            .expect("replace section should succeed");
        assert_eq!(
            replaced_false,
            "# Root\n\nupdated root body\n\n# Next\n\nnext body\n"
        );

        let replaced_true = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "updated root body\n\n## New child\n\nchild body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect("replace section should succeed");
        assert_eq!(
            replaced_true,
            "# Root\n\nupdated root body\n\n## New child\n\nchild body\n\n# Next\n\nnext body\n"
        );
    }

    #[test]
    fn replace_section_content_handles_last_section_body_ranges() {
        let service = McpService::new();
        let source =
            "# Root\n\nroot body\n\n# Last\n\nlast body\n\n## Child\n\nchild body\n";

        let replaced_false = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ByTitle("Last".to_string()),
                    title: None,
                    content: "updated last body".to_string(),
                    include_subsection: Some(false),
                },
            )
            .expect("replace section should succeed");
        assert_eq!(
            replaced_false,
            "# Root\n\nroot body\n\n# Last\n\nupdated last body\n\n## Child\n\nchild body\n"
        );

        let replaced_true = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ByTitle("Last".to_string()),
                    title: None,
                    content: "updated last body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect("replace section should succeed");
        assert_eq!(
            replaced_true,
            "# Root\n\nroot body\n\n# Last\n\nupdated last body\n"
        );
    }

    #[test]
    fn replace_section_content_combines_title_change_and_body_range_selection() {
        let service = McpService::new();
        let source =
            "# Root\n\nintro\n\n## Child\n\nchild text\n\n# Next\n\nnext\n";

        let keep_children = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("Renamed root".to_string()),
                    content: "updated root body".to_string(),
                    include_subsection: Some(false),
                },
            )
            .expect("replace section should succeed");
        assert!(keep_children.starts_with("# Renamed root\n\nupdated root body"));
        assert!(keep_children.contains("## Child\n\nchild text"));

        let replace_children = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("Renamed root".to_string()),
                    content: "updated root body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect("replace section should succeed");
        assert!(replace_children.starts_with("# Renamed root\n\nupdated root body"));
        assert!(!replace_children.contains("## Child"));
    }

    #[test]
    fn replace_section_content_rejects_heading_misuse() {
        let service = McpService::new();
        let source = "# Root\n\nintro\n\n## Child\n\nchild text\n";

        let invalid_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("# Root".to_string()),
                    content: "updated".to_string(),
                    include_subsection: None,
                },
            )
            .expect_err("heading title must fail");
        assert_eq!(invalid_title.code(), McpErrorCode::InvalidInput);

        let invalid_content_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "updated\n\n## Unexpected\n".to_string(),
                    include_subsection: None,
                },
            )
            .expect_err("heading content must fail");
        assert_eq!(invalid_content_heading.code(), McpErrorCode::InvalidInput);

        let invalid_peer_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "updated\n\n# Peer\n".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect_err("same level heading content must fail");
        assert_eq!(invalid_peer_heading.code(), McpErrorCode::InvalidInput);
    }

    #[test]
    fn replace_section_content_rejects_target_heading_as_first_content_line() {
        let service = McpService::new();
        let source = "# Root\n\nintro\n\n## Child\n\nchild text\n";

        let existing_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "\n\n# Root\n\nupdated".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect_err("existing heading line in content should fail");
        assert_eq!(existing_heading.code(), McpErrorCode::InvalidInput);

        let new_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: Some("Renamed root".to_string()),
                    content: "\n# Renamed root\n\nupdated".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect_err("new heading line in content should fail");
        assert_eq!(new_heading.code(), McpErrorCode::InvalidInput);
    }

    #[test]
    fn replace_section_content_validates_content_heading_levels() {
        let service = McpService::new();
        let source = "# Root\n\nintro\n\n## Child\n\nchild text\n";

        let allowed_deeper_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "updated\n\n## New child\n\nchild body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect("deeper heading should be allowed");
        assert!(allowed_deeper_heading.contains("## New child\n\nchild body"));

        let same_level_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-002".to_string()),
                    title: None,
                    content: "updated\n\n## Peer\n\npeer body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect_err("same level heading should fail");
        assert_eq!(same_level_heading.code(), McpErrorCode::InvalidInput);

        let shallower_heading = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-002".to_string()),
                    title: None,
                    content: "updated\n\n# Parent\n\nparent body".to_string(),
                    include_subsection: Some(true),
                },
            )
            .expect_err("shallower heading should fail");
        assert_eq!(shallower_heading.code(), McpErrorCode::InvalidInput);
    }

    #[test]
    fn replace_section_content_does_not_over_reject_non_heading_content() {
        let service = McpService::new();
        let source = "# Root\n\nintro\n\n# Next\n\nnext\n";

        let hash_tag_content = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "Use #tag in plain text".to_string(),
                    include_subsection: Some(false),
                },
            )
            .expect("hash tag content should not be rejected");
        assert!(hash_tag_content.contains("Use #tag in plain text"));

        let setext_like_content = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-001".to_string()),
                    title: None,
                    content: "Setext-like title\n===\nplain body".to_string(),
                    include_subsection: Some(false),
                },
            )
            .expect("setext-like content should not be rejected");
        assert!(setext_like_content.contains("Setext-like title\n===\nplain body"));
    }

    #[test]
    fn insert_section_content_supports_before_and_after_anchor() {
        let service = McpService::new();
        let source = "# Root\n\nroot body\n\n# Next\n\nnext body\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let root = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve root failed");

        let inserted_before = service.insert_section_content(
            source,
            &root,
            EditPageInsertSectionPlacement::Before,
            "# Inserted Before\n\nbefore body\n\n",
        );
        assert!(inserted_before.starts_with("# Inserted Before\n\nbefore body\n\n# Root"));

        let inserted_after = service.insert_section_content(
            source,
            &root,
            EditPageInsertSectionPlacement::After,
            "# Inserted After\n\nafter body\n\n",
        );
        assert!(inserted_after.contains(
            "# Root\n\nroot body\n\n# Inserted After\n\nafter body\n\n# Next"
        ));
    }

    #[test]
    fn delete_section_content_removes_only_target_section_range() {
        let service = McpService::new();
        let source =
            "# Root\n\nroot body\n\n## Child\n\nchild body\n\n# Next\n\nnext body\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let target = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve section failed");

        let deleted = service.delete_section_content(source, &target);

        assert!(!deleted.contains("# Root"));
        assert!(!deleted.contains("root body"));
        assert!(!deleted.contains("## Child"));
        assert!(!deleted.contains("child body"));
        assert!(deleted.contains("# Next"));
        assert!(deleted.contains("next body"));
    }

    #[test]
    fn replace_text_content_supports_default_first_and_all() {
        let service = McpService::new();
        let source = "alpha beta alpha beta";

        let replaced_default = service.replace_text_content(
            source,
            "alpha",
            "gamma",
            None,
        );
        assert_eq!(replaced_default, "gamma beta alpha beta");

        let replaced_first = service.replace_text_content(
            source,
            "alpha",
            "gamma",
            Some(EditPageReplaceTextOccurrence::First),
        );
        assert_eq!(replaced_first, "gamma beta alpha beta");

        let replaced_all = service.replace_text_content(
            source,
            "alpha",
            "gamma",
            Some(EditPageReplaceTextOccurrence::All),
        );
        assert_eq!(replaced_all, "gamma beta gamma beta");
    }

    #[test]
    fn insert_section_content_keeps_inserted_section_heading() {
        let service = McpService::new();
        let source = "# Root\n\nroot body\n\n# Next\n\nnext body\n";
        let sections = service
            .parse_markdown_toc_sections(source)
            .expect("parse toc sections failed");
        let anchor = service
            .resolve_section_selector(
                &sections,
                SectionSelector::ById("s-001".to_string()),
            )
            .expect("resolve anchor failed");

        let inserted = service.insert_section_content(
            source,
            &anchor,
            EditPageInsertSectionPlacement::After,
            "## Inserted\n\ninserted body\n",
        );

        assert!(inserted.contains("## Inserted\n\ninserted body\n"));
        assert!(inserted.contains("# Root\n\nroot body\n\n## Inserted"));
        assert!(inserted.contains("inserted body\n# Next"));
    }

    #[test]
    fn apply_edit_page_operation_preserves_edit_operation_representatives() {
        let service = McpService::new();
        let source = "# Root\n\nroot body\n\n# Next\n\nnext body\n";

        let inserted_before = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::InsertSection {
                    anchor: SectionSelector::ByTitle("Root".to_string()),
                    placement: EditPageInsertSectionPlacement::Before,
                    content: "# Before\n\nbefore body\n\n".to_string(),
                },
            )
            .expect("insert before should succeed");
        assert!(inserted_before.starts_with("# Before\n\nbefore body\n\n# Root"));

        let inserted_after = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::InsertSection {
                    anchor: SectionSelector::ById("s-001".to_string()),
                    placement: EditPageInsertSectionPlacement::After,
                    content: "# After\n\nafter body\n\n".to_string(),
                },
            )
            .expect("insert after should succeed");
        assert!(inserted_after.contains("# Root\n\nroot body\n\n# After"));
        assert!(inserted_after.contains("after body\n\n# Next"));

        let deleted = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ByTitle("Root".to_string()),
                },
            )
            .expect("delete section should succeed");
        assert_eq!(deleted, "# Next\n\nnext body\n");

        let replaced_first = service
            .apply_edit_page_operation(
                "alpha beta alpha",
                &EditPageOperation::ReplaceText {
                    old_text: "alpha".to_string(),
                    new_text: "gamma".to_string(),
                    occurrence: None,
                },
            )
            .expect("replace first should succeed");
        assert_eq!(replaced_first, "gamma beta alpha");

        let replaced_all = service
            .apply_edit_page_operation(
                "alpha beta alpha",
                &EditPageOperation::ReplaceText {
                    old_text: "alpha".to_string(),
                    new_text: "gamma".to_string(),
                    occurrence: Some(EditPageReplaceTextOccurrence::All),
                },
            )
            .expect("replace all should succeed");
        assert_eq!(replaced_all, "gamma beta gamma");
    }

    #[test]
    fn apply_edit_page_operation_preserves_selector_failure_classification() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n\n## Child\n\nfirst\n\n## Child\n\nsecond\n";

        let missing_by_id = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ById("s-999".to_string()),
                },
            )
            .expect_err("missing id should fail");
        assert_eq!(missing_by_id.code(), McpErrorCode::NotFound);

        let missing_by_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ByTitle("Missing".to_string()),
                },
            )
            .expect_err("missing title should fail");
        assert_eq!(missing_by_title.code(), McpErrorCode::NotFound);

        let empty_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ByTitle("   ".to_string()),
                },
            )
            .expect_err("empty title should fail");
        assert_eq!(empty_title.code(), McpErrorCode::InvalidInput);

        let ambiguous_title = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ByTitle("Child".to_string()),
                },
            )
            .expect_err("ambiguous title should fail");
        assert_eq!(ambiguous_title.code(), McpErrorCode::InvalidInput);
    }

    #[test]
    fn apply_edit_page_operation_classifies_section_operation_failures() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n";

        let missing_replace_section = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceSection {
                    section: SectionSelector::ById("s-999".to_string()),
                    title: None,
                    content: "updated".to_string(),
                    include_subsection: None,
                },
            )
            .expect_err("missing replace target should fail");
        assert_eq!(missing_replace_section.code(), McpErrorCode::NotFound);

        let missing_insert_anchor = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::InsertSection {
                    anchor: SectionSelector::ById("s-999".to_string()),
                    placement: EditPageInsertSectionPlacement::After,
                    content: "# Inserted\n\nbody\n".to_string(),
                },
            )
            .expect_err("missing insert anchor should fail");
        assert_eq!(missing_insert_anchor.code(), McpErrorCode::NotFound);

        let missing_delete_section = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::DeleteSection {
                    section: SectionSelector::ById("s-999".to_string()),
                },
            )
            .expect_err("missing delete target should fail");
        assert_eq!(missing_delete_section.code(), McpErrorCode::NotFound);
    }

    #[test]
    fn apply_edit_page_operation_classifies_replace_text_failures() {
        let service = McpService::new();
        let source = "# Root\n\nbody\n";

        let empty_old_text = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceText {
                    old_text: "".to_string(),
                    new_text: "x".to_string(),
                    occurrence: None,
                },
            )
            .expect_err("empty old_text should fail");
        assert_eq!(empty_old_text.code(), McpErrorCode::InvalidInput);

        let missing_text = service
            .apply_edit_page_operation(
                source,
                &EditPageOperation::ReplaceText {
                    old_text: "missing".to_string(),
                    new_text: "x".to_string(),
                    occurrence: None,
                },
            )
            .expect_err("missing text should fail");
        assert_eq!(missing_text.code(), McpErrorCode::NotFound);
    }

    #[test]
    fn create_page_creates_page_with_create_scope() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Create]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * create の保存結果を検証する
         */
        let result = service
            .create_page(&auth, &manager, "/mcp/create", "# created")
            .expect("create page failed");
        let page_id = manager
            .get_page_id_by_path("/mcp/create")
            .expect("lookup page id failed")
            .expect("page id missing");
        let source = manager
            .get_page_source(&page_id, 1)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/create");
        assert_eq!(result.revision(), 1);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("created instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page created");
        assert_eq!(source.source(), "# created");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn create_page_rejects_root_path() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Create]),
            PathPrefixSet::from_iter(["/"]),
            None,
        );

        /*
         * root path 作成禁止を検証する
         */
        let err = service
            .create_page(&auth, &manager, "/", "# root")
            .expect_err("root create must fail");

        assert_eq!(err.code(), McpErrorCode::Forbidden);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn update_page_overwrites_content_and_advances_revision() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/update", "user", "# before".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * update の保存結果を検証する
         */
        let result = service
            .update_page(&auth, &manager, "/mcp/update", "# after")
            .expect("update page failed");
        let source = manager
            .get_page_source(&page_id, 2)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/update");
        assert_eq!(result.revision(), 2);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("updated instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page updated");
        assert_eq!(source.source(), "# after");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn rename_page_moves_page_and_returns_new_path() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        let page_id = manager
            .create_page("/mcp/rename", "user", "# page".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * rename の保存結果を検証する
         */
        let result = service
            .rename_page(&auth, &manager, "/mcp/rename", "/mcp/renamed")
            .expect("rename page failed");
        let new_page_id = manager
            .get_page_id_by_path("/mcp/renamed")
            .expect("lookup renamed page id failed")
            .expect("renamed page id missing");

        assert_eq!(result.path(), "/mcp/renamed");
        assert_eq!(result.revision(), 2);
        assert_eq!(
            result.instance_id(),
            &manager
                .get_page_source(&new_page_id, 2)
                .expect("lookup renamed source failed")
                .expect("renamed source missing")
                .instance_id()
                .expect("renamed instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page renamed from /mcp/rename");
        assert_eq!(new_page_id, page_id);
        assert!(manager
            .get_page_id_by_path("/mcp/rename")
            .expect("lookup old path failed")
            .is_none());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// MCP root指定を再解釈せず従来の末尾記法を維持する
    ///
    /// # 概要
    /// rootへのMCP移動は衝突、従来DB入口はbasename付加として比較する。
    ///
    /// # 注記
    /// 同じ一時DBでroot衝突の無変更と、従来root/末尾補完を確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn rename_root_destination_keeps_mcp_and_legacy_contracts_separate() {
        /*
         * 衝突するrootと移動元を用意する
         */
        let (base_dir, manager) = open_test_manager();
        manager.add_user("user", "pass", None).unwrap();
        if manager.get_page_id_by_path("/").unwrap().is_none() {
            manager.create_page("/", "user", "root".to_string()).unwrap();
        }
        let page_id = manager.create_page(
            "/mcp/item", "user", "body".to_string(),
        ).unwrap();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::new(), None,
        );

        /*
         * MCP root指定で別pathへ移動せず衝突として返す
         */
        let error = McpService::new().rename_page(
            &auth, &manager, "/mcp/item", "/",
        ).unwrap_err();
        assert_eq!(error.code(), McpErrorCode::Conflict);
        assert_eq!(manager.get_page_id_by_path("/mcp/item").unwrap(),
            Some(page_id.clone()));
        assert!(manager.get_page_id_by_path("/item").unwrap().is_none());
        assert_eq!(manager.get_page_index_by_id(&page_id).unwrap()
            .unwrap().latest(), 1);

        /*
         * 従来入口のrootおよび末尾basename補完を維持する
         */
        manager.rename_pages_recursive_by_id(&page_id, "/").unwrap();
        assert_eq!(manager.get_page_id_by_path("/item").unwrap(),
            Some(page_id.clone()));
        manager.rename_pages_recursive_by_id(&page_id, "/archive/")
            .unwrap();
        assert_eq!(manager.get_page_id_by_path("/archive/item").unwrap(),
            Some(page_id));
        fs::remove_dir_all(base_dir).unwrap();
    }

    #[test]
    fn rename_page_rejects_descendant_destination() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("user", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/mcp/tree", "user", "# page".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("user".to_string()),
            BearerScopeSet::from_iter([BearerScope::Update]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 子孫 path への移動禁止を検証する
         */
        let err = service
            .rename_page(&auth, &manager, "/mcp/tree", "/mcp/tree/child")
            .expect_err("descendant rename must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn append_page_adds_new_revision_for_different_latest_user() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .add_user("bob", "pass", None)
            .expect("add bob failed");
        let page_id = manager
            .create_page("/mcp/append", "alice", "# base".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("bob".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 別ユーザ追記時は新規 revision 追加になることを検証する
         */
        let result = service
            .append_page(&auth, &manager, "/mcp/append", "\nnext")
            .expect("append page failed");
        let source = manager
            .get_page_source(&page_id, 2)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/append");
        assert_eq!(result.revision(), 2);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("appended instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page appended");
        assert!(!result.amended());
        assert_eq!(source.source(), "# base\nnext");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn append_page_amends_when_latest_user_matches() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        let page_id = manager
            .create_page("/mcp/amend", "alice", "# base".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 同一ユーザ追記時は amend 相当になることを検証する
         */
        let result = service
            .append_page(&auth, &manager, "/mcp/amend", "\namended")
            .expect("append page failed");
        let source = manager
            .get_page_source(&page_id, 1)
            .expect("lookup source failed")
            .expect("source missing");

        assert_eq!(result.path(), "/mcp/amend");
        assert_eq!(result.revision(), 1);
        assert_eq!(
            result.instance_id(),
            &source
                .instance_id()
                .expect("amended instance_id missing")
                .to_string()
        );
        assert_eq!(result.summary(), "page appended (amended)");
        assert!(result.amended());
        assert_eq!(source.source(), "# base\namended");
        assert!(
            !manager
                .has_page_source_for_test(&page_id, 2)
                .expect("revision 2 lookup failed")
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `append_page` がロック解放を待機してから成功できることを確認する。
    ///
    /// # 注記
    /// 先にページロックを取得し、別スレッドで短時間後に解除して
    /// 待機後成功することを検証する。
    ///
    #[test]
    fn append_page_waits_for_lock_release_then_succeeds() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .add_user("bob", "pass", None)
            .expect("add bob failed");
        let page_id = manager
            .create_page("/mcp/wait", "alice", "# base".to_string())
            .expect("create page failed");
        let lock_info = manager
            .acquire_page_lock(&page_id, "alice")
            .expect("acquire lock failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("bob".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * ロック解放待ち後に追記成功することを検証する
         */
        let started_at = Instant::now();
        std::thread::scope(|scope| {
            let release_page_id = page_id.clone();
            let release_token = lock_info.token();
            let manager_ref = &manager;
            scope.spawn(move || {
                thread::sleep(Duration::from_millis(200));
                manager_ref
                    .release_page_lock(
                        &release_page_id,
                        "alice",
                        &release_token,
                    )
                    .expect("release lock failed");
            });

            let result = service
                .append_page(&auth, &manager, "/mcp/wait", "\nnext")
                .expect("append page failed");
            let source = manager
                .get_page_source(&page_id, 2)
                .expect("lookup source failed")
                .expect("source missing");

            assert_eq!(result.path(), "/mcp/wait");
            assert_eq!(result.revision(), 2);
            assert_eq!(result.summary(), "page appended");
            assert!(!result.amended());
            assert_eq!(source.source(), "# base\nnext");
        });
        assert!(
            started_at.elapsed()
                >= Duration::from_millis(APPEND_WAIT_INTERVAL_MS)
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `append_page` がロック待機 timeout 時に conflict を返すことを
    /// 確認する。
    ///
    /// # 注記
    /// ロックを保持したまま待機上限を超過させ、部分保存が無いことも
    /// 併せて確認する。
    ///
    #[test]
    fn append_page_returns_conflict_after_lock_wait_timeout() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .add_user("bob", "pass", None)
            .expect("add bob failed");
        let page_id = manager
            .create_page("/mcp/timeout", "alice", "# base".to_string())
            .expect("create page failed");
        let _lock_info = manager
            .acquire_page_lock(&page_id, "alice")
            .expect("acquire lock failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("bob".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * timeout 時に conflict になり保存されないことを検証する
         */
        let started_at = Instant::now();
        let err = service
            .append_page(&auth, &manager, "/mcp/timeout", "\nnext")
            .expect_err("append must timeout");

        assert_eq!(err.code(), McpErrorCode::Conflict);
        assert_eq!(err.message(), "page is locked");
        assert!(
            started_at.elapsed()
                >= Duration::from_millis(APPEND_WAIT_TIMEOUT_MS)
        );
        assert!(
            !manager
                .has_page_source_for_test(&page_id, 2)
                .expect("revision 2 lookup failed")
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn append_page_rejects_empty_content() {
        /*
         * テスト用データベースを準備する
         */
        let (base_dir, manager) = open_test_manager();
        manager
            .add_user("alice", "pass", None)
            .expect("add alice failed");
        manager
            .create_page("/mcp/empty", "alice", "# base".to_string())
            .expect("create page failed");
        let service = McpService::new();
        let auth = AuthContext::new(
            AuthUser::new("alice".to_string()),
            BearerScopeSet::from_iter([BearerScope::Append]),
            PathPrefixSet::from_iter(["/mcp"]),
            None,
        );

        /*
         * 空文字追記は禁止されることを検証する
         */
        let err = service
            .append_page(&auth, &manager, "/mcp/empty", "")
            .expect_err("empty append must fail");

        assert_eq!(err.code(), McpErrorCode::InvalidInput);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn unsupported_operations_return_unsupported_code() {
        let service = McpService::new();

        let deleted = service
            .get_deleted_page("/mcp/deleted")
            .expect_err("deleted page request must fail");
        let restore = service
            .restore_page("/mcp/deleted", "/mcp/restored")
            .expect_err("restore request must fail");
        let asset = service
            .asset_operation("/mcp/page")
            .expect_err("asset operation must fail");
        let lock = service
            .lock_operation("/mcp/page")
            .expect_err("lock operation must fail");

        assert_eq!(deleted.code(), McpErrorCode::Unsupported);
        assert_eq!(restore.code(), McpErrorCode::Unsupported);
        assert_eq!(asset.code(), McpErrorCode::Unsupported);
        assert_eq!(lock.code(), McpErrorCode::Unsupported);
    }
