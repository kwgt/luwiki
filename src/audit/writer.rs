/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 監査ログ JSONL writer の骨格を定義するモジュール
//!

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, warn};

use super::model::AuditRecord;
use super::rotation::{
    AuditRotationPolicy,
    active_log_path,
    rotate_active_file,
    should_rotate,
};

///
/// 監査ログ書込先情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuditWriterConfig {
    /// 監査ログ出力ディレクトリ
    pub(crate) output_dir: PathBuf,

    /// ローテーション設定
    pub(crate) rotation_policy: AuditRotationPolicy,
}

///
/// 監査ログ writer の骨格
///
pub(crate) struct AuditWriter {
    /// writer 設定
    config: AuditWriterConfig,

    /// アクティブファイル writer
    file: Option<BufWriter<File>>,

    /// 現在のアクティブファイルサイズ
    current_size: u64,
}

impl std::fmt::Debug for AuditWriter {
    ///
    /// Debug 表現を生成する
    ///
    /// # 引数
    /// * `f` - フォーマッタ
    ///
    /// # 戻り値
    /// Debug 構造体出力結果を返す。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditWriter")
            .field("config", &self.config)
            .field("current_size", &self.current_size)
            .finish()
    }
}

impl AuditWriter {
    ///
    /// 監査ログ writer の生成
    ///
    /// # 引数
    /// * `config` - writer 設定
    ///
    /// # 戻り値
    /// 生成した writer を返す。
    ///
    pub(crate) fn new(config: AuditWriterConfig) -> Self {
        Self {
            config,
            file: None,
            current_size: 0,
        }
    }

    ///
    /// 監査ログ出力ディレクトリへのアクセサ
    ///
    /// # 戻り値
    /// 監査ログ出力ディレクトリを返す。
    ///
    pub(crate) fn output_dir(&self) -> &Path {
        &self.config.output_dir
    }

    ///
    /// 監査レコード1件の書込
    ///
    /// # 引数
    /// * `record` - 書き込む監査レコード
    ///
    /// # 戻り値
    /// 現時点では骨格のため常に成功を返す。
    ///
    pub(crate) fn write_record(
        &mut self,
        record: &AuditRecord,
    ) -> Result<()> {
        /*
         * JSONL 1 行の生成
         */
        let line = encode_jsonl_line(record)?;
        let line_len = line.len() as u64;

        /*
         * アクティブファイル準備とローテーション
         */
        self.ensure_output_dir()?;
        self.ensure_current_size()?;
        self.rotate_if_needed(line_len)?;
        self.ensure_file_opened()?;

        /*
         * 1 行追記
         */
        let writer = self
            .file
            .as_mut()
            .context("audit writer is not opened")?;
        writer
            .write_all(&line)
            .context("audit jsonl write failed")?;
        self.current_size = self.current_size.saturating_add(line_len);

        Ok(())
    }

    ///
    /// writer の flush
    ///
    /// # 戻り値
    /// 現時点では骨格のため常に成功を返す。
    ///
    pub(crate) fn flush(&mut self) -> Result<()> {
        if let Some(writer) = self.file.as_mut() {
            writer.flush().context("audit writer flush failed")?;
        }

        Ok(())
    }

    ///
    /// 監査ログ出力ディレクトリを用意する
    ///
    /// # 戻り値
    /// ディレクトリ準備に成功した場合は `Ok(())` を返す。
    ///
    fn ensure_output_dir(&self) -> Result<()> {
        fs::create_dir_all(self.output_dir()).with_context(|| {
            format!(
                "create audit output dir failed: {}",
                self.output_dir().display()
            )
        })?;

        Ok(())
    }

    ///
    /// 現在のアクティブファイルサイズを初期化する
    ///
    /// # 戻り値
    /// 初期化に成功した場合は `Ok(())` を返す。
    ///
    fn ensure_current_size(&mut self) -> Result<()> {
        if self.file.is_some() || self.current_size > 0 {
            return Ok(());
        }

        let active_path = active_log_path(self.output_dir());
        if !active_path.exists() {
            return Ok(());
        }

        self.current_size = active_path
            .metadata()
            .with_context(|| {
                format!(
                    "read audit log metadata failed: {}",
                    active_path.display()
                )
            })?
            .len();

        Ok(())
    }

    ///
    /// 必要時にアクティブファイルをローテーションする
    ///
    /// # 引数
    /// * `incoming_size` - 今回追加するレコードサイズ(バイト)
    ///
    /// # 戻り値
    /// ローテーション処理に成功した場合は `Ok(())` を返す。
    ///
    fn rotate_if_needed(&mut self, incoming_size: u64) -> Result<()> {
        if !should_rotate(
            &self.config.rotation_policy,
            self.current_size,
            incoming_size,
        ) {
            return Ok(());
        }

        /*
         * アクティブ writer の flush と close
         */
        self.flush()?;
        self.file = None;

        /*
         * アクティブファイルのリネーム
         */
        let rotated = rotate_active_file(self.output_dir(), Utc::now())?;
        if let Some(path) = rotated {
            debug!(path = %path.display(), "audit log rotated");
        } else {
            warn!("audit rotation requested without active file");
        }

        self.current_size = 0;

        Ok(())
    }

    ///
    /// アクティブファイルを開く
    ///
    /// # 戻り値
    /// オープンに成功した場合は `Ok(())` を返す。
    ///
    fn ensure_file_opened(&mut self) -> Result<()> {
        if self.file.is_some() {
            return Ok(());
        }

        let active_path = active_log_path(self.output_dir());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_path)
            .with_context(|| {
                format!(
                    "open audit log failed: {}",
                    active_path.display()
                )
            })?;
        self.file = Some(BufWriter::new(file));

        Ok(())
    }
}

///
/// 監査レコードを JSONL 1 行へ変換する
///
/// # 引数
/// * `record` - 変換対象監査レコード
///
/// # 戻り値
/// 末尾 LF を含む JSONL 1 行を返す。
///
fn encode_jsonl_line(record: &AuditRecord) -> Result<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec(record).context("audit json encode failed")?;
    bytes.push(b'\n');

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;
    use crate::audit::model::{AuditOperation, AuditResult};
    use crate::database::types::{TokenId, UserId};

    fn test_record(path: &str) -> AuditRecord {
        AuditRecord::new(
            AuditOperation::Create,
            UserId::new(),
            Some(TokenId::new()),
            None,
            Some(path.to_string()),
            AuditResult::Success,
            Utc::now(),
            Some("page created".to_string()),
            Some(1),
        )
    }

    ///
    /// 監査レコードが `audit.current.jsonl` へ追記されることを確認する。
    ///
    /// 注記:
    /// 1 件書込後に flush し、ファイル内容を JSONL として検証する。
    ///
    #[test]
    fn write_record_appends_jsonl_line() {
        let dir = tempdir().expect("tempdir failed");
        let mut writer = AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().to_path_buf(),
            rotation_policy: AuditRotationPolicy::new(1024),
        });

        writer
            .write_record(&test_record("/audit/write"))
            .expect("write record failed");
        writer.flush().expect("flush failed");

        let body = fs::read_to_string(active_log_path(dir.path()))
            .expect("read active failed");
        assert!(body.contains("\"target_path\":\"/audit/write\""));
        assert!(body.ends_with('\n'));
    }

    ///
    /// 閾値超過前にローテーションしてから新規レコードを書き込むことを確認する。
    ///
    /// 注記:
    /// 小さい閾値を与えて 2 件書き込み、ローテーション済みファイル生成を検証する。
    ///
    #[test]
    fn write_record_rotates_before_exceeding_threshold() {
        let dir = tempdir().expect("tempdir failed");
        let mut writer = AuditWriter::new(AuditWriterConfig {
            output_dir: dir.path().to_path_buf(),
            rotation_policy: AuditRotationPolicy::new(150),
        });

        writer
            .write_record(&test_record("/audit/first"))
            .expect("write first failed");
        writer.flush().expect("flush first failed");
        writer
            .write_record(&test_record("/audit/second"))
            .expect("write second failed");
        writer.flush().expect("flush second failed");

        let mut names = fs::read_dir(dir.path())
            .expect("read dir failed")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();
        names.sort();

        assert!(
            names.iter().any(|name| name.starts_with("audit-")),
            "rotated file missing: {names:?}"
        );
        assert!(names.iter().any(|name| name == "audit.current.jsonl"));
    }
}
