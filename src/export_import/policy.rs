/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! backup / migrate 差分ポリシー定義
//!
#![allow(dead_code)]

use anyhow::{Result, bail};

use super::model::{ExportType, ManifestContext};

///
/// revision.rename の出力方式
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RevisionRenameMode {
    Preserve,
    RemoveByMigrate,
}

///
/// pages.jsonl.rename_revisions の出力方式
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PageRenameRevisionsMode {
    Preserve,
    Omit,
}

///
/// import 先配置ルール
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlacementRule {
    RestoreIntoEmptyDatabase,
    RelocateByPrefix,
}

///
/// import 検証プロファイル
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ValidationPolicy {
    pub(crate) check_manifest_counts: bool,
    pub(crate) check_id_duplicates: bool,
    pub(crate) check_username_duplicates: bool,
    pub(crate) check_reference_integrity: bool,
    pub(crate) check_asset_blob_integrity: bool,
    pub(crate) check_tree_external_links: bool,
    pub(crate) check_absolute_page_links: bool,
    pub(crate) check_destination_conflicts: bool,
    pub(crate) normalize_rename_for_migrate: bool,
}

///
/// export/import の差分をまとめたポリシー
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExportImportPolicy {
    manifest_context: ManifestContext,
    revision_rename_mode: RevisionRenameMode,
    page_rename_revisions_mode: PageRenameRevisionsMode,
    placement_rule: PlacementRule,
    validation_policy: ValidationPolicy,
}

impl ExportImportPolicy {
    ///
    /// backup 用ポリシーの生成
    ///
    /// # 戻り値
    /// backup 用ポリシーを返す。
    ///
    pub(crate) fn backup() -> Self {
        Self {
            manifest_context: ManifestContext {
                export_type: ExportType::Backup,
                export_root: "/".to_string(),
                relocate_prefix: None,
            },
            revision_rename_mode: RevisionRenameMode::Preserve,
            page_rename_revisions_mode: PageRenameRevisionsMode::Preserve,
            placement_rule: PlacementRule::RestoreIntoEmptyDatabase,
            validation_policy: ValidationPolicy {
                check_manifest_counts: true,
                check_id_duplicates: true,
                check_username_duplicates: true,
                check_reference_integrity: true,
                check_asset_blob_integrity: true,
                check_tree_external_links: false,
                check_absolute_page_links: false,
                check_destination_conflicts: false,
                normalize_rename_for_migrate: false,
            },
        }
    }

    ///
    /// migrate 用ポリシーの生成
    ///
    /// # 引数
    /// * `export_root` - エクスポート対象サブツリー
    ///
    /// # 戻り値
    /// migrate 用ポリシーを返す。
    ///
    pub(crate) fn migrate(export_root: &str) -> Result<Self> {
        if export_root.is_empty() || export_root == "/" {
            bail!("migrate export_root must not be root");
        }

        Ok(Self {
            manifest_context: ManifestContext {
                export_type: ExportType::Migrate,
                export_root: export_root.to_string(),
                relocate_prefix: None,
            },
            revision_rename_mode: RevisionRenameMode::RemoveByMigrate,
            page_rename_revisions_mode: PageRenameRevisionsMode::Omit,
            placement_rule: PlacementRule::RelocateByPrefix,
            validation_policy: ValidationPolicy {
                check_manifest_counts: true,
                check_id_duplicates: true,
                check_username_duplicates: true,
                check_reference_integrity: true,
                check_asset_blob_integrity: true,
                check_tree_external_links: true,
                check_absolute_page_links: true,
                check_destination_conflicts: true,
                normalize_rename_for_migrate: true,
            },
        })
    }

    ///
    /// export 種別へのアクセサ
    ///
    /// # 戻り値
    /// export 種別を返す。
    ///
    pub(crate) fn export_type(&self) -> ExportType {
        self.manifest_context.export_type
    }

    ///
    /// export_root へのアクセサ
    ///
    /// # 戻り値
    /// export_root を返す。
    ///
    pub(crate) fn export_root(&self) -> &str {
        self.manifest_context.export_root.as_str()
    }

    ///
    /// manifest 補助情報の取得
    ///
    /// # 戻り値
    /// manifest 補助情報を返す。
    ///
    pub(crate) fn manifest_context(&self) -> ManifestContext {
        self.manifest_context.clone()
    }

    ///
    /// import 用 relocate_prefix の設定
    ///
    /// # 引数
    /// * `prefix` - import 先 prefix
    ///
    /// # 戻り値
    /// 更新後ポリシーを返す。
    ///
    pub(crate) fn with_relocate_prefix(mut self, prefix: String) -> Self {
        self.manifest_context.relocate_prefix = Some(prefix);
        self
    }

    ///
    /// rename 出力方式へのアクセサ
    ///
    /// # 戻り値
    /// rename 出力方式を返す。
    ///
    pub(crate) fn revision_rename_mode(&self) -> RevisionRenameMode {
        self.revision_rename_mode
    }

    ///
    /// page rename_revisions 出力方式へのアクセサ
    ///
    /// # 戻り値
    /// page rename_revisions 出力方式を返す。
    ///
    pub(crate) fn page_rename_revisions_mode(
        &self,
    ) -> PageRenameRevisionsMode {
        self.page_rename_revisions_mode
    }

    ///
    /// import 先配置ルールへのアクセサ
    ///
    /// # 戻り値
    /// import 先配置ルールを返す。
    ///
    pub(crate) fn placement_rule(&self) -> PlacementRule {
        self.placement_rule
    }

    ///
    /// import 検証ポリシーへのアクセサ
    ///
    /// # 戻り値
    /// import 検証ポリシーを返す。
    ///
    pub(crate) fn validation_policy(&self) -> ValidationPolicy {
        self.validation_policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_policy_preserves_rename_metadata() {
        let policy = ExportImportPolicy::backup();

        assert_eq!(policy.export_type(), ExportType::Backup);
        assert_eq!(policy.export_root(), "/");
        assert_eq!(
            policy.revision_rename_mode(),
            RevisionRenameMode::Preserve
        );
        assert_eq!(
            policy.page_rename_revisions_mode(),
            PageRenameRevisionsMode::Preserve
        );
        assert_eq!(
            policy.placement_rule(),
            PlacementRule::RestoreIntoEmptyDatabase
        );
        assert!(!policy.validation_policy().check_tree_external_links);
    }

    #[test]
    fn migrate_policy_normalizes_rename_metadata() {
        let policy = ExportImportPolicy::migrate("/docs")
            .expect("build migrate policy failed");

        assert_eq!(policy.export_type(), ExportType::Migrate);
        assert_eq!(policy.export_root(), "/docs");
        assert_eq!(
            policy.revision_rename_mode(),
            RevisionRenameMode::RemoveByMigrate
        );
        assert_eq!(
            policy.page_rename_revisions_mode(),
            PageRenameRevisionsMode::Omit
        );
        assert_eq!(
            policy.placement_rule(),
            PlacementRule::RelocateByPrefix
        );
        assert!(policy.validation_policy().check_tree_external_links);
        assert!(policy.validation_policy().normalize_rename_for_migrate);
    }

    #[test]
    fn migrate_policy_rejects_root() {
        let err = ExportImportPolicy::migrate("/")
            .expect_err("root subtree must be rejected");

        assert!(
            err.to_string().contains("must not be root"),
            "unexpected error: {err}"
        );
    }
}
