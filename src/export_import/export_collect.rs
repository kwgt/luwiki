/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export 用データ収集処理
//!
#![allow(dead_code)]

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};

use super::model::{
    ExportActiveRename,
    ExportAsset,
    ExportAssetBlob,
    ExportBundle,
    ExportPage,
    ExportRemovedByMigrate,
    ExportRevision,
    ExportRevisionRename,
    ExportUser,
};
use super::policy::{
    ExportImportPolicy,
    PageRenameRevisionsMode,
    RevisionRenameMode,
};
use crate::database::DatabaseManager;
use crate::database::types::{PageId, RenameInfo};

///
/// migrate export 成功後に削除する対象
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MigrateDeletePlan {
    pub(crate) exported_page_ids: Vec<PageId>,
    pub(crate) draft_page_ids: Vec<PageId>,
    pub(crate) lock_page_ids: Vec<PageId>,
}

///
/// export 収集結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExportCollectResult {
    pub(crate) bundle: ExportBundle,
    pub(crate) migrate_delete_plan: Option<MigrateDeletePlan>,
}

///
/// export 用 bundle の収集
///
/// # 引数
/// * `db` - DB マネージャ
/// * `policy` - export ポリシー
///
/// # 戻り値
/// 収集結果を返す。
///
pub(crate) fn collect_export(
    db: &DatabaseManager,
    policy: &ExportImportPolicy,
) -> Result<ExportCollectResult> {
    let read_set = db.collect_export_read_set(
        policy.export_root(),
        policy.export_type().as_str() == "migrate",
    )?;
    let mut bundle = ExportBundle::new(policy.manifest_context());
    let mut exported_page_ids = Vec::new();
    let draft_page_ids = read_set.draft_page_ids;

    for entry in read_set.pages {
        exported_page_ids.push(entry.page_id.clone());
        bundle.pages.push(ExportPage {
            id: entry.page_id,
            path: relativize_path(policy.export_root(), &entry.path)?,
            latest: entry.index.latest(),
            earliest: entry.index.earliest(),
            rename_revisions: match policy.page_rename_revisions_mode() {
                PageRenameRevisionsMode::Preserve => {
                    Some(entry.index.rename_revisions())
                }
                PageRenameRevisionsMode::Omit => None,
            },
        });
    }

    for source_entry in read_set.revisions {
        let page_source = source_entry.source;
        bundle.revisions.push(ExportRevision {
            page: source_entry.page_id,
            revision: source_entry.revision,
            timestamp: page_source.timestamp(),
            user: page_source.user(),
            rename: convert_revision_rename(
                &page_source.rename(),
                policy.revision_rename_mode(),
            ),
            source: page_source.source(),
        });
    }

    for asset_entry in read_set.assets {
        let asset = asset_entry.asset;
        bundle.assets.push(ExportAsset {
            id: asset.id(),
            page: asset
                .page_id()
                .ok_or_else(|| anyhow!("asset page_id not found"))?,
            file_name: asset.file_name(),
            mime: asset.mime(),
            size: asset.size(),
            user: asset.user(),
            timestamp: asset.timestamp(),
        });
        bundle.asset_blobs.push(ExportAssetBlob {
            asset_id: asset.id(),
            data: asset_entry.data,
        });
    }

    for user in read_set.users {
        bundle.users.push(ExportUser {
            id: user.id(),
            username: user.username(),
            password: user.password(),
            salt: user.salt(),
            display_name: user.display_name(),
            attributes: user.attributes(),
        });
    }

    sort_bundle(&mut bundle);
    bundle.sync_manifest_counts();

    let migrate_delete_plan = if policy.export_type().as_str() == "migrate" {
        let lock_page_ids = build_lock_page_ids(&exported_page_ids, &draft_page_ids);
        Some(MigrateDeletePlan {
            exported_page_ids,
            draft_page_ids,
            lock_page_ids,
        })
    } else {
        None
    };

    Ok(ExportCollectResult {
        bundle,
        migrate_delete_plan,
    })
}

fn convert_revision_rename(
    rename: &RenameInfo,
    mode: RevisionRenameMode,
) -> Option<ExportRevisionRename> {
    match mode {
        RevisionRenameMode::Preserve => match rename {
            RenameInfo::None => None,
            RenameInfo::Active { from, to, link_refs } => {
                Some(ExportRevisionRename::Active(ExportActiveRename {
                    from: from.clone(),
                    to: to.clone(),
                    link_refs: link_refs.clone(),
                }))
            }
            RenameInfo::RemovedByMigrate => Some(
                ExportRevisionRename::RemovedByMigrate(
                    ExportRemovedByMigrate::RemovedByMigrate,
                ),
            ),
        },
        RevisionRenameMode::RemoveByMigrate => {
            if matches!(rename, RenameInfo::None) {
                None
            } else {
                Some(ExportRevisionRename::RemovedByMigrate(
                    ExportRemovedByMigrate::RemovedByMigrate,
                ))
            }
        }
    }
}

fn sort_bundle(bundle: &mut ExportBundle) {
    bundle.users.sort_by_key(|user| user.username.clone());
    bundle.pages.sort_by_key(|page| page.path.clone());
    bundle.revisions.sort_by_key(|revision| {
        (revision.page.to_string(), revision.revision)
    });
    bundle.assets.sort_by_key(|asset| asset.id.to_string());
    bundle.asset_blobs.sort_by_key(|blob| blob.asset_id.to_string());
}

fn build_lock_page_ids(
    exported_page_ids: &[PageId],
    draft_page_ids: &[PageId],
) -> Vec<PageId> {
    let mut ids = BTreeSet::new();
    for page_id in exported_page_ids {
        ids.insert(page_id.clone());
    }
    for page_id in draft_page_ids {
        ids.insert(page_id.clone());
    }
    ids.into_iter().collect()
}

fn relativize_path(base_path: &str, path: &str) -> Result<String> {
    if base_path == "/" {
        if path == "/" {
            return Ok(String::new());
        }
        return Ok(path.trim_start_matches('/').to_string());
    }

    if path == base_path {
        return Ok(String::new());
    }

    let prefix = format!("{}/", base_path.trim_end_matches('/'));
    match path.strip_prefix(&prefix) {
        Some(value) => Ok(value.to_string()),
        None => Err(anyhow!(
            "path is outside export_root: path={}, export_root={}",
            path,
            base_path
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::database::DatabaseManager;
    use crate::database::types::UserAttribute;
    use crate::export_import::policy::ExportImportPolicy;

    #[test]
    fn backup_collect_preserves_rename_and_assets() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");

        let page_id = manager
            .create_page("/tree/a", "alice", "# body".to_string())
            .expect("create page failed");
        manager
            .rename_page("/tree/a", "/tree/b")
            .expect("rename page failed");
        manager
            .create_asset(
                &page_id,
                "note.txt",
                "text/plain",
                "alice",
                b"hello",
            )
            .expect("create asset failed");
        let _ = manager
            .create_draft_page("/tree/draft", "alice")
            .expect("create draft failed");

        let collected = collect_export(&manager, &ExportImportPolicy::backup())
            .expect("collect export failed");

        assert!(collected.migrate_delete_plan.is_none());
        assert_eq!(collected.bundle.pages.len(), 1);
        assert_eq!(collected.bundle.pages[0].path, "tree/b");
        assert_eq!(
            collected.bundle.pages[0].rename_revisions.as_ref(),
            Some(&vec![1, 2])
        );
        assert_eq!(collected.bundle.revisions.len(), 2);
        assert!(matches!(
            collected.bundle.revisions[0].rename,
            Some(ExportRevisionRename::Active(_))
        ));
        assert!(matches!(
            collected.bundle.revisions[1].rename,
            Some(ExportRevisionRename::Active(_))
        ));
        assert_eq!(collected.bundle.assets.len(), 1);
        assert_eq!(collected.bundle.asset_blobs.len(), 1);

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn migrate_collect_normalizes_rename_and_collects_delete_targets() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");

        let page_id = manager
            .create_page("/tree/a", "alice", "# body".to_string())
            .expect("create page failed");
        manager
            .rename_page("/tree/a", "/tree/b")
            .expect("rename page failed");
        let (draft_id, _) = manager
            .create_draft_page("/tree/draft", "alice")
            .expect("create draft failed");
        manager
            .create_page("/other/page", "alice", "# other".to_string())
            .expect("create extra page failed");

        let policy = ExportImportPolicy::migrate("/tree")
            .expect("build migrate policy failed");
        let collected =
            collect_export(&manager, &policy).expect("collect export failed");
        let delete_plan = collected
            .migrate_delete_plan
            .expect("migrate delete plan missing");

        assert_eq!(collected.bundle.pages.len(), 1);
        assert_eq!(collected.bundle.pages[0].path, "b");
        assert!(collected.bundle.pages[0].rename_revisions.is_none());
        assert_eq!(collected.bundle.revisions.len(), 2);
        assert!(collected
            .bundle
            .revisions
            .iter()
            .all(|revision| matches!(
                revision.rename,
                Some(ExportRevisionRename::RemovedByMigrate(_))
            )));
        assert_eq!(delete_plan.exported_page_ids, vec![page_id.clone()]);
        assert_eq!(delete_plan.draft_page_ids, vec![draft_id.clone()]);
        assert_eq!(delete_plan.lock_page_ids.len(), 2);
        assert!(delete_plan.lock_page_ids.contains(&page_id));
        assert!(delete_plan.lock_page_ids.contains(&draft_id));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// export 収集時にユーザ属性が `users.jsonl` モデルへ保持されることを確認する。
    ///
    /// # 注記
    /// `cargo test backup_collect_preserves_user_attributes -- --exact`
    /// で実行する。
    ///
    #[test]
    fn backup_collect_preserves_user_attributes() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user_with_attributes(
                "alice",
                Some("pass"),
                None,
                crate::database::types::UserAttributeSet::from_iter([
                    UserAttribute::ReadOnly,
                ]),
            )
            .expect("add user failed");
        manager
            .create_page("/tree/a", "alice", "# body".to_string())
            .expect("create page failed");

        let collected = collect_export(&manager, &ExportImportPolicy::backup())
            .expect("collect export failed");

        assert_eq!(collected.bundle.users.len(), 1);
        assert!(collected.bundle.users[0]
            .attributes
            .contains(UserAttribute::ReadOnly));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    fn prepare_test_dirs() -> (PathBuf, PathBuf, PathBuf) {
        let base = Path::new("tests").join("tmp").join(unique_suffix());
        fs::create_dir_all(&base).expect("create test dir failed");
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
}
