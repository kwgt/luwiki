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
    BearerScope,
    BearerScopeSet,
    BearerTokenPlaintext,
    PathPrefixSet,
    TokenId,
    UserAttribute,
    UserAttributeSet,
    UserId,
};

use crate::wiki_path::{NormalizedWikiPath, path_matches_prefix};

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
    /// 認証時に確定した不変主体ID（互換constructorでは未指定）
    actor_id: Option<UserId>,
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
            actor_id: None,
        }
    }

    ///
    /// 検証済みの不変主体IDを持つ認証文脈を生成する
    ///
    /// # 引数
    /// * `user` - 認証済みユーザ名
    /// * `scopes` - 付与スコープ
    /// * `path_prefixes` - パス制約
    /// * `user_attributes` - ユーザ属性
    /// * `token_id` - BearerトークンID（BasicはNone）
    /// * `token_name` - 既存のトークン任意名
    /// * `actor_id` - 認証で検証した不変ユーザID
    ///
    /// # 戻り値
    /// 不変主体を保持する認証文脈。
    ///
    pub(crate) fn new_with_actor(
        user: AuthUser,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
        user_attributes: UserAttributeSet,
        token_id: Option<TokenId>,
        token_name: Option<String>,
        actor_id: UserId,
    ) -> Self {
        let mut context = Self::new_with_attributes(
            user, scopes, path_prefixes, user_attributes,
            token_id, token_name,
        );
        context.actor_id = Some(actor_id);
        context
    }

    ///
    /// 認証時に確定した不変主体IDを取得する
    ///
    /// # 戻り値
    /// 検証済みID。互換constructorではNone。
    ///
    pub(crate) fn actor_id(&self) -> Option<&UserId> {
        self.actor_id.as_ref()
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
    let auth = AuthContext::new_with_actor(
        AuthUser::new(user_info.username()),
        token_info.scopes(),
        token_info.path_prefixes(),
        user_info.attributes(),
        Some(token_id),
        token_info.name(),
        user_info.id(),
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

    ///
    /// 互換constructorと名前変更時の不変actor補足を検証する
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 旧constructorのNoneと異なるユーザ名の同一actorを比較する。
    ///
    #[test]
    fn audit_actor_constructor_preserves_name_compatibility() {
        /*
         * 旧constructorはIDをユーザ名から推測しない
         */
        let old = AuthContext::new(
            AuthUser::new("before".into()), BearerScopeSet::all(),
            PathPrefixSet::new(), None,
        );
        assert_eq!(old.actor_id(), None);
        assert_eq!(old.user_id(), "before");
        let attributed = AuthContext::new_with_attributes(
            AuthUser::new("before".into()), BearerScopeSet::all(),
            PathPrefixSet::new(), UserAttributeSet::new(), None, None,
        );
        assert_eq!(attributed.actor_id(), None);

        /*
         * ユーザ名は既存accessorで保持しactorとは独立させる
         */
        let id = UserId::new();
        for name in ["before", "after"] {
            let context = AuthContext::new_with_actor(
                AuthUser::new(name.into()), BearerScopeSet::all(),
                PathPrefixSet::new(), UserAttributeSet::new(),
                None, None, id.clone(),
            );
            assert_eq!(context.actor_id(), Some(&id));
            assert_eq!(context.user_id(), name);
            assert_eq!(context.token_id(), None);
        }
    }

    ///
    /// 実Bearer認証後のユーザ更新・削除でもactorを失わない
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 認証後に表示名更新、削除、同名再作成し旧contextと比較する。
    ///
    #[test]
    fn audit_actor_bearer_survives_user_replacement() {
        /*
         * 隔離DBで実際のBearer検証から不変IDを取得する
         */
        let dir = tempfile::tempdir().unwrap();
        let db = DatabaseManager::open(
            dir.path().join("db"), dir.path().join("assets"),
        ).unwrap();
        db.add_user("alice", "password", None).unwrap();
        let id = db.get_user_info_by_name("alice").unwrap().unwrap().id();
        let (token, info) = db.create_bearer_token(
            "alice", BearerScopeSet::all(), PathPrefixSet::new(),
            chrono::Duration::minutes(30), None,
        ).unwrap();
        let success = authenticate_bearer_token(&db, &token, Local::now())
            .unwrap().unwrap();
        let context = success.auth().clone();
        assert_eq!(context.actor_id(), Some(&id));
        assert_eq!(context.token_id(), Some(&info.token_id()));

        /*
         * 要求途中のDB主体変更を監査終了時のlookupへ依存させない
         */
        db.update_user("alice", Some("updated display"), None).unwrap();
        assert_eq!(context.actor_id(), Some(&id));
        db.delete_user("alice").unwrap();
        assert!(db.get_user_info_by_name("alice").unwrap().is_none());
        assert_eq!(context.actor_id(), Some(&id));
        db.add_user("alice", "new password", None).unwrap();
        let new_id = db.get_user_info_by_name("alice").unwrap().unwrap().id();
        assert_ne!(new_id, id);
        assert_eq!(context.actor_id(), Some(&id));
        assert_eq!(context.user_id(), "alice");
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

///
/// 保存済みprefixを変換しないページ認可policy
///
#[derive(Clone, Copy, Debug)]
pub(crate) struct PageAccessPolicy<'a> {
    /// 認証文脈またはDB操作入口が保持するprefix集合
    prefixes: &'a PathPrefixSet,
}

impl<'a> PageAccessPolicy<'a> {
    ///
    /// 保存済みprefixから認可policyを作る
    ///
    /// # 引数
    /// * `prefixes` - 変換しないprefix集合
    ///
    /// # 戻り値
    /// prefix集合を参照するpolicyを返す。
    ///
    pub(crate) fn new(prefixes: &'a PathPrefixSet) -> Self {
        Self { prefixes }
    }

    ///
    /// 最終パスが許可範囲にあるかを判定する
    ///
    /// # 引数
    /// * `path` - 保存表記の対象パス。不正規形は変換せず拒否する
    ///
    /// # 戻り値
    /// 無制約またはいずれかのprefix内ならtrueを返す。
    ///
    pub(crate) fn allows(&self, path: &str) -> bool {
        if NormalizedWikiPath::from_stored(path).is_err() {
            return false;
        }
        self.prefixes.allows_all() || self.prefixes.iter()
            .any(|prefix| path_matches_prefix(path, prefix))
    }
}

///
/// 公開境界で区別するscope認可の拒否理由
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScopeAuthorizationError {
    /// 要求scopeの不足
    ScopeDenied,
    /// 書き込みscopeより優先するユーザ属性による拒否
    ReadOnlyDenied,
}

impl std::fmt::Display for ScopeAuthorizationError {
    ///
    /// 拒否理由の固定文字列を生成する
    ///
    /// # 引数
    /// * `f` - 文字列出力先
    ///
    /// # 戻り値
    /// 書き込み結果を返す。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ScopeDenied => "required scope denied",
            Self::ReadOnlyDenied => "read-only operation denied",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ScopeAuthorizationError {}

///
/// 単一要求scopeを共有規則で判定する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `required` - 操作ごとに確定したscope
///
/// # 戻り値
/// 許可、または型付き拒否理由を含むエラーを返す。
///
pub(crate) fn authorize_scope(
    auth: &AuthContext,
    required: BearerScope,
) -> Result<()> {
    authorize_any_scope(auth, &[required])
}

///
/// 明示されたOR要求scopeをReadOnly優先で判定する
///
/// # 引数
/// * `auth` - 認証文脈
/// * `required` - 当該操作を許可するscope候補
///
/// # 戻り値
/// 少なくとも一つの候補が許可されれば成功。空候補は拒否する。
///
/// # 概要
/// 共有POSTの分岐は呼び出し側で確定してから渡す。
/// 別アクションのscopeをORに混ぜて許可しない。
///
pub(crate) fn authorize_any_scope(
    auth: &AuthContext,
    required: &[BearerScope],
) -> Result<()> {
    /*
     * ReadOnlyがある場合は書き込み候補を無効にする
     */
    let read_only = auth.user_attributes().contains(UserAttribute::ReadOnly);
    let allowed = required.iter().any(|scope| {
        (!read_only || *scope == BearerScope::Read)
            && auth.scopes().allows(*scope)
    });
    if allowed {
        return Ok(());
    }

    /*
     * scope不足と属性による書き込み拒否を区別する
     */
    if read_only && required.iter().any(|scope| *scope != BearerScope::Read) {
        return Err(ScopeAuthorizationError::ReadOnlyDenied.into());
    }
    Err(ScopeAuthorizationError::ScopeDenied.into())
}

#[cfg(test)]
mod scope_and_path_tests {
    use super::{
        AuthContext, AuthUser, PageAccessPolicy, ScopeAuthorizationError,
        authorize_any_scope, authorize_scope,
    };
    use crate::database::types::{
        BearerScope, BearerScopeSet, PathPrefixSet, UserAttribute,
        UserAttributeSet,
    };

    ///
    /// 単一scopeと属性を持つテスト文脈を生成する
    ///
    /// # 引数
    /// * `scope` - テスト付与scope
    /// * `read_only` - ReadOnly属性を持たせる指定
    ///
    /// # 戻り値
    /// ファイルやDBを使わない認証文脈を返す。
    ///
    fn context(scope: BearerScope, read_only: bool) -> AuthContext {
        let mut attributes = UserAttributeSet::new();
        if read_only {
            attributes.insert(UserAttribute::ReadOnly);
        }
        AuthContext::new_with_attributes(
            AuthUser::new("scope_test".to_string()),
            BearerScopeSet::from_iter([scope]),
            PathPrefixSet::new(), attributes, None, None,
        )
    }

    ///
    /// ORの許可とaction単一要求、read非包含を検証する
    ///
    /// # 注記
    /// create単独でlock ORが通りupdate/read要求は拒否されることを確認。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn scope_or_keeps_action_and_read_requirements_separate() {
        /*
         * 細分scopeが他操作やread/writeを暗黙包含しないことを確認する
         */
        for scope in [
            BearerScope::Create, BearerScope::Update, BearerScope::Delete,
            BearerScope::Append,
        ] {
            let auth = context(scope, false);
            assert!(authorize_scope(&auth, scope).is_ok());
            assert!(authorize_scope(&auth, BearerScope::Read).is_err());
            assert!(authorize_scope(&auth, BearerScope::Write).is_err());
        }

        /*
         * lockのORをaction別の単一要求と分ける
         */
        let lock = [BearerScope::Create, BearerScope::Update];
        for scope in lock {
            assert!(authorize_any_scope(&context(scope, false), &lock)
                .is_ok());
        }
        let auth = context(BearerScope::Create, false);
        assert!(authorize_scope(&auth, BearerScope::Update).is_err());
        assert!(authorize_any_scope(&auth, &[]).is_err());
        assert!(authorize_any_scope(&context(BearerScope::Delete, false),
            &lock).is_err());
    }

    ///
    /// write上位互換とReadOnly優先を検証する
    ///
    /// # 注記
    /// 全細分scope許可と、ReadOnly時の書き込み拒否/参照許可を比較。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn legacy_write_respects_read_only_before_scope() {
        /*
         * writeで細分操作を許可する
         */
        let auth = context(BearerScope::Write, false);
        for scope in [
            BearerScope::Read, BearerScope::Create, BearerScope::Update,
            BearerScope::Append, BearerScope::Delete,
        ] {
            assert!(authorize_scope(&auth, scope).is_ok());
        }

        /*
         * ReadOnlyが書き込み許可より優先することを確認する
         */
        let auth = context(BearerScope::Write, true);
        let err = authorize_scope(&auth, BearerScope::Create).unwrap_err();
        assert_eq!(err.downcast_ref::<ScopeAuthorizationError>(),
            Some(&ScopeAuthorizationError::ReadOnlyDenied));
        assert!(authorize_scope(&auth, BearerScope::Read).is_ok());
    }

    ///
    /// prefixの境界・複数指定・全領域を検証する
    ///
    /// # 注記
    /// 完全一致/配下と似た接頭辞を比較し、保存prefixを縮約しない。
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    /// # 戻り値
    /// なし
    ///
    #[test]
    fn policy_preserves_boundaries_and_stored_prefixes() {
        /*
         * 複数prefixと不正規保存表記を変換せず比較する
         */
        let mut prefixes = PathPrefixSet::new();
        prefixes.insert("/docs");
        prefixes.insert("/archive");
        prefixes.insert("/private/../secret");
        let policy = PageAccessPolicy::new(&prefixes);
        assert!(policy.allows("/docs"));
        assert!(policy.allows("/docs/a"));
        assert!(policy.allows("/archive/a"));
        assert!(!policy.allows("/docs2"));
        assert!(!policy.allows("/secret"));

        /*
         * 未設定およびroot指定の全領域許可を確認する
         */
        let empty = PathPrefixSet::new();
        assert!(PageAccessPolicy::new(&empty).allows("/any"));
        for path in ["", "/docs/../secret", "/docs/", "//", "/bad name"] {
            assert!(!policy.allows(path));
            assert!(!PageAccessPolicy::new(&empty).allows(path));
        }
        prefixes.insert("/");
        assert!(PageAccessPolicy::new(&prefixes).allows("/any"));
    }
    ///
    /// Basic相当のfullscopeと無制約prefixを検証する
    ///
    /// # 注記
    /// Basicと同じ集合を作り全操作を許可、ReadOnlyは書込み拒否する。
    ///
    /// # 戻り値
    /// なし
    ///
    /// # 概要
    /// 操作ごとの許可条件と拒否条件を比較して確認する。
    ///
    #[test]
    fn basic_equivalent_context_respects_read_only() {
        /*
         * 認証入口と同じfullscopeと無制約prefixを利用する
         */
        let auth = AuthContext::new(
            AuthUser::new("basic_test".to_string()),
            BearerScopeSet::all(), PathPrefixSet::new(), None,
        );
        assert!(PageAccessPolicy::new(auth.path_prefixes()).allows("/any"));
        assert!(authorize_scope(&auth, BearerScope::Create).is_ok());
        assert!(authorize_scope(&auth, BearerScope::Read).is_ok());

        /*
         * fullscopeでもユーザ属性を優先する
         */
        let mut read_only = auth;
        read_only.user_attributes.insert(UserAttribute::ReadOnly);
        assert!(authorize_scope(&read_only, BearerScope::Read).is_ok());
        assert!(authorize_any_scope(&read_only,
            &[BearerScope::Create, BearerScope::Update]).is_err());
    }

}
