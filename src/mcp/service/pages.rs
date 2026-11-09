/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPサービスのpages処理を定義するモジュール
//!

use super::{
    APPEND_WAIT_INTERVAL_MS,
    AppendPageRequest,
    AppendServiceResult,
    AuthContext,
    DEFAULT_LIST_LIMIT,
    DEFAULT_SEARCH_LIMIT,
    DatabaseManager,
    Duration,
    EditPageRequest,
    EditPageResult,
    FtsIndexConfig,
    FtsSearchTarget,
    GetPageResult,
    GetPageSectionResult,
    GetPageTocResult,
    HashMap,
    Instant,
    ListPageItem,
    ListPagesResult,
    McpError,
    McpErrorCode,
    McpOperation,
    McpService,
    NormalizedWikiPath,
    Ordering,
    PageAccessPolicy,
    SearchPageItem,
    SearchPagesResult,
    SectionSelector,
    TocSection,
    WritePageResult,
    format_mcp_timestamp,
    fts,
    map_append_db_error,
    map_create_db_error,
    map_rename_db_error,
    map_search_error,
    path_matches_prefix,
    thread,
};

// 可読性確保のため、業務別のimplを子モジュールへ分離する。
impl McpService {
    ///
    /// `get_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象 path
    /// * `revision` - 取得 revision
    ///
    /// # 戻り値
    /// ページ本文取得結果を返す。
    ///
    pub(crate) fn get_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        revision: Option<u64>,
    ) -> Result<GetPageResult, McpError> {
        /*
         * path 認可とページ解決
         */
        let normalized_path =
            self.ensure_authorized_path(auth, McpOperation::GetPage, raw_path)?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * revision と source の解決
         */
        let (revision, instance_id, source) =
            self.resolve_revision_source(db, &resolved, revision)?;

        Ok(GetPageResult::new(
            resolved.normalized_path().to_string(),
            revision,
            instance_id,
            source,
        ))
    }

    ///
    /// `get_page_toc` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象 path
    /// * `revision` - 取得 revision
    ///
    /// # 戻り値
    /// TOC 取得結果を返す。
    ///
    pub(crate) fn get_page_toc(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        revision: Option<u64>,
    ) -> Result<GetPageTocResult, McpError> {
        /*
         * path 認可とページ解決
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::GetPageToc,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * revision と本文の解決
         */
        let (revision, instance_id, source) =
            self.resolve_revision_source(db, &resolved, revision)?;
        let parsed_sections = self.parse_markdown_toc_sections(&source)?;
        let sections = parsed_sections
            .into_iter()
            .map(|section| {
                let content = self.extract_section_content(&source, &section);
                TocSection::new(
                    section.id,
                    section.title,
                    section.level,
                    section.ordinal,
                    section.parent_id,
                    content.chars().count(),
                )
            })
            .collect();

        Ok(GetPageTocResult::new(
            resolved.normalized_path().to_string(),
            revision,
            instance_id,
            sections,
        ))
    }

    ///
    /// `list_pages` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_prefix` - 一覧対象 prefix
    /// * `limit` - 取得件数上限
    /// * `cursor` - 次ページ cursor
    ///
    /// # 戻り値
    /// 一覧取得結果を返す。
    ///
    pub(crate) fn list_pages(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_prefix: &str,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> Result<ListPagesResult, McpError> {
        /*
         * 入力検証
         */
        self.ensure_operation_scope(auth, McpOperation::ListPages)?;
        let resolved_prefix =
            self.resolve_list_prefix_request(auth, Some(raw_prefix))?;
        let limit = self.resolve_limit(limit, DEFAULT_LIST_LIMIT)?;
        let normalized_prefix =
            resolved_prefix.requested_prefix().unwrap_or("/");
        let normalized_cursor = match cursor {
            Some(cursor) => {
                let cursor = self.validate_and_normalize_path(cursor)?;
                if !path_matches_prefix(&cursor, normalized_prefix) {
                    return Err(McpError::new(
                        McpErrorCode::InvalidInput,
                        "cursor must be under requested prefix",
                    ));
                }
                Some(cursor)
            }
            None => None,
        };

        /*
         * 一覧取得と後段フィルタ
         */
        let mut entries = db
            .list_page_entries_by_prefix(normalized_prefix, false)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("list pages failed: {}", err),
                )
            })?;
        entries.sort_by_key(|entry| entry.path());

        let filtered_items = entries
            .into_iter()
            .filter(|entry| self.entry_visible_for_list(auth, entry))
            .filter(|entry| match normalized_cursor.as_deref() {
                Some(cursor) => entry.path().as_str() > cursor,
                None => true,
            })
            .collect::<Vec<_>>();

        let has_more = filtered_items.len() > limit;
        let selected = filtered_items
            .into_iter()
            .take(limit)
            .map(|entry| {
                ListPageItem::new(
                    entry.path(),
                    entry.latest_revision(),
                    format_mcp_timestamp(entry.timestamp()),
                    entry.user_name(),
                )
            })
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| selected.last().map(|item| item.path().to_string()))
            .flatten();

        Ok(ListPagesResult::new(selected, has_more, next_cursor))
    }

    ///
    /// `search_pages` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `fts_config` - FTS 設定
    /// * `query` - 検索式
    /// * `raw_prefix` - 検索 prefix
    /// * `limit` - 取得件数上限
    ///
    /// # 戻り値
    /// 検索結果を返す。
    ///
    pub(crate) fn search_pages(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        fts_config: &FtsIndexConfig,
        query: &str,
        targets: &[FtsSearchTarget],
        raw_prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<SearchPagesResult, McpError> {
        /*
         * 入力検証
         */
        self.ensure_operation_scope(auth, McpOperation::SearchPages)?;
        let query = query.trim();
        if query.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "query must not be empty",
            ));
        }
        if targets.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "target must not be empty",
            ));
        }
        let resolved_prefix =
            self.resolve_search_prefix_request(auth, raw_prefix)?;
        let limit = self.resolve_limit(limit, DEFAULT_SEARCH_LIMIT)?;

        /*
         * FTS の実行とスコアマージ
         */
        let mut merged = HashMap::new();
        for target in targets {
            let results = fts::search_index(
                fts_config,
                *target,
                query,
                false,
                false,
            )
            .map_err(map_search_error)?;
            self.merge_search_results(&mut merged, results);
        }

        /*
         * current path 解決と後段フィルタ
         */
        let page_ids = merged
            .values()
            .map(|result| result.page_id())
            .collect::<Vec<_>>();
        let current_paths = db.get_current_page_paths_by_ids(&page_ids).map_err(
            |err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("current path resolution failed: {}", err),
                )
            },
        )?;
        let mut items = Vec::new();
        for result in merged.into_values() {
            let Some(current_path) = current_paths.get(&result.page_id()) else {
                continue;
            };
            if current_path.deleted() || current_path.draft() {
                continue;
            }
            if !self.is_path_prefix_allowed(auth, current_path.current_path()) {
                continue;
            }
            if !self.matches_requested_prefix(
                &resolved_prefix,
                current_path.current_path(),
            ) {
                continue;
            }

            items.push(SearchPageItem::new(
                current_path.current_path().to_string(),
                result.revision(),
                result.score(),
                result.snippet(),
            ));
        }

        items.sort_by(|lhs, rhs| {
            rhs.score()
                .partial_cmp(&lhs.score())
                .unwrap_or(Ordering::Equal)
                .then_with(|| lhs.path().cmp(rhs.path()))
        });
        items.truncate(limit);

        Ok(SearchPagesResult::new(items))
    }

    ///
    /// `get_page_section` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 対象 path
    /// * `section_selector` - section selector
    /// * `revision` - 取得 revision
    ///
    /// # 戻り値
    /// section 取得結果を返す。
    ///
    pub(crate) fn get_page_section(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        section_selector: SectionSelector,
        revision: Option<u64>,
    ) -> Result<GetPageSectionResult, McpError> {
        /*
         * path 認可とページ解決
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::GetPageSection,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * revision と本文の解決
         */
        let (revision, _, source) =
            self.resolve_revision_source(db, &resolved, revision)?;
        let parsed_sections = self.parse_markdown_toc_sections(&source)?;
        let target = self.resolve_section_selector(
            &parsed_sections,
            section_selector,
        )?;
        let content = self.extract_section_content(&source, &target);
        let section = TocSection::new(
            target.id,
            target.title,
            target.level,
            target.ordinal,
            target.parent_id,
            content.chars().count(),
        );

        Ok(GetPageSectionResult::new(
            resolved.normalized_path().to_string(),
            revision,
            section,
            content,
        ))
    }

    ///
    /// `create_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 作成先 path
    /// * `content` - 初期 Markdown 本文
    ///
    /// # 戻り値
    /// ページ作成結果を返す。
    ///
    pub(crate) fn create_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        content: &str,
    ) -> Result<WritePageResult, McpError> {
        /*
         * 入力と認可を検証する
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::CreatePage,
            raw_path,
        )?;
        if normalized_path == "/" {
            return Err(McpError::new(
                McpErrorCode::Forbidden,
                "operation is not allowed for root page",
            ));
        }

        /*
         * 作成処理を実行する
         */
        let created = db.create_page_authorized(
            &normalized_path, auth.user_id(), content.to_string(),
            &PageAccessPolicy::new(auth.path_prefixes()),
        ).map_err(map_create_db_error)?;
        let instance_id = created.outcome.instance_id.ok_or_else(|| {
            McpError::new(McpErrorCode::InternalError,
                "created page source instance_id is missing")
        })?;
        Ok(WritePageResult::new(created.outcome.path,
            created.outcome.revision.unwrap_or(1), instance_id,
            "page created".to_string()))
    }

    ///
    /// `update_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 更新対象 path
    /// * `content` - 更新後 Markdown 本文
    ///
    /// # 戻り値
    /// ページ更新結果を返す。
    ///
    pub(crate) fn update_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        content: &str,
    ) -> Result<WritePageResult, McpError> {
        /*
         * path 認可とページ解決を行う
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::UpdatePage,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * ロック状態を確認してから保存する
         */
        let (saved_path, revision, instance_id) = self.save_updated_page(
            auth, db,
            &resolved,
            auth.user().user_id(),
            content.to_string(), None,
        )?;

        Ok(WritePageResult::new(
            saved_path,
            revision,
            instance_id,
            "page updated".to_string(),
        ))
    }

    ///
    /// `edit_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `request` - `edit_page` service 入力
    ///
    /// # 戻り値
    /// ページ編集結果を返す。
    ///
    pub(crate) fn edit_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        request: &EditPageRequest,
    ) -> Result<EditPageResult, McpError> {
        /*
         * path 認可とページ解決を先に行い、
         * 後続タスクで整合確認と operation 適用を接続する。
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::EditPage,
            request.path(),
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;
        let (latest_revision, _, _latest_source) =
            self.resolve_revision_source(db, &resolved, None)?;
        if request.revision() != latest_revision {
            return Err(McpError::new(
                McpErrorCode::NotLatestRevision,
                "revision is not latest",
            ));
        }
        let latest_page_source = db
            .get_page_source(&resolved.page_id(), latest_revision)
            .map_err(|err| {
                McpError::new(
                    McpErrorCode::InternalError,
                    format!("latest page source lookup failed: {}", err),
                )
            })?
            .ok_or_else(|| {
                McpError::new(
                    McpErrorCode::InternalError,
                    "latest page source not found",
                )
            })?;
        let latest_instance_id = latest_page_source.instance_id().ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "latest page source instance_id is missing",
            )
        })?;
        if request.instance_id() != latest_instance_id.to_string() {
            return Err(McpError::new(
                McpErrorCode::InstanceIdNotMatch,
                "instance_id does not match latest content",
            ));
        }

        /*
         * 整合確認を通過した後にだけ本文変換と update 系保存へ進める。
         */
        let updated_source = self.apply_edit_page_operation(
            &latest_page_source.source(),
            request.operation(),
        )?;
        let (saved_path, revision, instance_id) = self.save_updated_page(
            auth, db,
            &resolved,
            auth.user().user_id(),
            updated_source, Some((request.revision(), request.instance_id())),
        )?;

        Ok(EditPageResult::new(
            saved_path,
            revision,
            instance_id,
            "page edited".to_string(),
        ))
    }

    ///
    /// `rename_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - リネーム元 path
    /// * `raw_rename_to` - リネーム先 path
    ///
    /// # 戻り値
    /// ページリネーム結果を返す。
    ///
    pub(crate) fn rename_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        raw_rename_to: &str,
    ) -> Result<WritePageResult, McpError> {
        /*
         * 入力 path を正規化し、移動元ページを解決する
         */
        let normalized_path = self.validate_and_normalize_path(raw_path)?;
        let normalized_rename_to =
            self.validate_and_normalize_path(raw_rename_to)?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;

        /*
         * 認可と MCP 固有制約を検証する
         */
        self.ensure_rename_authorized(
            auth,
            resolved.normalized_path(),
            &normalized_rename_to,
        )?;
        if resolved.normalized_path() == "/" {
            return Err(McpError::new(
                McpErrorCode::Forbidden,
                "operation is not allowed for root page",
            ));
        }
        if path_matches_prefix(
            &normalized_rename_to,
            resolved.normalized_path(),
        ) {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "invalid destination path",
            ));
        }

        /*
         * 再帰 rename を実行する
         */
        let destination = NormalizedWikiPath::from_stored(
            &normalized_rename_to,
        )
        .map_err(|err| {
            McpError::new(McpErrorCode::InvalidInput, err.to_string())
        })?;
        let access = crate::database::MutationAccess {
            policy: PageAccessPolicy::new(auth.path_prefixes()),
            lock: crate::database::MutationLock::Unlocked,
            allow_draft: false, allow_deleted: false, cleanup_draft: false,
            expected_version: None,
        };
        let result = db
            .move_page_authorized(
                &resolved.page_id(),
                crate::database::PageDestination::Final(&destination),
                false,
                true,
                &access,
                false,
            )
            .map_err(map_rename_db_error)?;
        let revision = result.outcome.revision.ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "renamed page revision missing",
            )
        })?;
        let instance_id = result.outcome.instance_id.ok_or_else(|| {
            McpError::new(
                McpErrorCode::InternalError,
                "renamed page source instance_id is missing",
            )
        })?;
        let summary = if result.value {
            format!("page renamed from {}", result.outcome.source_path)
        } else {
            "page rename skipped".to_string()
        };
        Ok(WritePageResult::new(
            result.outcome.path,
            revision,
            instance_id,
            summary,
        ))
    }

    ///
    /// `append_page` を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `raw_path` - 追記対象 path
    /// * `content` - 追記文字列
    ///
    /// # 戻り値
    /// ページ追記結果を返す。
    ///
    pub(crate) fn append_page(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        raw_path: &str,
        content: &str,
    ) -> Result<AppendServiceResult, McpError> {
        /*
         * path と入力内容を検証する
         */
        let normalized_path = self.ensure_authorized_path(
            auth,
            McpOperation::AppendPage,
            raw_path,
        )?;
        let resolved = self.resolve_page_by_path(db, &normalized_path)?;
        if content.is_empty() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                "content must not be empty",
            ));
        }
        let user_id = self.resolve_auth_user_id(db, auth.user_id())?;

        /*
         * 競合が解消するまで短時間だけ再試行する
         */
        let started_at = Instant::now();
        loop {
            let state = self
                .get_append_conflict_state(auth, db, &resolved.page_id())?;
            if state.draft() {
                return Err(McpError::new(
                    McpErrorCode::Conflict,
                    "draft page is not supported",
                ));
            }

            if state.locked() {
                if self.append_wait_timed_out(started_at) {
                    return Err(McpError::new(
                        McpErrorCode::Conflict,
                        "page is locked",
                    ));
                }

                thread::sleep(Duration::from_millis(
                    APPEND_WAIT_INTERVAL_MS,
                ));
                continue;
            }

            let latest_revision = state.latest_revision().ok_or_else(|| {
                McpError::new(
                    McpErrorCode::Conflict,
                    "draft page does not have latest revision",
                )
            })?;
            let source = self.resolve_append_base_source(
                db,
                &resolved.page_id(),
                latest_revision,
            )?;
            let appended_source = format!("{}{}", source, content);
            let allow_amend = state.latest_user_id() == Some(user_id.clone());
            let request = AppendPageRequest::new(
                resolved.page_id(),
                auth.user_id().to_string(),
                appended_source,
                latest_revision,
                allow_amend,
            );

            let access = crate::database::MutationAccess {
                policy: PageAccessPolicy::new(auth.path_prefixes()),
                lock: crate::database::MutationLock::Unlocked,
                allow_draft: false, allow_deleted: false, cleanup_draft: false,
                expected_version: None,
            };
            match db.append_page_authorized(&request, &access) {
                Ok(result) => {
                    let instance =
                        result.outcome.instance_id.ok_or_else(|| {
                            McpError::new(
                                McpErrorCode::InternalError,
                                "appended page source instance_id is missing",
                            )
                        })?;
                    let summary = if result.value.amended() {
                        "page appended (amended)"
                    } else { "page appended" };
                    return Ok(AppendServiceResult::new(result.outcome.path,
                        result.value.revision(), instance, summary.to_string(),
                        result.value.amended()));
                }
                Err(err) => {
                    if self.is_retryable_append_conflict(&err) {
                        if self.append_wait_timed_out(started_at) {
                            return Err(McpError::new(
                                McpErrorCode::Conflict,
                                "append conflict",
                            ));
                        }

                        thread::sleep(Duration::from_millis(
                            APPEND_WAIT_INTERVAL_MS,
                        ));
                        continue;
                    }

                    return Err(map_append_db_error(err));
                }
            }
        }
    }

}

