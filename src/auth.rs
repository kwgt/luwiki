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
    token_id: Option<TokenId>,
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
        Self {
            user,
            scopes,
            path_prefixes,
            token_id,
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
    /// BearerトークンIDへのアクセサ
    ///
    /// # 戻り値
    /// BearerトークンIDが存在する場合は参照を返す。
    ///
    pub(crate) fn token_id(&self) -> Option<&TokenId> {
        self.token_id.as_ref()
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
    let auth = AuthContext::new(
        AuthUser::new(user_info.username()),
        token_info.scopes(),
        token_info.path_prefixes(),
        Some(token_id),
    );

    Ok(Ok(BearerAuthSuccess::new(auth, updated_expire_at)))
}
