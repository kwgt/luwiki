/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export/import 機能の公開入口
//!
#![allow(dead_code)]
#![allow(unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) mod model;
pub(crate) mod policy;
pub(crate) mod export_collect;
pub(crate) mod archive_read;
pub(crate) mod archive_write;
pub(crate) mod validate;
pub(crate) mod link_plan;
pub(crate) mod import_apply;

use anyhow::{Result, bail};

use crate::database::DatabaseManager;
use crate::database::types::PageId;

pub(crate) use export_collect::*;
pub(crate) use archive_read::*;
pub(crate) use archive_write::*;
pub(crate) use import_apply::*;
pub(crate) use link_plan::*;
pub(crate) use model::*;
pub(crate) use policy::*;
pub(crate) use validate::*;

///
/// export 実行要求
///
#[derive(Clone, Debug)]
pub(crate) struct ExportRequest {
    pub(crate) policy: ExportImportPolicy,
    pub(crate) dry_run: bool,
    pub(crate) output_path: String,
    pub(crate) password: Option<String>,
}

///
/// import 実行要求
///
#[derive(Clone, Debug)]
pub(crate) struct ImportRequest {
    pub(crate) policy: ExportImportPolicy,
    pub(crate) dry_run: bool,
    pub(crate) input_path: String,
    pub(crate) password: Option<String>,
    pub(crate) user_map: Vec<(String, String)>,
    pub(crate) strict_mode: bool,
    pub(crate) fix_broken_link: bool,
}

///
/// export 実行結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExportResult {
    pub(crate) export_type: ExportType,
    pub(crate) bundle: ExportBundle,
    pub(crate) migrate_delete_plan: Option<MigrateDeletePlan>,
}

///
/// import 実行結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImportResult {
    pub(crate) export_type: ExportType,
    pub(crate) dry_run: bool,
}

///
/// migrate export 削除直前の再確認用ページ情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MigrateExportPageSnapshot {
    pub(crate) page_id: PageId,
    pub(crate) path: String,
    pub(crate) latest: u64,
}

///
/// export の公開入口
///
/// # 引数
/// * `_db` - DB マネージャ
/// * `request` - 実行要求
///
/// # 戻り値
/// export 実行結果を返す。
///
pub(crate) fn export(
    _db: &DatabaseManager,
    request: ExportRequest,
) -> Result<ExportResult> {
    if request.output_path.is_empty() {
        bail!("output path is required");
    }

    match request.policy.export_type() {
        ExportType::Backup => export_backup(_db, request),
        ExportType::Migrate => export_migrate(_db, request),
    }
}

fn export_backup(
    db: &DatabaseManager,
    request: ExportRequest,
) -> Result<ExportResult> {
    let collected = export_collect::collect_export(db, &request.policy)?;
    if !request.dry_run {
        archive_write::write_bundle_to_output(
            &collected.bundle,
            &request.output_path,
            request.password.as_deref(),
        )?;
    }

    Ok(ExportResult {
        export_type: request.policy.export_type(),
        bundle: collected.bundle,
        migrate_delete_plan: collected.migrate_delete_plan,
    })
}

fn export_migrate(
    db: &DatabaseManager,
    request: ExportRequest,
) -> Result<ExportResult> {
    if request.output_path == "-" {
        bail!("migrate export does not support stdout output");
    }

    let collected = export_collect::collect_export(db, &request.policy)?;
    if request.dry_run {
        return Ok(ExportResult {
            export_type: request.policy.export_type(),
            bundle: collected.bundle,
            migrate_delete_plan: collected.migrate_delete_plan,
        });
    }

    let delete_plan = collected
        .migrate_delete_plan
        .clone()
        .ok_or_else(|| anyhow::anyhow!("migrate delete plan missing"))?;
    let exported_pages =
        build_migrate_export_page_snapshots(&collected.bundle);
    let prepared = archive_write::prepare_bundle_file_output(
        &collected.bundle,
        &request.output_path,
        request.password.as_deref(),
    )?;
    let mut output_guard = PreparedArchiveOutputGuard::new(prepared);
    output_guard.activate()?;

    match db.delete_for_migrate_export(
        &collected.bundle.manifest.export_root,
        &exported_pages,
        &delete_plan.draft_page_ids,
        &delete_plan.lock_page_ids,
    ) {
        Ok(()) => {
            output_guard.commit();
            Ok(ExportResult {
                export_type: request.policy.export_type(),
                bundle: collected.bundle,
                migrate_delete_plan: collected.migrate_delete_plan,
            })
        }
        Err(delete_err) => {
            if let Err(rollback_err) = output_guard.rollback() {
                return Err(anyhow::anyhow!(
                    "migrate export rollback failed after delete error: delete_error={:#}; rollback_error={:#}",
                    delete_err,
                    rollback_err
                ));
            }
            Err(delete_err)
        }
    }
}

fn build_migrate_export_page_snapshots(
    bundle: &ExportBundle,
) -> Vec<MigrateExportPageSnapshot> {
    bundle
        .pages
        .iter()
        .map(|page| MigrateExportPageSnapshot {
            page_id: page.id.clone(),
            path: rebuild_absolute_path(
                &bundle.manifest.export_root,
                &page.path,
            ),
            latest: page.latest,
        })
        .collect()
}

struct PreparedArchiveOutputGuard {
    prepared: PreparedArchiveOutput,
    backup_path: Option<PathBuf>,
    activated: bool,
}

impl PreparedArchiveOutputGuard {
    fn new(prepared: PreparedArchiveOutput) -> Self {
        Self {
            prepared,
            backup_path: None,
            activated: false,
        }
    }

    fn activate(&mut self) -> Result<()> {
        let output_path = self.prepared.output_path.clone();
        if output_path.exists() {
            let backup_path = build_archive_backup_path(&output_path)?;
            fs::rename(&output_path, &backup_path)?;
            self.backup_path = Some(backup_path);
        }

        if let Err(err) =
            archive_write::commit_prepared_bundle_output(&self.prepared)
        {
            if let Some(backup_path) = self.backup_path.take() {
                let _ = fs::rename(&backup_path, &output_path);
            }
            let _ =
                archive_write::discard_prepared_bundle_output(&self.prepared);
            return Err(err);
        }

        self.activated = true;
        Ok(())
    }

    fn commit(&mut self) {
        if let Some(backup_path) = self.backup_path.take() {
            let _ = fs::remove_file(backup_path);
        }
    }

    fn rollback(&mut self) -> Result<()> {
        if !self.activated {
            return archive_write::discard_prepared_bundle_output(
                &self.prepared,
            );
        }

        match fs::remove_file(&self.prepared.output_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }

        if let Some(backup_path) = self.backup_path.take() {
            fs::rename(backup_path, &self.prepared.output_path)?;
        }

        self.activated = false;
        Ok(())
    }
}

fn build_archive_backup_path(output_path: &Path) -> Result<PathBuf> {
    let directory = output_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let file_name = output_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "luwiki-export.zip".to_string());
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos();

    Ok(directory.join(format!(
        ".{}.{}.{}.bak",
        file_name,
        std::process::id(),
        timestamp
    )))
}

fn rebuild_absolute_path(export_root: &str, rel_path: &str) -> String {
    if export_root == "/" {
        if rel_path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", rel_path.trim_start_matches('/'))
        }
    } else if rel_path.is_empty() {
        export_root.to_string()
    } else {
        format!(
            "{}/{}",
            export_root.trim_end_matches('/'),
            rel_path.trim_start_matches('/'),
        )
    }
}

///
/// import の公開入口
///
/// # 引数
/// * `_db` - DB マネージャ
/// * `request` - 実行要求
///
/// # 戻り値
/// import 実行結果を返す。
///
pub(crate) fn import(
    _db: &DatabaseManager,
    request: ImportRequest,
) -> Result<ImportResult> {
    if request.input_path.is_empty() {
        bail!("input path is required");
    }

    let bundle = archive_read::read_bundle_from_input(
        &request.input_path,
        request.password.as_deref(),
    )?;
    let validated = validate::validate_import(
        _db,
        &request.policy,
        &request.user_map,
        request.strict_mode,
        request.fix_broken_link,
        bundle,
    )?;
    if !request.dry_run {
        import_apply::apply_import(
            _db,
            &request.policy,
            &request.user_map,
            validated.clone(),
        )?;
    }

    Ok(ImportResult {
        export_type: validated.bundle.manifest.export_type,
        dry_run: request.dry_run,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::database::DatabaseManager;

    #[test]
    fn migrate_export_writes_archive_and_deletes_tree() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");

        let page_id = manager
            .create_page("/tree", "alice", "# tree".to_string())
            .expect("create page failed");
        let child_id = manager
            .create_page("/tree/child", "alice", "# child".to_string())
            .expect("create child failed");
        let (_draft_id, _) = manager
            .create_draft_page("/tree/draft", "alice")
            .expect("create draft failed");
        manager
            .acquire_page_lock(&page_id, "alice")
            .expect("lock page failed");

        let output_path = base_dir.join("migrate.zip");
        let result = export(
            &manager,
            ExportRequest {
                policy: ExportImportPolicy::migrate("/tree")
                    .expect("build policy failed"),
                dry_run: false,
                output_path: output_path.to_string_lossy().to_string(),
                password: None,
            },
        )
        .expect("migrate export failed");

        assert_eq!(result.export_type, ExportType::Migrate);
        assert!(output_path.exists());
        assert!(
            manager
                .get_page_id_by_path("/tree")
                .expect("resolve page failed")
                .is_none()
        );
        assert!(
            manager
                .get_page_id_by_path("/tree/child")
                .expect("resolve child failed")
                .is_none()
        );
        assert!(
            manager
                .get_page_id_by_path("/tree/draft")
                .expect("resolve draft failed")
                .is_none()
        );
        assert!(
            manager
                .list_locks()
                .expect("list locks failed")
                .is_empty()
        );
        assert_eq!(result.bundle.pages.len(), 2);
        assert!(
            result
                .bundle
                .pages
                .iter()
                .any(|page| page.id == child_id)
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn migrate_delete_rejects_changed_tree_before_commit() {
        let (base_dir, db_path, asset_path) = prepare_test_dirs();
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "pass", None)
            .expect("add user failed");

        let page_id = manager
            .create_page("/tree", "alice", "# tree".to_string())
            .expect("create page failed");
        let policy = ExportImportPolicy::migrate("/tree")
            .expect("build policy failed");
        let collected =
            collect_export(&manager, &policy).expect("collect export failed");
        let delete_plan = collected
            .migrate_delete_plan
            .clone()
            .expect("delete plan missing");
        let exported_pages =
            build_migrate_export_page_snapshots(&collected.bundle);

        manager
            .put_page(&page_id, "alice", "# updated".to_string(), false)
            .expect("update page failed");

        let err = manager
            .delete_for_migrate_export(
                "/tree",
                &exported_pages,
                &delete_plan.draft_page_ids,
                &delete_plan.lock_page_ids,
            )
            .expect_err("delete should fail");

        assert!(err.to_string().contains("changed before delete"));
        assert_eq!(
            manager
                .get_page_id_by_path("/tree")
                .expect("resolve page failed"),
            Some(page_id)
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    #[test]
    fn prepared_output_guard_rollback_restores_existing_output() {
        let base_dir = Path::new("tests").join("tmp").join(unique_suffix());
        fs::create_dir_all(&base_dir).expect("create base dir failed");

        let output_path = base_dir.join("migrate.zip");
        fs::write(&output_path, b"old").expect("write existing output failed");
        let prepared_path = base_dir.join("staged.zip");
        fs::write(&prepared_path, b"new").expect("write staged output failed");
        let prepared = PreparedArchiveOutput {
            output_path: output_path.clone(),
            temp_path: prepared_path,
            write_result: ArchiveWriteResult {
                encryption_method: None,
            },
        };
        let mut guard = PreparedArchiveOutputGuard::new(prepared);

        guard.activate().expect("activate guard failed");
        guard.rollback().expect("rollback guard failed");

        assert_eq!(
            fs::read(&output_path).expect("read restored output failed"),
            b"old"
        );

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
