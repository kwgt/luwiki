/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンドrunの実装
//!

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use mime_guess::MimeGuess;

use super::CommandContext;
use crate::cmd_args::{FrontendConfig, Options, RunOpts};
use crate::database::DatabaseManager;
use crate::fts::FtsIndexConfig;
use crate::http_server;
use crate::mcp;
use crate::rest_api::validate_page_path;

///
/// addサブコマンドのコンテキスト情報をパックした構造体
///
#[derive(Clone)]
pub(crate) struct RunCommandContext {
    /// バインド先のアドレス
    bind_addr: String,

    /// バインド先のポート番号
    bind_port: u16,

    /// データベースファイルへのパス
    db_path: PathBuf,

    /// アセットデータ格納ディレクトリへのパス
    asset_path: PathBuf,

    /// frontend設定
    frontend_config: FrontendConfig,

    /// FTSインデックス格納ディレクトリへのパス
    fts_index_path: PathBuf,

    /// TLSの使用フラグ
    use_tls: bool,

    /// サーバ証明書ファイルのパス
    cert_path: PathBuf,

    /// サーバ証明書パスの明示指定フラグ
    cert_is_explicit: bool,

    /// MCP有効フラグ
    mcp_enabled: bool,

    /// MCP resource URI authority
    mcp_authority: String,

    /// 起動時にブラウザを開くか否かのフラグ
    #[allow(dead_code)]
    open_browser: bool,

    /// Windowsサービス実行フラグ
    #[cfg(windows)]
    win_service: bool,

    /// テンプレートルート
    template_root: Option<String>,

    /// Wikiタイトル
    wiki_title: String,

    /// Wikiアイコン画像ファイルのパス
    wiki_icon: Option<PathBuf>,

    /// アセットサイズ上限
    asset_limit_size: u64,

    /// 監査ログ設定
    audit_log_config: http_server::AuditLogConfig,
}

impl RunCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &RunOpts) -> Result<Self> {
        /*
         * テンプレートルートの検証
         */
        let template_root = match opts.template_root() {
            Some(path) => {
                if let Err(message) = validate_page_path(&path) {
                    return Err(anyhow!(message));
                }
                Some(normalize_template_root(path))
            }
            None => None,
        };
        let wiki_icon = opts.wiki_icon();
        if let Some(path) = wiki_icon.as_ref() {
            validate_wiki_icon(path)?;
        }

        /*
         * オプションの集約
         */
        Ok(Self {
            db_path: opts.db_path(),
            asset_path: opts.assets_path(),
            bind_addr: sub_opts.bind_addr(),
            bind_port: sub_opts.bind_port(),
            frontend_config: opts.frontend_config()?,
            fts_index_path: opts.fts_index_path(),
            use_tls: sub_opts.use_tls(),
            cert_path: sub_opts.cert_path(),
            cert_is_explicit: sub_opts.is_cert_path_explicit(),
            mcp_enabled: sub_opts.use_mcp(),
            mcp_authority: sub_opts.mcp_authority(),
            open_browser: sub_opts.is_browser_open(),
            #[cfg(windows)]
            win_service: sub_opts.is_win_service(),
            template_root,
            wiki_title: opts.wiki_title(),
            wiki_icon,
            asset_limit_size: opts.asset_limit_size()?,
            audit_log_config: http_server::AuditLogConfig::new(
                opts.audit_log_dir(),
                opts.audit_log_retention()?,
                opts.audit_log_rotate_size()?,
            ),
        })
    }
}

impl CommandContext for RunCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// サーバ起動処理に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        #[cfg(windows)]
        if self.win_service {
            return super::windows_service::run(self.clone());
        }

        self.run_server(None, None)
    }
}

impl RunCommandContext {
    ///
    /// HTTPサーバを起動する
    ///
    /// # 引数
    /// * `shutdown_signal` - 外部停止通知
    /// * `on_started` - サーバ起動完了通知
    ///
    /// # 戻り値
    /// サーバ起動処理に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn run_server(
        &self,
        shutdown_signal: Option<http_server::ShutdownSignal>,
        on_started: Option<Arc<dyn Fn() -> Result<()> + Send + Sync>>,
    ) -> Result<()> {
        /*
         * データベースのオープン
         */
        let manager = DatabaseManager::open(&self.db_path, &self.asset_path)?;

        /*
         * FTS設定の構築
         */
        let fts_config = FtsIndexConfig::new(self.fts_index_path.clone());

        /*
         * ユーザ登録の検証
         */
        if !manager.is_users_registered()? {
            return Err(anyhow!("no users registered"));
        }

        /*
         * MCP endpoint情報の解決
         */
        let mcp_endpoint = if self.mcp_enabled {
            Some(mcp::create_endpoint(self.mcp_authority.clone()))
        } else {
            None
        };

        /*
         * HTTPサーバの起動
         */
        http_server::run(
            self.bind_addr.clone(),
            self.bind_port,
            manager,
            self.frontend_config.clone(),
            fts_config,
            self.template_root.clone(),
            self.wiki_title.clone(),
            self.wiki_icon.clone(),
            self.asset_limit_size,
            self.use_tls,
            self.cert_path.clone(),
            self.cert_is_explicit,
            if self.mcp_enabled {
                Some(self.audit_log_config.clone())
            } else {
                None
            },
            mcp_endpoint,
            shutdown_signal,
            on_started,
        )
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &RunOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(RunCommandContext::new(opts, sub_opts)?))
}

///
/// テンプレートルートの正規化
///
/// # 引数
/// * `path` - テンプレートルート
///
/// # 戻り値
/// 正規化したテンプレートルートを返す。
///
fn normalize_template_root(path: String) -> String {
    if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    }
}

///
/// Wikiアイコン画像ファイル設定を検証する
///
/// # 引数
/// * `path` - 検証対象の画像ファイルパス
///
/// # 戻り値
/// 検証に成功した場合は`Ok(())`を返す。
///
fn validate_wiki_icon(path: &Path) -> Result<()> {
    /*
     * ファイル存在確認
     */
    let metadata = fs::metadata(path)
        .map_err(|err| anyhow!("wiki icon metadata error: {}", err))?;
    if !metadata.is_file() {
        return Err(anyhow!("wiki icon path is not a file"));
    }

    /*
     * MIME種別確認
     */
    let mime = MimeGuess::from_path(path).first_or_octet_stream();
    if mime.type_() != mime_guess::mime::IMAGE {
        return Err(anyhow!(
            "wiki icon must be an image file: {}",
            path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::validate_wiki_icon;

    #[test]
    fn validate_wiki_icon_accepts_existing_image_path() {
        let dir = tempdir().expect("tempdir failed");
        let path = dir.path().join("wiki-icon.png");
        fs::write(&path, b"png").expect("write icon failed");

        validate_wiki_icon(&path).expect("icon validation failed");
    }

    #[test]
    fn validate_wiki_icon_rejects_missing_file() {
        let dir = tempdir().expect("tempdir failed");
        let path = dir.path().join("missing.png");

        let err = validate_wiki_icon(&path).expect_err("must fail");
        assert!(
            err.to_string().contains("wiki icon metadata error"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_wiki_icon_rejects_non_image_file() {
        let dir = tempdir().expect("tempdir failed");
        let path = dir.path().join("wiki-icon.txt");
        fs::write(&path, b"text").expect("write icon failed");

        let err = validate_wiki_icon(&path).expect_err("must fail");
        assert!(
            err.to_string().contains("wiki icon must be an image file"),
            "unexpected error: {}",
            err
        );
    }
}
