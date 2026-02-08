/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース関連処理をまとめたモジュール
//!

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::ops::{Deref, RangeInclusive};

use anyhow::{Error, Result};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use argon2::password_hash::{PasswordHash, SaltString};
use chrono::{DateTime, Duration, Local};
use rand_core::{OsRng, RngCore};
use redb::{Key, TypeName, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::de;
use ulid::{DecodeError, Ulid};

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
    /// IDの全域を表す範囲オブジェクトを返す
    ///
    #[allow(dead_code)]
    pub(crate) fn range_all() -> RangeInclusive<Id> {
        Self::min()..=Self::max()
    }

    ///
    /// IDの最小値を返す
    ///
    #[allow(dead_code)]
    pub(crate) fn min() -> Self {
        Self::from_string("00000000000000000000000000")
            .expect("invalid ULID string")
    }

    ///
    /// IDの最大値を返す
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
        Self: 'b
    {
        value.to_bytes()
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a
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
            Ok(Id(Ulid::from_bytes(
               <[u8; 16]>::deserialize(deserializer)?
            )))
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
/// ロック解除トークン型の定義(可読性を向上させるための別名定義)
///
pub(crate) type LockToken = Id;

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
                if let Some(path) = info.path_state.current().map(|value| value.to_string()) {
                    info.path_state = PagePathState::LastDeleted(path);
                }
            } else if let Some(path) = info.path_state.last_deleted().map(|value| value.to_string()) {
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
        Self: 'a
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b
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

    /// 作成した日時
    timestamp: DateTime<Local>,

    /// このリビジョンを作成したユーザ識別子
    user: UserId,

    /// path が割り当て／変更されたリビジョンのみ Some
    rename: Option<RenameInfo>,

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
            timestamp: Local::now(),
            user,
            rename: Some(rename),
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
        rename: Option<RenameInfo>,
    ) -> Self {
        Self {
            revision,
            timestamp: Local::now(),
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
    /// ページソースの更新
    ///
    /// # 引数
    /// * `source` - 更新後のページソース
    ///
    pub(crate) fn update_source(&mut self, source: String) {
        self.source = source;
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
    pub(crate) fn rename(&self) -> Option<RenameInfo> {
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
        Self: 'a
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}

///
/// リネーム操作情報構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RenameInfo {
    /// 旧パス（作成時は None 相当として扱う）
    from: Option<String>,

    /// 新パス
    to: String,

    /// リネーム直前時点でのページ中リンク解決状態（1段分）
    /// key: 正規化済み path
    /// value: 解決された page_id（未作成等で解決できなかった場合 None）
    link_refs: BTreeMap<String, Option<Id>>,
}

impl RenameInfo {
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
        Self { from, to, link_refs }
    }

    ///
    /// 旧パスへのアクセサ
    ///
    /// # 戻り値
    /// 旧パスを返す。
    ///
    pub(crate) fn from(&self) -> Option<String> {
        self.from.clone()
    }

    ///
    /// 新パスへのアクセサ
    ///
    /// # 戻り値
    /// 新パスを返す。
    ///
    pub(crate) fn to(&self) -> String {
        self.to.clone()
    }

    ///
    /// リンク解決情報へのアクセサ
    ///
    /// # 戻り値
    /// リンク解決情報を返す。
    ///
    pub(crate) fn link_refs(&self) -> BTreeMap<String, Option<Id>> {
        self.link_refs.clone()
    }
}

///
/// アセット情報構造体
///
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AssetInfo {
    /// アセットID
    id: AssetId, 

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
        Self: 'a
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b
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

    /// 最終更新日時
    timestamp: DateTime<Local>,
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
    pub(crate) fn new<S>(name: S, password: S, display_name: Option<S>,) -> Self
    where 
        S: AsRef<str>,
    {
        /*
         * ソルトデータの生成
         */
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        /*
         * パスワードのハッシュ化
         */
        let salt_string = SaltString::encode_b64(&salt)
            .expect("salt encode failed");
        let argon2 = Argon2::default();
        let hashed = argon2
            .hash_password(password.as_ref().as_bytes(), &salt_string)
            .expect("hash failed")
            .to_string();

        Self {
            id: UserId::new(),
            username: name.as_ref().to_string(),
            password: hashed,
            salt,
            display_name: display_name
                .unwrap_or(name)
                .as_ref()
                .to_string(),
            timestamp: Local::now(),
        }
    }

    ///
    /// ユーザIDへのアクセサ
    ///
    /// # 戻り値
    /// ユーザIDオブジェクトを返す
    ///
    pub (crate) fn id(&self) -> UserId {
        self.id.clone()
    }

    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub (crate) fn username(&self) -> String {
        self.username.clone()
    }

    ///
    /// 表示名へのアクセサ
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub (crate) fn display_name(&self) -> String {
        self.display_name.clone()
    }

    ///
    /// 更新日時へのアクセサ
    ///
    /// # 戻り値
    /// 更新日時表を返す
    ///
    pub (crate) fn timestamp(&self) -> DateTime<Local> {
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
        let salt_string = SaltString::encode_b64(&salt)
            .expect("salt encode failed");
        let argon2 = Argon2::default();
        let hashed = argon2
            .hash_password(password.as_bytes(), &salt_string)
            .expect("hash failed")
            .to_string();

        self.password = hashed;
        self.salt = salt;
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
    ///
    /// # 戻り値
    /// 生成したユーザ情報を返す。
    ///
    pub(crate) fn new_for_test(
        id: UserId,
        timestamp: DateTime<Local>,
        username: &str,
        display_name: &str,
    ) -> Self {
        Self {
            id,
            username: username.to_string(),
            password: "dummy".to_string(),
            salt: [0u8; 16],
            display_name: display_name.to_string(),
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
        Self: 'a
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b
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
    pub(crate) fn new(page: &PageId, user: &UserId,) -> Self {
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
        Self: 'a
    {
        rmp_serde::from_slice::<Self>(data)
            .expect("invalid MessagePack packed bytes")
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b
    {
        rmp_serde::to_vec_named(value)
            .expect("failed to serialize to MessagePack bytes")
    }
}
