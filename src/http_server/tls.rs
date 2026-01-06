/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! TLS設定と証明書の取り扱い
//!

use std::fs;
use std::io::{BufReader, Cursor};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use anyhow::{anyhow, Result};
use log::info;
use rcgen::{
    CertificateParams,
    DistinguishedName,
    DnType,
    KeyPair,
    SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use time::{Duration, OffsetDateTime};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

///
/// TLSサーバ設定を読み込みまたは生成する
///
/// # 引数
/// * `cert_path` - 証明書ファイルのパス
/// * `cert_is_explicit` - 証明書パスが明示指定か否か
///
/// # 戻り値
/// 設定に成功した場合はTLSサーバ設定を返す。
///
pub(crate) fn load_server_config(
    cert_path: &Path,
    cert_is_explicit: bool,
) -> Result<ServerConfig> {
    /*
     * 証明書ファイルの存在確認と生成
     */
    if !cert_path.exists() {
        if cert_is_explicit {
            return Err(anyhow!(
                "cert file not found: {}",
                cert_path.display()
            ));
        }

        generate_self_signed(cert_path)?;
    }

    /*
     * PEMの読み込みとTLS設定の構築
     */
    let (certs, key) = load_pem(cert_path)?;
    log_certificate_info(&certs);
    build_server_config(certs, key)
}

///
/// TLSサーバ設定を構築する
///
/// # 引数
/// * `certs` - 証明書チェーン
/// * `key` - 秘密鍵
///
/// # 戻り値
/// 設定に成功した場合はTLSサーバ設定を返す。
///
fn build_server_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig> {
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| anyhow!("tls config error: {}", err))
}

///
/// PEMから証明書と秘密鍵を読み込む
///
/// # 引数
/// * `cert_path` - 証明書ファイルのパス
///
/// # 戻り値
/// 読み込みに成功した場合は証明書と秘密鍵を返す。
///
fn load_pem(
    cert_path: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    /*
     * PEMの読み込み
     */
    let pem = fs::read(cert_path)?;

    /*
     * 証明書の抽出
     */
    let mut cert_reader = BufReader::new(Cursor::new(&pem));
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow!("read cert error: {}", err))?;
    if certs.is_empty() {
        return Err(anyhow!(
            "no certificate found: {}",
            cert_path.display()
        ));
    }

    /*
     * 秘密鍵の抽出
     */
    let mut key_reader = BufReader::new(Cursor::new(&pem));
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|err| anyhow!("read private key error: {}", err))?
        .ok_or_else(|| anyhow!("no private key found: {}", cert_path.display()))?;

    Ok((certs, key))
}

///
/// 自己署名証明書を生成して保存する
///
/// # 引数
/// * `cert_path` - 保存先の証明書ファイルパス
///
/// # 戻り値
/// 保存に成功した場合は`Ok(())`を返す。
///
fn generate_self_signed(cert_path: &Path) -> Result<()> {
    /*
     * 出力先ディレクトリの準備
     */
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)?;
    }

    /*
     * 証明書パラメータの構築
     */
    let mut params = CertificateParams::new(vec!["localhost".to_string()])?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "localhost");
    params.distinguished_name = dn;
    params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ];

    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + Duration::days(365 * 5);

    /*
     * 自己署名証明書の生成
     */
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let pem = format!("{}\n{}", cert_pem.trim_end(), key_pem);

    /*
     * 証明書の保存
     */
    fs::write(cert_path, pem)?;
    info!(
        "generated self-signed certificate: {}",
        cert_path.display()
    );
    Ok(())
}

///
/// 証明書の主要パラメータをログに出力する
///
/// # 引数
/// * `certs` - 証明書チェーン
///
/// # 戻り値
/// なし
///
fn log_certificate_info(certs: &[CertificateDer<'static>]) {
    /*
     * 最初の証明書の取得
     */
    let cert = match certs.first() {
        Some(cert) => cert,
        None => return,
    };
    /*
     * 証明書の解析
     */
    let parsed = match X509Certificate::from_der(cert.as_ref()) {
        Ok((_, parsed)) => parsed,
        Err(err) => {
            info!("certificate info: parse failed: {}", err);
            return;
        }
    };

    /*
     * SubjectとSANの抽出
     */
    let subject_cn = parsed
        .subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .unwrap_or("-");

    let san = parsed
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|san| {
            san.value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(name) => Some(name.to_string()),
                    GeneralName::IPAddress(bytes) => {
                        let ip_bytes: &[u8] = bytes.as_ref();
                        match ip_bytes {
                            [a, b, c, d] => Some(
                                IpAddr::V4(Ipv4Addr::new(*a, *b, *c, *d))
                                    .to_string()
                            ),
                            [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p] => {
                                Some(IpAddr::V6(Ipv6Addr::new(
                                    ((*a as u16) << 8) | (*b as u16),
                                    ((*c as u16) << 8) | (*d as u16),
                                    ((*e as u16) << 8) | (*f as u16),
                                    ((*g as u16) << 8) | (*h as u16),
                                    ((*i as u16) << 8) | (*j as u16),
                                    ((*k as u16) << 8) | (*l as u16),
                                    ((*m as u16) << 8) | (*n as u16),
                                    ((*o as u16) << 8) | (*p as u16),
                                ))
                                .to_string())
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    /*
     * 有効期間の取得
     */
    let validity = parsed.validity();
    let not_before = validity.not_before;
    let not_after = validity.not_after;

    info!(
        "certificate info: subject_cn={}, san={:?}, not_before={}, not_after={}",
        subject_cn,
        san,
        not_before,
        not_after
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::CertificateParams;
    use tempfile::TempDir;

    ///
    /// 一時ファイルのパスを作成する
    ///
    /// # 引数
    /// * `dir` - 一時ディレクトリ
    /// * `file_name` - ファイル名
    ///
    /// # 戻り値
    /// 生成したファイルパスを返す。
    ///
    fn temp_cert_path(dir: &TempDir, file_name: &str) -> std::path::PathBuf {
        dir.path().join(file_name)
    }

    ///
    /// 明示指定の証明書パス未存在時のエラーを確認する
    ///
    /// # 注記
    /// 1) 一時パスを作成する
    /// 2) 証明書読込を明示指定で実行する
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn explicit_cert_path_missing_is_error() {
        let dir = TempDir::new().expect("temp dir");
        let cert_path = temp_cert_path(&dir, "missing.pem");

        let result = load_server_config(&cert_path, true);
        assert!(result.is_err());
        assert!(!cert_path.exists());
    }

    ///
    /// 既定パス未存在時に自己署名証明書を生成することを確認する
    ///
    /// # 注記
    /// 1) 一時パスを作成する
    /// 2) 証明書読込を非明示指定で実行する
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn implicit_cert_path_missing_generates_pem() {
        let dir = TempDir::new().expect("temp dir");
        let cert_path = temp_cert_path(&dir, "auto.pem");

        let result = load_server_config(&cert_path, false);
        assert!(result.is_ok());
        assert!(cert_path.exists());
    }

    ///
    /// PEMに秘密鍵が無い場合にエラーとなることを確認する
    ///
    /// # 注記
    /// 1) 秘密鍵を含まないPEMを作成する
    /// 2) 証明書読込を明示指定で実行する
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn missing_private_key_is_error() {
        let dir = TempDir::new().expect("temp dir");
        let cert_path = temp_cert_path(&dir, "cert_only.pem");

        let params = CertificateParams::new(vec!["localhost".to_string()])
            .expect("params");
        let key_pair = KeyPair::generate().expect("key");
        let cert = params.self_signed(&key_pair).expect("cert");
        let cert_pem = cert.pem();
        fs::write(&cert_path, cert_pem).expect("write cert");

        let result = load_server_config(&cert_path, true);
        assert!(result.is_err());
    }
}
