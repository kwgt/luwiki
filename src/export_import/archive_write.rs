/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! export bundle の ZIP 書込
//!
#![allow(dead_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Seek, Write};
#[cfg(target_family = "windows")]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use zip::write::{FileOptions, ZipWriter};
use zip::{AesMode, CompressionMethod};

use super::model::{ExportAssetBlob, ExportBundle};

pub(crate) const MANIFEST_ENTRY_NAME: &str = "manifest.json";
pub(crate) const USERS_ENTRY_NAME: &str = "users.jsonl";
pub(crate) const PAGES_ENTRY_NAME: &str = "pages.jsonl";
pub(crate) const REVISIONS_ENTRY_NAME: &str = "revisions.jsonl";
pub(crate) const ASSETS_ENTRY_NAME: &str = "assets.jsonl";
pub(crate) const ASSET_ENTRY_PREFIX: &str = "assets/";

///
/// ZIP で使用した暗号方式
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchiveEncryptionMethod {
    Aes256,
}

///
/// ZIP 書込結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArchiveWriteResult {
    pub(crate) encryption_method: Option<ArchiveEncryptionMethod>,
}

///
/// 最終確定前の一時アーカイブ出力
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedArchiveOutput {
    pub(crate) output_path: PathBuf,
    pub(crate) temp_path: PathBuf,
    pub(crate) write_result: ArchiveWriteResult,
}

///
/// export bundle を出力先へ書き出す
///
/// # 引数
/// * `bundle` - 出力対象 bundle
/// * `output_path` - 出力先パス、`"-"` の場合は標準出力
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 書込結果を返す。
///
pub(crate) fn write_bundle_to_output(
    bundle: &ExportBundle,
    output_path: &str,
    password: Option<&str>,
) -> Result<ArchiveWriteResult> {
    /*
     * 標準出力向け出力を処理
     */
    if output_path == "-" {
        return write_bundle_to_stdout(bundle, password);
    }

    /*
     * 一時アーカイブを生成して最終出力先へ確定
     */
    let output = Path::new(output_path);
    let temp_path = build_temp_archive_path(Some(output))?;
    let result = write_bundle_to_file(bundle, &temp_path, password)?;
    replace_output_file(&temp_path, output)?;

    Ok(result)
}

///
/// export bundle をファイル出力向けに一時アーカイブへ書き出す
///
/// # 引数
/// * `bundle` - 出力対象 bundle
/// * `output_path` - 最終出力先パス
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 確定前の一時出力情報を返す。
///
pub(crate) fn prepare_bundle_file_output(
    bundle: &ExportBundle,
    output_path: &str,
    password: Option<&str>,
) -> Result<PreparedArchiveOutput> {
    if output_path == "-" {
        return Err(anyhow!(
            "prepared archive output does not support stdout"
        ));
    }

    let output = Path::new(output_path);
    let temp_path = build_temp_archive_path(Some(output))?;
    let write_result = write_bundle_to_file(bundle, &temp_path, password)?;

    Ok(PreparedArchiveOutput {
        output_path: output.to_path_buf(),
        temp_path,
        write_result,
    })
}

///
/// 一時アーカイブを最終出力先へ確定する
///
/// # 引数
/// * `prepared` - 一時出力情報
///
/// # 戻り値
/// 確定に成功した場合は `Ok(())` を返す。
///
pub(crate) fn commit_prepared_bundle_output(
    prepared: &PreparedArchiveOutput,
) -> Result<()> {
    replace_output_file(&prepared.temp_path, &prepared.output_path)
}

///
/// 一時アーカイブを破棄する
///
/// # 引数
/// * `prepared` - 一時出力情報
///
/// # 戻り値
/// 破棄に成功した場合は `Ok(())` を返す。
///
pub(crate) fn discard_prepared_bundle_output(
    prepared: &PreparedArchiveOutput,
) -> Result<()> {
    match fs::remove_file(&prepared.temp_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

///
/// export bundle を任意 writer へ ZIP 書込する
///
/// # 引数
/// * `bundle` - 出力対象 bundle
/// * `writer` - 書込先 writer
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 書込結果を返す。
///
pub(crate) fn write_bundle_to_writer<W>(
    bundle: &ExportBundle,
    writer: W,
    password: Option<&str>,
) -> Result<ArchiveWriteResult>
where
    W: Write + Seek,
{
    /*
     * ZIP ライタを初期化して全エントリを書き込む
     */
    let mut zip = ZipWriter::new(writer);
    let write_result = write_entries(&mut zip, bundle, password)?;
    zip.finish().context("zip finish failed")?;
    Ok(write_result)
}

///
/// export bundle を一時ファイルへ ZIP 書込する
///
/// # 引数
/// * `bundle` - 出力対象 bundle
/// * `path` - 一時ファイルパス
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 書込結果を返す。
///
fn write_bundle_to_file(
    bundle: &ExportBundle,
    path: &Path,
    password: Option<&str>,
) -> Result<ArchiveWriteResult> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create temp archive failed: {}", path.display()))?;

    let result = write_bundle_to_writer(bundle, file, password)?;
    Ok(result)
}

///
/// export bundle を標準出力へ ZIP 書込する
///
/// # 引数
/// * `bundle` - 出力対象 bundle
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 書込結果を返す。
///
fn write_bundle_to_stdout(
    bundle: &ExportBundle,
    password: Option<&str>,
) -> Result<ArchiveWriteResult> {
    /*
     * 一時アーカイブへ書き込む
     */
    let temp_path = build_temp_archive_path(None)?;
    let result = write_bundle_to_file(bundle, &temp_path, password)?;

    /*
     * 完成済みアーカイブを標準出力へ転送して一時ファイルを削除
     */
    let mut temp_file = File::open(&temp_path).with_context(|| {
        format!("open temp archive failed: {}", temp_path.display())
    })?;
    let mut stdout = io::stdout().lock();
    io::copy(&mut temp_file, &mut stdout).context("write archive to stdout failed")?;
    stdout.flush().context("flush stdout failed")?;
    fs::remove_file(&temp_path).with_context(|| {
        format!("remove temp archive failed: {}", temp_path.display())
    })?;

    Ok(result)
}

///
/// ZIP エントリ群を書き込む
///
/// # 引数
/// * `zip` - ZIP ライタ
/// * `bundle` - 出力対象 bundle
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// 書込結果を返す。
///
fn write_entries<W>(
    zip: &mut ZipWriter<W>,
    bundle: &ExportBundle,
    password: Option<&str>,
) -> Result<ArchiveWriteResult>
where
    W: Write + Seek,
{
    /*
     * 共通オプションを生成して固定順序で書き込む
     */
    let write_options = build_file_options(password)?;

    write_json_entry(
        zip,
        MANIFEST_ENTRY_NAME,
        &bundle.manifest,
        write_options.file_options,
    )?;
    write_jsonl_entry(
        zip,
        USERS_ENTRY_NAME,
        &bundle.users,
        write_options.file_options,
    )?;
    write_jsonl_entry(
        zip,
        PAGES_ENTRY_NAME,
        &bundle.pages,
        write_options.file_options,
    )?;
    write_jsonl_entry(
        zip,
        REVISIONS_ENTRY_NAME,
        &bundle.revisions,
        write_options.file_options,
    )?;
    write_jsonl_entry(
        zip,
        ASSETS_ENTRY_NAME,
        &bundle.assets,
        write_options.file_options,
    )?;

    for blob in &bundle.asset_blobs {
        let entry_name = asset_entry_name(blob);
        write_binary_entry(
            zip,
            &entry_name,
            &blob.data,
            write_options.file_options,
        )?;
    }

    Ok(ArchiveWriteResult {
        encryption_method: write_options.encryption_method,
    })
}

///
/// JSON エントリを書き込む
///
/// # 引数
/// * `zip` - ZIP ライタ
/// * `entry_name` - エントリ名
/// * `value` - 書込対象
/// * `options` - ZIP ファイルオプション
///
/// # 戻り値
/// 書込に成功した場合は `Ok(())` を返す。
///
fn write_json_entry<'a, W, T>(
    zip: &mut ZipWriter<W>,
    entry_name: &str,
    value: &T,
    options: FileOptions<'a, ()>,
) -> Result<()>
where
    W: Write + Seek,
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("serialize {} failed", entry_name))?;
    write_binary_entry(zip, entry_name, &bytes, options)
}

///
/// JSONL エントリを書き込む
///
/// # 引数
/// * `zip` - ZIP ライタ
/// * `entry_name` - エントリ名
/// * `rows` - 書込対象行
/// * `options` - ZIP ファイルオプション
///
/// # 戻り値
/// 書込に成功した場合は `Ok(())` を返す。
///
fn write_jsonl_entry<'a, W, T>(
    zip: &mut ZipWriter<W>,
    entry_name: &str,
    rows: &[T],
    options: FileOptions<'a, ()>,
) -> Result<()>
where
    W: Write + Seek,
    T: Serialize,
{
    let bytes = encode_jsonl(rows)
        .with_context(|| format!("serialize {} failed", entry_name))?;
    write_binary_entry(zip, entry_name, &bytes, options)
}

///
/// バイナリエントリを書き込む
///
/// # 引数
/// * `zip` - ZIP ライタ
/// * `entry_name` - エントリ名
/// * `bytes` - 書込データ
/// * `options` - ZIP ファイルオプション
///
/// # 戻り値
/// 書込に成功した場合は `Ok(())` を返す。
///
fn write_binary_entry<'a, W>(
    zip: &mut ZipWriter<W>,
    entry_name: &str,
    bytes: &[u8],
    options: FileOptions<'a, ()>,
) -> Result<()>
where
    W: Write + Seek,
{
    zip.start_file(entry_name, options)
        .with_context(|| format!("start zip entry failed: {}", entry_name))?;
    zip.write_all(bytes)
        .with_context(|| format!("write zip entry failed: {}", entry_name))?;
    Ok(())
}

///
/// JSONL バイト列へ変換する
///
/// # 引数
/// * `rows` - JSONL 行データ
///
/// # 戻り値
/// JSONL 化したバイト列を返す。
///
fn encode_jsonl<T>(rows: &[T]) -> Result<Vec<u8>>
where
    T: Serialize,
{
    let mut cursor = Cursor::new(Vec::new());
    for row in rows {
        serde_json::to_writer(&mut cursor, row).context("jsonl encode failed")?;
        cursor.write_all(b"\n").context("jsonl newline write failed")?;
    }
    Ok(cursor.into_inner())
}

///
/// ZIP 書込オプション
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArchiveFileOptions<'a> {
    file_options: FileOptions<'a, ()>,
    encryption_method: Option<ArchiveEncryptionMethod>,
}

///
/// ZIP ファイルオプションを構築する
///
/// # 引数
/// * `password` - ZIP パスワード
///
/// # 戻り値
/// ZIP 書込オプションを返す。
///
fn build_file_options(password: Option<&str>) -> Result<ArchiveFileOptions<'_>> {
    let base = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let Some(password) = password else {
        return Ok(ArchiveFileOptions {
            file_options: base,
            encryption_method: None,
        });
    };

    if password.is_empty() {
        return Err(anyhow!("zip password must not be empty"));
    }

    Ok(ArchiveFileOptions {
        file_options: base.with_aes_encryption(AesMode::Aes256, password),
        encryption_method: Some(ArchiveEncryptionMethod::Aes256),
    })
}

///
/// 一時ファイルを最終出力先へ確定する
///
/// # 引数
/// * `temp_path` - 一時ファイルパス
/// * `output` - 最終出力先パス
///
/// # 戻り値
/// 確定に成功した場合は `Ok(())` を返す。
///
fn replace_output_file(temp_path: &Path, output: &Path) -> Result<()> {
    #[cfg(target_family = "windows")]
    {
        replace_output_file_windows(temp_path, output)
    }

    #[cfg(not(target_family = "windows"))]
    {
        replace_output_file_unix(temp_path, output)
    }
}

///
/// 一時ファイルを最終出力先へ確定する
///
/// # 引数
/// * `temp_path` - 一時ファイルパス
/// * `output` - 最終出力先パス
///
/// # 戻り値
/// 確定に成功した場合は `Ok(())` を返す。
///
#[cfg(not(target_family = "windows"))]
fn replace_output_file_unix(temp_path: &Path, output: &Path) -> Result<()> {
    fs::rename(temp_path, output).with_context(|| {
        format!(
            "replace archive failed: temp={}, output={}",
            temp_path.display(),
            output.display()
        )
    })?;
    Ok(())
}

///
/// 一時ファイルを最終出力先へ確定する
///
/// # 引数
/// * `temp_path` - 一時ファイルパス
/// * `output` - 最終出力先パス
///
/// # 戻り値
/// 確定に成功した場合は `Ok(())` を返す。
///
#[cfg(target_family = "windows")]
fn replace_output_file_windows(temp_path: &Path, output: &Path) -> Result<()> {
    use std::ptr::null;

    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    /*
     * 出力先が存在しない場合は通常 rename を使う
     */
    if !output.exists() {
        fs::rename(temp_path, output).with_context(|| {
            format!(
                "replace archive failed: temp={}, output={}",
                temp_path.display(),
                output.display()
            )
        })?;
        return Ok(());
    }

    /*
     * 既存ファイルがある場合は ReplaceFileW で置換する
     */
    let replaced = to_wide_path(output);
    let replacement = to_wide_path(temp_path);
    let result = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            null(),
            0,
            null(),
            null(),
        )
    };

    if result == 0 {
        return Err(anyhow!(
            "replace archive failed: temp={}, output={}",
            temp_path.display(),
            output.display()
        ));
    }

    Ok(())
}

///
/// Windows API 用の UTF-16 パスへ変換する
///
/// # 引数
/// * `path` - 変換対象パス
///
/// # 戻り値
/// NUL 終端付き UTF-16 パスを返す。
///
#[cfg(target_family = "windows")]
fn to_wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

///
/// ZIP 一時ファイルパスを生成する
///
/// # 引数
/// * `output` - 最終出力先パス
///
/// # 戻り値
/// 一時ファイルパスを返す。
///
fn build_temp_archive_path(output: Option<&Path>) -> Result<PathBuf> {
    let directory = match output.and_then(Path::parent) {
        Some(path) if !path.as_os_str().is_empty() => path.to_path_buf(),
        _ if output.is_some() => PathBuf::from("."),
        _ => std::env::temp_dir(),
    };
    let file_name = match output.and_then(Path::file_name) {
        Some(name) => name.to_string_lossy().to_string(),
        None => "luwiki-export.zip".to_string(),
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?
        .as_nanos();

    Ok(directory.join(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        timestamp
    )))
}

///
/// アセットエントリ名を生成する
///
/// # 引数
/// * `blob` - アセット実体
///
/// # 戻り値
/// `assets/<asset_id>` 形式のエントリ名を返す。
///
pub(crate) fn asset_entry_name(blob: &ExportAssetBlob) -> String {
    format!("{}{}", ASSET_ENTRY_PREFIX, blob.asset_id)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    use super::*;
    use crate::database::types::{AssetId, Id, PageId, UserId};
    use crate::export_import::model::{
        ExportAsset,
        ExportManifest,
        ExportPage,
        ExportRevision,
        ExportType,
        ExportUser,
        ManifestContext,
    };

    ///
    /// 固定順序・固定エントリ名で ZIP が出力されることを確認
    ///
    /// # 戻り値
    /// なし
    ///
    /// # 注記
    /// `cargo test writes_fixed_entries_in_expected_order` で実行する。
    ///
    #[test]
    fn writes_fixed_entries_in_expected_order() {
        let bundle = sample_bundle();
        let asset_entry_name = asset_entry_name(&bundle.asset_blobs[0]);
        let mut cursor = Cursor::new(Vec::new());

        let result = write_bundle_to_writer(&bundle, &mut cursor, None)
            .expect("write zip failed");

        assert_eq!(result.encryption_method, None);
        cursor.seek(SeekFrom::Start(0)).expect("rewind failed");

        let archive =
            zip::ZipArchive::new(cursor).expect("open written zip failed");
        let names = archive
            .file_names()
            .map(|name| name.to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                MANIFEST_ENTRY_NAME.to_string(),
                USERS_ENTRY_NAME.to_string(),
                PAGES_ENTRY_NAME.to_string(),
                REVISIONS_ENTRY_NAME.to_string(),
                ASSETS_ENTRY_NAME.to_string(),
                asset_entry_name,
            ]
        );
    }

    ///
    /// AES-256 付き ZIP が出力されることを確認
    ///
    /// # 戻り値
    /// なし
    ///
    /// # 注記
    /// `cargo test writes_password_protected_zip_with_aes` で実行する。
    ///
    #[test]
    fn writes_password_protected_zip_with_aes() {
        let bundle = sample_bundle();
        let mut cursor = Cursor::new(Vec::new());

        let result = write_bundle_to_writer(&bundle, &mut cursor, Some("secret"))
            .expect("write encrypted zip failed");

        assert_eq!(
            result.encryption_method,
            Some(ArchiveEncryptionMethod::Aes256)
        );
        cursor.seek(SeekFrom::Start(0)).expect("rewind failed");

        let mut archive =
            zip::ZipArchive::new(cursor).expect("open written zip failed");
        let mut file = archive
            .by_name_decrypt(MANIFEST_ENTRY_NAME, b"secret")
            .expect("open encrypted manifest failed");
        let mut body = String::new();
        file.read_to_string(&mut body)
            .expect("read encrypted manifest failed");
        assert!(body.contains("\"export_type\": \"backup\""));
    }

    ///
    /// テスト用 bundle を生成する
    ///
    /// # 戻り値
    /// テスト用 bundle を返す。
    ///
    fn sample_bundle() -> ExportBundle {
        let user_id = UserId::new();
        let page_id = PageId::new();
        let asset_id = AssetId::new();
        let manifest_context = ManifestContext {
            export_type: ExportType::Backup,
            export_root: "/".to_string(),
            relocate_prefix: None,
        };
        let mut bundle = ExportBundle::new(manifest_context);
        bundle.manifest = ExportManifest::new(ExportType::Backup, "/".to_string());
        bundle.users.push(ExportUser {
            id: user_id.clone(),
            username: "alice".to_string(),
            password: "hash".to_string(),
            salt: [1u8; 16],
            display_name: "Alice".to_string(),
        });
        bundle.pages.push(ExportPage {
            id: page_id.clone(),
            path: "docs/page".to_string(),
            latest: 1,
            earliest: 1,
            rename_revisions: Some(vec![1]),
        });
        bundle.revisions.push(ExportRevision {
            page: page_id.clone(),
            revision: 1,
            timestamp: chrono::Local::now(),
            user: user_id.clone(),
            rename: None,
            source: "# title".to_string(),
        });
        bundle.assets.push(ExportAsset {
            id: asset_id.clone(),
            page: page_id,
            file_name: "image.png".to_string(),
            mime: "image/png".to_string(),
            size: 3,
            user: user_id,
            timestamp: chrono::Local::now(),
        });
        bundle.asset_blobs.push(ExportAssetBlob {
            asset_id,
            data: vec![1, 2, 3],
        });
        bundle.sync_manifest_counts();
        let _unused_id = Id::new();
        bundle
    }
}
