/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンドrunの実装
//!

use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::cmd_args::{FrontendConfig, Options, RunOpts};
use crate::database::DatabaseManager;
use crate::fts::FtsIndexConfig;
use crate::http_server;
use crate::rest_api::validate_page_path;
use super::CommandContext;

///
/// addサブコマンドのコンテキスト情報をパックした構造体
///
struct RunCommandContext {
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

    /// 起動時にブラウザを開くか否かのフラグ
    #[allow(dead_code)]
    open_browser: bool,

    /// テンプレートルート
    template_root: Option<String>,
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
            open_browser: sub_opts.is_browser_open(),
            template_root,
        })
    }
}

// トレイトCommandContextの実装
impl CommandContext for RunCommandContext {
    fn exec(&self) -> Result<()> {
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
         * HTTPサーバの起動
         */
        http_server::run(
            self.bind_addr.clone(),
            self.bind_port,
            manager,
            self.frontend_config.clone(),
            fts_config,
            self.template_root.clone(),
            self.use_tls,
            self.cert_path.clone(),
            self.cert_is_explicit,
        )
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(opts: &Options, sub_opts: &RunOpts)
    -> Result<Box<dyn CommandContext>>
{
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
