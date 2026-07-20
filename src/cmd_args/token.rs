/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"token"のコマンドライン定義
//!

use anyhow::{anyhow, Result};
use chrono::Duration;
use clap::{Args, Subcommand};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;
use crate::database::types::{
    BearerScope,
    BearerScopeSet,
    PathPrefixSet,
    TokenId,
};
use crate::rest_api::validate_page_path;

/// `token create` のデフォルトスコープ文字列
const DEFAULT_TOKEN_CREATE_SCOPE: &str = "write";

/// `token create` のデフォルトTTL文字列
const DEFAULT_TOKEN_CREATE_TTL: &str = "30d";

#[derive(Clone, Args, Debug)]
pub(crate) struct TokenCommand {
    #[command(subcommand)]
    pub(crate) subcommand: TokenSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum TokenSubCommand {
    /// トークンの生成
    #[command(name = "create", alias = "c")]
    Create(TokenCreateOpts),

    /// path制約の追加
    #[command(name = "add_path", alias = "a", alias = "add")]
    AddPath(TokenPathUpdateOpts),

    /// path制約の削除
    #[command(name = "remove_path", alias = "rm")]
    RemovePath(TokenPathUpdateOpts),

    /// トークンの失効
    #[command(name = "revoke", alias = "r")]
    Revoke(TokenRevokeOpts),

    /// トークンの削除
    #[command(name = "purge", alias = "p")]
    Purge(TokenPurgeOpts),

    /// トークン一覧の表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(TokenListOpts),

    /// トークン情報の詳細表示
    #[command(name = "info")]
    Info(TokenInfoOpts),
}

///
/// サブコマンドtoken_createのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct TokenCreateOpts {
    /// スコープの指定
    #[arg(short = 's', long = "scope", value_name = "PERMISSION")]
    scope: Option<String>,

    /// TTLの指定
    #[arg(short = 't', long = "ttl", value_name = "DURATION")]
    ttl: Option<String>,

    /// トークン名の指定
    #[arg(short = 'n', long = "name", value_name = "TOKEN-NAME")]
    name: Option<String>,

    /// 操作可能な path prefix 制約
    #[arg(long = "path-prefix", value_name = "PATH")]
    path_prefixes: Vec<String>,

    /// 発行対象のユーザ名
    #[arg()]
    user_name: String,
}

impl TokenCreateOpts {
    ///
    /// スコープ指定の実効文字列へのアクセサ
    ///
    /// # 戻り値
    /// デフォルト補完後のスコープ指定文字列を返す。
    ///
    pub(crate) fn resolved_scope(&self) -> String {
        self.scope
            .clone()
            .unwrap_or_else(|| DEFAULT_TOKEN_CREATE_SCOPE.to_string())
    }

    ///
    /// TTL指定の実効文字列へのアクセサ
    ///
    /// # 戻り値
    /// デフォルト補完後のTTL指定文字列を返す。
    ///
    pub(crate) fn resolved_ttl(&self) -> String {
        self.ttl
            .clone()
            .unwrap_or_else(|| DEFAULT_TOKEN_CREATE_TTL.to_string())
    }

    ///
    /// 検証後のトークン名へのアクセサ
    ///
    /// # 戻り値
    /// 前後空白を除去したトークン名を返す。未指定時は `None` を返す。
    ///
    pub(crate) fn normalized_name(&self) -> Option<String> {
        self.name.as_deref().map(str::trim).map(str::to_string)
    }

    ///
    /// 発行対象ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 発行対象ユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 検証済みスコープ集合の取得
    ///
    /// # 戻り値
    /// デフォルト補完および解析後のスコープ集合を返す。
    ///
    pub(crate) fn scopes(&self) -> Result<BearerScopeSet> {
        parse_token_scopes(&self.resolved_scope())
    }

    ///
    /// 検証済みTTLの取得
    ///
    /// # 戻り値
    /// デフォルト補完および解析後のTTLを返す。
    ///
    pub(crate) fn ttl_duration(&self) -> Result<Duration> {
        parse_token_ttl(&self.resolved_ttl())
    }

    ///
    /// 検証済み path prefix 制約集合の取得
    ///
    /// # 戻り値
    /// 解析済みの path prefix 制約集合を返す。
    ///
    pub(crate) fn path_prefixes(&self) -> Result<PathPrefixSet> {
        parse_path_prefixes(&self.path_prefixes)
    }
}

// Validateトレイトの実装
impl Validate for TokenCreateOpts {
    fn validate(&mut self) -> Result<()> {
        /*
         * 主要入力を検証する
         */
        self.scopes()?;
        self.ttl_duration()?;
        self.path_prefixes()?;

        /*
         * 任意名の妥当性を検証する
         */
        if let Some(name) = self.normalized_name() {
            if name.is_empty() {
                return Err(anyhow!("token name must not be empty"));
            }
            if TokenId::from_string(&name).is_ok() {
                return Err(anyhow!("token name must not be ULID-formatted"));
            }
        }

        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for TokenCreateOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

// ShowOptionsトレイトの実装
impl ShowOptions for TokenCreateOpts {
    fn show_options(&self) {
        println!("token create command options");
        println!("   user_name: {}", self.user_name());
        println!("   scope:     {}", self.resolved_scope());
        println!("   ttl:       {}", self.resolved_ttl());
        println!("   name:      {:?}", self.normalized_name());
        println!("   path:      {:?}", self.path_prefixes);
    }
}

///
/// サブコマンドtoken_add_path / token_remove_pathのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct TokenPathUpdateOpts {
    /// 更新対象のトークンID
    #[arg()]
    token_id: String,

    /// 追加または削除する path prefix
    #[arg()]
    path_prefix: String,
}

impl TokenPathUpdateOpts {
    ///
    /// トークンIDへのアクセサ
    ///
    /// # 戻り値
    /// 指定されたトークンIDを返す。
    ///
    pub(crate) fn token_id(&self) -> String {
        self.token_id.clone()
    }

    ///
    /// 検証済み path prefix へのアクセサ
    ///
    /// # 戻り値
    /// trim 後の path prefix を返す。
    ///
    pub(crate) fn normalized_path_prefix(&self) -> String {
        self.path_prefix.trim().to_string()
    }
}

impl Validate for TokenPathUpdateOpts {
    fn validate(&mut self) -> Result<()> {
        validate_path_prefix(&self.normalized_path_prefix())
    }
}

impl ApplyConfig for TokenPathUpdateOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

impl ShowOptions for TokenPathUpdateOpts {
    fn show_options(&self) {
        println!("token path update command options");
        println!("   token_id:    {:?}", self.token_id());
        println!("   path_prefix: {:?}", self.normalized_path_prefix());
    }
}

///
/// サブコマンドtoken_revokeのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct TokenRevokeOpts {
    /// 無効化対象のユーザ名
    #[arg(short = 'u', long = "user", value_name = "USER-NAME")]
    user_name: Option<String>,

    /// 全トークンを対象とする
    #[arg(short = 'a', long = "all")]
    all: bool,

    /// 確認プロンプトを省略する
    #[arg(short = 'y', long = "yes")]
    yes: bool,

    /// 無効化対象のトークンID
    #[arg()]
    token_id: Option<String>,
}

impl TokenRevokeOpts {
    ///
    /// ユーザ名指定へのアクセサ
    ///
    /// # 戻り値
    /// 指定されたユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> Option<String> {
        self.user_name.clone()
    }

    ///
    /// 全件指定へのアクセサ
    ///
    /// # 戻り値
    /// 全件指定が有効な場合はtrueを返す。
    ///
    pub(crate) fn is_all(&self) -> bool {
        self.all
    }

    ///
    /// 確認省略指定へのアクセサ
    ///
    /// # 戻り値
    /// `--yes` が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_yes(&self) -> bool {
        self.yes
    }

    ///
    /// トークンID指定へのアクセサ
    ///
    /// # 戻り値
    /// 指定されたトークンIDを返す。
    ///
    pub(crate) fn token_id(&self) -> Option<String> {
        self.token_id.clone()
    }
}

// Validateトレイトの実装
impl Validate for TokenRevokeOpts {
    fn validate(&mut self) -> Result<()> {
        let mut specified = 0usize;
        if self.token_id.is_some() {
            specified += 1;
        }
        if self.user_name.is_some() {
            specified += 1;
        }
        if self.all {
            specified += 1;
        }

        if specified == 0 {
            return Err(anyhow!(
                "one of TOKEN-ID, --user, or --all must be specified"
            ));
        }

        if specified > 1 {
            return Err(anyhow!(
                "TOKEN-ID, --user, and --all are mutually exclusive"
            ));
        }

        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for TokenRevokeOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

// ShowOptionsトレイトの実装
impl ShowOptions for TokenRevokeOpts {
    fn show_options(&self) {
        println!("token revoke command options");
        println!("   token_id:  {:?}", self.token_id());
        println!("   user_name: {:?}", self.user_name());
        println!("   all:       {:?}", self.is_all());
        println!("   yes:       {:?}", self.is_yes());
    }
}

///
/// サブコマンドtoken_purgeのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct TokenPurgeOpts {
    /// 期限切れトークンを対象とする
    #[arg(short = 'e', long = "expired")]
    expired: bool,

    /// 失効済みトークンを対象とする
    #[arg(short = 'r', long = "revoked")]
    revoked: bool,

    /// 確認プロンプトを省略する
    #[arg(short = 'y', long = "yes")]
    yes: bool,

    /// 削除対象のトークンID
    #[arg()]
    token_id: Option<String>,
}

impl TokenPurgeOpts {
    ///
    /// 期限切れ指定へのアクセサ
    ///
    /// # 戻り値
    /// `--expired` が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_expired(&self) -> bool {
        self.expired
    }

    ///
    /// 失効済み指定へのアクセサ
    ///
    /// # 戻り値
    /// `--revoked` が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_revoked(&self) -> bool {
        self.revoked
    }

    ///
    /// 確認省略指定へのアクセサ
    ///
    /// # 戻り値
    /// `--yes` が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_yes(&self) -> bool {
        self.yes
    }

    ///
    /// トークンID指定へのアクセサ
    ///
    /// # 戻り値
    /// 指定されたトークンIDを返す。
    ///
    pub(crate) fn token_id(&self) -> Option<String> {
        self.token_id.clone()
    }
}

// Validateトレイトの実装
impl Validate for TokenPurgeOpts {
    fn validate(&mut self) -> Result<()> {
        if self.token_id.is_some() && (self.expired || self.revoked) {
            return Err(anyhow!(
                "TOKEN-ID cannot be combined with --expired or --revoked"
            ));
        }

        if self.token_id.is_none() && !self.expired && !self.revoked {
            return Err(anyhow!(
                "one of TOKEN-ID, --expired, or --revoked must be specified"
            ));
        }

        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for TokenPurgeOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

// ShowOptionsトレイトの実装
impl ShowOptions for TokenPurgeOpts {
    fn show_options(&self) {
        println!("token purge command options");
        println!("   token_id: {:?}", self.token_id());
        println!("   expired:  {:?}", self.is_expired());
        println!("   revoked:  {:?}", self.is_revoked());
        println!("   yes:      {:?}", self.is_yes());
    }
}

///
/// サブコマンドtoken_listのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct TokenListOpts {
    /// 詳細情報で表示
    #[arg(short = 'l', long = "long-info")]
    long_info: bool,

    /// 対象ユーザでのフィルタリングを指定
    #[arg(short = 'u', long = "user", value_name = "USER-NAME")]
    user_name: Option<String>,

    /// 失効済みトークンで絞り込む
    #[arg(short = 'r', long = "revoked")]
    revoked: bool,

    /// 期限切れトークンで絞り込む
    #[arg(short = 'e', long = "expired")]
    expired: bool,

    /// 対象ユーザ名
    #[arg()]
    target_user_name: Option<String>,
}

impl TokenListOpts {
    ///
    /// 詳細表示指定へのアクセサ
    ///
    /// # 戻り値
    /// 詳細表示が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_long_info(&self) -> bool {
        self.long_info
    }

    ///
    /// `--user` 指定へのアクセサ
    ///
    /// # 戻り値
    /// `--user` で指定されたユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> Option<String> {
        self.user_name.clone()
    }

    ///
    /// 失効済み指定へのアクセサ
    ///
    /// # 戻り値
    /// `--revoked` が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_revoked(&self) -> bool {
        self.revoked
    }

    ///
    /// 期限切れ指定へのアクセサ
    ///
    /// # 戻り値
    /// `--expired` が指定されている場合はtrueを返す。
    ///
    pub(crate) fn is_expired(&self) -> bool {
        self.expired
    }

    ///
    /// 位置引数ユーザ名指定へのアクセサ
    ///
    /// # 戻り値
    /// 位置引数で指定されたユーザ名を返す。
    ///
    pub(crate) fn target_user_name(&self) -> Option<String> {
        self.target_user_name.clone()
    }
}

// Validateトレイトの実装
impl Validate for TokenListOpts {
    fn validate(&mut self) -> Result<()> {
        if self.user_name.is_some() && self.target_user_name.is_some() {
            return Err(anyhow!(
                "USER-NAME and --user cannot be specified together"
            ));
        }

        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for TokenListOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

// ShowOptionsトレイトの実装
impl ShowOptions for TokenListOpts {
    fn show_options(&self) {
        println!("token list command options");
        println!("   long_info:        {:?}", self.is_long_info());
        println!("   user_name:        {:?}", self.user_name());
        println!("   target_user_name: {:?}", self.target_user_name());
        println!("   revoked:          {:?}", self.is_revoked());
        println!("   expired:          {:?}", self.is_expired());
    }
}

///
/// サブコマンドtoken_infoのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct TokenInfoOpts {
    /// 取得対象のトークンID
    #[arg()]
    token_id: String,
}

impl TokenInfoOpts {
    ///
    /// トークンIDへのアクセサ
    ///
    /// # 戻り値
    /// 指定されたトークンIDを返す。
    ///
    pub(crate) fn token_id(&self) -> String {
        self.token_id.clone()
    }
}

// Validateトレイトの実装
impl Validate for TokenInfoOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for TokenInfoOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

// ShowOptionsトレイトの実装
impl ShowOptions for TokenInfoOpts {
    fn show_options(&self) {
        println!("token info command options");
        println!("   token_id: {:?}", self.token_id());
    }
}

///
/// スコープ指定文字列の解析
///
/// # 引数
/// * `raw` - カンマ区切りスコープ指定
///
/// # 戻り値
/// 解析済みスコープ集合を返す。
///
fn parse_token_scopes(raw: &str) -> Result<BearerScopeSet> {
    let mut scopes = BearerScopeSet::new();
    for part in raw.split(',') {
        let scope_name = part.trim();
        if scope_name.is_empty() {
            return Err(anyhow!("scope must not be empty"));
        }

        scopes.insert(BearerScope::try_from(scope_name)?);
    }

    if scopes.is_empty() {
        return Err(anyhow!("scope must not be empty"));
    }

    Ok(scopes)
}

///
/// path prefix 指定群の解析
///
/// # 引数
/// * `raw_prefixes` - path prefix 指定群
///
/// # 戻り値
/// 解析済みの path prefix 集合を返す。
///
fn parse_path_prefixes(raw_prefixes: &[String]) -> Result<PathPrefixSet> {
    let mut prefixes = PathPrefixSet::new();

    /*
     * 各 path prefix を順に検証する
     */
    for raw_prefix in raw_prefixes {
        let prefix = raw_prefix.trim();
        validate_path_prefix(prefix)?;
        prefixes.insert(prefix.to_string());
    }

    Ok(prefixes)
}

///
/// TTL指定文字列の解析
///
/// # 引数
/// * `raw` - TTL指定文字列
///
/// # 戻り値
/// 解析済みの `chrono::Duration` を返す。
///
fn parse_token_ttl(raw: &str) -> Result<Duration> {
    let raw = raw.trim();
    if raw.len() < 2 {
        return Err(anyhow!("ttl format is invalid"));
    }

    let unit = raw
        .chars()
        .last()
        .ok_or_else(|| anyhow!("ttl format is invalid"))?;
    let value_text = &raw[..raw.len() - unit.len_utf8()];
    if value_text.is_empty() {
        return Err(anyhow!("ttl format is invalid"));
    }

    let value: i64 = value_text
        .parse()
        .map_err(|_| anyhow!("ttl format is invalid"))?;
    if value <= 0 {
        return Err(anyhow!("ttl must be greater than zero"));
    }

    let duration = match unit {
        'd' => Duration::days(value),
        'h' => Duration::hours(value),
        'm' => Duration::minutes(value),
        _ => return Err(anyhow!("ttl format is invalid")),
    };

    Ok(duration)
}

///
/// path prefix の妥当性を検証する
///
/// # 引数
/// * `prefix` - 検証対象の path prefix
///
/// # 戻り値
/// 検証に成功した場合は `Ok(())` を返す。
///
fn validate_path_prefix(prefix: &str) -> Result<()> {
    /*
     * 既存 page path 規則との整合を確認する
     */
    if let Err(message) = validate_page_path(prefix) {
        return Err(anyhow!("invalid path prefix: {}", message));
    }

    /*
     * 正規化済み絶対 path 制約を検証する
     */
    if prefix != "/" && prefix.ends_with('/') {
        return Err(anyhow!(
            "invalid path prefix: path must be normalized"
        ));
    }

    if prefix.split('/').any(|segment| segment == "." || segment == "..") {
        return Err(anyhow!(
            "invalid path prefix: path must be normalized"
        ));
    }

    if prefix.contains("//") {
        return Err(anyhow!(
            "invalid path prefix: path must be normalized"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::{
        TokenCreateOpts,
        TokenInfoOpts,
        TokenListOpts,
        TokenPurgeOpts,
        TokenRevokeOpts,
        parse_path_prefixes,
        parse_token_ttl,
    };
    use crate::cmd_args::Validate;

    ///
    /// TTL解析が許可形式とエラー条件を
    /// 設計どおりに扱うことを確認する。
    ///
    /// # 注記
    /// `30d`、`12h`、`90m`、不正形式、0以下を
    /// 検証する。
    ///
    #[test]
    fn parse_token_ttl_accepts_supported_units_and_rejects_invalid_values() {
        /*
         * 正常系を検証する
         */
        assert_eq!(
            parse_token_ttl("30d").expect("parse 30d failed"),
            Duration::days(30),
        );
        assert_eq!(
            parse_token_ttl("12h").expect("parse 12h failed"),
            Duration::hours(12),
        );
        assert_eq!(
            parse_token_ttl("90m").expect("parse 90m failed"),
            Duration::minutes(90),
        );

        /*
         * 不正形式を検証する
         */
        assert!(parse_token_ttl("").is_err());
        assert!(parse_token_ttl("d").is_err());
        assert!(parse_token_ttl("abc").is_err());
        assert!(parse_token_ttl("30x").is_err());

        /*
         * 0以下を検証する
         */
        assert!(parse_token_ttl("0d").is_err());
        assert!(parse_token_ttl("-1h").is_err());
    }

    ///
    /// path prefix 解析が正規化済み絶対 path 制約を
    /// 設計どおりに扱うことを確認する。
    ///
    /// # 注記
    /// 正常系、末尾スラッシュ、`.` / `..`、重複区切りを
    /// 検証する。
    ///
    #[test]
    fn parse_path_prefixes_accepts_normalized_absolute_paths_only() {
        let parsed = parse_path_prefixes(&[
            "/docs".to_string(),
            "/notes/spec".to_string(),
        ])
        .expect("parse path prefixes failed");
        assert!(parsed.contains("/docs"));
        assert!(parsed.contains("/notes/spec"));

        assert!(parse_path_prefixes(&["docs".to_string()]).is_err());
        assert!(parse_path_prefixes(&["/docs/".to_string()]).is_err());
        assert!(parse_path_prefixes(&["/docs/../secret".to_string()]).is_err());
        assert!(parse_path_prefixes(&["/docs//topic".to_string()]).is_err());
    }

    ///
    /// token create の入力検証が path prefix と空名を
    /// 設計どおりに扱うことを確認する。
    ///
    #[test]
    fn token_create_validate_checks_name_and_path_prefixes() {
        let mut valid = TokenCreateOpts {
            scope: Some("write".to_string()),
            ttl: Some("30d".to_string()),
            name: Some("api".to_string()),
            path_prefixes: vec!["/docs".to_string()],
            user_name: "alice".to_string(),
        };
        valid.validate().expect("valid token create must pass");

        let mut blank_name = TokenCreateOpts {
            scope: Some("write".to_string()),
            ttl: Some("30d".to_string()),
            name: Some("   ".to_string()),
            path_prefixes: Vec::new(),
            user_name: "alice".to_string(),
        };
        assert!(blank_name.validate().is_err());

        let mut ulid_name = TokenCreateOpts {
            scope: Some("write".to_string()),
            ttl: Some("30d".to_string()),
            name: Some("01KXM000000000000000000000".to_string()),
            path_prefixes: Vec::new(),
            user_name: "alice".to_string(),
        };
        assert!(ulid_name.validate().is_err());

        let mut invalid_prefix = TokenCreateOpts {
            scope: Some("write".to_string()),
            ttl: Some("30d".to_string()),
            name: None,
            path_prefixes: vec!["/docs/".to_string()],
            user_name: "alice".to_string(),
        };
        assert!(invalid_prefix.validate().is_err());
    }

    ///
    /// token revoke の引数制約が設計どおりに
    /// 検証されることを確認する。
    ///
    #[test]
    fn token_revoke_validate_enforces_exclusive_targets() {
        let mut empty = TokenRevokeOpts {
            user_name: None,
            all: false,
            yes: false,
            token_id: None,
        };
        assert!(empty.validate().is_err());

        let mut token_and_user = TokenRevokeOpts {
            user_name: Some("alice".to_string()),
            all: false,
            yes: false,
            token_id: Some("01JTESTTOKENID".to_string()),
        };
        assert!(token_and_user.validate().is_err());

        let mut user_and_all = TokenRevokeOpts {
            user_name: Some("alice".to_string()),
            all: true,
            yes: false,
            token_id: None,
        };
        assert!(user_and_all.validate().is_err());

        let mut token_only = TokenRevokeOpts {
            user_name: None,
            all: false,
            yes: false,
            token_id: Some("01JTESTTOKENID".to_string()),
        };
        token_only.validate().expect("token-only revoke must be valid");
    }

    ///
    /// token purge の引数制約が設計どおりに
    /// 検証されることを確認する。
    ///
    #[test]
    fn token_purge_validate_enforces_target_constraints() {
        let mut empty = TokenPurgeOpts {
            expired: false,
            revoked: false,
            yes: false,
            token_id: None,
        };
        assert!(empty.validate().is_err());

        let mut token_and_expired = TokenPurgeOpts {
            expired: true,
            revoked: false,
            yes: false,
            token_id: Some("01JTESTTOKENID".to_string()),
        };
        assert!(token_and_expired.validate().is_err());

        let mut token_and_revoked = TokenPurgeOpts {
            expired: false,
            revoked: true,
            yes: false,
            token_id: Some("01JTESTTOKENID".to_string()),
        };
        assert!(token_and_revoked.validate().is_err());

        let mut expired_and_revoked = TokenPurgeOpts {
            expired: true,
            revoked: true,
            yes: false,
            token_id: None,
        };
        expired_and_revoked
            .validate()
            .expect("expired+revoked purge must be valid");
    }

    ///
    /// token list の引数制約が設計どおりに
    /// 検証されることを確認する。
    ///
    #[test]
    fn token_list_validate_rejects_duplicate_user_filters() {
        let mut both_users = TokenListOpts {
            long_info: false,
            user_name: Some("alice".to_string()),
            revoked: false,
            expired: false,
            target_user_name: Some("bob".to_string()),
        };
        assert!(both_users.validate().is_err());

        let mut option_only = TokenListOpts {
            long_info: false,
            user_name: Some("alice".to_string()),
            revoked: false,
            expired: false,
            target_user_name: None,
        };
        option_only
            .validate()
            .expect("--user only list must be valid");

        let mut positional_only = TokenListOpts {
            long_info: false,
            user_name: None,
            revoked: false,
            expired: false,
            target_user_name: Some("alice".to_string()),
        };
        positional_only
            .validate()
            .expect("positional USER-NAME only list must be valid");
    }

    ///
    /// token info の基本入力が検証を通過することを確認する。
    ///
    #[test]
    fn token_info_validate_accepts_token_id_argument() {
        let mut opts = TokenInfoOpts {
            token_id: "01JTESTTOKENID1234567890123".to_string(),
        };
        opts.validate().expect("token info validate must pass");
    }
}
