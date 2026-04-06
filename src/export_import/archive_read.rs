/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export bundle の ZIP 読込
//!
#![allow(dead_code)]

use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use zip::ZipArchive;

use super::archive_write::{
    ASSETS_ENTRY_NAME,
    ASSET_ENTRY_PREFIX,
    MANIFEST_ENTRY_NAME,
    PAGES_ENTRY_NAME,
    REVISIONS_ENTRY_NAME,
    USERS_ENTRY_NAME,
};
use super::model::{
    ExportAsset,
    ExportAssetBlob,
    ExportBundle,
    ExportManifest,
    ExportPage,
    ExportRevision,
    ExportType,
    ExportUser,
    ManifestContext,
};

///
/// ZIP から export bundle を読み戻す
///
/// # 引数
/// * `input_path` - 入力パス、`"-"` の場合は標準入力
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 読み込んだ bundle を返す。
///
pub(crate) fn read_bundle_from_input(
    input_path: &str,
    password: Option<&str>,
) -> Result<ExportBundle> {
    /*
     * 標準入力と通常ファイル入力を切り替える
     */
    if input_path == "-" {
        let mut input = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut input)
            .context("read archive from stdin failed")?;
        return read_bundle_from_reader(Cursor::new(input), password);
    }

    let file = File::open(input_path)
        .with_context(|| format!("open archive failed: {}", input_path))?;
    read_bundle_from_reader(file, password)
}

///
/// reader から export bundle を読み戻す
///
/// # 引数
/// * `reader` - ZIP reader
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 読み込んだ bundle を返す。
///
pub(crate) fn read_bundle_from_reader<R>(
    reader: R,
    password: Option<&str>,
) -> Result<ExportBundle>
where
    R: Read + Seek,
{
    /*
     * 固定エントリを順に読み込む
     */
    let mut archive = ZipArchive::new(reader).context("open zip archive failed")?;
    let manifest: ExportManifest =
        read_json_entry(&mut archive, MANIFEST_ENTRY_NAME, password)?;
    let users: Vec<ExportUser> =
        read_jsonl_entry(&mut archive, USERS_ENTRY_NAME, password)?;
    let pages: Vec<ExportPage> =
        read_jsonl_entry(&mut archive, PAGES_ENTRY_NAME, password)?;
    let revisions: Vec<ExportRevision> =
        read_jsonl_entry(&mut archive, REVISIONS_ENTRY_NAME, password)?;
    let assets: Vec<ExportAsset> =
        read_jsonl_entry(&mut archive, ASSETS_ENTRY_NAME, password)?;

    let manifest_context = ManifestContext {
        export_type: manifest.export_type,
        export_root: manifest.export_root.clone(),
        relocate_prefix: None,
    };

    let mut bundle = ExportBundle {
        manifest,
        users,
        pages,
        revisions,
        assets,
        asset_blobs: Vec::new(),
        manifest_context,
    };

    /*
     * assets.jsonl に対応する実体ファイルを読み込む
     */
    for asset in &bundle.assets {
        let entry_name = asset_entry_name(&asset.id.to_string());
        let data = read_binary_entry(&mut archive, &entry_name, password)?;
        bundle.asset_blobs.push(ExportAssetBlob {
            asset_id: asset.id.clone(),
            data,
        });
    }

    Ok(bundle)
}

///
/// JSON エントリを読み込む
///
/// # 引数
/// * `archive` - ZIP アーカイブ
/// * `entry_name` - エントリ名
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// デシリアライズ済み値を返す。
///
fn read_json_entry<R, T>(
    archive: &mut ZipArchive<R>,
    entry_name: &str,
    password: Option<&str>,
) -> Result<T>
where
    R: Read + Seek,
    T: DeserializeOwned,
{
    let bytes = read_binary_entry(archive, entry_name, password)?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("deserialize {} failed", entry_name))
}

///
/// JSONL エントリを読み込む
///
/// # 引数
/// * `archive` - ZIP アーカイブ
/// * `entry_name` - エントリ名
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// デシリアライズ済み行配列を返す。
///
fn read_jsonl_entry<R, T>(
    archive: &mut ZipArchive<R>,
    entry_name: &str,
    password: Option<&str>,
) -> Result<Vec<T>>
where
    R: Read + Seek,
    T: DeserializeOwned,
{
    let bytes = read_binary_entry(archive, entry_name, password)?;
    decode_jsonl(&bytes, entry_name)
}

///
/// バイナリエントリを読み込む
///
/// # 引数
/// * `archive` - ZIP アーカイブ
/// * `entry_name` - エントリ名
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 読み込んだバイト列を返す。
///
fn read_binary_entry<R>(
    archive: &mut ZipArchive<R>,
    entry_name: &str,
    password: Option<&str>,
) -> Result<Vec<u8>>
where
    R: Read + Seek,
{
    /*
     * パスワード指定時は復号付きでエントリを開く
     */
    let mut file = if let Some(password) = password {
        archive
            .by_name_decrypt(entry_name, password.as_bytes())
            .with_context(|| format!("open zip entry failed: {}", entry_name))?
    } else {
        archive
            .by_name(entry_name)
            .with_context(|| format!("open zip entry failed: {}", entry_name))?
    };

    validate_entry_path(&file.name())?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .with_context(|| format!("read zip entry failed: {}", entry_name))?;
    Ok(bytes)
}

///
/// JSONL バイト列を復元する
///
/// # 引数
/// * `bytes` - JSONL バイト列
/// * `entry_name` - エントリ名
///
/// # 戻り値
/// デシリアライズ済み行配列を返す。
///
fn decode_jsonl<T>(
    bytes: &[u8],
    entry_name: &str,
) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let mut rows = Vec::new();
    let reader = BufReader::new(Cursor::new(bytes));
    for (index, line_result) in reader.lines().enumerate() {
        let line = line_result.with_context(|| {
            format!("read {} line {} failed", entry_name, index + 1)
        })?;
        if line.trim().is_empty() {
            continue;
        }

        let row = serde_json::from_str(&line).with_context(|| {
            format!("deserialize {} line {} failed", entry_name, index + 1)
        })?;
        rows.push(row);
    }
    Ok(rows)
}

///
/// ZIP エントリパスを検証する
///
/// # 引数
/// * `entry_name` - ZIP エントリ名
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_entry_path(entry_name: &str) -> Result<()> {
    if entry_name == MANIFEST_ENTRY_NAME
        || entry_name == USERS_ENTRY_NAME
        || entry_name == PAGES_ENTRY_NAME
        || entry_name == REVISIONS_ENTRY_NAME
        || entry_name == ASSETS_ENTRY_NAME
    {
        return Ok(());
    }

    if entry_name.starts_with(ASSET_ENTRY_PREFIX) {
        let path = Path::new(entry_name);
        if path.is_absolute() {
            return Err(anyhow!("zip entry path must be relative: {}", entry_name));
        }
        return Ok(());
    }

    Err(anyhow!("unexpected zip entry: {}", entry_name))
}

///
/// アセットエントリ名を生成する
///
/// # 引数
/// * `asset_id` - アセット ID
///
/// # 戻り値
/// `assets/<asset_id>` 形式のエントリ名を返す。
///
fn asset_entry_name(asset_id: &str) -> String {
    format!("{}{}", ASSET_ENTRY_PREFIX, asset_id)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::database::types::{AssetId, PageId, UserId};
    use crate::export_import::archive_write::write_bundle_to_writer;
    use crate::export_import::model::{
        ExportAsset,
        ExportManifest,
        ExportPage,
        ExportRevision,
        ManifestContext,
    };

    ///
    /// ZIP 入出力の往復で bundle が保持されることを確認
    ///
    /// # 戻り値
    /// なし
    ///
    /// # 注記
    /// `cargo test reads_round_trip_bundle` で実行する。
    ///
    #[test]
    fn reads_round_trip_bundle() {
        let bundle = sample_bundle(ExportType::Backup);
        let mut cursor = Cursor::new(Vec::new());

        write_bundle_to_writer(&bundle, &mut cursor, None)
            .expect("write zip failed");
        cursor.set_position(0);

        let restored =
            read_bundle_from_reader(cursor, None).expect("read zip failed");

        assert_eq!(restored.manifest, bundle.manifest);
        assert_eq!(restored.users, bundle.users);
        assert_eq!(restored.pages, bundle.pages);
        assert_eq!(restored.revisions, bundle.revisions);
        assert_eq!(restored.assets, bundle.assets);
        assert_eq!(restored.asset_blobs, bundle.asset_blobs);
    }

    ///
    /// AES-256 付き ZIP を読めることを確認
    ///
    /// # 戻り値
    /// なし
    ///
    /// # 注記
    /// `cargo test reads_password_protected_bundle` で実行する。
    ///
    #[test]
    fn reads_password_protected_bundle() {
        let bundle = sample_bundle(ExportType::Migrate);
        let mut cursor = Cursor::new(Vec::new());

        write_bundle_to_writer(&bundle, &mut cursor, Some("secret"))
            .expect("write encrypted zip failed");
        cursor.set_position(0);

        let restored = read_bundle_from_reader(cursor, Some("secret"))
            .expect("read encrypted zip failed");

        assert_eq!(restored.manifest.export_type, ExportType::Migrate);
        assert_eq!(restored.asset_blobs.len(), 1);
    }

    ///
    /// テスト用 bundle を生成する
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
            "/tree".to_string()
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
            username: "alice".to_string(),
            password: "hash".to_string(),
            salt: [2u8; 16],
            display_name: "Alice".to_string(),
            attributes: crate::database::types::UserAttributeSet::new(),
        });
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: "docs/page".to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: None,
        });
        bundle.revisions.push(ExportRevision {
            page: page_id.clone(),
            revision: 1,
            timestamp: chrono::Local::now(),
            user: user_id.clone(),
            rename: None,
            source: "# body".to_string(),
        });
        bundle.assets.push(ExportAsset {
            id: asset_id.clone(),
            page: page_id,
            file_name: "asset.txt".to_string(),
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
}
