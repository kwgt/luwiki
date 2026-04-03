/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"run"のコマンドライン定義
//!

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::Args;

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;
use crate::cmd_args::default_cert_path;

///
/// サブコマンドrunのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct RunOpts {
    /// バインド先のアドレス
    #[arg(short = 'b', long = "open-browser", help = "ブラウザを起動する")]
    open_browser: bool,

    /// MCPの有効化指定
    #[arg(
        long = "mcp",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true"
    )]
    use_mcp: Option<bool>,

    /// TLSでの通信を有効にする
    #[arg(short = 'T', long = "tls")]
    use_tls: bool,

    /// TLS用のサーバ証明書ファイルのパス
    #[arg(short = 'C', long = "cert", value_name = "FILE")]
    cert_path: Option<PathBuf>,

    /// サーバのバインド先
    #[arg()]
    bind_addr: Option<String>,

    /// サーバのバインド先ポート
    #[arg(skip)]
    bind_port: Option<u16>,
}

impl RunOpts {
    ///
    /// 検索キーへのアクセサ
    ///
    /// # 戻り値
    /// キー文字列を返す
    ///
    pub(crate) fn is_browser_open(&self) -> bool {
        self.open_browser
    }

    ///
    /// MCP有効フラグへのアクセサ
    ///
    /// # 戻り値
    /// MCPが有効ならtrueを返す。
    ///
    pub(crate) fn use_mcp(&self) -> bool {
        self.use_mcp.unwrap_or(false)
    }

    ///
    /// TLS有効フラグへのアクセサ
    ///
    /// # 戻り値
    /// TLSが有効ならtrueを返す。
    ///
    pub(crate) fn use_tls(&self) -> bool {
        self.use_tls
    }

    ///
    /// サーバ証明書ファイルのパスへのアクセサ
    ///
    /// # 戻り値
    /// オプションで指定された証明書パスを返す。未指定の場合はデフォルトのパス
    /// を返す。
    ///
    pub(crate) fn cert_path(&self) -> PathBuf {
        if let Some(path) = &self.cert_path {
            path.clone()
        } else {
            default_cert_path()
        }
    }

    ///
    /// サーバ証明書パスの明示指定フラグへのアクセサ
    ///
    /// # 戻り値
    /// 証明書パスが明示指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_cert_path_explicit(&self) -> bool {
        self.cert_path.is_some()
    }

    ///
    /// バインド先のアドレスへのアクセサ
    ///
    pub(crate) fn bind_addr(&self) -> String {
        if let Some(addr) = &self.bind_addr {
            addr.clone()
        } else {
            "0.0.0.0".to_string()
        }
    }

    ///
    /// バインド先のポート番号へのアクセサ
    ///
    pub(crate) fn bind_port(&self) -> u16 {
        if let Some(port) = self.bind_port {
            port
        } else {
            8080
        }
    }
}

// Validateトレイトの実装
impl Validate for RunOpts {
    ///
    /// run サブコマンドのオプション整合性を検証
    ///
    /// # 戻り値
    /// 矛盾がなければ `Ok(())`
    ///
    fn validate(&mut self) -> Result<()> {
        /*
         * bindアドレスからポート指定を補完し整合性を確認
         */
        if let Some(value) = &self.bind_addr {
            let (addr, port) = parse_bind_value(value)?;

            if let Some(current_port) = self.bind_port {
                if let Some(parsed_port) = port {
                    if current_port != parsed_port {
                        return Err(anyhow!(
                            "bind port is inconsistent: {} vs {}",
                            current_port,
                            parsed_port
                        ));
                    }
                }
            }

            self.bind_addr = Some(addr);
            if self.bind_port.is_none() {
                self.bind_port = port;
            }
        }

        /*
         * 検証結果を返却
         */
        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for RunOpts {
    ///
    /// run サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * bind関連の設定を解決
         */
        if let Some(value) = &self.bind_addr {
            if self.bind_port.is_none() {
                if let Ok((addr, port)) = parse_bind_value(value) {
                    self.bind_addr = Some(addr);
                    self.bind_port = port;
                }
            }
        } else if let Some(addr) = config.run_bind_addr() {
            self.bind_addr = Some(addr);
        }

        if self.bind_port.is_none() {
            if let Some(port) = config.run_bind_port() {
                self.bind_port = Some(port);
            }
        }

        /*
         * MCP有効化設定を補完
         */
        if self.use_mcp.is_none() {
            self.use_mcp = config.run_use_mcp();
        }

        /*
         * TLS関連の設定を補完
         */
        if !self.use_tls {
            if let Some(use_tls) = config.use_tls() {
                self.use_tls = use_tls;
            }
        }

        if self.cert_path.is_none() {
            if let Some(path) = config.server_cert() {
                self.cert_path = Some(path);
            }
        }
    }
}

///
/// BIND-ADDR[:PORT]形式の値を解析する
///
/// # 概要
/// IPv6の`[ADDR]:PORT`形式、`ADDR:PORT`形式、`ADDR`のみの形式を解析し、
/// バインド先のアドレスとポートを返す。
///
/// # 引数
/// * `value` - 解析対象の文字列
///
/// # 戻り値
/// 解析に成功した場合は`(bind_addr, bind_port)`を返す。ポートが指定されて
/// いない場合は`None`を返す。
///
fn parse_bind_value(value: &str) -> Result<(String, Option<u16>)> {
    /*
     * 入力の事前チェック
     */
    if value.is_empty() {
        return Err(anyhow!("bind address is empty"));
    }

    /*
     * IPv6角括弧形式の解析
     */
    if let Some(rest) = value.strip_prefix('[') {
        let close_pos = rest.find(']')
            .ok_or_else(|| anyhow!("invalid bind address: {}", value))?;
        let addr = &rest[..close_pos];
        if addr.is_empty() {
            return Err(anyhow!("bind address is empty"));
        }

        let tail = &rest[close_pos + 1..];
        if tail.is_empty() {
            return Ok((addr.to_string(), None));
        }

        if let Some(port_str) = tail.strip_prefix(':') {
            if port_str.is_empty() {
                return Err(anyhow!("bind port is empty"));
            }

            return Ok((addr.to_string(), Some(port_str.parse()?)));
        }

        return Err(anyhow!("invalid bind address: {}", value));
    }

    /*
     * IPv4/ホスト名形式の解析
     */
    let colon_count = value.matches(':').count();
    if colon_count == 0 {
        return Ok((value.to_string(), None));
    }

    if colon_count == 1 {
        let mut iter = value.splitn(2, ':');
        let addr = iter.next().unwrap_or_default();
        let port_str = iter.next().unwrap_or_default();

        if addr.is_empty() {
            return Err(anyhow!("bind address is empty"));
        }
        if port_str.is_empty() {
            return Err(anyhow!("bind port is empty"));
        }

        return Ok((addr.to_string(), Some(port_str.parse()?)));
    }

    /*
     * IPv6リテラル形式の解析
     */
    Ok((value.to_string(), None))
}

// ShowOptionsトレイトの実装
impl ShowOptions for RunOpts {
    fn show_options(&self) {
        println!("run command options");
        println!("   browser_open:   {:?}", self.is_browser_open());
        println!("   mcp enabled:    {}", self.use_mcp());
        println!("   tls enabled:    {}", self.use_tls());
        println!("   cert path:      {}", self.cert_path().display());
        println!("   bind:  {}:{}", self.bind_addr(), self.bind_port());
    }
}
