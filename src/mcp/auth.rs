/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP向けBearer認証入口の骨格を定義するモジュール
//!

use std::fmt;

use chrono::Local;

use crate::auth::{AuthContext, authenticate_bearer_token};
use crate::database::DatabaseManager;
use crate::database::types::BearerTokenPlaintext;

///
/// MCP認証失敗種別
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum McpAuthErrorKind {
    /// Bearerトークンが提示されていない
    MissingBearer,

    /// MCPでは受理しない認証方式
    UnsupportedScheme,

    /// Bearerトークンの形式が不正
    InvalidBearerFormat,

    /// Bearer認証失敗
    Unauthorized,

    /// 内部失敗
    Internal,
}

///
/// MCP認証失敗情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct McpAuthError {
    kind: McpAuthErrorKind,
    message: &'static str,
}

///
/// MCP認証入口で解釈したAuthorization情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
enum ParsedMcpAuthorization {
    Bearer(BearerTokenPlaintext),
    Basic,
    Unsupported,
}

///
/// MCP認証入口の骨格
///
#[derive(Clone, Debug, Default)]
pub(crate) struct McpAuthGateway;

impl McpAuthError {
    ///
    /// MCP認証失敗情報を生成
    ///
    /// # 引数
    /// * `kind` - 失敗種別
    /// * `message` - エラー説明
    ///
    /// # 戻り値
    /// 生成した認証失敗情報を返す。
    ///
    fn new(kind: McpAuthErrorKind, message: &'static str) -> Self {
        Self { kind, message }
    }

    ///
    /// 失敗種別を返す
    ///
    /// # 戻り値
    /// 失敗種別を返す。
    ///
    pub(crate) fn kind(&self) -> McpAuthErrorKind {
        self.kind
    }

    ///
    /// エラー説明を返す
    ///
    /// # 戻り値
    /// エラー説明文字列を返す。
    ///
    pub(crate) fn message(&self) -> &'static str {
        self.message
    }
}

impl fmt::Display for McpAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl std::error::Error for McpAuthError {}

impl McpAuthGateway {
    ///
    /// MCP認証入口の生成
    ///
    /// # 戻り値
    /// 生成した認証入口オブジェクトを返す。
    ///
    pub(crate) fn new() -> Self {
        Self
    }

    ///
    /// MCP向けAuthorization入力を検証し、共通認証文脈を生成する
    ///
    /// # 引数
    /// * `manager` - データベースマネージャ
    /// * `authorization` - Authorization相当の入力値
    ///
    /// # 戻り値
    /// 認証成功時は共通認証文脈を返す。
    ///
    pub(crate) fn authenticate(
        &self,
        manager: &DatabaseManager,
        authorization: Option<&str>,
    ) -> Result<AuthContext, McpAuthError> {
        let token = match parse_mcp_authorization(authorization)? {
            ParsedMcpAuthorization::Bearer(token) => token,
            ParsedMcpAuthorization::Basic => {
                return Err(McpAuthError::new(
                    McpAuthErrorKind::UnsupportedScheme,
                    "basic authorization is not supported for MCP",
                ));
            }
            ParsedMcpAuthorization::Unsupported => {
                return Err(McpAuthError::new(
                    McpAuthErrorKind::UnsupportedScheme,
                    "unsupported authorization scheme for MCP",
                ));
            }
        };

        self.authenticate_bearer(manager, &token)
    }

    ///
    /// Bearerトークン平文を共通認証コアへ委譲する
    ///
    /// # 引数
    /// * `manager` - データベースマネージャ
    /// * `token` - Bearerトークン平文
    ///
    /// # 戻り値
    /// 認証成功時は共通認証文脈を返す。
    ///
    pub(crate) fn authenticate_bearer(
        &self,
        manager: &DatabaseManager,
        token: &BearerTokenPlaintext,
    ) -> Result<AuthContext, McpAuthError> {
        match authenticate_bearer_token(manager, token, Local::now()) {
            Ok(Ok(success)) => Ok(success.auth().clone()),
            Ok(Err(_)) => Err(McpAuthError::new(
                McpAuthErrorKind::Unauthorized,
                "unauthorized",
            )),
            Err(_) => Err(McpAuthError::new(
                McpAuthErrorKind::Internal,
                "mcp auth failed",
            )),
        }
    }
}

///
/// MCP向けAuthorization入力を解釈する
///
/// # 引数
/// * `authorization` - Authorization相当の入力値
///
/// # 戻り値
/// 解釈に成功した場合は認証方式と資格情報を返す。
///
fn parse_mcp_authorization(
    authorization: Option<&str>,
) -> Result<ParsedMcpAuthorization, McpAuthError> {
    let value = authorization.ok_or_else(|| {
        McpAuthError::new(
            McpAuthErrorKind::MissingBearer,
            "missing bearer token for MCP",
        )
    })?;

    let mut parts = value.split_whitespace();
    let scheme = parts.next().ok_or_else(|| {
        McpAuthError::new(
            McpAuthErrorKind::InvalidBearerFormat,
            "invalid bearer token format",
        )
    })?;
    let credentials = parts.next().ok_or_else(|| {
        McpAuthError::new(
            McpAuthErrorKind::InvalidBearerFormat,
            "invalid bearer token format",
        )
    })?;

    if credentials.is_empty() || parts.next().is_some() {
        return Err(McpAuthError::new(
            McpAuthErrorKind::InvalidBearerFormat,
            "invalid bearer token format",
        ));
    }

    if scheme.eq_ignore_ascii_case("bearer") {
        return Ok(ParsedMcpAuthorization::Bearer(
            BearerTokenPlaintext::new(credentials.to_string()),
        ));
    }
    if scheme.eq_ignore_ascii_case("basic") {
        return Ok(ParsedMcpAuthorization::Basic);
    }

    Ok(ParsedMcpAuthorization::Unsupported)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::database::types::BearerScope;

    ///
    /// Bearer入力が共通認証文脈へ変換されることを確認する。
    ///
    #[test]
    fn authenticate_returns_auth_context_for_valid_bearer_token() {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "password123", None)
            .expect("add user failed");
        let (token_plaintext, token_info) = manager
            .create_bearer_token(
                "alice",
                crate::database::types::BearerScopeSet::from_iter([
                    BearerScope::Read,
                ]),
                crate::database::types::PathPrefixSet::new(),
                chrono::Duration::minutes(30),
                Some("mcp token".to_string()),
            )
            .expect("create bearer token failed");

        let gateway = McpAuthGateway::new();
        let auth = gateway
            .authenticate(
                &manager,
                Some(&format!("Bearer {}", token_plaintext.expose())),
            )
            .expect("authenticate failed");

        assert_eq!(auth.user_id(), "alice");
        assert!(auth.scopes().contains(BearerScope::Read));
        assert!(auth.path_prefixes().allows_all());
        assert_eq!(
            auth.token_id().expect("missing token id").to_string(),
            token_info.token_id().to_string(),
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// Bearer欠落が認証失敗として扱われることを確認する。
    ///
    #[test]
    fn authenticate_rejects_missing_bearer_token() {
        let gateway = McpAuthGateway::new();
        let err = parse_mcp_authorization(None).expect_err("missing token must fail");

        assert_eq!(err.kind(), McpAuthErrorKind::MissingBearer);
        assert_eq!(err.message(), "missing bearer token for MCP");

        let _ = gateway;
    }

    ///
    /// Basic認証をMCP入口で受理しないことを確認する。
    ///
    #[test]
    fn authenticate_rejects_basic_authorization() {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        let gateway = McpAuthGateway::new();
        let err = gateway
            .authenticate(&manager, Some("Basic dGVzdA=="))
            .expect_err("basic auth must fail");

        assert_eq!(err.kind(), McpAuthErrorKind::UnsupportedScheme);
        assert_eq!(
            err.message(),
            "basic authorization is not supported for MCP",
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// Bearer形式不正が認証失敗として扱われることを確認する。
    ///
    #[test]
    fn authenticate_rejects_invalid_bearer_format() {
        for value in ["Bearer", "Bearer token extra", ""] {
            let err = parse_mcp_authorization(Some(value))
                .expect_err("invalid bearer format must fail");
            assert_eq!(err.kind(), McpAuthErrorKind::InvalidBearerFormat);
        }
    }

    ///
    /// 失効済みBearerトークンが認証失敗として扱われることを確認する。
    ///
    #[test]
    fn authenticate_rejects_revoked_bearer_token() {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user("alice", "password123", None)
            .expect("add user failed");
        let (token_plaintext, token_info) = manager
            .create_bearer_token(
                "alice",
                crate::database::types::BearerScopeSet::from_iter([
                    BearerScope::Read,
                ]),
                crate::database::types::PathPrefixSet::new(),
                chrono::Duration::minutes(30),
                Some("mcp token".to_string()),
            )
            .expect("create bearer token failed");
        manager
            .revoke_bearer_token_by_id(&token_info.token_id())
            .expect("revoke token failed");

        let gateway = McpAuthGateway::new();
        let err = gateway
            .authenticate(
                &manager,
                Some(&format!("Bearer {}", token_plaintext.expose())),
            )
            .expect_err("revoked token must fail");

        assert_eq!(err.kind(), McpAuthErrorKind::Unauthorized);
        assert_eq!(err.message(), "unauthorized");

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    ///
    /// `NoBasicAuth` ユーザでも Bearer 認証なら MCP へ入れることを
    /// 確認する。
    ///
    /// # 注記
    /// MCP 入口は Basic を拒否するが、Bearer 認証時には
    /// `NoBasicAuth` を拒否条件へ使わない責務境界を確認する。
    ///
    #[test]
    fn authenticate_allows_bearer_token_for_no_basic_auth_user() {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user_with_attributes(
                "alice",
                None,
                None,
                crate::database::types::UserAttributeSet::from_iter([
                    crate::database::types::UserAttribute::NoBasicAuth,
                ]),
            )
            .expect("add restricted user failed");
        let (token_plaintext, token_info) = manager
            .create_bearer_token(
                "alice",
                crate::database::types::BearerScopeSet::from_iter([
                    BearerScope::Read,
                ]),
                crate::database::types::PathPrefixSet::new(),
                chrono::Duration::minutes(30),
                Some("mcp token".to_string()),
            )
            .expect("create bearer token failed");

        let gateway = McpAuthGateway::new();

        /*
         * Bearer 認証なら `NoBasicAuth` ユーザでも通ることを検証する
         */
        let auth = gateway
            .authenticate(
                &manager,
                Some(&format!("Bearer {}", token_plaintext.expose())),
            )
            .expect("authenticate failed");

        assert_eq!(auth.user_id(), "alice");
        assert!(auth.scopes().contains(BearerScope::Read));
        assert_eq!(
            auth.token_id().expect("missing token id").to_string(),
            token_info.token_id().to_string(),
        );

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    fn prepare_test_dirs() -> (PathBuf, PathBuf) {
        let base = Path::new("tests").join("tmp").join(unique_suffix());
        fs::create_dir_all(&base).expect("create test dir failed");
        let db_path = base.join("database.redb");
        (base, db_path)
    }

    fn unique_suffix() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        format!("mcp-auth-{}-{}", std::process::id(), timestamp)
    }
}
