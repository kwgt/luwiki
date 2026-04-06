/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export/import の中核モデル定義
//!
#![allow(dead_code)]

use std::collections::BTreeMap;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::database::types::{
    AssetId,
    Id,
    PageId,
    UserAttributeSet,
    UserId,
};

pub(crate) const EXPORT_FORMAT_VERSION: u64 = 1;

///
/// エクスポート種別
///
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExportType {
    Backup,
    Migrate,
}

impl ExportType {
    ///
    /// 文字列表現への変換
    ///
    /// # 戻り値
    /// エクスポート種別の文字列表現を返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ExportType::Backup => "backup",
            ExportType::Migrate => "migrate",
        }
    }
}

///
/// manifest.json のモデル
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExportManifest {
    pub(crate) version: u64,
    pub(crate) export_type: ExportType,
    pub(crate) export_root: String,
    pub(crate) timestamp: DateTime<Local>,
    pub(crate) page_count: u64,
    pub(crate) revision_count: u64,
    pub(crate) asset_count: u64,
}

impl ExportManifest {
    ///
    /// manifest の生成
    ///
    /// # 引数
    /// * `export_type` - エクスポート種別
    /// * `export_root` - エクスポート基準パス
    ///
    /// # 戻り値
    /// 生成した manifest を返す。
    ///
    pub(crate) fn new(
        export_type: ExportType,
        export_root: String,
    ) -> Self {
        Self {
            version: EXPORT_FORMAT_VERSION,
            export_type,
            export_root,
            timestamp: Local::now(),
            page_count: 0,
            revision_count: 0,
            asset_count: 0,
        }
    }
}

///
/// ZIP 内には格納しない補助情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManifestContext {
    pub(crate) export_type: ExportType,
    pub(crate) export_root: String,
    pub(crate) relocate_prefix: Option<String>,
}

///
/// users.jsonl の 1 行モデル
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExportUser {
    pub(crate) id: UserId,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) salt: [u8; 16],
    pub(crate) display_name: String,
    #[serde(default, skip_serializing_if = "UserAttributeSet::is_empty")]
    pub(crate) attributes: UserAttributeSet,
}

///
/// pages.jsonl の 1 行モデル
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExportPage {
    pub(crate) id: PageId,
    pub(crate) path: String,
    pub(crate) latest: u64,
    pub(crate) earliest: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rename_revisions: Option<Vec<u64>>,
}

///
/// revisions.jsonl の rename 情報
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum ExportRevisionRename {
    Active(ExportActiveRename),
    RemovedByMigrate(ExportRemovedByMigrate),
}

///
/// revisions.jsonl の有効 rename 情報
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExportActiveRename {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) from: Option<String>,
    pub(crate) to: String,
    pub(crate) link_refs: BTreeMap<String, Option<Id>>,
}

///
/// migrate 用の失効 rename 表現
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ExportRemovedByMigrate {
    #[serde(rename = "removed_by_migrate")]
    RemovedByMigrate,
}

///
/// revisions.jsonl の 1 行モデル
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExportRevision {
    pub(crate) page: PageId,
    pub(crate) revision: u64,
    pub(crate) timestamp: DateTime<Local>,
    pub(crate) user: UserId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rename: Option<ExportRevisionRename>,
    pub(crate) source: String,
}

///
/// assets.jsonl の 1 行モデル
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ExportAsset {
    pub(crate) id: AssetId,
    pub(crate) page: PageId,
    pub(crate) file_name: String,
    pub(crate) mime: String,
    pub(crate) size: u64,
    pub(crate) user: UserId,
    pub(crate) timestamp: DateTime<Local>,
}

///
/// ZIP 書込専用のアセット実体モデル
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExportAssetBlob {
    pub(crate) asset_id: AssetId,
    pub(crate) data: Vec<u8>,
}

///
/// ZIP 入出力直前に集約する bundle
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExportBundle {
    pub(crate) manifest: ExportManifest,
    pub(crate) users: Vec<ExportUser>,
    pub(crate) pages: Vec<ExportPage>,
    pub(crate) revisions: Vec<ExportRevision>,
    pub(crate) assets: Vec<ExportAsset>,
    pub(crate) asset_blobs: Vec<ExportAssetBlob>,
    pub(crate) manifest_context: ManifestContext,
}

impl ExportBundle {
    ///
    /// 空 bundle の生成
    ///
    /// # 引数
    /// * `manifest_context` - 付随する manifest 補助情報
    ///
    /// # 戻り値
    /// 空の bundle を返す。
    ///
    pub(crate) fn new(manifest_context: ManifestContext) -> Self {
        let manifest = ExportManifest::new(
            manifest_context.export_type,
            manifest_context.export_root.clone(),
        );

        Self {
            manifest,
            users: Vec::new(),
            pages: Vec::new(),
            revisions: Vec::new(),
            assets: Vec::new(),
            asset_blobs: Vec::new(),
            manifest_context,
        }
    }

    ///
    /// 件数情報を manifest へ反映する
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn sync_manifest_counts(&mut self) {
        self.manifest.page_count = self.pages.len() as u64;
        self.manifest.revision_count = self.revisions.len() as u64;
        self.manifest.asset_count = self.assets.len() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::ExportUser;
    use crate::database::types::{
        UserAttribute,
        UserAttributeSet,
        UserId,
    };

    ///
    /// users.jsonl の `attributes` 欠落を後方互換として空集合で読めることを確認する。
    ///
    /// # 注記
    /// `cargo test export_user_deserialize_accepts_missing_attributes -- --exact`
    /// で実行する。
    ///
    #[test]
    fn export_user_deserialize_accepts_missing_attributes() {
        let json = r#"{"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","username":"legacy","password":"hashed","salt":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"display_name":"Legacy User"}"#;
        let user: ExportUser =
            serde_json::from_str(json).expect("deserialize user failed");

        assert!(user.attributes.is_empty());
    }

    ///
    /// users.jsonl へ `ReadOnly` 属性を含めて直列化・復元できることを確認する。
    ///
    /// # 注記
    /// `cargo test export_user_serde_preserves_attributes -- --exact`
    /// で実行する。
    ///
    #[test]
    fn export_user_serde_preserves_attributes() {
        let user = ExportUser {
            id: UserId::new(),
            username: "alice".to_string(),
            password: "hashed".to_string(),
            salt: [7u8; 16],
            display_name: "Alice".to_string(),
            attributes: UserAttributeSet::from_iter([
                UserAttribute::NoBasicAuth,
                UserAttribute::ReadOnly,
            ]),
        };

        let json = serde_json::to_string(&user).expect("serialize user failed");
        let decoded: ExportUser =
            serde_json::from_str(&json).expect("deserialize user failed");

        assert!(json.contains("\"attributes\""));
        assert!(decoded.attributes.contains(UserAttribute::NoBasicAuth));
        assert!(decoded.attributes.contains(UserAttribute::ReadOnly));
    }
}
