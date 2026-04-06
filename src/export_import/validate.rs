/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! import 前検証
//!

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};

use super::link_plan::{LinkRewritePlan, build_migrate_link_plan};
use super::model::{
    ExportBundle,
    ExportRemovedByMigrate,
    ExportRevisionRename,
    ExportType,
};
use super::policy::{ExportImportPolicy, PlacementRule};
use crate::database::DatabaseManager;
use crate::database::types::{AssetId, PageId, UserId};

///
/// import 前検証の warning
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidationWarning {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

///
/// import 前検証の結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedImportBundle {
    pub(crate) bundle: ExportBundle,
    pub(crate) warnings: Vec<ValidationWarning>,
    pub(crate) link_plan: Option<LinkRewritePlan>,
}

///
/// import 前検証の入口
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `policy` - import ポリシー
/// * `strict_mode` - strict-mode 有効時は true
/// * `fix_broken_link` - 破損ページリンクを `about:invalid` へ置換する場合は true
/// * `bundle` - 読み込み済み export bundle
///
/// # 戻り値
/// 正規化済み bundle と warning 一覧を返す。
///
pub(crate) fn validate_import(
    db: &DatabaseManager,
    policy: &ExportImportPolicy,
    user_map: &[(String, String)],
    strict_mode: bool,
    fix_broken_link: bool,
    mut bundle: ExportBundle,
) -> Result<ValidatedImportBundle> {
    /*
     * import 種別と共通整合性の検証
     */
    validate_export_type(policy, &bundle)?;
    validate_import_target(db, policy)?;

    let validation = policy.validation_policy();
    if validation.check_manifest_counts {
        validate_manifest_counts(&bundle)?;
    }
    if validation.check_id_duplicates {
        validate_bundle_id_duplicates(&bundle)?;
        validate_existing_id_conflicts(db, &bundle, user_map)?;
    }
    if validation.check_username_duplicates {
        validate_username_duplicates(db, &bundle, user_map)?;
    }
    if validation.check_reference_integrity {
        validate_reference_integrity(&bundle)?;
    }
    if validation.check_asset_blob_integrity {
        validate_asset_blob_integrity(&bundle)?;
    }
    if validation.check_destination_conflicts {
        validate_destination_conflicts(db, policy, &bundle)?;
    }

    /*
     * migrate import 用の rename 正規化
     */
    let mut warnings = Vec::new();
    if validation.normalize_rename_for_migrate {
        normalize_rename_metadata(strict_mode, &mut bundle, &mut warnings)?;
    }

    let link_plan = if validation.check_tree_external_links
        || validation.check_absolute_page_links
    {
        let page_final_paths = build_final_page_paths(policy, &bundle);
        let destination_root = policy
            .manifest_context()
            .relocate_prefix
            .unwrap_or_else(|| policy.export_root().to_string());
        let plan = build_migrate_link_plan(
            &mut bundle,
            &page_final_paths,
            &destination_root,
            strict_mode,
            fix_broken_link,
        )?;
        warnings.extend(link_plan_warnings(&plan, fix_broken_link));
        Some(plan)
    } else {
        None
    };

    Ok(ValidatedImportBundle {
        bundle,
        warnings,
        link_plan,
    })
}

///
/// import 先条件を検証
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `policy` - import ポリシー
///
/// # 戻り値
/// 条件を満たす場合は `Ok(())` を返す。
///
fn validate_import_target(
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
/// policy と bundle の export 種別一致を検証
///
/// # 引数
/// * `policy` - import ポリシー
/// * `bundle` - 読み込み済み export bundle
///
/// # 戻り値
/// 一致している場合は `Ok(())` を返す。
///
fn validate_export_type(
    policy: &ExportImportPolicy,
    bundle: &ExportBundle,
) -> Result<()> {
    let expected = policy.export_type();
    let actual = bundle.manifest.export_type;
    if expected != actual {
        bail!(
            "import policy/export type mismatch: expected {}, actual {}",
            expected.as_str(),
            actual.as_str()
        );
    }
    Ok(())
}

///
/// manifest 件数と実データ件数の一致を検証
///
/// # 引数
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 一致している場合は `Ok(())` を返す。
///
fn validate_manifest_counts(bundle: &ExportBundle) -> Result<()> {
    let manifest = &bundle.manifest;

    if manifest.page_count != bundle.pages.len() as u64 {
        bail!(
            "manifest page_count mismatch: expected {}, actual {}",
            manifest.page_count,
            bundle.pages.len()
        );
    }
    if manifest.revision_count != bundle.revisions.len() as u64 {
        bail!(
            "manifest revision_count mismatch: expected {}, actual {}",
            manifest.revision_count,
            bundle.revisions.len()
        );
    }
    if manifest.asset_count != bundle.assets.len() as u64 {
        bail!(
            "manifest asset_count mismatch: expected {}, actual {}",
            manifest.asset_count,
            bundle.assets.len()
        );
    }

    Ok(())
}

///
/// bundle 内の ID 重複を検証
///
/// # 引数
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 重複がない場合は `Ok(())` を返す。
///
fn validate_bundle_id_duplicates(bundle: &ExportBundle) -> Result<()> {
    ensure_unique_ids(
        bundle.users.iter().map(|user| &user.id),
        "duplicate user_id in bundle",
    )?;
    ensure_unique_ids(
        bundle.pages.iter().map(|page| &page.id),
        "duplicate page_id in bundle",
    )?;
    ensure_unique_ids(
        bundle.assets.iter().map(|asset| &asset.id),
        "duplicate asset_id in bundle",
    )?;
    Ok(())
}

///
/// import 先既存データとの ID 衝突を検証
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 衝突がない場合は `Ok(())` を返す。
///
fn validate_existing_id_conflicts(
    db: &DatabaseManager,
    bundle: &ExportBundle,
    user_map: &[(String, String)],
) -> Result<()> {
    let user_mapping = resolve_user_map(db, bundle, user_map)?;
    for user in &bundle.users {
        if user_mapping.mapped_source_usernames.contains(&user.username) {
            continue;
        }
        if db.get_user_name_by_id(&user.id)?.is_some() {
            bail!("user_id already exists: {}", user.id);
        }
    }
    for page in &bundle.pages {
        if db.get_page_index_by_id(&page.id)?.is_some() {
            bail!("page_id already exists: {}", page.id);
        }
    }
    for asset in &bundle.assets {
        if db.get_asset_info_by_id(&asset.id)?.is_some() {
            bail!("asset_id already exists: {}", asset.id);
        }
    }
    Ok(())
}

///
/// username 重複を検証
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 重複がない場合は `Ok(())` を返す。
///
fn validate_username_duplicates(
    db: &DatabaseManager,
    bundle: &ExportBundle,
    user_map: &[(String, String)],
) -> Result<()> {
    let user_mapping = resolve_user_map(db, bundle, user_map)?;
    let mut seen = HashSet::new();
    for user in &bundle.users {
        if !seen.insert(user.username.clone()) {
            bail!("duplicate username in bundle: {}", user.username);
        }
        if user_mapping.mapped_source_usernames.contains(&user.username) {
            continue;
        }
        if db.get_user_id_by_name(&user.username)?.is_some() {
            bail!("username already exists: {}", user.username);
        }
    }
    Ok(())
}

///
/// JSONL 間の参照整合性を検証
///
/// # 引数
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 参照整合性が保たれている場合は `Ok(())` を返す。
///
fn validate_reference_integrity(bundle: &ExportBundle) -> Result<()> {
    /*
     * 参照先 ID 集合の構築
     */
    let page_ids: HashSet<PageId> =
        bundle.pages.iter().map(|page| page.id.clone()).collect();
    let user_ids: HashSet<UserId> =
        bundle.users.iter().map(|user| user.id.clone()).collect();
    let asset_ids: HashSet<AssetId> =
        bundle.assets.iter().map(|asset| asset.id.clone()).collect();

    /*
     * revision 参照とページ毎 revision 集合の検証
     */
    let mut revisions_by_page: HashMap<PageId, HashSet<u64>> = HashMap::new();
    for revision in &bundle.revisions {
        if !page_ids.contains(&revision.page) {
            bail!("revision references unknown page_id: {}", revision.page);
        }
        if !user_ids.contains(&revision.user) {
            bail!("revision references unknown user_id: {}", revision.user);
        }
        revisions_by_page
            .entry(revision.page.clone())
            .or_default()
            .insert(revision.revision);
    }

    /*
     * page/revision 関係の検証
     */
    for page in &bundle.pages {
        let revisions = revisions_by_page
            .get(&page.id)
            .ok_or_else(|| anyhow!("page has no revisions: {}", page.id))?;
        if page.earliest > page.latest {
            bail!(
                "page revision range is invalid: {} earliest={} latest={}",
                page.id,
                page.earliest,
                page.latest
            );
        }
        if !revisions.contains(&page.earliest) {
            bail!(
                "page earliest revision is missing: {} revision={}",
                page.id,
                page.earliest
            );
        }
        if !revisions.contains(&page.latest) {
            bail!(
                "page latest revision is missing: {} revision={}",
                page.id,
                page.latest
            );
        }
        if let Some(rename_revisions) = &page.rename_revisions {
            for revision in rename_revisions {
                if !revisions.contains(revision) {
                    bail!(
                        "page rename_revisions references missing revision: {} revision={}",
                        page.id,
                        revision
                    );
                }
            }
        }
    }

    /*
     * asset と asset blob の参照検証
     */
    for asset in &bundle.assets {
        if !page_ids.contains(&asset.page) {
            bail!("asset references unknown page_id: {}", asset.page);
        }
        if !user_ids.contains(&asset.user) {
            bail!("asset references unknown user_id: {}", asset.user);
        }
    }

    for blob in &bundle.asset_blobs {
        if !asset_ids.contains(&blob.asset_id) {
            bail!("asset blob references unknown asset_id: {}", blob.asset_id);
        }
    }

    Ok(())
}

///
/// アセット実体 blob の存在とサイズ一致を検証
///
/// # 引数
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 欠落やサイズ不一致がない場合は `Ok(())` を返す。
///
fn validate_asset_blob_integrity(bundle: &ExportBundle) -> Result<()> {
    /*
     * blob 側の重複とサイズ表の構築
     */
    let mut blob_sizes = HashMap::new();
    for blob in &bundle.asset_blobs {
        if blob_sizes
            .insert(blob.asset_id.clone(), blob.data.len() as u64)
            .is_some()
        {
            bail!("duplicate asset blob in bundle: {}", blob.asset_id);
        }
    }

    /*
     * asset メタデータとの突き合わせ
     */
    for asset in &bundle.assets {
        let actual_size = blob_sizes
            .get(&asset.id)
            .ok_or_else(|| anyhow!("asset blob is missing: {}", asset.id))?;
        if *actual_size != asset.size {
            bail!(
                "asset blob size mismatch: {} expected={} actual={}",
                asset.id,
                asset.size,
                actual_size
            );
        }
    }

    Ok(())
}

///
/// migrate import 時の配置衝突を検証
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `policy` - import ポリシー
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// 衝突がない場合は `Ok(())` を返す。
///
fn validate_destination_conflicts(
    db: &DatabaseManager,
    policy: &ExportImportPolicy,
    bundle: &ExportBundle,
) -> Result<()> {
    if policy.placement_rule() != PlacementRule::RelocateByPrefix {
        return Ok(());
    }

    /*
     * 最終配置パスの重複と既存ページ衝突を検証
     */
    let mut final_paths = HashSet::new();
    for page in &bundle.pages {
        let final_path = build_final_page_path(policy, bundle, &page.path);
        if !final_paths.insert(final_path.clone()) {
            bail!("duplicate destination page path: {}", final_path);
        }
        if db.get_page_id_by_path(&final_path)?.is_some() {
            bail!("destination page path already exists: {}", final_path);
        }

        for entry in db.list_page_entries_by_prefix(&final_path, false)? {
            let existing_path = entry.path();
            if existing_path != final_path {
                bail!(
                    "destination page path has existing child page: {} child={}",
                    final_path,
                    existing_path
                );
            }
        }
    }

    Ok(())
}

///
/// migrate import 用に rename 情報を正規化
///
/// # 引数
/// * `strict_mode` - strict-mode 有効時は true
/// * `bundle` - 正規化対象 bundle
/// * `warnings` - 追加する warning 一覧
///
/// # 戻り値
/// 正規化に成功した場合は `Ok(())` を返す。
///
fn normalize_rename_metadata(
    strict_mode: bool,
    bundle: &mut ExportBundle,
    warnings: &mut Vec<ValidationWarning>,
) -> Result<()> {
    /*
     * page 側 rename_revisions の正規化
     */
    for page in &mut bundle.pages {
        if page
            .rename_revisions
            .as_ref()
            .is_some_and(|revisions| !revisions.is_empty())
        {
            if strict_mode {
                bail!(
                    "active page rename metadata is not allowed in strict-mode: {}",
                    page.id
                );
            }
            page.rename_revisions = None;
            warnings.push(ValidationWarning {
                code: "rename_revisions_normalized",
                message: format!(
                    "page rename_revisions normalized for migrate import: {}",
                    page.id
                ),
            });
        }
    }

    /*
     * revision 側 rename の正規化
     */
    for revision in &mut bundle.revisions {
        if matches!(revision.rename, Some(ExportRevisionRename::Active(_))) {
            if strict_mode {
                bail!(
                    "active revision rename metadata is not allowed in strict-mode: {} revision={}",
                    revision.page,
                    revision.revision
                );
            }
            revision.rename = Some(ExportRevisionRename::RemovedByMigrate(
                ExportRemovedByMigrate::RemovedByMigrate,
            ));
            warnings.push(ValidationWarning {
                code: "revision_rename_normalized",
                message: format!(
                    "revision rename normalized for migrate import: {} revision={}",
                    revision.page,
                    revision.revision
                ),
            });
        }
    }

    Ok(())
}

///
/// import 後の最終ページパスを生成
///
/// # 引数
/// * `policy` - import ポリシー
/// * `bundle` - 検証対象 bundle
/// * `rel_path` - export_root 基準の相対パス
///
/// # 戻り値
/// 最終配置パスを返す。
///
fn build_final_page_path(
    policy: &ExportImportPolicy,
    bundle: &ExportBundle,
    rel_path: &str,
) -> String {
    let prefix = policy
        .manifest_context()
        .relocate_prefix
        .unwrap_or_else(|| policy.export_root().to_string());

    match bundle.manifest.export_type {
        ExportType::Backup => rebuild_absolute_path(
            &bundle.manifest.export_root,
            rel_path,
        ),
        ExportType::Migrate => rebuild_absolute_path(&prefix, rel_path),
    }
}

///
/// 絶対パスを再構築
///
/// # 引数
/// * `base_path` - 基準パス
/// * `rel_path` - 相対パス
///
/// # 戻り値
/// 再構築した絶対パスを返す。
///
fn rebuild_absolute_path(base_path: &str, rel_path: &str) -> String {
    if base_path == "/" {
        if rel_path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", rel_path.trim_start_matches('/'))
        }
    } else if rel_path.is_empty() {
        base_path.to_string()
    } else {
        format!(
            "{}/{}",
            base_path.trim_end_matches('/'),
            rel_path.trim_start_matches('/'),
        )
    }
}

///
/// page_id ごとの最終配置パスを構築する。
///
/// # 引数
/// * `policy` - import ポリシー
/// * `bundle` - 検証対象 bundle
///
/// # 戻り値
/// page_id と最終配置パスの対応表を返す。
///
fn build_final_page_paths(
    policy: &ExportImportPolicy,
    bundle: &ExportBundle,
) -> HashMap<PageId, String> {
    let mut paths = HashMap::new();
    for page in &bundle.pages {
        paths.insert(
            page.id.clone(),
            build_final_page_path(policy, bundle, &page.path),
        );
    }
    paths
}

///
/// user-map を解決する
///
/// # 引数
/// * `db` - 検証対象 DB
/// * `bundle` - 検証対象 bundle
/// * `user_map` - ユーザマッピング
///
/// # 戻り値
/// 解決済み user-map 情報を返す。
///
fn resolve_user_map(
    db: &DatabaseManager,
    bundle: &ExportBundle,
    user_map: &[(String, String)],
) -> Result<ResolvedUserMap> {
    let mut users_by_name = HashMap::new();
    for user in &bundle.users {
        users_by_name.insert(user.username.clone(), user.id.clone());
    }

    let mut resolved = ResolvedUserMap::default();
    for (source_user, target_user) in user_map {
        if let Some(previous) = resolved
            .mapping
            .insert(source_user.clone(), target_user.clone())
        {
            if previous != *target_user {
                bail!("duplicate user_map source: {}", source_user);
            }
            continue;
        }

        let _ = users_by_name.get(source_user).ok_or_else(|| {
            anyhow!("user_map source not found in bundle: {}", source_user)
        })?;
        if db.get_user_id_by_name(target_user)?.is_none() {
            bail!("user_map target not found in database: {}", target_user);
        }

        resolved
            .mapped_source_usernames
            .insert(source_user.clone());
    }

    Ok(resolved)
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

#[derive(Default)]
struct ResolvedUserMap {
    mapping: HashMap<String, String>,
    mapped_source_usernames: HashSet<String>,
}

///
/// リンク計画由来の warning 一覧を生成する。
///
/// # 引数
/// * `plan` - 生成済みリンク計画
/// * `fix_broken_link` - `about:invalid` 置換有効時は true
///
/// # 戻り値
/// warning 一覧を返す。
///
fn link_plan_warnings(
    plan: &LinkRewritePlan,
    fix_broken_link: bool,
) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    for issue in &plan.issues {
        warnings.push(ValidationWarning {
            code: issue.kind.warning_code(),
            message: format!(
                "migration link issue detected: {} revision={} kind={} target={}",
                issue.page,
                issue.revision,
                issue.kind.warning_code(),
                issue.raw_target
            ),
        });
    }

    if fix_broken_link {
        for action in &plan.actions {
            warnings.push(ValidationWarning {
                code: "broken_link_rewritten",
                message: format!(
                    "broken page link rewritten to about:invalid: {} revision={} target={}",
                    action.page,
                    action.revision,
                    action.original_target
                ),
            });
        }
    }

    warnings
}

///
/// ID 列の一意性を検証
///
/// # 引数
/// * `ids` - 検証対象 ID 列
/// * `error_prefix` - エラー文言の接頭辞
///
/// # 戻り値
/// 重複がない場合は `Ok(())` を返す。
///
fn ensure_unique_ids<'a, T, I>(
    ids: I,
    error_prefix: &str,
) -> Result<()>
where
    T: Clone + Eq + std::hash::Hash + std::fmt::Display + 'a,
    I: IntoIterator<Item = &'a T>,
{
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id.clone()) {
            bail!("{}: {}", error_prefix, id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::database::DatabaseManager;
    use crate::database::types::{AssetId, PageId, UserId};
    use crate::export_import::model::{
        ExportActiveRename,
        ExportAsset,
        ExportAssetBlob,
        ExportManifest,
        ExportPage,
        ExportRevision,
        ExportUser,
        ManifestContext,
    };

    ///
    /// manifest 件数不一致を検出できることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_rejects_manifest_count_mismatch` で実行する。
    ///
    #[test]
    fn validate_import_rejects_manifest_count_mismatch() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let mut bundle = sample_bundle(ExportType::Backup);
        bundle.manifest.page_count += 1;

        let err = validate_import(
            &manager,
            &ExportImportPolicy::backup(),
            &[],
            false,
            false,
            bundle,
        )
        .expect_err("manifest mismatch must fail");

        assert!(
            err.to_string().contains("manifest page_count mismatch"),
            "unexpected error: {err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// bundle 内重複 ID と既存 ID 衝突を検出できることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_rejects_duplicate_and_existing_ids` で実行する。
    ///
    #[test]
    fn validate_import_rejects_duplicate_and_existing_ids() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let existing_page_id = manager
            .create_page("/existing", "alice", "# body".to_string())
            .expect("create page failed");
        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());

        let mut duplicate_bundle = sample_bundle(ExportType::Migrate);
        duplicate_bundle.pages.push(duplicate_bundle.pages[0].clone());
        duplicate_bundle.manifest.page_count = duplicate_bundle.pages.len() as u64;

        let duplicate_err = validate_import(
            &manager,
            &policy,
            &[],
            false,
            false,
            duplicate_bundle,
        )
        .expect_err("duplicate page id must fail");
        assert!(
            duplicate_err
                .to_string()
                .contains("duplicate page_id in bundle"),
            "unexpected error: {duplicate_err}"
        );

        let mut existing_bundle = sample_bundle(ExportType::Migrate);
        existing_bundle.pages[0].id = existing_page_id;

        let existing_err = validate_import(
            &manager,
            &policy,
            &[],
            false,
            false,
            existing_bundle,
        )
        .expect_err("existing page id conflict must fail");
        assert!(
            existing_err.to_string().contains("page_id already exists"),
            "unexpected error: {existing_err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// username 衝突、参照切れ、asset blob 不整合を検出できることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_rejects_username_reference_and_blob_errors` で実行する。
    ///
    #[test]
    fn validate_import_rejects_username_reference_and_blob_errors() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        let migrate_policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());

        let mut username_bundle = sample_bundle(ExportType::Migrate);
        username_bundle.users[0].username = "alice".to_string();
        let username_err = validate_import(
            &manager,
            &migrate_policy,
            &[],
            false,
            false,
            username_bundle,
        )
        .expect_err("username conflict must fail");
        assert!(
            username_err
                .to_string()
                .contains("username already exists"),
            "unexpected error: {username_err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");

        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let mut reference_bundle = sample_bundle(ExportType::Backup);
        reference_bundle.revisions[0].user = UserId::new();
        let reference_err = validate_import(
            &manager,
            &ExportImportPolicy::backup(),
            &[],
            false,
            false,
            reference_bundle,
        )
        .expect_err("reference mismatch must fail");
        assert!(
            reference_err
                .to_string()
                .contains("revision references unknown user_id"),
            "unexpected error: {reference_err}"
        );

        let mut blob_bundle = sample_bundle(ExportType::Backup);
        blob_bundle.asset_blobs[0].data = b"toolong".to_vec();
        let blob_err = validate_import(
            &manager,
            &ExportImportPolicy::backup(),
            &[],
            false,
            false,
            blob_bundle,
        )
        .expect_err("blob size mismatch must fail");
        assert!(
            blob_err
                .to_string()
                .contains("asset blob size mismatch"),
            "unexpected error: {blob_err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// migrate import の配置衝突を検出できることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_rejects_migrate_destination_conflicts` で実行する。
    ///
    #[test]
    fn validate_import_rejects_migrate_destination_conflicts() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");
        manager
            .create_page("/dst/child", "alice", "# child".to_string())
            .expect("create existing child failed");

        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());
        let bundle = sample_bundle(ExportType::Migrate);

        let err = validate_import(&manager, &policy, &[], false, false, bundle)
            .expect_err("destination conflict must fail");

        assert!(
            err.to_string()
                .contains("destination page path has existing child page"),
            "unexpected error: {err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// migrate import で rename 情報を warning 付きで正規化できることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_normalizes_migrate_rename_metadata` で実行する。
    ///
    #[test]
    fn validate_import_normalizes_migrate_rename_metadata() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());
        let bundle = sample_bundle(ExportType::Migrate);

        let validated =
            validate_import(&manager, &policy, &[], false, false, bundle)
                .expect("validate failed");

        assert!(validated.bundle.pages[0].rename_revisions.is_none());
        assert!(matches!(
            validated.bundle.revisions[0].rename,
            Some(ExportRevisionRename::RemovedByMigrate(_))
        ));
        assert_eq!(validated.warnings.len(), 2);
        assert!(validated.link_plan.is_some());

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// strict-mode では有効 rename をエラーにできることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_rejects_active_rename_in_strict_mode` で実行する。
    ///
    #[test]
    fn validate_import_rejects_active_rename_in_strict_mode() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());
        let bundle = sample_bundle(ExportType::Migrate);

        let err = validate_import(&manager, &policy, &[], true, false, bundle)
            .expect_err("strict-mode must reject active rename");

        assert!(
            err.to_string()
                .contains("active page rename metadata is not allowed"),
            "unexpected error: {err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// migrate import でリンク問題を warning と書換え計画へ反映できることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_builds_migrate_link_plan` で実行する。
    ///
    #[test]
    fn validate_import_builds_migrate_link_plan() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());
        let mut bundle = sample_bundle(ExportType::Migrate);
        bundle.revisions[0].rename = None;
        bundle.pages[0].rename_revisions = None;
        bundle.revisions[0].source = [
            "[outside](../../outside)",
            "[absolute](/src/page)",
            "![asset](asset:../../asset-owner:file.png)",
            "[keep](./child)",
        ]
        .join("\n");

        let validated =
            validate_import(&manager, &policy, &[], false, true, bundle)
                .expect("validate failed");

        let plan = validated.link_plan.expect("link plan must exist");
        assert_eq!(plan.issues.len(), 3);
        assert_eq!(plan.actions.len(), 2);
        assert!(
            validated.bundle.revisions[0]
                .source
                .contains("(about:invalid)"),
            "rewritten source missing: {}",
            validated.bundle.revisions[0].source
        );
        assert!(
            validated
                .warnings
                .iter()
                .any(|warning| warning.code == "tree_external_asset_link"),
            "asset warning missing"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// strict-mode では migrate 時のリンク問題をエラーにできることを確認する。
    ///
    /// # 注記
    /// `cargo test validate_import_rejects_migrate_link_issue_in_strict_mode` で実行する。
    ///
    #[test]
    fn validate_import_rejects_migrate_link_issue_in_strict_mode() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let policy = ExportImportPolicy::migrate("/src")
            .expect("build migrate policy failed")
            .with_relocate_prefix("/dst".to_string());
        let mut bundle = sample_bundle(ExportType::Migrate);
        bundle.revisions[0].rename = None;
        bundle.pages[0].rename_revisions = None;
        bundle.revisions[0].source = "[absolute](/src/page)".to_string();

        let err = validate_import(&manager, &policy, &[], true, false, bundle)
            .expect_err("strict-mode must reject migrate link issue");

        assert!(
            err.to_string().contains("migration link issue is not allowed"),
            "unexpected error: {err}"
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// import 検証用のサンプル bundle を生成する。
    ///
    /// # 引数
    /// * `export_type` - エクスポート種別
    ///
    /// # 戻り値
    /// テスト用 bundle を返す。
    ///
    fn sample_bundle(export_type: ExportType) -> ExportBundle {
        let user_id = UserId::new();
        let page_id = PageId::new();
        let asset_id = AssetId::new();
        let export_root = if export_type == ExportType::Backup {
            "/".to_string()
        } else {
            "/src".to_string()
        };
        let manifest_context = ManifestContext {
            export_type,
            export_root: export_root.clone(),
            relocate_prefix: None,
        };
        let mut bundle = ExportBundle::new(manifest_context);
        bundle.manifest = ExportManifest::new(export_type, export_root);
        bundle.users.push(ExportUser {
            id: user_id.clone(),
            username: "bob".to_string(),
            password: "hash".to_string(),
            salt: [1u8; 16],
            display_name: "Bob".to_string(),
            attributes: crate::database::types::UserAttributeSet::new(),
        });
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: if export_type == ExportType::Backup {
                "docs/page".to_string()
            } else {
                "".to_string()
            },
            latest: 1,
            earliest: 1,
            rename_revisions: Some(vec![1]),
        });
        bundle.revisions.push(ExportRevision {
            page: page_id.clone(),
            revision: 1,
            timestamp: chrono::Local::now(),
            user: user_id.clone(),
            rename: Some(ExportRevisionRename::Active(ExportActiveRename {
                from: Some("/src/old".to_string()),
                to: "/src".to_string(),
                link_refs: Default::default(),
            })),
            source: "# body".to_string(),
        });
        bundle.assets.push(ExportAsset {
            id: asset_id.clone(),
            page: page_id,
            file_name: "note.txt".to_string(),
            mime: "text/plain".to_string(),
            size: 5,
            user: user_id,
            timestamp: chrono::Local::now(),
        });
        bundle.asset_blobs.push(ExportAssetBlob {
            asset_id,
            data: b"hello".to_vec(),
        });
        bundle.sync_manifest_counts();
        bundle
    }

    ///
    /// テスト用ディレクトリを準備する。
    ///
    /// # 戻り値
    /// (ベースディレクトリ, DB パス, アセットディレクトリ) を返す。
    ///
    fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
        let base = Path::new("tests").join("tmp").join(unique_suffix());
        fs::create_dir_all(&base).expect("create test dir failed");
        let db_path = base.join("database.redb");
        let asset_path = base.join("assets");
        fs::create_dir_all(&asset_path).expect("create asset dir failed");
        (base, db_path, asset_path)
    }

    ///
    /// テスト用の一意サフィックスを生成する。
    ///
    /// # 戻り値
    /// 一意サフィックス文字列を返す。
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
}
