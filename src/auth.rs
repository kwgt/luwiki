/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! REST API / MCP で共有する認証コアを提供するモジュール
//!

use anyhow::Result;
use chrono::{DateTime, Local};

use crate::database::{DatabaseManager, VerifyBearerTokenFailureReason};
use crate::database::types::{
    BearerScopeSet,
    BearerTokenPlaintext,
    PathPrefixSet,
    TokenId,
    UserAttributeSet,
};

///
/// 認証済みユーザ情報
///
#[derive(Clone, Debug)]
pub(crate) struct AuthUser {
    user_id: String,
}

///
/// 認証済みリクエストの共通文脈
///
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct AuthContext {
    user: AuthUser,
    scopes: BearerScopeSet,
    path_prefixes: PathPrefixSet,
    user_attributes: UserAttributeSet,
    token_id: Option<TokenId>,
    token_name: Option<String>,
}

///
/// Bearer認証コアの成功結果
///
#[derive(Clone, Debug)]
pub(crate) struct BearerAuthSuccess {
    auth: AuthContext,
    updated_expire_at: Option<DateTime<Local>>,
}

impl AuthUser {
    ///
    /// 認証済みユーザ情報の生成
    ///
    /// # 引数
    /// * `user_id` - ユーザID
    ///
    /// # 戻り値
    /// 生成したユーザ情報を返す。
    ///
    pub(crate) fn new(user_id: String) -> Self {
        Self { user_id }
    }

    ///
    /// ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// ユーザIDを返す。
    ///
    pub(crate) fn user_id(&self) -> &str {
        &self.user_id
    }
}

#[allow(dead_code)]
impl AuthContext {
    ///
    /// 認証文脈の生成
    ///
    /// # 引数
    /// * `user` - 認証済みユーザ
    /// * `scopes` - 付与スコープ集合
    /// * `path_prefixes` - path prefix 制約集合
    /// * `token_id` - BearerトークンID
    ///
    /// # 戻り値
    /// 生成した認証文脈を返す。
    ///
    pub(crate) fn new(
        user: AuthUser,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
        token_id: Option<TokenId>,
    ) -> Self {
        Self::new_with_attributes(
            user,
            scopes,
            path_prefixes,
            UserAttributeSet::new(),
            token_id,
            None,
        )
    }

    ///
    /// ユーザ属性付き認証文脈の生成
    ///
    /// # 引数
    /// * `user` - 認証済みユーザ
    /// * `scopes` - 付与スコープ集合
    /// * `path_prefixes` - path prefix 制約集合
    /// * `user_attributes` - ユーザ属性集合
    /// * `token_id` - BearerトークンID
    /// * `token_name` - Bearerトークン任意名
    ///
    /// # 戻り値
    /// 生成した認証文脈を返す。
    ///
    pub(crate) fn new_with_attributes(
        user: AuthUser,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
        user_attributes: UserAttributeSet,
        token_id: Option<TokenId>,
        token_name: Option<String>,
    ) -> Self {
        Self {
            user,
            scopes,
            path_prefixes,
            user_attributes,
            token_id,
            token_name,
        }
    }

    ///
    /// 認証済みユーザへのアクセサ
    ///
    /// # 戻り値
    /// 認証済みユーザを返す。
    ///
    pub(crate) fn user(&self) -> &AuthUser {
        &self.user
    }

    ///
    /// ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// 認証済みユーザのユーザIDを返す。
    ///
    pub(crate) fn user_id(&self) -> &str {
        self.user.user_id()
    }

    ///
    /// 付与スコープ集合へのアクセサ
    ///
    /// # 戻り値
    /// 認証文脈が保持するスコープ集合を返す。
    ///
    pub(crate) fn scopes(&self) -> &BearerScopeSet {
        &self.scopes
    }

    ///
    /// path prefix 制約集合へのアクセサ
    ///
    /// # 戻り値
    /// 認証文脈が保持する path prefix 制約集合を返す。
    ///
    pub(crate) fn path_prefixes(&self) -> &PathPrefixSet {
        &self.path_prefixes
    }

    ///
    /// ユーザ属性集合へのアクセサ
    ///
    /// # 戻り値
    /// 認証文脈が保持するユーザ属性集合を返す。
    ///
    pub(crate) fn user_attributes(&self) -> &UserAttributeSet {
        &self.user_attributes
    }

    ///
    /// BearerトークンIDへのアクセサ
    ///
    /// # 戻り値
    /// BearerトークンIDが存在する場合は参照を返す。
    ///
    pub(crate) fn token_id(&self) -> Option<&TokenId> {
        self.token_id.as_ref()
    }

    ///
    /// Bearerトークン任意名へのアクセサ
    ///
    /// # 戻り値
    /// Bearerトークン任意名が存在する場合は参照を返す。
    ///
    pub(crate) fn token_name(&self) -> Option<&str> {
        self.token_name.as_deref()
    }
}

impl BearerAuthSuccess {
    ///
    /// Bearer認証コアの成功結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `updated_expire_at` - TTL 延長後の有効期限
    ///
    /// # 戻り値
    /// 生成した成功結果を返す。
    ///
    pub(crate) fn new(
        auth: AuthContext,
        updated_expire_at: Option<DateTime<Local>>,
    ) -> Self {
        Self {
            auth,
            updated_expire_at,
        }
    }

    ///
    /// 共通認証文脈を返す
    ///
    /// # 戻り値
    /// 生成済みの認証文脈を返す。
    ///
    pub(crate) fn auth(&self) -> &AuthContext {
        &self.auth
    }

    ///
    /// TTL 延長後の有効期限を返す
    ///
    /// # 戻り値
    /// TTL 延長が発生した場合は更新後の有効期限を返す。
    ///
    pub(crate) fn updated_expire_at(&self) -> Option<DateTime<Local>> {
        self.updated_expire_at
    }
}

///
/// 共通 Bearer認証コア
///
/// # 引数
/// * `db` - データベースマネージャ
/// * `token` - Bearerトークン平文
/// * `now` - TTL 延長判定に用いる現在時刻
///
/// # 戻り値
/// 認証に成功した場合は共通認証文脈と TTL 延長結果を返す。
/// 認証失敗時は Bearer認証失敗理由を返す。
///
pub(crate) fn authenticate_bearer_token(
    db: &DatabaseManager,
    token: &BearerTokenPlaintext,
    now: DateTime<Local>,
) -> Result<
    std::result::Result<BearerAuthSuccess, VerifyBearerTokenFailureReason>,
> {
    let result = match db.verify_bearer_token(token)? {
        Ok(result) => result,
        Err(reason) => return Ok(Err(reason)),
    };

    let user_info = result.user_info();
    let token_info = result.token_info();
    let token_id = token_info.token_id();
    let updated_expire_at =
        db.extend_bearer_token_ttl_if_needed(&token_id, now)?;
    let auth = AuthContext::new_with_attributes(
        AuthUser::new(user_info.username()),
        token_info.scopes(),
        token_info.path_prefixes(),
        user_info.attributes(),
        Some(token_id),
        token_info.name(),
    );

    Ok(Ok(BearerAuthSuccess::new(auth, updated_expire_at)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::Local;

    use super::*;
    use crate::database::types::{
        BearerScope,
        PathPrefixSet,
        UserAttribute,
        UserAttributeSet,
    };

    ///
    /// Bearer 認証成功時に `ReadOnly` 属性が認証文脈へ引き継がれることを確認する。
    ///
    /// # 注記
    /// `cargo test authenticate_bearer_token_propagates_user_attributes -- --exact`
    /// で実行する。
    ///
    #[test]
    fn authenticate_bearer_token_propagates_user_attributes() {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");
        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager
            .add_user_with_attributes(
                "alice",
                Some("password123"),
                None,
                UserAttributeSet::from_iter([
                    UserAttribute::NoBasicAuth,
                    UserAttribute::ReadOnly,
                ]),
            )
            .expect("add user failed");
        let (token, token_info) = manager
            .create_bearer_token(
                "alice",
                BearerScopeSet::from_iter([BearerScope::Read]),
                PathPrefixSet::new(),
                chrono::Duration::minutes(30),
                Some("auth test token".to_string()),
            )
            .expect("create bearer token failed");

        let success = authenticate_bearer_token(&manager, &token, Local::now())
            .expect("authenticate failed")
            .expect("bearer auth must succeed");
        let auth = success.auth();

        assert_eq!(auth.user_id(), "alice");
        assert!(auth.user_attributes().contains(UserAttribute::NoBasicAuth));
        assert!(auth.user_attributes().contains(UserAttribute::ReadOnly));
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
        format!("auth-{}-{}", std::process::id(), timestamp)
    }
}
