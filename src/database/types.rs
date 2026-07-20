/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース関連処理をまとめたモジュール
//!

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::ops::{Deref, RangeInclusive};

use anyhow::{Error, Result, anyhow};
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use chrono::{DateTime, Duration, Local};
use rand_core::{OsRng, RngCore};
use redb::{Key, TypeName, Value};
use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use ulid::{DecodeError, Ulid};

/// 短縮IDで利用する base62 文字集合
#[allow(dead_code)]
const BASE62_ALPHABET: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 128bit 値を base62 で表現する際の固定長
#[allow(dead_code)]
const BASE62_FIXED_WIDTH_128: usize = 22;

///
/// base62 文字を数値へ変換する
///
/// # 引数
/// * `ch` - 変換対象文字
///
/// # 戻り値
/// 対応する数値を返す。base62 に含まれない文字の場合は `None` を返す。
///
fn decode_base62_char(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'A'..=b'Z' => Some(ch - b'A' + 10),
        b'a'..=b'z' => Some(ch - b'a' + 36),
        _ => None,
    }
}

///
/// 短縮ID復元時のエラー
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShortIdDecodeError {
    /// 固定長でない
    InvalidLength,

    /// base62 に含まれない文字を含む
    InvalidCharacter,

    /// 128bit 範囲へ復元できない
    Overflow,
}

// Displayトレイトの実装
impl Display for ShortIdDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength => write!(f, "invalid short id length"),
            Self::InvalidCharacter => {
                write!(f, "invalid short id character")
            }
            Self::Overflow => write!(f, "short id overflow"),
        }
    }
}

// std::error::Error トレイトの実装
impl std::error::Error for ShortIdDecodeError {}

///
/// データベース用のIDを表す構造体
///
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) struct Id(Ulid);

impl Id {
    ///
    /// IDオブジェクトの生成
    ///
    pub(crate) fn new() -> Self {
        Self(Ulid::new())
    }

    ///
    /// 文字列からの変換
    ///
    /// # 引数
    /// * `s` - 変換対象の文字列
    ///
    /// # 戻り値
    /// 変換に成功した場合は、サービスIDオブジェクトを`Ok()`でラップして返す。失
    /// 敗した場合はエラー情報を`Err()`でラップして返す。
    ///
    pub(crate) fn from_string(s: &str) -> Result<Self, DecodeError> {
        Ulid::from_string(s).map(Self)
    }

    ///
    /// ID を base62 固定長文字列へ変換する
    ///
    /// # 戻り値
    /// ID が保持する 128bit 値を base62 で表現した固定長文字列を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn to_base62_fixed(&self) -> String {
        /*
         * 128bit 値の取り出し
         */
        let mut value = u128::from_be_bytes(self.0.to_bytes());
        let mut encoded = [BASE62_ALPHABET[0]; BASE62_FIXED_WIDTH_128];

        /*
         * 下位桁から base62 へ変換
         */
        for index in (0..BASE62_FIXED_WIDTH_128).rev() {
            let digit = (value % 62) as usize;
            encoded[index] = BASE62_ALPHABET[digit];
            value /= 62;
        }

        String::from_utf8(encoded.to_vec()).expect("invalid base62 text")
    }

    ///
    /// base62 固定長文字列から ID を復元する
    ///
    /// # 引数
    /// * `value` - 復元対象の base62 固定長文字列
    ///
    /// # 戻り値
    /// 復元に成功した場合は対応する ID を返す。入力が不正な場合はエラーを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn from_base62_fixed(
        value: &str,
    ) -> std::result::Result<Self, ShortIdDecodeError> {
        /*
         * 入力長の検証
         */
        if value.len() != BASE62_FIXED_WIDTH_128 {
            return Err(ShortIdDecodeError::InvalidLength);
        }

        /*
         * base62 文字列の復元
         */
        let mut decoded: u128 = 0;
        for ch in value.bytes() {
            let digit = match decode_base62_char(ch) {
                Some(digit) => digit as u128,
                None => return Err(ShortIdDecodeError::InvalidCharacter),
            };
            decoded = match decoded.checked_mul(62) {
                Some(value) => value,
                None => return Err(ShortIdDecodeError::Overflow),
            };
            decoded = match decoded.checked_add(digit) {
                Some(value) => value,
                None => return Err(ShortIdDecodeError::Overflow),
            };
        }

        Ok(Self(Ulid::from_bytes(decoded.to_be_bytes())))
    }

    ///
    /// IDの全域を表す範囲オブジェクトを返す
    ///
    /// # 戻り値
    /// IDの全域を表す範囲オブジェクトを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn range_all() -> RangeInclusive<Id> {
        Self::min()..=Self::max()
    }

    ///
    /// IDの最小値を返す
    ///
    /// # 戻り値
    /// IDの最小値を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn min() -> Self {
        Self::from_string("00000000000000000000000000")
            .expect("invalid ULID string")
    }

    ///
    /// IDの最大値を返す
    ///
    /// # 戻り値
    /// IDの最大値を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn max() -> Self {
        Self::from_string("7ZZZZZZZZZZZZZZZZZZZZZZZZZ")
            .expect("invalid ULID string")
    }
}

// Derefトレイトの実装
impl Deref for Id {
    type Target = Ulid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// TryFromトレイトの実装
impl TryFrom<&str> for Id {
    type Error = Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match Ulid::from_string(value) {
            Ok(ulid) => Ok(Self(ulid)),
            Err(err) => Err(err.into()),
        }
    }
}

// Fromトレイトの実装
impl From<&Ulid> for Id {
    fn from(value: &Ulid) -> Self {
        Self(value.to_owned())
    }
}

// Intoトレイトの実装
impl Into<String> for Id {
    fn into(self) -> String {
        self.0.to_string()
    }
}

// Displayトレイトの実装
impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

// Valueトレイトの実装
impl Value for Id {
    type SelfType<'a> = Id;
    type AsBytes<'a> = [u8; 16];

    fn fixed_width() -> Option<usize> {
        Some(16)
    }

    fn type_name() -> TypeName {
        TypeName::new("Id")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        value.to_bytes()
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(data);

        Self(Ulid::from_bytes(bytes))
    }
}

// Keyトレイトの実装
impl Key for Id {
    fn compare(a: &[u8], b: &[u8]) -> Ordering {
        a.cmp(b)
    }
}

// Serializeトレイトの実装
impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.0.to_string())
        } else {
            serializer.serialize_bytes(&self.0.to_bytes())
        }
    }
}

// Deserializeトレイトの実装
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let string = String::deserialize(deserializer)?;
            Ulid::from_string(&string)
                .map(Id)
                .map_err(de::Error::custom)
        } else {
            Ok(Id(Ulid::from_bytes(<[u8; 16]>::deserialize(deserializer)?)))
        }
    }
}

///
/// ページID型の定義(可読性を向上させるための別名定義)
///
pub(crate) type PageId = Id;

///
/// アセットID型の定義(可読性を向上させるための別名定義)
///
pub(crate) type AssetId = Id;

///
/// ユーザID型の定義(可読性を向上させるための別名定義)
///
pub(crate) type UserId = Id;

///
/// BearerトークンID型の定義(可読性を向上させるための別名定義)
///
#[allow(dead_code)]
pub(crate) type TokenId = Id;

///
/// ロック解除トークン型の定義(可読性を向上させるための別名定義)
///
pub(crate) type LockToken = Id;

///
/// Bearer認証のスコープ種別
///
#[allow(dead_code)]
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
pub(crate) enum BearerScope {
    /// 参照系操作を表すスコープ
    #[serde(rename = "read")]
    Read,

    /// 後方互換の更新系操作を表すスコープ
    #[serde(rename = "write")]
    Write,

    /// 新規作成系操作を表すスコープ
    #[serde(rename = "create")]
    Create,

    /// 上書き更新系操作を表すスコープ
    #[serde(rename = "update")]
    Update,

    /// 追記系操作を表すスコープ
    #[serde(rename = "append")]
    Append,

    /// 削除系操作を表すスコープ
    #[serde(rename = "delete")]
    Delete,
}

#[allow(dead_code)]
impl BearerScope {
    ///
    /// スコープの文字列表現を返す
    ///
    /// # 戻り値
    /// 外部仕様で利用するスコープ名を返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Create => "create",
            Self::Update => "update",
            Self::Append => "append",
            Self::Delete => "delete",
        }
    }
}

// Displayトレイトの実装
impl Display for BearerScope {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// TryFromトレイトの実装
impl TryFrom<&str> for BearerScope {
    type Error = Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "create" => Ok(Self::Create),
            "update" => Ok(Self::Update),
            "append" => Ok(Self::Append),
            "delete" => Ok(Self::Delete),
            _ => Err(anyhow!("invalid bearer scope: {}", value)),
        }
    }
}

///
/// Bearer認証のスコープ集合
///
#[allow(dead_code)]
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct BearerScopeSet {
    /// 保持しているスコープ集合
    scopes: BTreeSet<BearerScope>,
}

#[allow(dead_code)]
impl BearerScopeSet {
    ///
    /// 空のスコープ集合を生成する
    ///
    /// # 戻り値
    /// スコープを持たない集合を返す。
    ///
    pub(crate) fn new() -> Self {
        Self::default()
    }

    ///
    /// 全スコープ相当の集合を生成する
    ///
    /// # 戻り値
    /// 現時点で定義されている全スコープを持つ集合を返す。
    ///
    pub(crate) fn all() -> Self {
        Self::from_iter([
            BearerScope::Read,
            BearerScope::Write,
            BearerScope::Create,
            BearerScope::Update,
            BearerScope::Append,
            BearerScope::Delete,
        ])
    }

    ///
    /// スコープを追加する
    ///
    /// # 引数
    /// * `scope` - 追加するスコープ
    ///
    /// # 戻り値
    /// 追加前に同一スコープが存在しなかった場合は `true` を返す。
    ///
    pub(crate) fn insert(&mut self, scope: BearerScope) -> bool {
        self.scopes.insert(scope)
    }

    ///
    /// スコープが明示的に含まれているかを返す
    ///
    /// # 引数
    /// * `scope` - 判定対象のスコープ
    ///
    /// # 戻り値
    /// 集合内に同一スコープが存在する場合は `true` を返す。
    ///
    pub(crate) fn contains(&self, scope: BearerScope) -> bool {
        self.scopes.contains(&scope)
    }

    ///
    /// 必要スコープを満たすかを返す
    ///
    /// # 引数
    /// * `required` - 要求されるスコープ
    ///
    /// # 戻り値
    /// 要求スコープを満たす場合は `true` を返す。
    ///
    pub(crate) fn allows(&self, required: BearerScope) -> bool {
        match required {
            BearerScope::Read => {
                self.contains(BearerScope::Read) ||
                    self.contains(BearerScope::Write)
            }
            BearerScope::Write => self.contains(BearerScope::Write),
            BearerScope::Create => {
                self.contains(BearerScope::Create) ||
                    self.contains(BearerScope::Write)
            }
            BearerScope::Update => {
                self.contains(BearerScope::Update) ||
                    self.contains(BearerScope::Write)
            }
            BearerScope::Append => {
                self.contains(BearerScope::Append) ||
                    self.contains(BearerScope::Write)
            }
            BearerScope::Delete => {
                self.contains(BearerScope::Delete) ||
                    self.contains(BearerScope::Write)
            }
        }
    }

    ///
    /// 保持スコープ数を返す
    ///
    /// # 戻り値
    /// 集合に保持されているスコープ数を返す。
    ///
    pub(crate) fn len(&self) -> usize {
        self.scopes.len()
    }

    ///
    /// スコープ集合が空かを返す
    ///
    /// # 戻り値
    /// 集合が空の場合は `true` を返す。
    ///
    pub(crate) fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    ///
    /// スコープ列挙子へのイテレータを返す
    ///
    /// # 戻り値
    /// スコープ列挙子への参照を順序付きで返すイテレータを返す。
    ///
    pub(crate) fn iter(&self) -> impl Iterator<Item = &BearerScope> {
        self.scopes.iter()
    }
}

// FromIteratorトレイトの実装
impl FromIterator<BearerScope> for BearerScopeSet {
    fn from_iter<T: IntoIterator<Item = BearerScope>>(iter: T) -> Self {
        Self {
            scopes: iter.into_iter().collect(),
        }
    }
}

///
/// Bearerトークンの path prefix 制約集合
///
#[allow(dead_code)]
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PathPrefixSet {
    /// 保持している path prefix 集合
    prefixes: BTreeSet<String>,
}

#[allow(dead_code)]
impl PathPrefixSet {
    ///
    /// 空の path prefix 集合を生成する
    ///
    /// # 戻り値
    /// path prefix を持たない集合を返す。
    ///
    pub(crate) fn new() -> Self {
        Self::default()
    }

    ///
    /// path prefix を追加する
    ///
    /// # 引数
    /// * `prefix` - 追加する path prefix
    ///
    /// # 戻り値
    /// 追加前に同一 path prefix が存在しなかった場合は `true` を返す。
    ///
    pub(crate) fn insert<S>(&mut self, prefix: S) -> bool
    where
        S: Into<String>,
    {
        self.prefixes.insert(prefix.into())
    }

    ///
    /// path prefix を削除する
    ///
    /// # 引数
    /// * `prefix` - 削除する path prefix
    ///
    /// # 戻り値
    /// 削除対象が存在した場合は `true` を返す。
    ///
    pub(crate) fn remove(&mut self, prefix: &str) -> bool {
        self.prefixes.remove(prefix)
    }

    ///
    /// path prefix が明示的に含まれているかを返す
    ///
    /// # 引数
    /// * `prefix` - 判定対象の path prefix
    ///
    /// # 戻り値
    /// 集合内に同一 path prefix が存在する場合は `true` を返す。
    ///
    pub(crate) fn contains(&self, prefix: &str) -> bool {
        self.prefixes.contains(prefix)
    }

    ///
    /// 全領域アクセス可の状態かを返す
    ///
    /// # 戻り値
    /// path prefix 未設定または `/` を含む場合は `true` を返す。
    ///
    pub(crate) fn allows_all(&self) -> bool {
        self.is_empty() || self.contains("/")
    }

    ///
    /// path prefix 集合が空かを返す
    ///
    /// # 戻り値
    /// 集合が空の場合は `true` を返す。
    ///
    pub(crate) fn is_empty(&self) -> bool {
        self.prefixes.is_empty()
    }

    ///
    /// 保持 path prefix 数を返す
    ///
    /// # 戻り値
    /// 集合に保持されている path prefix 数を返す。
    ///
    pub(crate) fn len(&self) -> usize {
        self.prefixes.len()
    }

    ///
    /// path prefix 列挙子へのイテレータを返す
    ///
    /// # 戻り値
    /// path prefix 文字列への参照を順序付きで返すイテレータを返す。
    ///
    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        self.prefixes.iter().map(|prefix| prefix.as_str())
    }
}

impl<T> FromIterator<T> for PathPrefixSet
where
    T: Into<String>,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            prefixes: iter.into_iter().map(Into::into).collect(),
        }
    }
}

///
/// Bearerトークン管理情報
///
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct BearerTokenInfo {
    /// BearerトークンID
    token_id: TokenId,

    /// 発行対象ユーザID
    user_id: UserId,

    /// 付与スコープ
    scopes: BearerScopeSet,

    /// 操作対象 path prefix 制約
    #[serde(default)]
    path_prefixes: PathPrefixSet,

    /// 作成日時
    created_at: DateTime<Local>,

    /// 最終更新日時
    updated_at: DateTime<Local>,

    /// トークンTTL
    ttl: Duration,

    /// 有効期限
    expire_at: DateTime<Local>,

    /// 失効状態
    revoked: bool,

    /// 任意のトークン名
    name: Option<String>,
}

#[allow(dead_code)]
impl BearerTokenInfo {
    ///
    /// Bearerトークン管理情報を生成する
    ///
    /// # 引数
    /// * `user_id` - 発行対象ユーザID
    /// * `scopes` - 付与スコープ集合
    /// * `path_prefixes` - path prefix 制約集合
    /// * `ttl` - トークンTTL
    /// * `name` - 任意のトークン名
    ///
    /// # 戻り値
    /// 生成した Bearerトークン管理情報を返す。
    ///
    pub(crate) fn new(
        user_id: UserId,
        scopes: BearerScopeSet,
        path_prefixes: PathPrefixSet,
        ttl: Duration,
        name: Option<String>,
    ) -> Self {
        /*
         * 現在時刻を共通利用する
         */
        let now = Local::now();

        /*
         * Bearerトークン管理情報を構築する
         */
        Self {
            token_id: TokenId::new(),
            user_id,
            scopes,
            path_prefixes,
            created_at: now,
            updated_at: now,
            ttl,
            expire_at: now + ttl,
            revoked: false,
            name,
        }
    }

    ///
    /// BearerトークンIDへのアクセサ
    ///
    /// # 戻り値
    /// BearerトークンIDを返す。
    ///
    pub(crate) fn token_id(&self) -> TokenId {
        self.token_id.clone()
    }

    ///
    /// 発行対象ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// 発行対象ユーザIDを返す。
    ///
    pub(crate) fn user_id(&self) -> UserId {
        self.user_id.clone()
    }

    ///
    /// 付与スコープ集合へのアクセサ
    ///
    /// # 戻り値
    /// 付与スコープ集合を返す。
    ///
    pub(crate) fn scopes(&self) -> BearerScopeSet {
        self.scopes.clone()
    }

    ///
    /// path prefix 制約集合へのアクセサ
    ///
    /// # 戻り値
    /// path prefix 制約集合を返す。
    ///
    pub(crate) fn path_prefixes(&self) -> PathPrefixSet {
        self.path_prefixes.clone()
    }

    ///
    /// 作成日時へのアクセサ
    ///
    /// # 戻り値
    /// 作成日時を返す。
    ///
    pub(crate) fn created_at(&self) -> DateTime<Local> {
        self.created_at
    }

    ///
    /// 最終更新日時へのアクセサ
    ///
    /// # 戻り値
    /// 最終更新日時を返す。
    ///
    pub(crate) fn updated_at(&self) -> DateTime<Local> {
        self.updated_at
    }

    ///
    /// TTLへのアクセサ
    ///
    /// # 戻り値
    /// TTLを返す。
    ///
    pub(crate) fn ttl(&self) -> Duration {
        self.ttl
    }

    ///
    /// 有効期限へのアクセサ
    ///
    /// # 戻り値
    /// 有効期限を返す。
    ///
    pub(crate) fn expire_at(&self) -> DateTime<Local> {
        self.expire_at
    }

    ///
    /// 失効状態へのアクセサ
    ///
    /// # 戻り値
    /// 失効済みの場合は `true` を返す。
    ///
    pub(crate) fn revoked(&self) -> bool {
        self.revoked
    }

    ///
    /// 任意名へのアクセサ
    ///
    /// # 戻り値
    /// 任意名を返す。
    ///
    pub(crate) fn name(&self) -> Option<String> {
        self.name.clone()
    }

    ///
    /// TTL延長を反映する
    ///
    /// # 引数
    /// * `now` - 延長基準時刻
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn extend_expire_at(&mut self, now: DateTime<Local>) {
        self.expire_at = now + self.ttl;
        self.updated_at = now;
    }

    ///
    /// 失効状態を反映する
    ///
    /// # 引数
    /// * `updated_at` - 更新時刻
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn revoke(&mut self, updated_at: DateTime<Local>) {
        self.revoked = true;
        self.updated_at = updated_at;
    }

    ///
    /// path prefix 制約集合を更新する
    ///
    /// # 引数
    /// * `path_prefixes` - 更新後の path prefix 制約集合
    /// * `updated_at` - 更新時刻
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_path_prefixes(
        &mut self,
        path_prefixes: PathPrefixSet,
        updated_at: DateTime<Local>,
    ) {
        self.path_prefixes = path_prefixes;
        self.updated_at = updated_at;
    }

    ///
    /// テスト用に日時項目を上書きする
    ///
    /// # 引数
    /// * `created_at` - 作成日時
    /// * `updated_at` - 最終更新日時
    /// * `expire_at` - 有効期限
    ///
    /// # 戻り値
    /// なし
    ///
    #[allow(dead_code)]
    pub(crate) fn overwrite_timestamps_for_test(
        &mut self,
        created_at: DateTime<Local>,
        updated_at: DateTime<Local>,
        expire_at: DateTime<Local>,
    ) {
        self.created_at = created_at;
        self.updated_at = updated_at;
        self.expire_at = expire_at;
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl BearerTokenInfo {
    ///
    /// テスト用の Bearerトークン管理情報を生成する
    ///
    /// # 引数
    /// * `token_id` - BearerトークンID
    /// * `user_id` - 発行対象ユーザID
    /// * `scopes` - 付与スコープ集合
    /// * `created_at` - 作成日時
    /// * `updated_at` - 最終更新日時
    /// * `ttl` - TTL
    /// * `expire_at` - 有効期限
    /// * `revoked` - 失効状態
    /// * `path_prefixes` - path prefix 制約集合
    /// * `name` - 任意名
    ///
    /// # 戻り値
    /// テスト用の Bearerトークン管理情報を返す。
    ///
    pub(crate) fn new_for_test(
        token_id: TokenId,
        user_id: UserId,
        scopes: BearerScopeSet,
        created_at: DateTime<Local>,
        updated_at: DateTime<Local>,
        ttl: Duration,
        expire_at: DateTime<Local>,
        revoked: bool,
        path_prefixes: PathPrefixSet,
        name: Option<String>,
    ) -> Self {
        Self {
            token_id,
            user_id,
            scopes,
            path_prefixes,
            created_at,
            updated_at,
            ttl,
            expire_at,
            revoked,
            name,
        }
    }
}

// Valueトレイトの実装
impl Value for BearerTokenInfo {
    type SelfType<'a> = BearerTokenInfo;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("BearerTokenInfo")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// Bearerトークン平文
///
/// # 注記
/// 既定の `Display` / `Debug` は伏字化し、CLI の明示出力など
/// 必要な箇所だけが平文へアクセスする。
///
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct BearerTokenPlaintext(String);

#[allow(dead_code)]
impl BearerTokenPlaintext {
    ///
    /// Bearerトークン平文を生成する
    ///
    /// # 引数
    /// * `value` - 保持する平文文字列
    ///
    /// # 戻り値
    /// Bearerトークン平文オブジェクトを返す。
    ///
    pub(crate) fn new<S>(value: S) -> Self
    where
        S: Into<String>,
    {
        Self(value.into())
    }

    ///
    /// 平文文字列へのアクセサ
    ///
    /// # 戻り値
    /// Bearerトークン平文を返す。
    ///
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Display for BearerTokenPlaintext {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted bearer token]")
    }
}

impl std::fmt::Debug for BearerTokenPlaintext {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted bearer token]")
    }
}

///
/// Bearerトークン照合用ハッシュ値
///
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) struct TokenHash([u8; 32]);

#[allow(dead_code)]
impl TokenHash {
    ///
    /// トークン平文から照合用ハッシュ値を生成する
    ///
    /// # 引数
    /// * `token` - Bearerトークン平文
    ///
    /// # 戻り値
    /// SHA-256 で計算した照合用ハッシュ値を返す。
    ///
    pub(crate) fn from_token(token: &str) -> Self {
        let digest = Sha256::digest(token.as_bytes());
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(digest.as_slice());
        Self(bytes)
    }

    ///
    /// 生バイト列表現へのアクセサ
    ///
    /// # 戻り値
    /// 32 バイト固定長のハッシュ値を返す。
    ///
    pub(crate) fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    ///
    /// 16進文字列へ変換する
    ///
    /// # 戻り値
    /// 小文字16進のハッシュ文字列を返す。
    ///
    pub(crate) fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// Fromトレイトの実装
impl From<[u8; 32]> for TokenHash {
    fn from(value: [u8; 32]) -> Self {
        Self(value)
    }
}

// Displayトレイトの実装
impl Display for TokenHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

// Valueトレイトの実装
impl Value for TokenHash {
    type SelfType<'a> = TokenHash;
    type AsBytes<'a> = [u8; 32];

    fn fixed_width() -> Option<usize> {
        Some(32)
    }

    fn type_name() -> TypeName {
        TypeName::new("TokenHash")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        value.to_bytes()
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(data);
        Self(bytes)
    }
}

// Keyトレイトの実装
impl Key for TokenHash {
    fn compare(a: &[u8], b: &[u8]) -> Ordering {
        a.cmp(b)
    }
}

// Serializeトレイトの実装
impl Serialize for TokenHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_hex())
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

// Deserializeトレイトの実装
impl<'de> Deserialize<'de> for TokenHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let string = String::deserialize(deserializer)?;
            if string.len() != 64 {
                return Err(de::Error::custom("invalid token hash length"));
            }

            let mut bytes = [0u8; 32];
            for (index, chunk) in string.as_bytes().chunks(2).enumerate() {
                let text = std::str::from_utf8(chunk)
                    .map_err(de::Error::custom)?;
                bytes[index] = u8::from_str_radix(text, 16)
                    .map_err(de::Error::custom)?;
            }

            Ok(Self(bytes))
        } else {
            Ok(Self(<[u8; 32]>::deserialize(deserializer)?))
        }
    }
}

///
/// ページインデックス管理構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum PageIndex {
    /// 通常ページ情報
    PageInfo(PageInfo),

    /// ドラフトページ情報
    DraftInfo(DraftInfo),
}

///
/// ページパス状態
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum PagePathState {
    /// 現在のパス
    Current(String),

    /// 削除時点のパス
    LastDeleted(String),
}

impl PagePathState {
    ///
    /// パス文字列への参照
    ///
    /// # 戻り値
    /// パス文字列への参照を返す。
    ///
    pub(crate) fn value(&self) -> &str {
        match self {
            PagePathState::Current(path) => path,
            PagePathState::LastDeleted(path) => path,
        }
    }

    ///
    /// 現在パス判定
    ///
    /// # 戻り値
    /// 現在パスの場合は`Some(&str)`を返す。
    ///
    pub(crate) fn current(&self) -> Option<&str> {
        match self {
            PagePathState::Current(path) => Some(path),
            PagePathState::LastDeleted(_) => None,
        }
    }

    ///
    /// 削除時パス判定
    ///
    /// # 戻り値
    /// 削除時点のパスの場合は`Some(&str)`を返す。
    ///
    pub(crate) fn last_deleted(&self) -> Option<&str> {
        match self {
            PagePathState::Current(_) => None,
            PagePathState::LastDeleted(path) => Some(path),
        }
    }
}

impl PageIndex {
    ///
    /// 通常ページの生成
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 生成したページ情報を含むページインデックスを返す。
    ///
    pub(crate) fn new_page(id: PageId, path: String) -> Self {
        PageIndex::PageInfo(PageInfo::new(id, path))
    }

    ///
    /// import 用の通常ページ生成
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    /// * `latest` - 最新リビジョン
    /// * `earliest` - 最古リビジョン
    /// * `rename_revisions` - path 確定リビジョン一覧
    ///
    /// # 戻り値
    /// import 用に復元したページインデックスを返す。
    ///
    pub(crate) fn new_page_import(
        id: PageId,
        path: String,
        latest: u64,
        earliest: u64,
        rename_revisions: Vec<u64>,
    ) -> Self {
        PageIndex::PageInfo(PageInfo {
            id,
            path_state: PagePathState::Current(path),
            latest,
            earliest,
            lock_token: None,
            rename_revisions,
        })
    }

    ///
    /// ドラフトページの生成
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 生成したドラフト情報を含むページインデックスを返す。
    ///
    pub(crate) fn new_draft(id: PageId, path: String) -> Self {
        PageIndex::DraftInfo(DraftInfo::new(id, path))
    }

    ///
    /// ドラフト判定
    ///
    /// # 戻り値
    /// ドラフトの場合は`true`を返す。
    ///
    pub(crate) fn is_draft(&self) -> bool {
        matches!(self, PageIndex::DraftInfo(_))
    }

    ///
    /// 通常ページ情報への参照
    ///
    /// # 戻り値
    /// 通常ページの場合は`Some(&PageInfo)`を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn as_page_info(&self) -> Option<&PageInfo> {
        match self {
            PageIndex::PageInfo(info) => Some(info),
            PageIndex::DraftInfo(_) => None,
        }
    }

    ///
    /// 通常ページ情報への可変参照
    ///
    /// # 戻り値
    /// 通常ページの場合は`Some(&mut PageInfo)`を返す。
    ///
    pub(crate) fn as_page_info_mut(&mut self) -> Option<&mut PageInfo> {
        match self {
            PageIndex::PageInfo(info) => Some(info),
            PageIndex::DraftInfo(_) => None,
        }
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn id(&self) -> PageId {
        match self {
            PageIndex::PageInfo(info) => info.id.clone(),
            PageIndex::DraftInfo(info) => info.id.clone(),
        }
    }

    ///
    /// ページパスへのアクセサ
    ///
    /// # 戻り値
    /// ページパスを返す。
    ///
    pub(crate) fn path(&self) -> String {
        match self {
            PageIndex::PageInfo(info) => info.path_state.value().to_string(),
            PageIndex::DraftInfo(info) => info.path.clone(),
        }
    }

    ///
    /// 現在パスへの参照
    ///
    /// # 戻り値
    /// 現在パスがある場合は`Some(&str)`を返す。
    ///
    pub(crate) fn current_path(&self) -> Option<&str> {
        match self {
            PageIndex::PageInfo(info) => info.path_state.current(),
            PageIndex::DraftInfo(info) => Some(info.path.as_str()),
        }
    }

    ///
    /// 削除時パスへの参照
    ///
    /// # 戻り値
    /// 削除時パスがある場合は`Some(&str)`を返す。
    ///
    pub(crate) fn last_deleted_path(&self) -> Option<&str> {
        match self {
            PageIndex::PageInfo(info) => info.path_state.last_deleted(),
            PageIndex::DraftInfo(_) => None,
        }
    }

    ///
    /// 最新リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// 最新リビジョン番号を返す。
    ///
    pub(crate) fn latest(&self) -> u64 {
        match self {
            PageIndex::PageInfo(info) => info.latest,
            PageIndex::DraftInfo(_) => 0,
        }
    }

    ///
    /// 最古リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// 最古リビジョン番号を返す。
    ///
    pub(crate) fn earliest(&self) -> u64 {
        match self {
            PageIndex::PageInfo(info) => info.earliest,
            PageIndex::DraftInfo(_) => 0,
        }
    }

    ///
    /// リネーム履歴リビジョン一覧へのアクセサ
    ///
    /// # 戻り値
    /// リネーム履歴のリビジョン番号一覧を返す。
    ///
    pub(crate) fn rename_revisions(&self) -> Vec<u64> {
        match self {
            PageIndex::PageInfo(info) => info.rename_revisions.clone(),
            PageIndex::DraftInfo(_) => Vec::new(),
        }
    }

    ///
    /// 最新リビジョン番号の更新
    ///
    /// # 引数
    /// * `latest` - 新しい最新リビジョン番号
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_latest(&mut self, latest: u64) {
        if let Some(info) = self.as_page_info_mut() {
            info.latest = latest;
        }
    }

    ///
    /// 最古リビジョン番号の更新
    ///
    /// # 引数
    /// * `earliest` - 新しい最古リビジョン番号
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_earliest(&mut self, earliest: u64) {
        if let Some(info) = self.as_page_info_mut() {
            info.earliest = earliest;
        }
    }

    ///
    /// ページパスの更新
    ///
    /// # 引数
    /// * `path` - 新しいページパス
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_path(&mut self, path: String) {
        match self {
            PageIndex::PageInfo(info) => {
                info.path_state = PagePathState::Current(path);
            }
            PageIndex::DraftInfo(info) => info.path = path,
        }
    }

    ///
    /// リネーム履歴リビジョンの追加
    ///
    /// # 引数
    /// * `revision` - 追加するリビジョン番号
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn push_rename_revision(&mut self, revision: u64) {
        if let Some(info) = self.as_page_info_mut() {
            info.rename_revisions.push(revision);
        }
    }

    ///
    /// 削除済みフラグへのアクセサ
    ///
    /// # 戻り値
    /// 削除済みの場合は`true`を返す。
    ///
    pub(crate) fn deleted(&self) -> bool {
        match self {
            PageIndex::PageInfo(info) => {
                matches!(info.path_state, PagePathState::LastDeleted(_))
            }
            PageIndex::DraftInfo(_) => false,
        }
    }

    ///
    /// 削除済みフラグの更新
    ///
    /// # 引数
    /// * `deleted` - 更新後の削除済みフラグ
    ///
    /// # 戻り値
    /// なし
    ///
    #[allow(dead_code)]
    pub(crate) fn set_deleted(&mut self, deleted: bool) {
        if let Some(info) = self.as_page_info_mut() {
            if deleted {
                if let Some(path) = info
                    .path_state
                    .current()
                    .map(|value| value.to_string())
                {
                    info.path_state = PagePathState::LastDeleted(path);
                }
            } else if let Some(path) = info
                .path_state
                .last_deleted()
                .map(|value| value.to_string())
            {
                info.path_state = PagePathState::Current(path);
            }
        }
    }

    ///
    /// 削除時パスの設定
    ///
    /// # 引数
    /// * `path` - 削除時点のパス
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_deleted_path(&mut self, path: String) {
        if let Some(info) = self.as_page_info_mut() {
            info.path_state = PagePathState::LastDeleted(path);
        }
    }

    ///
    /// ロック解除トークンへのアクセサ
    ///
    /// # 戻り値
    /// ロック解除トークンを返す。
    ///
    pub(crate) fn lock_token(&self) -> Option<LockToken> {
        match self {
            PageIndex::PageInfo(info) => info.lock_token.clone(),
            PageIndex::DraftInfo(_) => None,
        }
    }

    ///
    /// ロック解除トークンの更新
    ///
    /// # 引数
    /// * `token` - 新しいロック解除トークン
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_lock_token(&mut self, token: Option<LockToken>) {
        if let Some(info) = self.as_page_info_mut() {
            info.lock_token = token;
        }
    }
}

// Valueトレイトの実装
impl Value for PageIndex {
    type SelfType<'a> = PageIndex;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("PageIndex")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// 通常ページ情報
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PageInfo {
    /// ページ固有のID
    id: PageId,

    /// 現在パス／削除時パス
    path_state: PagePathState,

    /// 最新リビジョン
    latest: u64,

    /// 下限リビジョン
    earliest: u64,

    /// ロック解除トークン
    #[serde(default)]
    lock_token: Option<LockToken>,

    /// path が確定・変更されたリビジョン番号の一覧（昇順）
    /// ページ作成時の初期パス割り当ても必ず含める
    rename_revisions: Vec<u64>,
}

impl PageInfo {
    ///
    /// ページインデックスの生成
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 生成したページインデックスを返す。
    ///
    /// # 注記
    /// ページ作成時の生成専用として、リビジョン番号は1で固定する。
    ///
    pub(crate) fn new(id: PageId, path: String) -> Self {
        let revision = 1u64;

        Self {
            id,
            path_state: PagePathState::Current(path),
            latest: revision,
            earliest: revision,
            lock_token: None,
            rename_revisions: vec![revision],
        }
    }
}

///
/// ドラフトページ情報
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct DraftInfo {
    /// ページ固有のID
    id: PageId,

    /// 現在のパス
    path: String,
}

impl DraftInfo {
    ///
    /// ドラフトページ情報の生成
    ///
    /// # 引数
    /// * `id` - ページID
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 生成したドラフトページ情報を返す。
    ///
    pub(crate) fn new(id: PageId, path: String) -> Self {
        Self { id, path }
    }
}

///
/// ページソース管理構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PageSource {
    /// リビジョン番号
    revision: u64,

    /// 実体識別用のインスタンスID
    #[serde(default)]
    instance_id: Option<Id>,

    /// 作成した日時
    timestamp: DateTime<Local>,

    /// このリビジョンを作成したユーザ識別子
    user: UserId,

    /// path が割り当て／変更されたリビジョン情報
    rename: RenameInfo,

    /// ページのソース(Markdown形式)
    source: String,
}

impl PageSource {
    ///
    /// ページソースの生成
    ///
    /// # 引数
    /// * `source` - ページソース
    /// * `user` - 作成したユーザID
    /// * `rename` - リネーム情報
    ///
    /// # 戻り値
    /// 生成したページソースを返す。
    ///
    /// # 注記
    /// ページ作成時の生成専用として、リビジョン番号は1で固定する。
    ///
    pub(crate) fn new(
        source: String,
        user: UserId,
        rename: RenameInfo,
    ) -> Self {
        let revision = 1u64;

        Self {
            revision,
            instance_id: Some(Id::new()),
            timestamp: Local::now(),
            user,
            rename,
            source,
        }
    }

    ///
    /// ページソースの生成(任意リビジョン)
    ///
    /// # 引数
    /// * `revision` - リビジョン番号
    /// * `source` - ページソース
    /// * `user` - 作成したユーザID
    /// * `rename` - リネーム情報
    ///
    /// # 戻り値
    /// 生成したページソースを返す。
    ///
    pub(crate) fn new_revision(
        revision: u64,
        source: String,
        user: UserId,
        rename: RenameInfo,
    ) -> Self {
        Self {
            revision,
            instance_id: Some(Id::new()),
            timestamp: Local::now(),
            user,
            rename,
            source,
        }
    }

    ///
    /// import 用のページソース生成
    ///
    /// # 引数
    /// * `revision` - リビジョン番号
    /// * `instance_id` - 実体識別用インスタンスID
    /// * `timestamp` - 作成日時
    /// * `user` - 編集者ユーザID
    /// * `rename` - リネーム情報
    /// * `source` - Markdown ソース
    ///
    /// # 戻り値
    /// import 用に復元したページソースを返す。
    ///
    pub(crate) fn new_import(
        revision: u64,
        instance_id: Option<Id>,
        timestamp: DateTime<Local>,
        user: UserId,
        rename: RenameInfo,
        source: String,
    ) -> Self {
        Self {
            revision,
            instance_id,
            timestamp,
            user,
            rename,
            source,
        }
    }

    ///
    /// リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// リビジョン番号を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    ///
    /// ページソースへのアクセサ
    ///
    /// # 戻り値
    /// ページソースを返す。
    ///
    pub(crate) fn source(&self) -> String {
        self.source.clone()
    }

    ///
    /// インスタンスIDへのアクセサ
    ///
    /// # 戻り値
    /// インスタンスIDを返す。
    ///
    pub(crate) fn instance_id(&self) -> Option<Id> {
        self.instance_id.clone()
    }

    ///
    /// ページソースの更新
    ///
    /// # 引数
    /// * `source` - 更新後のページソース
    ///
    pub(crate) fn update_source(&mut self, source: String) {
        self.source = source;
        self.instance_id = Some(Id::new());
        self.timestamp = Local::now();
    }

    ///
    /// 作成日時へのアクセサ
    ///
    /// # 戻り値
    /// 作成日時を返す。
    ///
    pub(crate) fn timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    ///
    /// 記述したユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// 記述したユーザIDを返す。
    ///
    pub(crate) fn user(&self) -> UserId {
        self.user.clone()
    }

    ///
    /// リネーム情報へのアクセサ
    ///
    /// # 戻り値
    /// リネーム情報を返す。
    ///
    pub(crate) fn rename(&self) -> RenameInfo {
        self.rename.clone()
    }
}

// Valueトレイトの実装
impl Value for PageSource {
    type SelfType<'a> = PageSource;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("PageSource")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        match rmp_serde::from_slice::<Self>(data) {
            Ok(source) => source,
            Err(_) => rmp_serde::from_slice::<page_source_v1::PageSourceV1>(
                data,
            )
            .expect("invalid MessagePack packed bytes")
            .into_page_source(),
        }
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// テンプレート候補派生データ
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum TemplateCandidateSource {
    /// front matter 由来候補
    FrontMatter,

    /// legacy template_root 由来候補
    LegacyTemplateRoot,
}

impl Default for TemplateCandidateSource {
    fn default() -> Self {
        Self::FrontMatter
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct TemplateCandidateEntry {
    /// テンプレート表示名
    name: String,

    /// テンプレート説明
    description: Option<String>,

    /// マクロ即時展開可否
    macro_expand: Option<bool>,

    /// 候補の由来
    #[serde(default)]
    source: TemplateCandidateSource,
}

impl TemplateCandidateEntry {
    ///
    /// テンプレート候補派生データの生成
    ///
    /// # 引数
    /// * `name` - テンプレート表示名
    /// * `description` - テンプレート説明
    /// * `macro_expand` - マクロ即時展開可否
    ///
    /// # 戻り値
    /// 生成したテンプレート候補派生データを返す。
    ///
    pub(crate) fn new(
        name: String,
        description: Option<String>,
        macro_expand: Option<bool>,
        source: TemplateCandidateSource,
    ) -> Self {
        Self {
            name,
            description,
            macro_expand,
            source,
        }
    }

    ///
    /// テンプレート表示名へのアクセサ
    ///
    /// # 戻り値
    /// テンプレート表示名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// テンプレート説明へのアクセサ
    ///
    /// # 戻り値
    /// テンプレート説明を返す。
    ///
    pub(crate) fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    ///
    /// マクロ即時展開可否へのアクセサ
    ///
    /// # 戻り値
    /// マクロ即時展開可否を返す。
    ///
    pub(crate) fn macro_expand(&self) -> Option<bool> {
        self.macro_expand
    }

    ///
    /// 候補の由来へのアクセサ
    ///
    /// # 戻り値
    /// 候補の由来を返す。
    ///
    pub(crate) fn source(&self) -> &TemplateCandidateSource {
        &self.source
    }
}

// Valueトレイトの実装
impl Value for TemplateCandidateEntry {
    type SelfType<'a> = TemplateCandidateEntry;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("TemplateCandidateEntry")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// MCP primitive種別
///
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum McpPrimitiveKind {
    /// prompt primitive
    Prompt,
}

impl McpPrimitiveKind {
    ///
    /// primitive種別の固定文字列を取得する
    ///
    /// # 戻り値
    /// primitive種別を表す固定文字列を返す。
    ///
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
        }
    }

    ///
    /// redbキー用の識別子を取得する
    ///
    /// # 戻り値
    /// primitive種別に対応する固定識別子を返す。
    ///
    fn key_tag(self) -> u8 {
        match self {
            Self::Prompt => 0x01,
        }
    }

    ///
    /// redbキー用の識別子からprimitive種別を復元する
    ///
    /// # 引数
    /// * `tag` - primitive識別子
    ///
    /// # 戻り値
    /// 識別子に対応するprimitive種別を返す。
    ///
    fn from_key_tag(tag: u8) -> Self {
        match tag {
            0x01 => Self::Prompt,
            _ => panic!("invalid MCP primitive key tag"),
        }
    }
}

///
/// MCP primitive共通名前索引キー
///
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct McpPrimitiveNameKey {
    /// primitive種別
    primitive: McpPrimitiveKind,

    /// primitive名
    name: String,
}

impl McpPrimitiveNameKey {
    ///
    /// MCP primitive共通名前索引キーを生成する
    ///
    /// # 引数
    /// * `primitive` - primitive種別
    /// * `name` - primitive名
    ///
    /// # 戻り値
    /// primitive種別と名前を保持するキーを返す。
    ///
    pub(crate) fn new(
        primitive: McpPrimitiveKind,
        name: String,
    ) -> Self {
        Self { primitive, name }
    }

    ///
    /// primitive種別へのアクセサ
    ///
    /// # 戻り値
    /// primitive種別を返す。
    ///
    pub(crate) fn primitive(&self) -> McpPrimitiveKind {
        self.primitive
    }

    ///
    /// primitive名へのアクセサ
    ///
    /// # 戻り値
    /// primitive名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

// Valueトレイトの実装
impl Value for McpPrimitiveNameKey {
    type SelfType<'a> = McpPrimitiveNameKey;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("McpPrimitiveNameKey")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        let (tag, name) = data
            .split_first()
            .expect("invalid empty MCP primitive name key");
        let primitive = McpPrimitiveKind::from_key_tag(*tag);
        let name = String::from_utf8(name.to_vec())
            .expect("invalid UTF-8 MCP primitive name key");

        Self::new(primitive, name)
    }

    fn as_bytes<'a, 'b: 'a>(
        value: &'a Self::SelfType<'b>,
    ) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        let mut bytes = Vec::with_capacity(value.name.len() + 1);
        bytes.push(value.primitive.key_tag());
        bytes.extend_from_slice(value.name.as_bytes());
        bytes
    }
}

// Keyトレイトの実装
impl Key for McpPrimitiveNameKey {
    fn compare(a: &[u8], b: &[u8]) -> Ordering {
        a.cmp(b)
    }
}

///
/// prompt 引数派生データ
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PromptArgumentEntry {
    /// 引数名
    name: String,

    /// 引数説明
    description: String,

    /// 必須可否
    required: Option<bool>,
}

impl PromptArgumentEntry {
    ///
    /// prompt 引数派生データの生成
    ///
    /// # 引数
    /// * `name` - 引数名
    /// * `description` - 引数説明
    /// * `required` - 必須可否
    ///
    /// # 戻り値
    /// 生成した prompt 引数派生データを返す。
    ///
    pub(crate) fn new(
        name: String,
        description: String,
        required: Option<bool>,
    ) -> Self {
        Self {
            name,
            description,
            required,
        }
    }

    ///
    /// 引数名へのアクセサ
    ///
    /// # 戻り値
    /// 引数名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// 引数説明へのアクセサ
    ///
    /// # 戻り値
    /// 引数説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// 必須可否へのアクセサ
    ///
    /// # 戻り値
    /// 必須可否を返す。
    ///
    pub(crate) fn required(&self) -> Option<bool> {
        self.required
    }
}

///
/// prompt 候補派生データ
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PromptCandidateEntry {
    /// prompt 名
    name: String,

    /// prompt 説明
    description: String,

    /// system 情報
    system: Option<String>,

    /// prompt 引数
    arguments: Vec<PromptArgumentEntry>,
}

impl PromptCandidateEntry {
    ///
    /// prompt 候補派生データの生成
    ///
    /// # 引数
    /// * `name` - prompt 名
    /// * `description` - prompt 説明
    /// * `system` - system 情報
    /// * `arguments` - prompt 引数
    ///
    /// # 戻り値
    /// 生成した prompt 候補派生データを返す。
    ///
    pub(crate) fn new(
        name: String,
        description: String,
        system: Option<String>,
        arguments: Vec<PromptArgumentEntry>,
    ) -> Self {
        Self {
            name,
            description,
            system,
            arguments,
        }
    }

    ///
    /// prompt 名へのアクセサ
    ///
    /// # 戻り値
    /// prompt 名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// prompt 説明へのアクセサ
    ///
    /// # 戻り値
    /// prompt 説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// system 情報へのアクセサ
    ///
    /// # 戻り値
    /// system 情報が存在する場合はその値を返す。
    ///
    pub(crate) fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    ///
    /// prompt 引数へのアクセサ
    ///
    /// # 戻り値
    /// prompt 引数を定義順で返す。
    ///
    pub(crate) fn arguments(&self) -> &[PromptArgumentEntry] {
        &self.arguments
    }
}

// Valueトレイトの実装
impl Value for PromptCandidateEntry {
    type SelfType<'a> = PromptCandidateEntry;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("PromptCandidateEntry")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// resource 候補派生データ
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ResourceCandidateEntry {
    /// resource path
    resource_path: String,

    /// resource 名
    name: String,

    /// resource 説明
    description: String,

    /// MIME type
    mime_type: Option<String>,
}

impl ResourceCandidateEntry {
    ///
    /// resource 候補派生データの生成
    ///
    /// # 引数
    /// * `resource_path` - resource path
    /// * `name` - resource 名
    /// * `description` - resource 説明
    /// * `mime_type` - MIME type
    ///
    /// # 戻り値
    /// 生成した resource 候補派生データを返す。
    ///
    pub(crate) fn new(
        resource_path: String,
        name: String,
        description: String,
        mime_type: Option<String>,
    ) -> Self {
        Self {
            resource_path,
            name,
            description,
            mime_type,
        }
    }

    ///
    /// resource path へのアクセサ
    ///
    /// # 戻り値
    /// resource path を返す。
    ///
    pub(crate) fn resource_path(&self) -> &str {
        &self.resource_path
    }

    ///
    /// resource 名へのアクセサ
    ///
    /// # 戻り値
    /// resource 名を返す。
    ///
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    ///
    /// resource 説明へのアクセサ
    ///
    /// # 戻り値
    /// resource 説明を返す。
    ///
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    ///
    /// MIME type へのアクセサ
    ///
    /// # 戻り値
    /// MIME type が存在する場合はその値を返す。
    ///
    pub(crate) fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
}

// Valueトレイトの実装
impl Value for ResourceCandidateEntry {
    type SelfType<'a> = ResourceCandidateEntry;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("ResourceCandidateEntry")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(
        value: &'a Self::SelfType<'b>,
    ) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// リネーム操作情報構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum RenameInfo {
    None,
    Active {
        /// 旧パス（作成時は None 相当として扱う）
        from: Option<String>,

        /// 新パス
        to: String,

        /// リネーム直前時点でのページ中リンク解決状態（1段分）
        /// key: 正規化済み path
        /// value: 解決された page_id（未作成等で解決できなかった場合 None）
        link_refs: BTreeMap<String, Option<Id>>,
    },
    RemovedByMigrate,
}

impl RenameInfo {
    ///
    /// リネームなし情報の生成
    ///
    /// # 戻り値
    /// 生成したリネームなし情報を返す。
    ///
    pub(crate) fn none() -> Self {
        Self::None
    }

    ///
    /// リネーム情報の生成
    ///
    /// # 引数
    /// * `from` - 旧パス
    /// * `to` - 新パス
    /// * `link_refs` - リンク解決情報
    ///
    /// # 戻り値
    /// 生成したリネーム情報を返す。
    ///
    pub(crate) fn new(
        from: Option<String>,
        to: String,
        link_refs: BTreeMap<String, Option<Id>>,
    ) -> Self {
        Self::Active {
            from,
            to,
            link_refs,
        }
    }

    ///
    /// マイグレートにより失効したリネーム情報の生成
    ///
    /// # 戻り値
    /// 生成した失効リネーム情報を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn removed_by_migrate() -> Self {
        Self::RemovedByMigrate
    }

    ///
    /// 旧パスへのアクセサ
    ///
    /// # 戻り値
    /// 旧パスを返す。
    ///
    pub(crate) fn from(&self) -> Option<String> {
        match self {
            RenameInfo::Active { from, .. } => from.clone(),
            RenameInfo::None | RenameInfo::RemovedByMigrate => None,
        }
    }

    ///
    /// 新パスへのアクセサ
    ///
    /// # 戻り値
    /// 新パスを返す。
    ///
    pub(crate) fn to(&self) -> Option<String> {
        match self {
            RenameInfo::Active { to, .. } => Some(to.clone()),
            RenameInfo::None | RenameInfo::RemovedByMigrate => None,
        }
    }

    ///
    /// リンク解決情報へのアクセサ
    ///
    /// # 戻り値
    /// リンク解決情報を返す。
    ///
    pub(crate) fn link_refs(&self) -> Option<BTreeMap<String, Option<Id>>> {
        match self {
            RenameInfo::Active { link_refs, .. } => Some(link_refs.clone()),
            RenameInfo::None | RenameInfo::RemovedByMigrate => None,
        }
    }

    ///
    /// 通常リネーム判定
    ///
    /// # 戻り値
    /// 通常リネームの場合はtrueを返す。
    ///
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, RenameInfo::Active { .. })
    }

    ///
    /// マイグレート失効判定
    ///
    /// # 戻り値
    /// マイグレート失効の場合はtrueを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn is_removed_by_migrate(&self) -> bool {
        matches!(self, RenameInfo::RemovedByMigrate)
    }
}

mod page_source_v1 {
    use super::*;

    #[derive(Deserialize)]
    pub(super) struct PageSourceV1 {
        revision: u64,
        #[serde(default)]
        instance_id: Option<Id>,
        timestamp: DateTime<Local>,
        user: UserId,
        rename: Option<RenameInfoV1>,
        source: String,
    }

    #[derive(Deserialize)]
    struct RenameInfoV1 {
        from: Option<String>,
        to: String,
        link_refs: BTreeMap<String, Option<Id>>,
    }

    impl PageSourceV1 {
        pub(super) fn into_page_source(self) -> PageSource {
            PageSource {
                revision: self.revision,
                instance_id: self.instance_id,
                timestamp: self.timestamp,
                user: self.user,
                rename: match self.rename {
                    Some(rename) => RenameInfo::new(
                        rename.from,
                        rename.to,
                        rename.link_refs,
                    ),
                    None => RenameInfo::none(),
                },
                source: self.source,
            }
        }
    }
}

///
/// アセット情報構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AssetInfo {
    /// アセットID
    id: AssetId,

    /// 実体識別用のインスタンスID
    #[serde(default)]
    instance_id: Option<Id>,

    /// 所属ページID(Noneの場合はゾンビ状態)
    #[serde(default)]
    page_id: Option<PageId>,

    /// ファイル名
    file_name: String,

    /// MIME種別
    mime: String,

    /// バイナリサイズ
    size: u64,

    /// 登録ユーザID
    user: UserId,

    /// アップロードした日時
    timestamp: DateTime<Local>,

    /// 削除済みフラグ
    deleted: bool,
}

impl AssetInfo {
    ///
    /// アセット情報の生成
    ///
    /// # 引数
    /// * `id` - アセットID
    /// * `page_id` - 所属ページID
    /// * `file_name` - ファイル名
    /// * `mime` - MIME種別
    /// * `size` - バイナリサイズ(バイト)
    /// * `user` - 登録ユーザID
    ///
    /// # 戻り値
    /// 生成したアセット情報を返す。
    ///
    pub(crate) fn new(
        id: AssetId,
        page_id: PageId,
        file_name: String,
        mime: String,
        size: u64,
        user: UserId,
    ) -> Self {
        Self {
            id,
            instance_id: Some(Id::new()),
            page_id: Some(page_id),
            file_name,
            mime,
            size,
            user,
            timestamp: Local::now(),
            deleted: false,
        }
    }

    ///
    /// import 用のアセット情報生成
    ///
    /// # 引数
    /// * `id` - アセットID
    /// * `instance_id` - 実体識別用インスタンスID
    /// * `page_id` - 所属ページID
    /// * `file_name` - ファイル名
    /// * `mime` - MIME種別
    /// * `size` - サイズ
    /// * `user` - 登録ユーザID
    /// * `timestamp` - 登録日時
    /// * `deleted` - 削除済みフラグ
    ///
    /// # 戻り値
    /// import 用に復元したアセット情報を返す。
    ///
    pub(crate) fn new_import(
        id: AssetId,
        instance_id: Option<Id>,
        page_id: Option<PageId>,
        file_name: String,
        mime: String,
        size: u64,
        user: UserId,
        timestamp: DateTime<Local>,
        deleted: bool,
    ) -> Self {
        Self {
            id,
            instance_id,
            page_id,
            file_name,
            mime,
            size,
            user,
            timestamp,
            deleted,
        }
    }

    ///
    /// アセットIDへのアクセサ
    ///
    /// # 戻り値
    /// アセットIDを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn id(&self) -> AssetId {
        self.id.clone()
    }

    ///
    /// インスタンスIDへのアクセサ
    ///
    /// # 戻り値
    /// インスタンスIDを返す。
    ///
    pub(crate) fn instance_id(&self) -> Option<Id> {
        self.instance_id.clone()
    }

    ///
    /// 所属ページIDへのアクセサ
    ///
    /// # 戻り値
    /// 所属ページIDを返す。
    ///
    pub(crate) fn page_id(&self) -> Option<PageId> {
        self.page_id.clone()
    }

    ///
    /// ゾンビ状態の判定
    ///
    /// # 戻り値
    /// 所属ページを持たない場合は`true`を返す。
    ///
    pub(crate) fn is_zombie(&self) -> bool {
        self.page_id.is_none()
    }

    ///
    /// 所属ページIDの更新
    ///
    /// # 引数
    /// * `page_id` - 更新後の所属ページID
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn set_page_id(&mut self, page_id: PageId) {
        self.page_id = Some(page_id);
    }

    ///
    /// 所属ページIDのクリア
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn clear_page_id(&mut self) {
        self.page_id = None;
    }

    ///
    /// ファイル名へのアクセサ
    ///
    /// # 戻り値
    /// ファイル名を返す。
    ///
    pub(crate) fn file_name(&self) -> String {
        self.file_name.clone()
    }

    ///
    /// ファイル名の更新
    ///
    /// # 引数
    /// * `file_name` - 更新後のファイル名
    ///
    pub(crate) fn set_file_name(&mut self, file_name: String) {
        self.file_name = file_name;
    }

    ///
    /// MIME種別へのアクセサ
    ///
    /// # 戻り値
    /// MIME種別を返す。
    ///
    pub(crate) fn mime(&self) -> String {
        self.mime.clone()
    }

    ///
    /// バイナリサイズへのアクセサ
    ///
    /// # 戻り値
    /// バイナリサイズ(バイト)を返す。
    ///
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    ///
    /// 登録ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// 登録ユーザIDを返す。
    ///
    pub(crate) fn user(&self) -> UserId {
        self.user.clone()
    }

    ///
    /// 登録日時へのアクセサ
    ///
    /// # 戻り値
    /// 登録日時を返す。
    ///
    pub(crate) fn timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    ///
    /// 削除済みフラグへのアクセサ
    ///
    /// # 戻り値
    /// 削除済みの場合は`true`を返す。
    ///
    pub(crate) fn deleted(&self) -> bool {
        self.deleted
    }

    ///
    /// 削除済みフラグの更新
    ///
    /// # 引数
    /// * `deleted` - 更新後の削除済みフラグ
    ///
    pub(crate) fn set_deleted(&mut self, deleted: bool) {
        self.deleted = deleted;
    }
}

// Valueトレイトの実装
impl Value for AssetInfo {
    type SelfType<'a> = AssetInfo;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("AssetInfo")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// ユーザ情報構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct UserInfo {
    /// ユーザID
    id: UserId,

    /// 表示名
    username: String,

    /// ハッシュ化済みパスワード
    password: String,

    /// ハッシュ時に与えるソルトデータ
    salt: [u8; 16],

    /// 表示名
    display_name: String,

    /// ユーザ属性集合
    #[serde(default)]
    attributes: UserAttributeSet,

    /// 最終更新日時
    timestamp: DateTime<Local>,
}

///
/// ユーザ属性種別
///
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
pub(crate) enum UserAttribute {
    /// Basic認証を拒否する属性
    #[serde(rename = "NoBasicAuth")]
    NoBasicAuth,

    /// write 系操作を拒否する属性
    #[serde(rename = "ReadOnly")]
    ReadOnly,
}

impl UserAttribute {
    ///
    /// 属性の文字列表現を返す
    ///
    /// # 戻り値
    /// 外部仕様で利用する属性名を返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::NoBasicAuth => "NoBasicAuth",
            Self::ReadOnly => "ReadOnly",
        }
    }
}

impl Display for UserAttribute {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<&str> for UserAttribute {
    type Error = Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "NoBasicAuth" => Ok(Self::NoBasicAuth),
            "ReadOnly" => Ok(Self::ReadOnly),
            _ => Err(anyhow!("invalid user attribute: {}", value)),
        }
    }
}

///
/// ユーザ属性集合
///
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct UserAttributeSet {
    /// 保持している属性集合
    attributes: BTreeSet<UserAttribute>,
}

impl UserAttributeSet {
    ///
    /// 空の属性集合を生成する
    ///
    /// # 戻り値
    /// 属性を持たない集合を返す。
    ///
    pub(crate) fn new() -> Self {
        Self::default()
    }

    ///
    /// 属性を追加する
    ///
    /// # 引数
    /// * `attribute` - 追加する属性
    ///
    /// # 戻り値
    /// 追加前に同一属性が存在しなかった場合は `true` を返す。
    ///
    pub(crate) fn insert(&mut self, attribute: UserAttribute) -> bool {
        self.attributes.insert(attribute)
    }

    ///
    /// 属性を削除する
    ///
    /// # 引数
    /// * `attribute` - 削除する属性
    ///
    /// # 戻り値
    /// 削除対象が存在した場合は `true` を返す。
    ///
    pub(crate) fn remove(&mut self, attribute: UserAttribute) -> bool {
        self.attributes.remove(&attribute)
    }

    ///
    /// 属性が含まれているかを返す
    ///
    /// # 引数
    /// * `attribute` - 判定対象の属性
    ///
    /// # 戻り値
    /// 集合内に同一属性が存在する場合は `true` を返す。
    ///
    pub(crate) fn contains(&self, attribute: UserAttribute) -> bool {
        self.attributes.contains(&attribute)
    }

    ///
    /// 属性集合が空かを返す
    ///
    /// # 戻り値
    /// 集合が空の場合は `true` を返す。
    ///
    pub(crate) fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    ///
    /// 属性列挙子へのイテレータを返す
    ///
    /// # 戻り値
    /// 属性列挙子への参照を順序付きで返すイテレータを返す。
    ///
    pub(crate) fn iter(&self) -> impl Iterator<Item = &UserAttribute> {
        self.attributes.iter()
    }
}

impl FromIterator<UserAttribute> for UserAttributeSet {
    fn from_iter<T: IntoIterator<Item = UserAttribute>>(iter: T) -> Self {
        Self {
            attributes: iter.into_iter().collect(),
        }
    }
}

impl UserInfo {
    ///
    /// ユーザ情報の作成
    ///
    /// # 引数
    /// * `name` - ユーザ名
    /// * `password` - 登録するパスワード
    /// * `display_name` - 表示名
    ///
    /// # 戻り値
    /// 生成したユーザ情報をパックしたオブジェクトを返す。
    ///
    /// # 注記
    /// 本関数を呼び出すとユーザ情報を生成する。引数`name`で指定されたユーザ名は
    /// 表示名として使用される。引数`password`で指定されたパスワードはそのまま保
    /// 存せず、ランダムに生成されたソルトデータと掛け合わせたデータでハッシュ化
    /// し、その結果を文字列化した物を格納する。
    ///
    #[allow(dead_code)]
    pub(crate) fn new<S>(name: S, password: S, display_name: Option<S>) -> Self
    where
        S: AsRef<str>,
    {
        Self::new_with_attributes(
            name,
            Some(password.as_ref()),
            display_name,
            UserAttributeSet::new(),
        )
    }

    ///
    /// 属性付きユーザ情報の作成
    ///
    /// # 引数
    /// * `name` - ユーザ名
    /// * `password` - 登録するパスワード
    /// * `display_name` - 表示名
    /// * `attributes` - ユーザ属性集合
    ///
    /// # 戻り値
    /// 属性を反映したユーザ情報を返す。
    ///
    /// # 注記
    /// `password` が `None` の場合は、Basic認証を使わないユーザ向けに
    /// ランダムな内部専用文字列を用いてハッシュ値を生成する。
    ///
    pub(crate) fn new_with_attributes<S>(
        name: S,
        password: Option<&str>,
        display_name: Option<S>,
        attributes: UserAttributeSet,
    ) -> Self
    where
        S: AsRef<str>,
    {
        /*
         * パスワード入力値を決定する
         */
        let password_value = password
            .map(str::to_string)
            .unwrap_or_else(|| UserId::new().to_string());

        /*
         * ソルトデータの生成
         */
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let salt_string =
            SaltString::encode_b64(&salt).expect("salt encode failed");
        let argon2 = Argon2::default();
        let hashed = argon2
            .hash_password(password_value.as_bytes(), &salt_string)
            .expect("hash failed")
            .to_string();

        /*
         * ユーザ情報の構築
         */
        Self {
            id: UserId::new(),
            username: name.as_ref().to_string(),
            password: hashed,
            salt,
            display_name: display_name.unwrap_or(name).as_ref().to_string(),
            attributes,
            timestamp: Local::now(),
        }
    }

    ///
    /// import 用のユーザ情報生成
    ///
    /// # 引数
    /// * `id` - ユーザID
    /// * `username` - ユーザ名
    /// * `password` - ハッシュ済みパスワード
    /// * `salt` - ソルト
    /// * `display_name` - 表示名
    /// * `attributes` - ユーザ属性集合
    /// * `timestamp` - 更新日時
    ///
    /// # 戻り値
    /// import 用に復元したユーザ情報を返す。
    ///
    pub(crate) fn new_import(
        id: UserId,
        username: String,
        password: String,
        salt: [u8; 16],
        display_name: String,
        attributes: UserAttributeSet,
        timestamp: DateTime<Local>,
    ) -> Self {
        Self {
            id,
            username,
            password,
            salt,
            display_name,
            attributes,
            timestamp,
        }
    }

    ///
    /// ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// ユーザIDオブジェクトを返す
    ///
    pub(crate) fn id(&self) -> UserId {
        self.id.clone()
    }

    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn username(&self) -> String {
        self.username.clone()
    }

    ///
    /// ハッシュ化済みパスワードへのアクセサ
    ///
    /// # 戻り値
    /// ハッシュ化済みパスワードを返す。
    ///
    pub(crate) fn password(&self) -> String {
        self.password.clone()
    }

    ///
    /// ソルトデータへのアクセサ
    ///
    /// # 戻り値
    /// ソルトデータを返す。
    ///
    pub(crate) fn salt(&self) -> [u8; 16] {
        self.salt
    }

    ///
    /// 表示名へのアクセサ
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn display_name(&self) -> String {
        self.display_name.clone()
    }

    ///
    /// ユーザ属性集合へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ属性集合を返す。
    ///
    pub(crate) fn attributes(&self) -> UserAttributeSet {
        self.attributes.clone()
    }

    ///
    /// 更新日時へのアクセサ
    ///
    /// # 戻り値
    /// 更新日時表を返す
    ///
    pub(crate) fn timestamp(&self) -> DateTime<Local> {
        self.timestamp.clone()
    }

    ///
    /// 表示名の更新
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn set_display_name(&mut self, name: &str) {
        self.display_name = name.to_string();
        self.timestamp = Local::now();
    }

    ///
    /// パスワードの更新
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn set_password(&mut self, password: &str) {
        /*
         * ソルトデータの生成
         */
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        /*
         * パスワードのハッシュ化
         */
        let salt_string =
            SaltString::encode_b64(&salt).expect("salt encode failed");
        let argon2 = Argon2::default();
        let hashed = argon2
            .hash_password(password.as_bytes(), &salt_string)
            .expect("hash failed")
            .to_string();

        /*
         * 更新内容の反映
         */
        self.password = hashed;
        self.salt = salt;
        self.timestamp = Local::now();
    }

    ///
    /// ユーザ属性集合を更新する
    ///
    /// # 引数
    /// * `attributes` - 更新後のユーザ属性集合
    ///
    pub(crate) fn set_attributes(&mut self, attributes: UserAttributeSet) {
        self.attributes = attributes;
        self.timestamp = Local::now();
    }

    ///
    /// パスワードの検証
    ///
    /// # 引数
    /// * `password` - 検証対象のパスワード
    ///
    /// # 戻り値
    /// 検証に成功した場合は`true`、失敗した場合は`false`を返す。
    ///
    pub(crate) fn verify_password(&self, password: &str) -> bool {
        let parsed = match PasswordHash::new(&self.password) {
            Ok(hash) => hash,
            Err(_) => return false,
        };

        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    ///
    /// Basic認証を許可できるかを返す
    ///
    /// # 戻り値
    /// `NoBasicAuth` 属性を持たない場合は `true` を返す。
    ///
    pub(crate) fn allows_basic_auth(&self) -> bool {
        !self.attributes.contains(UserAttribute::NoBasicAuth)
    }

    ///
    /// write 系操作を許可できるかを返す
    ///
    /// # 戻り値
    /// `ReadOnly` 属性を持たない場合は `true` を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn allows_write(&self) -> bool {
        !self.attributes.contains(UserAttribute::ReadOnly)
    }
}

#[cfg(test)]
impl UserInfo {
    ///
    /// テスト用のユーザ情報生成
    ///
    /// # 引数
    /// * `id` - ユーザID
    /// * `timestamp` - 更新日時
    /// * `username` - ユーザ名
    /// * `display_name` - 表示名
    /// * `attributes` - ユーザ属性集合
    ///
    /// # 戻り値
    /// 生成したユーザ情報を返す。
    ///
    pub(crate) fn new_for_test(
        id: UserId,
        timestamp: DateTime<Local>,
        username: &str,
        display_name: &str,
        attributes: UserAttributeSet,
    ) -> Self {
        Self {
            id,
            username: username.to_string(),
            password: "dummy".to_string(),
            salt: [0u8; 16],
            display_name: display_name.to_string(),
            attributes,
            timestamp,
        }
    }
}

// Valueトレイトの実装
impl Value for UserInfo {
    type SelfType<'a> = UserInfo;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("UserInfo")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// ロック情報構造体
///
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct LockInfo {
    /// ロック解除トークン
    token: LockToken,

    /// ロック対象のページ
    page: PageId,

    /// ロックを行ったユーザ
    user: UserId,

    /// 表示名
    expire: DateTime<Local>,
}

#[allow(dead_code)]
impl LockInfo {
    ///
    /// ロック情報の作成
    ///
    /// # 引数
    /// * `page_id` - ロック対象のページのID
    /// * `username` - ロックを行ったユーザの名前
    ///
    /// # 戻り値
    /// 生成したロック情報をパックしたオブジェクトを返す。
    ///
    /// # 注記
    /// 本関数を呼び出すとロック情報を生成する。
    ///
    pub(crate) fn new(page: &PageId, user: &UserId) -> Self {
        Self {
            token: LockToken::new(),
            page: page.clone(),
            expire: Local::now() + Duration::minutes(5),
            user: user.clone(),
        }
    }

    ///
    /// ロック解除トークンへのアクセサ
    ///
    /// # 戻り値
    /// ロック解除トークンオブジェクトを返す
    ///
    pub(crate) fn token(&self) -> LockToken {
        self.token.clone()
    }

    ///
    /// ロック対象ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDオブジェクトを返す
    ///
    pub(crate) fn page(&self) -> PageId {
        self.page.clone()
    }

    ///
    /// 有効期限へのアクセサ
    ///
    /// # 戻り値
    /// ページIDオブジェクトを返す
    ///
    pub(crate) fn expire(&self) -> DateTime<Local> {
        self.expire.clone()
    }

    ///
    /// 記述したユーザのIDへのアクセサ
    ///
    /// # 戻り値
    /// ユーザIDオブジェクトを返す
    ///
    pub(crate) fn user(&self) -> UserId {
        self.user.clone()
    }

    ///
    /// ロックオブジェクトの有効期間の延長
    ///
    /// # 戻り値
    /// ページIDオブジェクトを返す
    ///
    /// # 注記
    /// 本メソッドを呼び出すと以下の処理を行いロックオブジェクトの更新を行う。
    ///
    ///   - 有効期限の延長 (延長幅は5分)
    ///   - ロック解除トークンの再振り出し
    ///
    /// なお本メソッドを呼び出してもロック対象のページ情報の更新は行われないので
    /// 注意すること。
    ///
    pub(crate) fn renew(self: &mut Self) {
        self.token = LockToken::new();
        self.expire = Local::now() + Duration::minutes(5);
    }
}

// Valueトレイトの実装
impl Value for LockInfo {
    type SelfType<'a> = LockInfo;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn type_name() -> TypeName {
        TypeName::new("LockInfo")
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}
