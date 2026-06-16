/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! import 検証済み bundle の反映処理
//!

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

use super::model::ExportBundle;
use super::policy::{ExportImportPolicy, PlacementRule};
use super::validate::ValidatedImportBundle;
use crate::database::DatabaseManager;
use crate::database::types::{AssetId, PageId, UserId};

///
/// import 反映の入口
///
/// # 引数
/// * `db` - 反映対象 DB
/// * `policy` - import ポリシー
/// * `user_map` - ユーザマッピング
/// * `validated` - 検証済み bundle
///
/// # 戻り値
/// 反映に成功した場合は `Ok(())` を返す。
///
pub(crate) fn apply_import(
    db: &DatabaseManager,
    policy: &ExportImportPolicy,
    user_map: &[(String, String)],
    validated: ValidatedImportBundle,
) -> Result<()> {
    /*
     * 反映前条件の再確認と bundle 整形
     */
    validate_apply_target(db, policy)?;

    let mut bundle = validated.bundle;
    apply_user_map(db, &mut bundle, user_map)?;
    relocate_bundle(policy, &mut bundle)?;

    /*
     * アセットを一時配置し、DB commit 後に本配置する
     */
    let staged_assets = stage_asset_blobs(db, &bundle)?;
    match db.insert_import_bundle(&bundle) {
        Ok(()) => {
            commit_staged_assets(db, staged_assets)?;

            /*
             * import対象ページのprompt候補を同期する
             */
            let page_ids: Vec<PageId> = bundle
                .pages
                .iter()
                .map(|page| page.id.clone())
                .collect();
            db.sync_prompt_candidates_for_page_ids(&page_ids)
        }
        Err(err) => {
            discard_staged_assets(db, staged_assets);
            Err(err)
        }
    }
}

///
/// import 先条件の検証
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `policy` - import ポリシー
///
/// # 戻り値
/// 条件を満たす場合は `Ok(())` を返す。
///
fn validate_apply_target(
    db: &DatabaseManager,
    policy: &ExportImportPolicy,
) -> Result<()> {
    if policy.placement_rule() == PlacementRule::RestoreIntoEmptyDatabase
        && !is_database_empty(db)?
    {
        bail!("backup import requires empty database");
    }

    Ok(())
}

///
/// DB が空状態か判定する
///
/// # 引数
/// * `db` - 判定対象 DB
///
/// # 戻り値
/// 空状態の場合は true を返す。
///
fn is_database_empty(db: &DatabaseManager) -> Result<bool> {
    Ok(
        db.list_users()?.is_empty()
            && db.list_pages()?.is_empty()
            && db.list_assets()?.is_empty(),
    )
}

///
/// user-map を bundle へ適用する
///
/// # 引数
/// * `db` - 参照対象 DB
/// * `bundle` - 変換対象 bundle
/// * `user_map` - ユーザマッピング
///
/// # 戻り値
/// 適用に成功した場合は `Ok(())` を返す。
///
fn apply_user_map(
    db: &DatabaseManager,
    bundle: &mut ExportBundle,
    user_map: &[(String, String)],
) -> Result<()> {
    /*
     * 変換対象ユーザの解決
     */
    let mut users_by_name = HashMap::new();
    for user in &bundle.users {
        users_by_name.insert(user.username.clone(), user.id.clone());
    }

    let mut mapped_source_usernames = HashSet::new();
    let mut mapped_user_ids = HashMap::<UserId, UserId>::new();
    for (source_user, target_user) in user_map {
        let source_id = users_by_name.get(source_user).ok_or_else(|| {
            anyhow!("user_map source not found in bundle: {}", source_user)
        })?;
        let target_id = db.get_user_id_by_name(target_user)?.ok_or_else(|| {
            anyhow!("user_map target not found in database: {}", target_user)
        })?;

        mapped_source_usernames.insert(source_user.clone());
        mapped_user_ids.insert(source_id.clone(), target_id);
    }

    /*
     * 参照中の user_id を差し替える
     */
    for revision in &mut bundle.revisions {
        if let Some(target_id) = mapped_user_ids.get(&revision.user) {
            revision.user = target_id.clone();
        }
    }
    for asset in &mut bundle.assets {
        if let Some(target_id) = mapped_user_ids.get(&asset.user) {
            asset.user = target_id.clone();
        }
    }

    /*
     * DB へ追加不要なユーザを除外する
     */
    bundle
        .users
        .retain(|user| !mapped_source_usernames.contains(&user.username));

    Ok(())
}

///
/// migrate import の再配置を bundle へ反映する
///
/// # 引数
/// * `policy` - import ポリシー
/// * `bundle` - 反映対象 bundle
///
/// # 戻り値
/// 更新に成功した場合は `Ok(())` を返す。
///
fn relocate_bundle(
    policy: &ExportImportPolicy,
    bundle: &mut ExportBundle,
) -> Result<()> {
    if policy.placement_rule() != PlacementRule::RelocateByPrefix {
        return Ok(());
    }

    let prefix = policy
        .manifest_context()
        .relocate_prefix
        .ok_or_else(|| anyhow!("migrate import destination prefix is missing"))?;
    bundle.manifest.export_root = prefix.clone();
    bundle.manifest_context.export_root = prefix;

    Ok(())
}

///
/// アセット実体を一時配置する
///
/// # 引数
/// * `db` - 反映対象 DB
/// * `bundle` - 反映対象 bundle
///
/// # 戻り値
/// staged 済みアセット一覧を返す。
///
fn stage_asset_blobs(
    db: &DatabaseManager,
    bundle: &ExportBundle,
) -> Result<Vec<(AssetId, PathBuf)>> {
    let mut staged_assets = Vec::new();

    for blob in &bundle.asset_blobs {
        match db.stage_asset_blob(&blob.asset_id, &blob.data) {
            Ok(staged_path) => {
                staged_assets.push((blob.asset_id.clone(), staged_path));
            }

            Err(err) => {
                discard_staged_assets(db, staged_assets);
                return Err(err);
            }
        }
    }

    Ok(staged_assets)
}

///
/// staged アセット群を本配置する
///
/// # 引数
/// * `db` - 反映対象 DB
/// * `staged_assets` - staged 済みアセット一覧
///
/// # 戻り値
/// 本配置に成功した場合は `Ok(())` を返す。
///
fn commit_staged_assets(
    db: &DatabaseManager,
    staged_assets: Vec<(AssetId, PathBuf)>,
) -> Result<()> {
    let mut committed_asset_ids = Vec::new();
    let mut pending_assets = staged_assets.into_iter();

    while let Some((asset_id, staged_path)) = pending_assets.next() {
        match db.commit_staged_asset_blob(&staged_path, &asset_id) {
            Ok(()) => {
                committed_asset_ids.push(asset_id);
            }

            Err(err) => {
                let _ = db.discard_staged_asset_blob(&staged_path);
                for (_, pending_path) in pending_assets {
                    let _ = db.discard_staged_asset_blob(pending_path);
                }
                remove_committed_asset_files(db, &committed_asset_ids);
                return Err(err);
            }
        }
    }

    Ok(())
}

///
/// staged アセット群を破棄する
///
/// # 引数
/// * `db` - 反映対象 DB
/// * `staged_assets` - staged 済みアセット一覧
///
/// # 戻り値
/// なし
///
fn discard_staged_assets(
    db: &DatabaseManager,
    staged_assets: Vec<(AssetId, PathBuf)>,
) {
    for (_, staged_path) in staged_assets {
        let _ = db.discard_staged_asset_blob(staged_path);
    }
}

///
/// 既に本配置したアセット実体を削除する
///
/// # 引数
/// * `db` - 反映対象 DB
/// * `asset_ids` - 削除対象アセットID一覧
///
/// # 戻り値
/// なし
///
fn remove_committed_asset_files(
    db: &DatabaseManager,
    asset_ids: &[AssetId],
) {
    for asset_id in asset_ids {
        let _ = fs::remove_file(db.asset_file_path(asset_id));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::Local;

    use super::apply_import;
    use crate::database::DatabaseManager;
    use crate::database::types::{
        AssetId,
        PageId,
        UserAttribute,
        UserAttributeSet,
        UserId,
    };
    use crate::export_import::model::{
        ExportActiveRename,
        ExportAsset,
        ExportAssetBlob,
        ExportBundle,
        ExportManifest,
        ExportPage,
        ExportRevision,
        ExportRevisionRename,
        ExportType,
        ExportUser,
        ManifestContext,
    };
    use crate::export_import::policy::ExportImportPolicy;
    use crate::export_import::validate::validate_import;

    #[test]
    fn apply_import_restores_backup_into_empty_database() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let policy = ExportImportPolicy::backup();
        let bundle = build_backup_bundle();
        let validated = validate_import(
            &manager,
            &policy,
            &[],
            false,
            false,
            bundle,
        )
        .expect("validate import failed");

        apply_import(&manager, &policy, &[], validated)
            .expect("apply import failed");

        let page_id = manager
            .get_page_id_by_path("/imported")
            .expect("resolve imported page failed")
            .expect("imported page missing");
        let source = manager
            .get_page_source(&page_id, 1)
            .expect("get page source failed")
            .expect("page source missing");
        let asset_id = manager
            .get_asset_id_by_page_file(&page_id, "hello.txt")
            .expect("resolve asset failed")
            .expect("asset missing");

        assert_eq!(source.source(), "# imported");
        assert_eq!(
            manager
                .read_asset_data(&asset_id)
                .expect("read asset data failed"),
            b"hello".to_vec()
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// 複数ページのimport後同期がprompt候補だけを
    /// 生成することを確認する。
    ///
    /// # 注記
    /// 低水準DB投入後にpromptページと通常ページを
    /// まとめて同期する。
    ///
    #[test]
    fn sync_imported_page_ids_builds_prompt_candidates() {
        /*
         * promptページと通常ページを含むbundleを準備する
         */
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let mut bundle = build_backup_bundle();
        let prompt_id = bundle.pages[0].id.clone();
        let user_id = bundle.users[0].id.clone();
        let normal_id = PageId::new();
        let timestamp = Local::now();
        bundle.revisions[0].source =
            prompt_source("imported", "import description");
        bundle.pages.push(ExportPage {
            id: normal_id.clone(),
            path: "normal".to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: Some(vec![1]),
        });
        bundle.revisions.push(ExportRevision {
            page: normal_id.clone(),
            revision: 1,
            timestamp,
            user: user_id,
            rename: None,
            source: "# normal".to_string(),
        });
        bundle.sync_manifest_counts();

        /*
         * 低水準投入後にimport対象ページを同期する
         */
        manager
            .insert_import_bundle(&bundle)
            .expect("insert import bundle failed");
        manager
            .sync_prompt_candidates_for_page_ids(&[])
            .expect("sync empty page IDs failed");
        manager
            .sync_prompt_candidates_for_page_ids(&[
                prompt_id.clone(),
                normal_id.clone(),
            ])
            .expect("sync imported pages failed");

        /*
         * promptページだけに候補が生成されたことを確認する
         */
        let candidate = manager
            .get_prompt_candidate_by_page_id(&prompt_id)
            .expect("get prompt candidate failed")
            .expect("prompt candidate missing");
        assert_eq!(candidate.name(), "imported");
        assert_eq!(candidate.description(), "import description");
        assert!(
            manager
                .get_prompt_candidate_by_page_id(&normal_id)
                .expect("get normal candidate failed")
                .is_none()
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// apply_import完了後にprompt候補が
    /// 自動同期されることを確認する。
    ///
    /// # 注記
    /// promptページを持つbackup bundleを検証してimportする。
    ///
    #[test]
    fn apply_import_auto_syncs_prompt_candidate() {
        /*
         * promptページを含む検証済みbundleを準備する
         */
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let policy = ExportImportPolicy::backup();
        let mut bundle = build_backup_bundle();
        let page_id = bundle.pages[0].id.clone();
        bundle.revisions[0].source =
            prompt_source("imported", "import description");
        let validated = validate_import(
            &manager,
            &policy,
            &[],
            false,
            false,
            bundle,
        )
        .expect("validate import failed");

        /*
         * importを適用する
         */
        apply_import(&manager, &policy, &[], validated)
            .expect("apply import failed");

        /*
         * import完了後のprompt候補を確認する
         */
        let candidate = manager
            .get_prompt_candidate_by_page_id(&page_id)
            .expect("get prompt candidate failed")
            .expect("prompt candidate missing");
        assert_eq!(candidate.name(), "imported");
        assert_eq!(candidate.description(), "import description");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn apply_import_relocates_migrate_bundle_and_applies_user_map() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("server-user", "password", Some("Server User"))
            .expect("add target user failed");

        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());
        let bundle = build_migrate_bundle();
        let validated = validate_import(
            &manager,
            &policy,
            &[("bundle-user".to_string(), "server-user".to_string())],
            false,
            false,
            bundle,
        )
        .expect("validate import failed");

        apply_import(
            &manager,
            &policy,
            &[("bundle-user".to_string(), "server-user".to_string())],
            validated,
        )
        .expect("apply migrate import failed");

        let page_id = manager
            .get_page_id_by_path("/dst/child")
            .expect("resolve migrated page failed")
            .expect("migrated page missing");
        let page_source = manager
            .get_page_source(&page_id, 1)
            .expect("get migrated page source failed")
            .expect("migrated page source missing");
        let page_index = manager
            .get_page_index_by_id(&page_id)
            .expect("get page index failed")
            .expect("migrated page index missing");
        let server_user = manager
            .get_user_info_by_name("server-user")
            .expect("get server user failed")
            .expect("server user missing");
        let asset_id = manager
            .get_asset_id_by_page_file(&page_id, "move.txt")
            .expect("resolve migrated asset failed")
            .expect("migrated asset missing");
        let asset_info = manager
            .get_asset_info_by_id(&asset_id)
            .expect("get asset info failed")
            .expect("asset info missing");

        assert_eq!(page_source.user(), server_user.id());
        assert!(page_source.rename().is_removed_by_migrate());
        assert!(page_index.rename_revisions().is_empty());
        assert_eq!(asset_info.user(), server_user.id());
        assert!(
            manager
                .get_user_info_by_name("bundle-user")
                .expect("get bundle user failed")
                .is_none()
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    fn build_backup_bundle() -> ExportBundle {
        let user_id = UserId::new();
        let page_id = PageId::new();
        let asset_id = AssetId::new();
        let timestamp = Local::now();

        ExportBundle {
            manifest: ExportManifest {
                version: 1,
                export_type: ExportType::Backup,
                export_root: "/".to_string(),
                timestamp,
                page_count: 1,
                revision_count: 1,
                asset_count: 1,
            },
            users: vec![ExportUser {
                id: user_id.clone(),
                username: "import-user".to_string(),
                password: "hashed".to_string(),
                salt: [1u8; 16],
                display_name: "Import User".to_string(),
                attributes: UserAttributeSet::from_iter([
                    UserAttribute::ReadOnly,
                ]),
            }],
            pages: vec![ExportPage {
                id: page_id.clone(),
                path: "imported".to_string(),
                latest: 1,
                earliest: 1,
                rename_revisions: Some(vec![1]),
            }],
            revisions: vec![ExportRevision {
                page: page_id.clone(),
                revision: 1,
                timestamp,
                user: user_id.clone(),
                rename: None,
                source: "# imported".to_string(),
            }],
            assets: vec![ExportAsset {
                id: asset_id.clone(),
                page: page_id.clone(),
                file_name: "hello.txt".to_string(),
                mime: "text/plain".to_string(),
                size: 5,
                user: user_id,
                timestamp,
            }],
            asset_blobs: vec![ExportAssetBlob {
                asset_id,
                data: b"hello".to_vec(),
            }],
            manifest_context: ManifestContext {
                export_type: ExportType::Backup,
                export_root: "/".to_string(),
                relocate_prefix: None,
            },
        }
    }

    fn build_migrate_bundle() -> ExportBundle {
        let user_id = UserId::new();
        let page_id = PageId::new();
        let asset_id = AssetId::new();
        let timestamp = Local::now();

        ExportBundle {
            manifest: ExportManifest {
                version: 1,
                export_type: ExportType::Migrate,
                export_root: "/src".to_string(),
                timestamp,
                page_count: 1,
                revision_count: 1,
                asset_count: 1,
            },
            users: vec![ExportUser {
                id: user_id.clone(),
                username: "bundle-user".to_string(),
                password: "hashed".to_string(),
                salt: [2u8; 16],
                display_name: "Bundle User".to_string(),
                attributes: UserAttributeSet::from_iter([
                    UserAttribute::ReadOnly,
                ]),
            }],
            pages: vec![ExportPage {
                id: page_id.clone(),
                path: "child".to_string(),
                latest: 1,
                earliest: 1,
                rename_revisions: Some(vec![1]),
            }],
            revisions: vec![ExportRevision {
                page: page_id.clone(),
                revision: 1,
                timestamp,
                user: user_id.clone(),
                rename: Some(ExportRevisionRename::Active(ExportActiveRename {
                    from: Some("/src/old".to_string()),
                    to: "/src/child".to_string(),
                    link_refs: std::collections::BTreeMap::new(),
                })),
                source: "# moved".to_string(),
            }],
            assets: vec![ExportAsset {
                id: asset_id.clone(),
                page: page_id.clone(),
                file_name: "move.txt".to_string(),
                mime: "text/plain".to_string(),
                size: 5,
                user: user_id,
                timestamp,
            }],
            asset_blobs: vec![ExportAssetBlob {
                asset_id,
                data: b"moved".to_vec(),
            }],
            manifest_context: ManifestContext {
                export_type: ExportType::Migrate,
                export_root: "/src".to_string(),
                relocate_prefix: None,
            },
        }
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

    fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
        let suffix = unique_suffix();
        let base = std::env::temp_dir().join(format!(
            "luwiki-export-import-apply-{}",
            suffix
        ));
        fs::create_dir_all(&base).expect("create base dir failed");
        let db_path = base.join("database.redb");
        let asset_path = base.join("assets");
        fs::create_dir_all(&asset_path).expect("create asset dir failed");
        (base, db_path, asset_path)
    }

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

    ///
    /// import 適用時にユーザ属性が DB へ保持されることを確認する。
    ///
    /// # 注記
    /// `cargo test apply_import_preserves_user_attributes -- --exact`
    /// で実行する。
    ///
    #[test]
    fn apply_import_preserves_user_attributes() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let bundle = build_backup_bundle();
        let validated = super::super::validate::ValidatedImportBundle {
            bundle,
            link_plan: None,
            warnings: Vec::new(),
        };

        apply_import(
            &manager,
            &ExportImportPolicy::backup(),
            &[],
            validated,
        )
        .expect("apply import failed");

        let user = manager
            .get_user_info_by_name("import-user")
            .expect("get user failed")
            .expect("user not found");
        assert!(user.attributes().contains(UserAttribute::ReadOnly));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }
}
