/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース関連処理をまとめたモジュール
//!

pub(crate) mod types;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Local};
use redb::{
    Database, MultimapTable, MultimapTableDefinition, ReadableDatabase,
    ReadableMultimapTable, ReadableTable, ReadableTableMetadata, Table,
    TableDefinition,
};

use crate::database::types::{
    AssetInfo, AssetId, LockInfo, LockToken, PageId, PageIndex, PageSource,
    RenameInfo, UserId, UserInfo
};

/// ページパスインデックステーブル (ページパス => ページID)
static PAGE_PATH_TABLE: TableDefinition<String, PageId> =
    TableDefinition::new("page_path_table");

/// 削除済みページパスインデックステーブル (ページパス => ページID)
static DELETED_PAGE_PATH_TABLE: MultimapTableDefinition<String, PageId> =
    MultimapTableDefinition::new("deleted_page_path_table");

/// ページインデックステーブル (ページID => ページインデックス情報)
static PAGE_INDEX_TABLE: TableDefinition<PageId, PageIndex> =
    TableDefinition::new("page_index_table");

/// ページソーステーブル (ページID,リビジョン番号 => ページソース情報)
static PAGE_SOURCE_TABLE: TableDefinition<(PageId, u64), PageSource> =
    TableDefinition::new("page_source_table");

/// ロック情報テーブル (ロック解除トークン => ロック情報)
static LOCK_INFO_TABLE: TableDefinition<LockToken, LockInfo> =
    TableDefinition::new("lock_info_table");

/// アセット情報テーブル (アセットID => アセット情報)
static ASSET_INFO_TABLE: TableDefinition<AssetId, AssetInfo> =
    TableDefinition::new("asset_info_table");

/// アセットID特定テーブル (ページID,ファイル名 => アセットID)
static ASSET_LOOKUP_TABLE: TableDefinition<(PageId, String), AssetId> =
    TableDefinition::new("asset_lookup_table");

/// ページ所属アセット群取得テーブル (ページID => [アセットID])
static ASSET_GROUP_TABLE: MultimapTableDefinition<PageId, AssetId> =
    MultimapTableDefinition::new("asset_group_table");

/// ユーザIDテーブル (ユーザ名 => ユーザID)
static USER_ID_TABLE: TableDefinition<String, UserId> =
    TableDefinition::new("user_id_table");

/// ユーザ情報テーブル (ユーザID => ユーザ情報)
static USER_INFO_TABLE: TableDefinition<UserId, UserInfo> =
    TableDefinition::new("user_info_table");

/// ルートページのパス
const ROOT_PAGE_PATH: &str = "/";

/// ルートページ雛形ソース
static DEFAULT_ROOT_SOURCE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/default_root.md"));

///
/// データベース操作で使用するエラー種別
///
#[derive(Debug)]
pub(crate) enum DbError {
    /// ページ作成時にパスが競合した
    PageAlreadyExists,

    /// ページが存在しない
    PageNotFound,

    /// 不正なパスが指定された
    InvalidPath,

    /// ユーザが存在しない
    UserNotFound,

    /// ルートページが保護されている
    #[allow(dead_code)]
    RootPageProtected,

    /// ページがロックされている
    PageLocked,

    /// ロック情報が存在しない
    LockNotFound,

    /// ロック情報に対する権限がない
    LockForbidden,

    /// amend指定が許可されない
    AmendForbidden,

    /// アセットが存在しない
    AssetNotFound,

    /// アセットが削除済み
    AssetDeleted,

    /// アセットがすでに存在する
    AssetAlreadyExists,

    /// アセットの移動先ページが削除済み
    #[allow(dead_code)]
    AssetMovePageDeleted,

    /// 移動先パスが不正
    InvalidMoveDestination,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::PageAlreadyExists => write!(f, "page already exists"),
            DbError::PageNotFound => write!(f, "page not found"),
            DbError::InvalidPath => write!(f, "page path is invalid"),
            DbError::UserNotFound => write!(f, "user not found"),
            DbError::RootPageProtected => write!(f, "root page is protected"),
            DbError::PageLocked => write!(f, "page is locked"),
            DbError::LockNotFound => write!(f, "lock not found"),
            DbError::LockForbidden => write!(f, "lock forbidden"),
            DbError::AmendForbidden => write!(f, "amend forbidden"),
            DbError::AssetNotFound => write!(f, "asset not found"),
            DbError::AssetDeleted => write!(f, "asset deleted"),
            DbError::AssetAlreadyExists => write!(f, "asset already exists"),
            DbError::AssetMovePageDeleted => {
                write!(f, "asset move page deleted")
            }
            DbError::InvalidMoveDestination => {
                write!(f, "invalid move destination")
            }
        }
    }
}

impl std::error::Error for DbError {}

///
/// ルートページかどうかの判定
///
/// # 引数
/// * `path` - 判定対象のパス
///
/// # 戻り値
/// ルートページの場合は`true`を返す。
///
#[allow(dead_code)]
fn is_root_path(path: &str) -> bool {
    path == ROOT_PAGE_PATH
}

///
/// ページIDからロック情報を探索する
///
/// # 引数
/// * `lock_table` - ロック情報テーブル
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// ロック情報を取得できた場合はOk(Some((LockToken, LockInfo)))を返す。
/// ロック情報が存在しない場合はOk(None)を返す。
///
fn find_lock_by_page<T>(
    lock_table: &T,
    page_id: &PageId,
) -> Result<Option<(LockToken, LockInfo)>>
where
    T: ReadableTable<LockToken, LockInfo>,
{
    for entry in lock_table.iter()? {
        let (token, info) = entry?;
        let info = info.value();
        if &info.page() == page_id {
            return Ok(Some((token.value().clone(), info)));
        }
    }

    Ok(None)
}

///
/// ドラフトページの削除(トランザクション内部処理)
///
/// # 概要
/// ドラフトページと紐付くテーブル情報を削除し、アセットIDを返す。
///
/// # 引数
/// * `txn` - 書き込みトランザクション
/// * `page_id` - 対象ページID
///
/// # 戻り値
/// 削除したアセットID一覧を返す。
///
fn delete_draft_in_txn(
    txn: &redb::WriteTransaction,
    page_id: &PageId,
) -> Result<Vec<AssetId>> {
    /*
     * テーブルの準備
     */
    let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
    let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
    let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
    let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
    let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
    let mut asset_ids = Vec::new();

    /*
     * ページ情報の取得と検証
     */
    let index = match index_table.get(page_id.clone())? {
        Some(entry) => entry.value(),
        None => return Err(anyhow!(DbError::PageNotFound)),
    };

    if !index.is_draft() {
        return Err(anyhow!(DbError::PageNotFound));
    }

    if is_root_path(&index.path()) {
        return Err(anyhow!(DbError::RootPageProtected));
    }

    /*
     * 付随アセットの削除
     */
    for entry in group_table.remove_all(page_id.clone())? {
        let asset_id = entry?.value();
        let file_name = match asset_table.get(asset_id.clone())? {
            Some(info_guard) => info_guard.value().file_name(),
            None => {
                asset_ids.push(asset_id);
                continue;
            }
        };
        let _ = lookup_table.remove((page_id.clone(), file_name));
        let _ = asset_table.remove(asset_id.clone());
        asset_ids.push(asset_id);
    }

    /*
     * ページインデックスの削除
     */
    let _ = path_table.remove(&index.path());
    let _ = index_table.remove(page_id.clone());

    Ok(asset_ids)
}


///
/// page list 用のページ情報
///
pub(crate) struct PageListEntry {
    id: PageId,
    path: String,
    latest_revision: u64,
    timestamp: DateTime<Local>,
    user_name: String,
    deleted: bool,
    draft: bool,
    locked: bool,
}

///
/// lock list 用のロック情報
///
pub(crate) struct LockListEntry {
    token: LockToken,
    #[allow(dead_code)]
    page_id: PageId,
    page_path: String,
    expire: DateTime<Local>,
    user_name: String,
}

struct RecursivePageTarget {
    page_id: PageId,
    path: String,
}

///
/// asset list 用のアセット情報
///
pub(crate) struct AssetListEntry {
    id: AssetId,
    file_name: String,
    mime: String,
    size: u64,
    timestamp: DateTime<Local>,
    user_name: String,
    page_path: Option<String>,
    deleted: bool,
}

///
/// アセット移動結果
///
pub(crate) enum AssetMoveResult {
    /// 移動成功
    Moved,

    /// 移動先ページが存在しない
    PageNotFound,

    /// 移動先ページが削除済み
    PageDeleted,

    /// 移動先に同名アセットが存在する
    NameConflict,
}

impl AssetListEntry {
    ///
    /// アセットIDへのアクセサ
    ///
    /// # 戻り値
    /// アセットIDを返す。
    ///
    pub(crate) fn id(&self) -> AssetId {
        self.id.clone()
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
    /// MIME種別へのアクセサ
    ///
    /// # 戻り値
    /// MIME種別を返す。
    ///
    pub(crate) fn mime(&self) -> String {
        self.mime.clone()
    }

    ///
    /// サイズへのアクセサ
    ///
    /// # 戻り値
    /// サイズ(バイト)を返す。
    ///
    pub(crate) fn size(&self) -> u64 {
        self.size
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
    /// 登録ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 登録ユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 所有ページパスへのアクセサ
    ///
    /// # 戻り値
    /// 所有ページのパスを返す。ゾンビの場合はNone。
    ///
    pub(crate) fn page_path(&self) -> Option<String> {
        self.page_path.clone()
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
    /// ゾンビ状態の判定
    ///
    /// # 戻り値
    /// ゾンビ状態の場合は`true`を返す。
    ///
    pub(crate) fn is_zombie(&self) -> bool {
        self.page_path.is_none()
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        id: AssetId,
        file_name: String,
        mime: String,
        size: u64,
        timestamp: DateTime<Local>,
        user_name: String,
        page_path: Option<String>,
        deleted: bool,
    ) -> Self {
        Self {
            id,
            file_name,
            mime,
            size,
            timestamp,
            user_name,
            page_path,
            deleted,
        }
    }
}

impl LockListEntry {
    ///
    /// ロック解除トークンへのアクセサ
    ///
    /// # 戻り値
    /// ロック解除トークンを返す。
    ///
    pub(crate) fn token(&self) -> LockToken {
        self.token.clone()
    }

    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn page_id(&self) -> PageId {
        self.page_id.clone()
    }

    ///
    /// ページパスへのアクセサ
    ///
    /// # 戻り値
    /// ページパスを返す。
    ///
    pub(crate) fn page_path(&self) -> String {
        self.page_path.clone()
    }

    ///
    /// 有効期限へのアクセサ
    ///
    /// # 戻り値
    /// 有効期限を返す。
    ///
    pub(crate) fn expire(&self) -> DateTime<Local> {
        self.expire
    }

    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }
}

impl PageListEntry {
    ///
    /// ページIDへのアクセサ
    ///
    /// # 戻り値
    /// ページIDを返す。
    ///
    pub(crate) fn id(&self) -> PageId {
        self.id.clone()
    }

    ///
    /// ページパスへのアクセサ
    ///
    /// # 戻り値
    /// ページパスを返す。
    ///
    pub(crate) fn path(&self) -> String {
        self.path.clone()
    }

    ///
    /// 最新リビジョン番号へのアクセサ
    ///
    /// # 戻り値
    /// 最新リビジョン番号を返す。
    ///
    pub(crate) fn latest_revision(&self) -> u64 {
        self.latest_revision
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
    /// 記述したユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 記述したユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
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
    /// ドラフト状態の判定
    ///
    /// # 戻り値
    /// ドラフト状態の場合は`true`を返す。
    ///
    pub(crate) fn is_draft(&self) -> bool {
        self.draft
    }

    ///
    /// ロック状態の判定
    ///
    /// # 戻り値
    /// ロック中の場合は`true`を返す。
    ///
    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        id: PageId,
        path: String,
        latest_revision: u64,
        timestamp: DateTime<Local>,
        user_name: String,
        deleted: bool,
        draft: bool,
        locked: bool,
    ) -> Self {
        Self {
            id,
            path,
            latest_revision,
            timestamp,
            user_name,
            deleted,
            draft,
            locked,
        }
    }
}

///
/// データベースのイニシャライズ
///
/// # 引数
/// * `db` - 初期化対象のデータベース
///
/// # 戻り値
/// 初期化に成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`でラ
/// ップして返す。
///
/// # 注記
/// 本関数では初期化処理として以下のテーブルの作成を試みる。
///
///  - PAGE_PATH_TABLE: ページパスインデックステーブル
///  - DELETED_PAGE_PATH_TABLE: 削除済みページパスインデックステーブル
///  - PAGE_INDEX_TABLE: ページインデックステーブル
///  - PAGE_SOURCE_TABLE: ページソーステーブル
///  - ASSET_GROUP_TABLE: アセット情報テーブル
///  - ASSET_LOOKUP_TABLE: アセットID特定テーブル
///  - ASSET_GROUP_TABLE: ページ所属アセット群取得テーブル
///  - USER_ID_TABLE: ユーザIDテーブル
///  - USER_INFO_TABLE: ユーザ情報テーブル
///
fn init_database(db: &mut Database) -> Result<()> {
    /*
     * 書き込みトランザクション開始
     */
    let txn = db.begin_write()?;

    /*
     * 各種テーブル作成
     */
    {
        /*
         * ページ関連テーブル作成
         */
        // ページパスインデックステーブル
        let _ = txn.open_table(PAGE_PATH_TABLE)
            .context("create PAGE_PATH_TABLE")?;

        // 削除済みページパスインデックステーブル
        let _ = txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)
            .context("create DELETED_PAGE_PATH_TABLE")?;

        // ページインデックステーブル
        let _ = txn.open_table(PAGE_INDEX_TABLE)
            .context("create PAGE_INDEX_TABLE")?;

        // ページソーステーブル
        let _ = txn.open_table(PAGE_SOURCE_TABLE)
            .context("create PAGE_SOURCE_TABLE")?;

        /*
         * ロック・アセット関連テーブル作成
         */
        // ロック情報テーブル
        let _ = txn.open_table(LOCK_INFO_TABLE)
            .context("create LOCK_INFO_TABLE")?;

        // アセット情報テーブル
        let _ = txn.open_table(ASSET_INFO_TABLE)
            .context("create ASSET_INFO_TABLE")?;

        // アセットID特定テーブル
        let _ = txn.open_table(ASSET_LOOKUP_TABLE)
            .context("create ASSET_LOOKUP_TABLE")?;

        // ページ所属アセット群取得テーブル
        let _ = txn.open_multimap_table(ASSET_GROUP_TABLE)
            .context("create ASSET_GROUP_TABLE")?;

        /*
         * ユーザ関連テーブル作成
         */
        // ユーザIDテーブル
        let _ = txn.open_table(USER_ID_TABLE)
            .context("create USER_ID_TABLE")?;

        // ユーザ情報テーブル
        let _ = txn.open_table(USER_INFO_TABLE)
            .context("create USER_INFO_TABLE")?;
    }

    /*
     * コミット
     */
    txn.commit()?;

    Ok(())
}

///
/// データベース操作手順を集約する構造体
///
pub(crate) struct DatabaseManager {
    /// データベースオブジェクト
    db: Database,

    /// アセットデータ格納ディレクトリへのパス
    #[allow(dead_code)]
    asset_path: PathBuf,
}

impl DatabaseManager {
    ///
    /// データベースマネージャのオープン
    ///
    /// # 引数
    /// * `path` - データベースファイルへのパス
    ///
    /// # 戻り値
    /// データベースのオープンに成功した場合はエントリーマネージャオブジェクトを
    /// `Ok()`でラップして返す。失敗した場合はエラー情報を `Err()`でラップして返
    /// す。
    ///
    pub(crate) fn open<P>(db_path: P, asset_path: P) -> Result<Self> 
    where
        P: AsRef<Path>
    {
        let db = match Database::create(db_path) {
            Ok(mut db) => {
                init_database(&mut db)?;
                db
            },

            Err(err) => return Err(err.into()),
        };

        Ok(Self {db, asset_path: asset_path.as_ref().into()})
    }

    ///
    /// ユーザ情報の追加
    ///
    /// # 引数
    /// * `username` - 登録するユーザ名
    /// * `password` - 登録するパスワード
    /// * `display_name` - 表示名
    ///
    /// # 戻り値
    /// 登録に成功した場合は`Ok(())`を返す。
    ///
    ///
    /// ルートページの初期化
    ///
    /// # 引数
    /// * `user_name` - 作成者のユーザ名
    ///
    /// # 戻り値
    /// 初期化に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn ensure_default_root(&self, user_name: &str) -> Result<()> {
        if self.page_exists(ROOT_PAGE_PATH)? {
            return Ok(());
        }

        match self.create_page(
            ROOT_PAGE_PATH,
            user_name,
            DEFAULT_ROOT_SOURCE.to_string(),
        ) {
            Ok(_) => Ok(()),
            Err(err) => {
                if let Some(DbError::PageAlreadyExists) =
                    err.downcast_ref::<DbError>()
                {
                    return Ok(());
                }

                Err(err)
            }
        }
    }

    ///
    /// ページが存在するかの確認
    ///
    /// # 引数
    /// * `path` - ページのパス
    ///
    /// # 戻り値
    /// ページが存在する場合は`true`を返す。
    ///
    fn page_exists(&self, path: &str) -> Result<bool> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_PATH_TABLE)?;
        Ok(table.get(&path.to_string())?.is_some())
    }

    ///
    /// ページIDからページインデックスとパスを取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some((path, PageIndex)))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn get_page_index_entry_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<(String, PageIndex)>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_INDEX_TABLE)?;
        match table.get(page_id.clone())? {
            Some(entry) => {
                let index = entry.value();
                Ok(Some((index.path(), index)))
            }
            None => Ok(None),
        }
    }


    ///
    /// ページIDからページインデックスを取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(PageIndex))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_page_index_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<Option<PageIndex>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_INDEX_TABLE)?;
        Ok(table.get(page_id.clone())?.map(|entry| entry.value()))
    }

    ///
    /// ページソースの取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `revision` - リビジョン番号
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(PageSource))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_page_source(
        &self,
        page_id: &PageId,
        revision: u64,
    ) -> Result<Option<PageSource>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_SOURCE_TABLE)?;
        Ok(table.get((page_id.clone(), revision))?.map(|entry| entry.value()))
    }

    ///
    /// ロック情報の取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// ロック情報を取得できた場合は`Ok(Some(LockInfo))`を返す。
    /// ロックが存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_page_lock_info(
        &self,
        page_id: &PageId,
    ) -> Result<Option<LockInfo>> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut commit = false;
        let mut result = None;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut delete_draft = false;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let now = Local::now();

            let mut index = {
                let entry = match index_table.get(page_id.clone())? {
                    Some(entry) => entry,
                    None => return Ok(result),
                };
                entry.value()
            };

            /*
             * ロック情報の検証
             */
            if index.is_draft() {
                match find_lock_by_page(&lock_table, page_id)? {
                    Some((token, lock_info)) => {
                        if lock_info.expire() <= now {
                            lock_table.remove(token)?;
                            delete_draft = true;
                            commit = true;
                        } else {
                            result = Some(lock_info);
                        }
                    }
                    None => {
                        delete_draft = true;
                        commit = true;
                    }
                }
            } else if let Some(token) = index.lock_token() {
                let lock_info = match lock_table.get(token.clone())? {
                    Some(info) => Some(info.value()),
                    None => {
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index.clone())?;
                        commit = true;
                        None
                    }
                };

                if let Some(lock_info) = lock_info {
                    if lock_info.expire() <= now {
                        lock_table.remove(token)?;
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index.clone())?;
                        commit = true;
                    } else {
                        result = Some(lock_info);
                    }
                }
            }
        }

        if delete_draft {
            asset_ids = delete_draft_in_txn(&txn, page_id)?;
            commit = true;
        }

        /*
         * コミット
         */
        if commit {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(result)
    }

    ///
    /// ロックの取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// 取得したロック情報を返す。
    ///
    pub(crate) fn acquire_page_lock(
        &self,
        page_id: &PageId,
        user_name: &str,
    ) -> Result<LockInfo> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let lock_info = {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let now = Local::now();

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::LockNotFound)),
            };

            /*
             * 既存ロックの確認
             */
            if index.is_draft() {
                if let Some((token, existing)) =
                    find_lock_by_page(&lock_table, page_id)?
                {
                    if existing.expire() > now {
                        return Err(anyhow!(DbError::PageLocked));
                    }
                    lock_table.remove(token)?;
                }
            } else if let Some(token) = index.lock_token() {
                let existing = lock_table
                    .get(token.clone())?
                    .map(|entry| entry.value());
                if let Some(lock_info) = existing {
                    if lock_info.expire() > now {
                        return Err(anyhow!(DbError::PageLocked));
                    }
                    lock_table.remove(token)?;
                }
                index.set_lock_token(None);
            }

            /*
             * ロック情報の生成と登録
             */
            let lock_info = LockInfo::new(page_id, &user_id);
            let token = lock_info.token();
            if !index.is_draft() {
                index.set_lock_token(Some(token.clone()));
                index_table.insert(page_id.clone(), index)?;
            }
            lock_table.insert(token, lock_info.clone())?;

            lock_info
        };

        /*
         * コミット
         */
        txn.commit()?;

        Ok(lock_info)
    }

    ///
    /// ロックの延長
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `user_name` - ユーザ名
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 更新したロック情報を返す。
    ///
    pub(crate) fn renew_page_lock(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: &LockToken,
    ) -> Result<LockInfo> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let lock_info = {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let now = Local::now();

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::LockNotFound)),
            };

            /*
             * トークン整合性の確認
             */
            if index.is_draft() {
                /*
                 * ドラフトページのロック更新
                 */
                let mut lock_info = match lock_table.get(token.clone())? {
                    Some(lock_info) => lock_info.value(),
                    None => return Err(anyhow!(DbError::LockNotFound)),
                };

                if lock_info.page() != *page_id {
                    return Err(anyhow!(DbError::LockForbidden));
                }

                if lock_info.expire() <= now {
                    lock_table.remove(token.clone())?;
                    return Err(anyhow!(DbError::LockNotFound));
                }

                if lock_info.user() != user_id {
                    return Err(anyhow!(DbError::LockForbidden));
                }

                lock_info.renew();
                let new_token = lock_info.token();
                lock_table.remove(token.clone())?;
                lock_table.insert(new_token.clone(), lock_info.clone())?;
                lock_info
            } else {
                let current = match index.lock_token() {
                    Some(current) => current,
                    None => token.clone(),
                };

                if current != *token {
                    return Err(anyhow!(DbError::LockForbidden));
                }

                /*
                 * ロック情報の取得と検証
                 */
                let mut lock_info = match lock_table.get(token.clone())? {
                    Some(lock_info) => lock_info.value(),
                    None => {
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index)?;
                        return Err(anyhow!(DbError::LockNotFound));
                    }
                };

                if lock_info.expire() <= now {
                    lock_table.remove(token.clone())?;
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index)?;
                    return Err(anyhow!(DbError::LockNotFound));
                }

                if lock_info.user() != user_id {
                    return Err(anyhow!(DbError::LockForbidden));
                }

                /*
                 * ロック情報の更新
                 */
                lock_info.renew();
                let new_token = lock_info.token();
                lock_table.remove(token.clone())?;
                lock_table.insert(new_token.clone(), lock_info.clone())?;
                index.set_lock_token(Some(new_token));
                index_table.insert(page_id.clone(), index)?;

                lock_info
            }
        };

        /*
         * コミット
         */
        txn.commit()?;

        Ok(lock_info)
    }

    ///
    /// ロックの解除
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `user_name` - ユーザ名
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 解除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn release_page_lock(
        &self,
        page_id: &PageId,
        user_name: &str,
        token: &LockToken,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut delete_draft = false;
        let mut needs_commit = false;
        let mut result: Result<()> = Ok(());

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let now = Local::now();

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::LockNotFound)),
            };

            /*
             * ロック情報の検証
             */
            let mut lock_info: Option<LockInfo> = None;
            match lock_table.get(token.clone())? {
                Some(info) => lock_info = Some(info.value()),
                None => {
                    if !index.is_draft() {
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index.clone())?;
                        needs_commit = true;
                    }
                    result = Err(anyhow!(DbError::LockNotFound));
                }
            }

            if result.is_ok() {
                let lock_info = lock_info.expect("lock info");
                if lock_info.page() != *page_id {
                    result = Err(anyhow!(DbError::LockForbidden));
                } else if lock_info.expire() <= now {
                    lock_table.remove(token.clone())?;
                    needs_commit = true;
                    if index.is_draft() {
                        delete_draft = true;
                    } else {
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index)?;
                    }
                    result = Err(anyhow!(DbError::LockNotFound));
                } else if lock_info.user() != user_id {
                    result = Err(anyhow!(DbError::LockForbidden));
                } else {
                    /*
                     * ロック情報の削除
                     */
                    lock_table.remove(token.clone())?;
                    needs_commit = true;
                    if index.is_draft() {
                        delete_draft = true;
                    } else {
                        index.set_lock_token(None);
                        index_table.insert(page_id.clone(), index)?;
                    }
                }
            }
        }

        if delete_draft {
            asset_ids = delete_draft_in_txn(&txn, page_id)?;
            needs_commit = true;
        }

        if needs_commit {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        result
    }

    ///
    /// ロック期限切れの掃除
    ///
    /// # 戻り値
    /// 削除したロック情報の件数を返す。
    ///
    pub(crate) fn cleanup_expired_locks(&self) -> Result<usize> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut draft_pages: Vec<PageId> = Vec::new();
        let (removed, mut needs_commit) = {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let now = Local::now();

            /*
             * 期限切れロックの収集
             */
            let mut expired = Vec::new();
            for entry in lock_table.iter()? {
                let (token, info) = entry?;
                let info = info.value();
                if info.expire() <= now {
                    expired.push((token.value().clone(), info.page()));
                }
            }

            /*
             * 期限切れが無ければ終了
             */
            if expired.is_empty() {
                Ok::<(usize, bool), anyhow::Error>((0, false))
            } else {
                /*
                 * 期限切れロックの削除
                 */
                let mut removed = 0usize;
                    for (token, page_id) in expired {
                        let _ = lock_table.remove(token.clone())?;

                        let index = match index_table.get(page_id.clone())? {
                            Some(entry) => Some(entry.value()),
                            None => None,
                        };

                        if let Some(mut index) = index {
                            if index.is_draft() {
                                draft_pages.push(page_id.clone());
                            } else if index.lock_token() == Some(token.clone()) {
                                index.set_lock_token(None);
                                index_table.insert(page_id.clone(), index)?;
                            }
                        }

                        removed += 1;
                    }

                Ok::<(usize, bool), anyhow::Error>((removed, true))
            }
        }?;

        /*
         * コミット
         */
        for page_id in draft_pages {
            let mut deleted = delete_draft_in_txn(&txn, &page_id)?;
            asset_ids.append(&mut deleted);
            needs_commit = true;
        }

        if needs_commit {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(removed)
    }

    ///
    /// ユーザ情報の追加
    ///
    /// # 引数
    /// * `username` - 登録するユーザ名
    /// * `password` - 登録するパスワード
    /// * `display_name` - 表示名
    ///
    /// # 戻り値
    /// 登録に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn add_user<S>(
        &self,
        username: S,
        password: S,
        display_name: Option<S>,
    ) -> Result<()>
    where 
        S: AsRef<str> + Copy,
    {
        /*
         * 事前情報の整形
         */
        let key = username.as_ref().to_string();

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * 登録処理
         */
        {
            let mut id_table = txn.open_table(USER_ID_TABLE)?;

            /*
             * 既存ユーザの確認
             */
            if id_table.get(&key)?.is_some() {
                return Err(anyhow::anyhow!(
                    "user already exists: {}",
                    username.as_ref()
                ));
            }

            /*
             * ユーザ情報の生成
             */
            let user_info = UserInfo::new(username, password, display_name);
            let user_id = user_info.id();

            /*
             * ユーザ情報の登録
             */
            let mut info_table = txn.open_table(USER_INFO_TABLE)?;
            info_table.insert(user_id.clone(), user_info)?;
            id_table.insert(&key, user_id)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ユーザ認証の検証
    ///
    /// # 引数
    /// * `username` - ユーザ名
    /// * `password` - パスワード
    ///
    /// # 戻り値
    /// 認証に成功した場合は`Ok(true)`を返す。
    ///
    pub(crate) fn verify_user(&self, username: &str, password: &str)
        -> Result<bool>
    {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;

        /*
         * ユーザID取得
         */
        let id_table = txn.open_table(USER_ID_TABLE)?;
        let key = username.to_string();
        let user_id = match id_table.get(&key)? {
            Some(id) => id.value(),
            None => return Ok(false),
        };

        /*
         * ユーザ情報取得
         */
        let info_table = txn.open_table(USER_INFO_TABLE)?;
        let user_info = match info_table.get(user_id)? {
            Some(info) => info.value(),
            None => return Ok(false),
        };

        /*
         * パスワード検証
         */
        Ok(user_info.verify_password(password))
    }

    ///
    /// ユーザIDからユーザ名を取得
    ///
    /// # 引数
    /// * `user_id` - ユーザID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(ユーザ名))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_user_name_by_id(
        &self,
        user_id: &UserId,
    ) -> Result<Option<String>> {
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(USER_INFO_TABLE)?;
        let info = match info_table.get(user_id.clone())? {
            Some(info) => info.value(),
            None => return Ok(None),
        };

        Ok(Some(info.username()))
    }

    ///
    /// ユーザ名からユーザIDを取得
    ///
    /// # 引数
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(UserId))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_user_id_by_name(
        &self,
        user_name: &str,
    ) -> Result<Option<UserId>> {
        let txn = self.db.begin_read()?;
        let id_table = txn.open_table(USER_ID_TABLE)?;
        let key = user_name.to_string();
        Ok(id_table.get(&key)?.map(|entry| entry.value()))
    }

    ///
    /// ユーザ情報の一覧取得
    ///
    /// # 戻り値
    /// ユーザ情報の一覧を返す。
    ///
    pub(crate) fn list_users(&self) -> Result<Vec<UserInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(USER_INFO_TABLE)?;
        let mut users = Vec::new();

        /*
         * ユーザ情報の収集
         */
        for entry in info_table.iter()? {
            let (_, info) = entry?;
            users.push(info.value());
        }

        Ok(users)
    }

    ///
    /// ユーザ登録の有無の確認
    ///
    /// # 戻り値
    /// ユーザが一人でも登録されている場合は`Ok(true)`を登録されていない場合は
    /// `Ok(false)`を返す。データベースアクセス時にエラーが発生した場合はエラー
    /// 情報を`Err()`でラップして返す。
    ///
    pub(crate) fn is_users_registered(&self) -> Result<bool> {
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(USER_INFO_TABLE)?;

        Ok(!info_table.is_empty()?)
    }

    ///
    /// ページ情報の一覧取得
    ///
    /// # 戻り値
    /// ページ情報の一覧を返す。
    ///
    pub(crate) fn list_pages(&self) -> Result<Vec<PageListEntry>> {
        /*
         * 期限切れロックの掃除
         */
        self.cleanup_expired_locks()?;

        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let lock_table = txn.open_table(LOCK_INFO_TABLE)?;
        let mut pages = Vec::new();
        let now = Local::now();

        /*
         * ロック状態の収集
         */
        let mut locked_pages = HashMap::new();
        for entry in lock_table.iter()? {
            let (_, info) = entry?;
            let info = info.value();
            if info.expire() <= now {
                continue;
            }
            locked_pages.insert(info.page(), true);
        }

        /*
         * ページ情報の収集
         */
        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            let index = index.value();
            let page_id = page_id.value().clone();
            let locked = locked_pages.contains_key(&page_id);

            if index.is_draft() {
                pages.push(PageListEntry {
                    id: page_id,
                    path: index.path(),
                    latest_revision: 0,
                    timestamp: Local::now(),
                    user_name: String::new(),
                    deleted: index.deleted(),
                    draft: true,
                    locked,
                });
                continue;
            }

            let revision = index.latest();
            let source = source_table
                .get((page_id.clone(), revision))?
                .ok_or_else(|| anyhow!("page source not found"))?
                .value();
            let user_id = source.user();

            let user_info = user_table
                .get(user_id.clone())?
                .ok_or_else(|| anyhow!("user not found"))?
                .value();

            pages.push(PageListEntry {
                id: page_id,
                path: index.path(),
                latest_revision: revision,
                timestamp: source.timestamp(),
                user_name: user_info.username(),
                deleted: index.deleted(),
                draft: false,
                locked,
            });
        }

        Ok(pages)
    }

    ///
    /// ロック情報の一覧取得
    ///
    /// # 戻り値
    /// ロック情報の一覧を返す。
    ///
    pub(crate) fn list_locks(&self) -> Result<Vec<LockListEntry>> {
        /*
         * 期限切れロックの掃除
         */
        self.cleanup_expired_locks()?;

        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let lock_table = txn.open_table(LOCK_INFO_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let now = Local::now();

        /*
         * ページパスのマップ構築
         */
        let mut page_map = HashMap::new();
        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            let index = index.value();
            page_map.insert(page_id.value().clone(), index.path());
        }

        /*
         * ユーザ名のマップ構築
         */
        let mut user_map = HashMap::new();
        for entry in user_table.iter()? {
            let (id, info) = entry?;
            let info = info.value();
            user_map.insert(id.value().clone(), info.username());
        }

        /*
         * ロック情報の収集
         */
        let mut locks = Vec::new();
        for entry in lock_table.iter()? {
            let (token, info) = entry?;
            let info = info.value();
            if info.expire() <= now {
                continue;
            }

            let page_path = match page_map.get(&info.page()) {
                Some(path) => path.clone(),
                None => return Err(anyhow!("page not found")),
            };

            let user_name = match user_map.get(&info.user()) {
                Some(name) => name.clone(),
                None => return Err(anyhow!("user not found")),
            };

            locks.push(LockListEntry {
                token: token.value().clone(),
                page_id: info.page(),
                page_path,
                expire: info.expire(),
                user_name,
            });
        }

        Ok(locks)
    }

    ///
    /// アセット情報の一覧取得
    ///
    /// # 戻り値
    /// アセット情報の一覧を返す。
    ///
    pub(crate) fn list_assets(&self) -> Result<Vec<AssetListEntry>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let info_table = txn.open_table(ASSET_INFO_TABLE)?;
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let index_table = txn.open_table(PAGE_INDEX_TABLE)?;
        let mut assets = Vec::new();

        /*
         * ページパスマップの構築
         */
        let mut page_map = HashMap::new();
        for entry in index_table.iter()? {
            let (page_id, index) = entry?;
            page_map.insert(page_id.value().clone(), index.value().path());
        }

        /*
         * アセット情報の収集
         */
        for entry in info_table.iter()? {
            let (asset_id, asset_info) = entry?;
            let asset_id = asset_id.value().clone();
            let asset_info = asset_info.value();
            let user_id = asset_info.user();
            let user_info = user_table
                .get(user_id.clone())?
                .ok_or_else(|| anyhow!("user not found"))?
                .value();
            let page_path = asset_info
                .page_id()
                .and_then(|page_id| page_map.get(&page_id).cloned());

            assets.push(AssetListEntry {
                id: asset_id,
                file_name: asset_info.file_name(),
                mime: asset_info.mime(),
                size: asset_info.size(),
                timestamp: asset_info.timestamp(),
                user_name: user_info.username(),
                page_path,
                deleted: asset_info.deleted(),
            });
        }

        Ok(assets)
    }

    ///
    /// アセット保存パスの生成
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// アセット保存パスを返す。
    ///
    fn asset_file_path(&self, asset_id: &AssetId) -> PathBuf {
        let raw = asset_id.to_string();
        if raw.len() < 5 {
            return self.asset_path.join(raw);
        }

        let (dir1, rest) = raw.split_at(2);
        let (dir2, _) = rest.split_at(3);
        self.asset_path.join(dir1).join(dir2).join(raw)
    }

    ///
    /// パスからページIDを取得
    ///
    /// # 引数
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 解決できたページIDを返す。存在しない場合は`None`を返す。
    ///
    pub(crate) fn get_page_id_by_path(
        &self,
        path: &str,
    ) -> Result<Option<PageId>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PAGE_PATH_TABLE)?;
        let key = path.to_string();
        Ok(table.get(&key)?.map(|entry| entry.value()))
    }

    ///
    /// 削除済みページ候補の取得
    ///
    /// # 引数
    /// * `path` - ページパス
    ///
    /// # 戻り値
    /// 対象となるページIDの一覧を返す。
    ///
    pub(crate) fn get_deleted_page_ids_by_path(
        &self,
        path: &str,
    ) -> Result<Vec<PageId>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
        let key = path.to_string();
        let mut page_ids = Vec::new();

        for entry in table.get(key)? {
            page_ids.push(entry?.value());
        }

        Ok(page_ids)
    }

    ///
    /// ページ内のアセットIDを取得
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `file_name` - ファイル名
    ///
    /// # 戻り値
    /// 解決できたアセットIDを返す。存在しない場合は`None`を返す。
    ///
    pub(crate) fn get_asset_id_by_page_file(
        &self,
        page_id: &PageId,
        file_name: &str,
    ) -> Result<Option<AssetId>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ASSET_LOOKUP_TABLE)?;
        let key = (page_id.clone(), file_name.to_string());
        Ok(table.get(key)?.map(|entry| entry.value()))
    }

    ///
    /// アセット情報の取得
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 取得に成功した場合は`Ok(Some(AssetInfo))`を返す。
    /// 存在しない場合は`Ok(None)`を返す。
    ///
    pub(crate) fn get_asset_info_by_id(
        &self,
        asset_id: &AssetId,
    ) -> Result<Option<AssetInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ASSET_INFO_TABLE)?;
        Ok(table.get(asset_id.clone())?.map(|entry| entry.value()))
    }

    ///
    /// ページ所属アセット情報の一覧取得
    ///
    /// # 概要
    /// ページに紐付くアセット情報を収集する。
    ///
    /// # 引数
    /// * `page_id` - ページID
    ///
    /// # 戻り値
    /// アセット情報の一覧を返す。
    ///
    pub(crate) fn list_page_assets(
        &self,
        page_id: &PageId,
    ) -> Result<Vec<AssetInfo>> {
        /*
         * 読み取りトランザクション開始
         */
        let txn = self.db.begin_read()?;
        let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
        let info_table = txn.open_table(ASSET_INFO_TABLE)?;
        let mut assets = Vec::new();

        /*
         * アセット情報の収集
         */
        for entry in group_table.get(page_id.clone())? {
            let asset_id = entry?.value();
            let asset_info = info_table
                .get(asset_id.clone())?
                .ok_or_else(|| anyhow!("asset info not found"))?
                .value();
            assets.push(asset_info);
        }

        Ok(assets)
    }

    ///
    /// アセットデータの取得
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// アセットデータを返す。
    ///
    pub(crate) fn read_asset_data(
        &self,
        asset_id: &AssetId,
    ) -> Result<Vec<u8>> {
        /*
         * アセットパスの生成
         */
        let path = self.asset_file_path(asset_id);
        Ok(fs::read(path)?)
    }

    ///
    /// アセットの作成
    ///
    /// # 概要
    /// アセット情報の登録とファイル保存を行う。
    ///
    /// # 引数
    /// * `page_id` - ページID
    /// * `file_name` - ファイル名
    /// * `mime` - MIME種別
    /// * `user_name` - 登録ユーザ名
    /// * `data` - アセットデータ
    ///
    /// # 戻り値
    /// 作成したアセットIDを返す。
    ///
    pub(crate) fn create_asset(
        &self,
        page_id: &PageId,
        file_name: &str,
        mime: &str,
        user_name: &str,
        data: &[u8],
    ) -> Result<AssetId> {
        /*
         * 事前情報の整形
         */
        let file_name = file_name.to_string();
        let mime = mime.to_string();
        let size = data.len() as u64;

        /*
         * アセットファイルの保存
         */
        let asset_id = AssetId::new();
        let asset_path = self.asset_file_path(&asset_id);
        if let Some(parent) = asset_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&asset_path, data)?;

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let insert_result = (|| -> Result<()> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let index_table = txn.open_table(PAGE_INDEX_TABLE)?;

            /*
             * ページ存在確認
             */
            if index_table.get(page_id.clone())?.is_none() {
                return Err(anyhow!(DbError::PageNotFound));
            }

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            /*
             * 既存アセットの確認
             */
            let lookup_key = (page_id.clone(), file_name.clone());
            if lookup_table.get(&lookup_key)?.is_some() {
                return Err(anyhow!(DbError::AssetAlreadyExists));
            }

            /*
             * アセット情報の登録
             */
            let asset_info = AssetInfo::new(
                asset_id.clone(),
                page_id.clone(),
                file_name.clone(),
                mime,
                size,
                user_id,
            );
            info_table.insert(asset_id.clone(), asset_info)?;
            lookup_table.insert(lookup_key, asset_id.clone())?;
            let _ = group_table.insert(page_id.clone(), asset_id.clone())?;

            Ok(())
        })();

        /*
         * 登録失敗時の巻き戻し
         */
        if let Err(err) = insert_result {
            let _ = fs::remove_file(&asset_path);
            return Err(err);
        }

        /*
         * コミット
         */
        if let Err(err) = txn.commit() {
            let _ = fs::remove_file(&asset_path);
            return Err(err.into());
        }

        Ok(asset_id)
    }

    ///
    /// アセットの削除
    ///
    /// # 概要
    /// アセット情報の削除フラグを更新する。
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 削除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn delete_asset(&self, asset_id: &AssetId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let delete_result = (|| -> Result<()> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(DbError::AssetNotFound)),
            };

            /*
             * 削除済み判定
             */
            if asset_info.deleted() {
                return Err(anyhow!(DbError::AssetDeleted));
            }

            /*
             * 削除フラグ更新
             */
            asset_info.set_deleted(true);
            info_table.insert(asset_id.clone(), asset_info)?;

            Ok(())
        })();

        if let Err(err) = delete_result {
            return Err(err);
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// アセットのハードデリート
    ///
    /// # 概要
    /// アセット情報と関連テーブルを削除し、ファイルも削除する。
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 削除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn delete_asset_hard(&self, asset_id: &AssetId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let delete_result = (|| -> Result<Option<(PageId, String)>> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * 対象アセットの取得
             */
            let asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(DbError::AssetNotFound)),
            };

            /*
             * 参照の削除
             */
            let page_id = asset_info.page_id();
            if let Some(page_id) = page_id.clone() {
                let lookup_key = (page_id.clone(), asset_info.file_name());
                let _ = lookup_table.remove(lookup_key);
                let _ = group_table.remove(page_id, asset_id.clone());
            } else {
                let mut remove_keys = Vec::new();
                for entry in lookup_table.iter()? {
                    let (key, value) = entry?;
                    if value.value() == asset_id.clone() {
                        remove_keys.push(key.value());
                    }
                }
                for (page_id, file_name) in remove_keys {
                    let _ = lookup_table.remove((page_id.clone(), file_name));
                    let _ = group_table.remove(page_id, asset_id.clone());
                }
            }

            /*
             * 情報の削除
             */
            let _ = info_table.remove(asset_id.clone())?;

            Ok(page_id.map(|id| (id, asset_info.file_name())))
        })();

        if let Err(err) = delete_result {
            return Err(err);
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        let asset_path = self.asset_file_path(asset_id);
        let _ = fs::remove_file(asset_path);

        Ok(())
    }

    ///
    /// アセットの復帰
    ///
    /// # 概要
    /// アセット情報の削除フラグを解除する。
    ///
    /// # 引数
    /// * `asset_id` - アセットID
    ///
    /// # 戻り値
    /// 復帰に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn undelete_asset(&self, asset_id: &AssetId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let update_result = (|| -> Result<()> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(DbError::AssetNotFound)),
            };

            if !asset_info.deleted() {
                return Err(anyhow!(DbError::AssetAlreadyExists));
            }

            asset_info.set_deleted(false);
            info_table.insert(asset_id.clone(), asset_info)?;
            Ok(())
        })();

        if let Err(err) = update_result {
            return Err(err);
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// アセットの移動
    ///
    /// # 概要
    /// アセットの所属ページを移動する。
    ///
    /// # 引数
    /// * `asset_id` - 移動対象のアセットID
    /// * `dst_page_id` - 移動先ページID
    /// * `force` - 移動先競合を許可する場合はtrue
    ///
    /// # 戻り値
    /// 移動に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn move_asset(
        &self,
        asset_id: &AssetId,
        dst_page_id: &PageId,
        force: bool,
    ) -> Result<AssetMoveResult> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut conflict_asset: Option<AssetId> = None;

        let move_result = (|| -> Result<AssetMoveResult> {
            let mut info_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let index_table = txn.open_table(PAGE_INDEX_TABLE)?;

            /*
             * 移動先ページの存在確認
             */
            let dst_index = match index_table.get(dst_page_id.clone())? {
                Some(index) => index.value(),
                None => return Ok(AssetMoveResult::PageNotFound),
            };

            if dst_index.deleted() {
                if !force {
                    return Ok(AssetMoveResult::PageDeleted);
                }
            }

            /*
             * 対象アセットの取得
             */
            let mut asset_info = match info_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!(DbError::AssetNotFound)),
            };

            let file_name = asset_info.file_name();
            let lookup_key = (dst_page_id.clone(), file_name.clone());

            let conflict_id = lookup_table
                .get(&lookup_key)?
                .map(|entry| entry.value());
            if let Some(conflict_id) = conflict_id {
                if conflict_id == asset_id.clone() {
                    // 同一アセットの再指定は競合として扱わない
                } else if !force {
                    return Ok(AssetMoveResult::NameConflict);
                } else {
                    conflict_asset = Some(conflict_id.clone());
                    let _ = lookup_table.remove(lookup_key.clone());
                    let _ = group_table.remove(dst_page_id.clone(), conflict_id.clone());
                    let _ = info_table.remove(conflict_id)?;
                }
            }

            /*
             * 旧所属の参照解除
             */
            if let Some(src_page_id) = asset_info.page_id() {
                let src_key = (src_page_id.clone(), file_name.clone());
                let _ = lookup_table.remove(src_key);
                let _ = group_table.remove(src_page_id, asset_id.clone());
            }

            /*
             * 新所属の登録
             */
            asset_info.set_page_id(dst_page_id.clone());
            info_table.insert(asset_id.clone(), asset_info)?;
            lookup_table.insert(lookup_key, asset_id.clone())?;
            let _ = group_table.insert(dst_page_id.clone(), asset_id.clone())?;

            Ok(AssetMoveResult::Moved)
        })()?;

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * 競合アセットの削除
         */
        if let Some(conflict) = conflict_asset {
            let asset_path = self.asset_file_path(&conflict);
            let _ = fs::remove_file(asset_path);
        }

        Ok(move_result)
    }

    ///
    /// ロック情報の削除
    ///
    /// # 引数
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 削除に成功した場合は`true`を返す。
    ///
    pub(crate) fn delete_lock(&self, token: &LockToken) -> Result<bool> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut removed = false;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut draft_to_delete: Option<PageId> = None;

        /*
         * ロック情報の削除とインデックス更新
         */
        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * ロック情報取得
             */
            let lock_info = match lock_table.get(token.clone())? {
                Some(info) => Some(info.value()),
                None => None,
            };

                if let Some(info) = lock_info {
                    /*
                     * ロック情報の削除
                     */
                    let _ = lock_table.remove(token.clone())?;
                    let page_id = info.page();
                    let index = match index_table.get(page_id.clone())? {
                        Some(entry) => Some(entry.value()),
                        None => None,
                    };
                    /*
                     * ページインデックスのロック解除
                     */
                    if let Some(mut index) = index {
                        if index.is_draft() {
                            draft_to_delete = Some(page_id.clone());
                        } else if index.lock_token() == Some(token.clone()) {
                            index.set_lock_token(None);
                            index_table.insert(page_id, index)?;
                        }
                    }

                removed = true;
            } else {
                /*
                 * インデックス側でロック解除を試行
                 */
                let mut target = None;
                for entry in index_table.iter()? {
                    let (page_id, index) = entry?;
                    let mut index = index.value();
                    if index.lock_token() == Some(token.clone()) {
                        index.set_lock_token(None);
                        target = Some((page_id.value().clone(), index));
                        removed = true;
                        break;
                    }
                }

                if let Some((page_id, index)) = target {
                    index_table.insert(page_id, index)?;
                }
            }
        }

        /*
         * ドラフト削除
         */
        if let Some(page_id) = draft_to_delete {
            let mut deleted = delete_draft_in_txn(&txn, &page_id)?;
            asset_ids.append(&mut deleted);
            removed = true;
        }

        /*
         * コミット
         */
        if removed {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(removed)
    }

    ///
    /// ページID指定でロックを削除する
    ///
    /// # 概要
    /// ページIDに紐付くロック情報を削除する。
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    ///
    /// # 戻り値
    /// ロック削除に成功した場合は`true`を返す。
    ///
    pub(crate) fn delete_page_lock_by_id(
        &self,
        page_id: &PageId,
    ) -> Result<bool> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut removed = false;
        let mut asset_ids: Vec<AssetId> = Vec::new();
        let mut draft_to_delete: Option<PageId> = None;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Ok(false),
            };

            if let Some((token, _)) = find_lock_by_page(&lock_table, page_id)? {
                let _ = lock_table.remove(token.clone())?;
                if index.is_draft() {
                    draft_to_delete = Some(page_id.clone());
                } else if index.lock_token() == Some(token.clone()) {
                    let mut index = index;
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index)?;
                }
                removed = true;
            } else if index.is_draft() {
                draft_to_delete = Some(page_id.clone());
                removed = true;
            } else if index.lock_token().is_some() {
                let mut index = index;
                index.set_lock_token(None);
                index_table.insert(page_id.clone(), index)?;
                removed = true;
            }
        }

        if let Some(draft_page_id) = draft_to_delete {
            asset_ids = delete_draft_in_txn(&txn, &draft_page_id)?;
            removed = true;
        }

        if removed {
            txn.commit()?;
        }

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(removed)
    }

    ///
    /// ユーザ情報の削除
    ///
    /// # 引数
    /// * `username` - 削除対象のユーザ名
    ///
    /// # 戻り値
    /// 削除に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn delete_user(&self, username: &str) -> Result<()> {
        /*
         * 事前情報の整形
         */
        let key = username.to_string();

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * ユーザ情報の削除
         */
        {
            let mut id_table = txn.open_table(USER_ID_TABLE)?;
            let user_id = match id_table.get(&key)? {
                Some(id) => id.value(),
                None => {
                    return Err(anyhow::anyhow!(
                        "user not found: {}",
                        username
                    ));
                }
            };

            let mut info_table = txn.open_table(USER_INFO_TABLE)?;
            let _ = info_table.remove(user_id)?;
            let _ = id_table.remove(&key)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ユーザ情報の更新
    ///
    /// # 引数
    /// * `username` - 更新対象のユーザ名
    /// * `display_name` - 表示名
    /// * `password` - パスワード
    ///
    /// # 戻り値
    /// 更新に成功した場合は`Ok(())`を返す。
    ///
    pub(crate) fn update_user(
        &self,
        username: &str,
        display_name: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        /*
         * 引数の妥当性チェック
         */
        if display_name.is_none() && password.is_none() {
            return Err(anyhow!("no update fields specified"));
        }

        /*
         * 事前情報の整形
         */
        let key = username.to_string();

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * ユーザ情報の更新
         */
        {
            let id_table = txn.open_table(USER_ID_TABLE)?;
            let user_id = match id_table.get(&key)? {
                Some(id) => id.value(),
                None => {
                    return Err(anyhow!("user not found: {}", username));
                }
            };

            let mut info_table = txn.open_table(USER_INFO_TABLE)?;
            let mut user_info = match info_table.get(user_id.clone())? {
                Some(info) => info.value(),
                None => {
                    return Err(anyhow!("user not found: {}", username));
                }
            };

            if let Some(name) = display_name {
                user_info.set_display_name(name);
            }

            if let Some(password) = password {
                user_info.set_password(password);
            }

            info_table.insert(user_id.clone(), user_info)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの作成
    ///
    /// # 引数
    /// * `path` - ページのパス
    /// * `user_id` - ページを作成したユーザID
    /// * `source` - ページソース
    ///
    /// # 戻り値
    /// 作成したページIDを返す。
    ///
    pub(crate) fn create_page<P, U>(
        &self,
        path: P,
        user_name: U,
        source: String,
    ) -> Result<PageId>
    where
        P: AsRef<str>,
        U: AsRef<str>,
    {
        let path = path.as_ref().to_string();
        let user_name = user_name.as_ref().to_string();
        let revision = 1u64;

        let txn = self.db.begin_write()?;
        let page_id = PageId::new();

        {
            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;

            /*
             * パス重複の確認
             */
            if path_table.get(&path)?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * 初期リネーム情報の生成
             */
            let link_refs = build_link_refs_with_table(&path_table, &path, &source)?;
            let rename_info = RenameInfo::new(
                None,
                path.clone(),
                link_refs,
            );
            let page_index = PageIndex::new_page(page_id.clone(), path.clone());
            let page_source = PageSource::new(source, user_id, rename_info);

            /*
             * インデックスとソースの登録
             */
            path_table.insert(&path, page_id.clone())?;
            index_table.insert(page_id.clone(), page_index)?;

            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            source_table.insert((page_id.clone(), revision), page_source)?;
        }

        txn.commit()?;

        Ok(page_id)
    }

    ///
    /// ドラフトページの作成(ロック取得を含む)
    ///
    /// # 概要
    /// ドラフトページを作成し、同時にロックを取得する。
    ///
    /// # 引数
    /// * `path` - ページパス
    /// * `user_name` - ユーザ名
    ///
    /// # 戻り値
    /// 作成したページIDとロック情報を返す。
    ///
    pub(crate) fn create_draft_page<P, U>(
        &self,
        path: P,
        user_name: U,
    ) -> Result<(PageId, LockInfo)>
    where
        P: AsRef<str>,
        U: AsRef<str>,
    {
        let path = path.as_ref().to_string();
        let user_name = user_name.as_ref().to_string();

        let txn = self.db.begin_write()?;
        let page_id = PageId::new();
        let lock_info = {
            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * パス重複の確認
             */
            if path_table.get(&path)?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * ドラフト情報の登録
             */
            let page_index = PageIndex::new_draft(page_id.clone(), path.clone());
            path_table.insert(&path, page_id.clone())?;
            index_table.insert(page_id.clone(), page_index)?;

            /*
             * ロック情報の登録
             */
            let lock_info = LockInfo::new(&page_id, &user_id);
            let token = lock_info.token();
            lock_table.insert(token, lock_info.clone())?;

            lock_info
        };

        txn.commit()?;

        Ok((page_id, lock_info))
    }

    ///
    /// ページの書き込み
    ///
    /// # 引数
    /// * `path` - ページのパス
    /// * `user` - ページを編集したユーザの名前
    /// * `source` - ページソース
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn put_page(
        &self,
        page_id: &PageId,
        user_name: &str,
        source: String,
        amend: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * ユーザIDの解決
             */
            let user_id = {
                let id_table = txn.open_table(USER_ID_TABLE)?;
                let key = user_name.to_string();
                match id_table.get(&key)? {
                    Some(id) => id.value(),
                    None => return Err(anyhow!(DbError::UserNotFound)),
                }
            };

            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                if amend {
                    return Err(anyhow!(DbError::AmendForbidden));
                }

                let path = index.path();
                let link_refs = build_link_refs(&txn, &path, &source)?;
                let rename_info = RenameInfo::new(
                    None,
                    path.clone(),
                    link_refs,
                );
                let mut page_index = PageIndex::new_page(
                    page_id.clone(),
                    path,
                );
                if let Some((token, _)) =
                    find_lock_by_page(&lock_table, page_id)?
                {
                    page_index.set_lock_token(Some(token));
                }
                let page_source = PageSource::new(source, user_id, rename_info);

                index_table.insert(page_id.clone(), page_index)?;
                source_table.insert((page_id.clone(), 1), page_source)?;
            } else if amend {
                /*
                 * 最新リビジョンの更新
                 */
                let revision = index.latest();
                let mut page_source = match source_table.get(
                    (page_id.clone(), revision)
                )? {
                    Some(entry) => entry.value(),
                    None => {
                        return Err(anyhow!(
                            "page source not found"
                        ));
                    }
                };

                if page_source.user() != user_id {
                    return Err(anyhow!(DbError::AmendForbidden));
                }

                page_source.update_source(source);
                source_table.insert((page_id.clone(), revision), page_source)?;
            } else {
                /*
                 * 新規リビジョンの追加
                 */
                let revision = index.latest() + 1;
                let page_source = PageSource::new_revision(
                    revision,
                    source,
                    user_id,
                    None,
                );

                index.set_latest(revision);
                index_table.insert(page_id.clone(), index)?;
                source_table.insert((page_id.clone(), revision), page_source)?;
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// 再帰判定用のパスプレフィックス生成
    ///
    /// # 引数
    /// * `base_path` - 起点パス
    ///
    /// # 戻り値
    /// 配下判定用のプレフィックスを返す。
    ///
    fn build_recursive_prefix(base_path: &str) -> String {
        let base = base_path.trim_end_matches('/');
        format!("{}/", base)
    }

    ///
    /// ロック状態の検証と期限切れロックの掃除
    ///
    /// # 概要
    /// ロックが有効ならPageLockedを返し、期限切れ・欠落ロックは解消する。
    ///
    /// # 引数
    /// * `page_id` - 対象ページID
    /// * `index` - ページインデックス
    /// * `index_table` - ページインデックステーブル
    /// * `lock_table` - ロック情報テーブル
    /// * `now` - 判定基準時刻
    ///
    /// # 戻り値
    /// ロックが問題ない場合は`Ok(())`を返す。
    ///
    fn verify_page_lock_in_txn<'txn>(
        page_id: &PageId,
        index: &mut PageIndex,
        index_table: &mut Table<'txn, PageId, PageIndex>,
        lock_table: &mut Table<'txn, LockToken, LockInfo>,
        now: &DateTime<Local>,
    ) -> Result<()> {
        if let Some(token) = index.lock_token() {
            let mut remove_lock = false;
            if let Some(info) = lock_table.get(token.clone())? {
                let info = info.value();
                if info.expire() > *now {
                    return Err(anyhow!(DbError::PageLocked));
                }
                remove_lock = true;
            }
            if remove_lock {
                lock_table.remove(token.clone())?;
            }
            index.set_lock_token(None);
            index_table.insert(page_id.clone(), index.clone())?;
        }

        Ok(())
    }

    ///
    /// 再帰削除対象のページID一覧を収集する
    ///
    /// # 概要
    /// ページパスインデックスのレンジイテレータを用いて指定パス配下のみを
    /// 走査し、ドラフトやロックページが存在する場合はエラーとする。
    ///
    /// # 引数
    /// * `path_table` - ページパスインデックステーブル
    /// * `index_table` - ページインデックステーブル
    /// * `lock_table` - ロック情報テーブル
    /// * `base_path` - 起点パス
    ///
    /// # 戻り値
    /// 対象ページIDの一覧を返す。
    ///
    fn collect_recursive_page_ids_in_txn<'txn>(
        path_table: &mut Table<'txn, String, PageId>,
        index_table: &mut Table<'txn, PageId, PageIndex>,
        lock_table: &mut Table<'txn, LockToken, LockInfo>,
        base_path: &str,
    ) -> Result<Vec<PageId>> {
        /*
         * 事前情報の準備
         */
        let prefix = Self::build_recursive_prefix(base_path);
        let now = Local::now();
        let mut targets = Vec::new();

        /*
         * 配下ページの収集とロック検証
         */
        let mut iter = path_table.range(base_path.to_string()..)?;
        for entry in &mut iter {
            let (path, page_id) = entry?;
            let path = path.value();
            if path != base_path && !path.starts_with(&prefix) {
                break;
            }

            let page_id = page_id.value().clone();
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if index.deleted() {
                return Err(anyhow!("page already deleted"));
            }

            Self::verify_page_lock_in_txn(
                &page_id,
                &mut index,
                index_table,
                lock_table,
                &now,
            )?;

            targets.push(page_id);
        }

        Ok(targets)
    }

    ///
    /// 再帰対象ページのパスとIDを収集する
    ///
    /// # 概要
    /// 配下ページのパスとIDを収集し、ドラフトやロック中が含まれる場合はエラー
    /// を返す。
    ///
    /// # 引数
    /// * `path_table` - ページパスインデックステーブル
    /// * `index_table` - ページインデックステーブル
    /// * `lock_table` - ロック情報テーブル
    /// * `base_path` - 起点パス
    ///
    /// # 戻り値
    /// 対象ページの一覧を返す。
    ///
    fn collect_recursive_page_targets_in_txn<'txn>(
        path_table: &mut Table<'txn, String, PageId>,
        index_table: &mut Table<'txn, PageId, PageIndex>,
        lock_table: &mut Table<'txn, LockToken, LockInfo>,
        base_path: &str,
    ) -> Result<Vec<RecursivePageTarget>> {
        /*
         * 事前情報の準備
         */
        let prefix = Self::build_recursive_prefix(base_path);
        let now = Local::now();
        let mut targets = Vec::new();

        /*
         * 配下ページの収集とロック検証
         */
        let mut iter = path_table.range(base_path.to_string()..)?;
        for entry in &mut iter {
            let (path, page_id) = entry?;
            let path = path.value();
            if path != base_path && !path.starts_with(&prefix) {
                break;
            }

            let page_id = page_id.value().clone();
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if index.deleted() {
                return Err(anyhow!("page already deleted"));
            }

            Self::verify_page_lock_in_txn(
                &page_id,
                &mut index,
                index_table,
                lock_table,
                &now,
            )?;

            targets.push(RecursivePageTarget {
                page_id,
                path: path.to_string(),
            });
        }

        Ok(targets)
    }

    ///
    /// 削除済みページの再帰対象を収集する
    ///
    /// # 概要
    /// 削除済みページパスのレンジ走査で対象パス配下を収集し、
    /// ロックや未削除が混在している場合はエラーを返す。
    ///
    /// # 引数
    /// * `deleted_path_table` - 削除済みページパスインデックステーブル
    /// * `index_table` - ページインデックステーブル
    /// * `lock_table` - ロック情報テーブル
    /// * `base_path` - 起点パス
    ///
    /// # 戻り値
    /// 対象ページの一覧を返す。
    ///
    fn collect_recursive_deleted_page_targets_in_txn<'txn>(
        deleted_path_table: &mut MultimapTable<'txn, String, PageId>,
        index_table: &mut Table<'txn, PageId, PageIndex>,
        lock_table: &mut Table<'txn, LockToken, LockInfo>,
        base_path: &str,
    ) -> Result<Vec<RecursivePageTarget>> {
        /*
         * 事前情報の準備
         */
        let prefix = Self::build_recursive_prefix(base_path);
        let now = Local::now();
        let mut targets = Vec::new();

        /*
         * 配下ページの収集とロック検証
         */
        let mut iter = deleted_path_table.range(base_path.to_string()..)?;
        for entry in &mut iter {
            let (path, page_ids) = entry?;
            let path = path.value();
            if path != base_path && !path.starts_with(&prefix) {
                break;
            }

            for page_id in page_ids {
                let page_id = page_id?.value().clone();
                let mut index = match index_table.get(page_id.clone())? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!(DbError::PageNotFound)),
                };

                if index.is_draft() {
                    return Err(anyhow!(DbError::PageLocked));
                }

                if !index.deleted() {
                    return Err(anyhow!("page not deleted"));
                }

                Self::verify_page_lock_in_txn(
                    &page_id,
                    &mut index,
                    index_table,
                    lock_table,
                    &now,
                )?;

                targets.push(RecursivePageTarget {
                    page_id,
                    path: path.to_string(),
                });
            }
        }

        Ok(targets)
    }

    ///
    /// ページのソフトデリート(トランザクション内部処理)
    ///
    /// # 概要
    /// ページの削除フラグ更新と関連アセットの削除フラグ更新を行う。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    /// * `path_table` - ページパスインデックステーブル
    /// * `deleted_path_table` - 削除済みページパスインデックステーブル
    /// * `index_table` - ページインデックステーブル
    /// * `lock_table` - ロック情報テーブル
    /// * `asset_table` - アセット情報テーブル
    /// * `group_table` - ページ所属アセット群取得テーブル
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    fn delete_page_soft_in_txn<'txn>(
        page_id: &PageId,
        path_table: &mut Table<'txn, String, PageId>,
        deleted_path_table: &mut MultimapTable<'txn, String, PageId>,
        index_table: &mut Table<'txn, PageId, PageIndex>,
        lock_table: &mut Table<'txn, LockToken, LockInfo>,
        asset_table: &mut Table<'txn, AssetId, AssetInfo>,
        group_table: &MultimapTable<'txn, PageId, AssetId>,
    ) -> Result<()> {
        /*
         * ページ情報取得と保護判定
         */
        let mut index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!(DbError::PageNotFound)),
        };

        if index.is_draft() {
            return Err(anyhow!(DbError::PageLocked));
        }

        if index.deleted() {
            return Err(anyhow!("page already deleted"));
        }

        if is_root_path(&index.path()) {
            return Err(anyhow!(DbError::RootPageProtected));
        }

        /*
         * ロック情報の削除
         */
        if let Some(token) = index.lock_token() {
            lock_table.remove(token)?;
            index.set_lock_token(None);
        }

        /*
         * ページ削除フラグの更新
         */
        let current_path = match index.current_path() {
            Some(path) => path.to_string(),
            None => return Err(anyhow!("page path not found")),
        };
        index.set_deleted_path(current_path.clone());
        index_table.insert(page_id.clone(), index)?;
        let _ = path_table.remove(&current_path)?;
        let _ = deleted_path_table.insert(current_path, page_id.clone())?;

        /*
         * 付随アセットの削除フラグ更新
         */
        for entry in group_table.get(page_id.clone())? {
            let asset_id = entry?.value();
            let mut asset_info = match asset_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => return Err(anyhow!("asset info not found")),
            };

            if asset_info.deleted() {
                continue;
            }

            asset_info.set_deleted(true);
            asset_info.clear_page_id();
            asset_table.insert(asset_id.clone(), asset_info)?;
        }

        Ok(())
    }

    ///
    /// ページのハードデリート(トランザクション内部処理)
    ///
    /// # 概要
    /// ページの全リビジョンとインデックスの削除、関連アセットの参照解除を行う。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    /// * `path_table` - ページパスインデックステーブル
    /// * `deleted_path_table` - 削除済みページパスインデックステーブル
    /// * `index_table` - ページインデックステーブル
    /// * `source_table` - ページソーステーブル
    /// * `lock_table` - ロック情報テーブル
    /// * `asset_table` - アセット情報テーブル
    /// * `lookup_table` - アセットID特定テーブル
    /// * `group_table` - ページ所属アセット群取得テーブル
    /// * `asset_ids` - 削除対象アセットIDの収集先
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    fn delete_page_hard_in_txn<'txn>(
        page_id: &PageId,
        path_table: &mut Table<'txn, String, PageId>,
        deleted_path_table: &mut MultimapTable<'txn, String, PageId>,
        index_table: &mut Table<'txn, PageId, PageIndex>,
        source_table: &mut Table<'txn, (PageId, u64), PageSource>,
        lock_table: &mut Table<'txn, LockToken, LockInfo>,
        asset_table: &mut Table<'txn, AssetId, AssetInfo>,
        lookup_table: &mut Table<'txn, (PageId, String), AssetId>,
        group_table: &mut MultimapTable<'txn, PageId, AssetId>,
        asset_ids: &mut Vec<AssetId>,
    ) -> Result<()> {
        /*
         * ページ情報取得と保護判定
         */
        let index = match index_table.get(page_id.clone())? {
            Some(entry) => entry.value(),
            None => return Err(anyhow!(DbError::PageNotFound)),
        };

        if index.is_draft() {
            return Err(anyhow!(DbError::PageLocked));
        }

        if is_root_path(&index.path()) {
            return Err(anyhow!(DbError::RootPageProtected));
        }

        /*
         * ロック情報の削除
         */
        if let Some(token) = index.lock_token() {
            lock_table.remove(token)?;
        }

        /*
         * 付随アセットの参照解除
         */
        for entry in group_table.remove_all(page_id.clone())? {
            let asset_id = entry?.value();
            let mut asset_info = match asset_table.get(asset_id.clone())? {
                Some(info) => info.value(),
                None => continue,
            };

            let file_name = asset_info.file_name();
            let _ = lookup_table.remove((page_id.clone(), file_name));

            asset_info.set_deleted(true);
            asset_info.clear_page_id();
            asset_table.insert(asset_id.clone(), asset_info)?;
            asset_ids.push(asset_id);
        }

        /*
         * ページソースの削除
         */
        for revision in index.earliest()..=index.latest() {
            let _ = source_table.remove((page_id.clone(), revision))?;
        }

        /*
         * パス・インデックスの削除
         */
        if let Some(path) = index.current_path() {
            let current_path = path.to_string();
            let _ = path_table.remove(&current_path)?;
        }
        if let Some(path) = index.last_deleted_path() {
            let _ = deleted_path_table.remove(path.to_string(), page_id.clone());
        }
        let _ = index_table.remove(page_id.clone())?;

        Ok(())
    }

    ///
    /// ページの再帰的削除
    ///
    /// # 概要
    /// パスインデックスのレンジ走査で対象ページ配下を収集し、トランザクション
    /// 内で一括削除を行う。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    /// * `hard_delete` - ハードデリートを行う場合はtrue
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn delete_pages_recursive_by_id(
        &self,
        page_id: &PageId,
        hard_delete: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids = Vec::new();

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * 起点ページの取得と検証
             */
            let mut base_index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if base_index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if base_index.deleted() && !hard_delete {
                return Err(anyhow!("page already deleted"));
            }

            let base_path = if let Some(path) = base_index.current_path() {
                path.to_string()
            } else if hard_delete {
                match base_index.last_deleted_path() {
                    Some(path) => path.to_string(),
                    None => return Err(anyhow!("page path not found")),
                }
            } else {
                return Err(anyhow!("page path not found"));
            };

            if is_root_path(&base_path) {
                return Err(anyhow!(DbError::RootPageProtected));
            }

            /*
             * 起点ページのロック検証
             */
            let now = Local::now();
            Self::verify_page_lock_in_txn(
                page_id,
                &mut base_index,
                &mut index_table,
                &mut lock_table,
                &now,
            )?;

            /*
             * 再帰対象の収集
             */
            let mut targets = Self::collect_recursive_page_ids_in_txn(
                &mut path_table,
                &mut index_table,
                &mut lock_table,
                &base_path,
            )?;

            if targets.iter().all(|id| id != page_id) {
                targets.insert(0, page_id.clone());
            }

            /*
             * ページ削除の実行
             */
            if hard_delete {
                let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
                let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
                for target_id in targets {
                    Self::delete_page_hard_in_txn(
                        &target_id,
                        &mut path_table,
                        &mut deleted_path_table,
                        &mut index_table,
                        &mut source_table,
                        &mut lock_table,
                        &mut asset_table,
                        &mut lookup_table,
                        &mut group_table,
                        &mut asset_ids,
                    )?;
                }
            } else {
                for target_id in targets {
                    Self::delete_page_soft_in_txn(
                        &target_id,
                        &mut path_table,
                        &mut deleted_path_table,
                        &mut index_table,
                        &mut lock_table,
                        &mut asset_table,
                        &mut group_table,
                    )?;
                }
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        if hard_delete {
            for asset_id in asset_ids {
                let asset_path = self.asset_file_path(&asset_id);
                let _ = fs::remove_file(asset_path);
            }
        }

        Ok(())
    }

    ///
    /// ページの再帰的リネーム
    ///
    /// # 概要
    /// 指定ページと配下ページのパスを一括で更新する。
    ///
    /// # 引数
    /// * `page_id` - 起点となるページID
    /// * `rename_to` - 移動先のパス
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn rename_pages_recursive_by_id(
        &self,
        page_id: &PageId,
        rename_to: &str,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;

            /*
             * 起点ページの取得と検証
             */
            let base_index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if base_index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            let rename_to = if
                let Some(suffix) = Self::get_suffix(base_index.path())
            {
                if rename_to.ends_with('/') {
                    format!("{}{}", rename_to, suffix)
                } else {
                    rename_to.to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            };

            let base_path = match base_index.current_path() {
                Some(path) => path.to_string(),
                None => return Err(anyhow!("page path not found")),
            };

            if is_root_path(&base_path) {
                return Err(anyhow!(DbError::RootPageProtected));
            }

            if base_path == rename_to {
                return Ok(());
            }

            let base_prefix = Self::build_recursive_prefix(&base_path);
            if rename_to.starts_with(&base_prefix) {
                return Err(anyhow!(DbError::InvalidMoveDestination));
            }

            /*
             * 再帰対象の収集
             */
            let targets = Self::collect_recursive_page_targets_in_txn(
                &mut path_table,
                &mut index_table,
                &mut lock_table,
                &base_path,
            )?;

            let target_ids: HashSet<PageId> =
                targets.iter().map(|item| item.page_id.clone()).collect();

            /*
             * 衝突チェックと移動パス生成
             */
            let mut new_paths = HashSet::new();
            let mut mappings: Vec<(PageId, String, String)> = Vec::new();
            for target in &targets {
                let new_path = if target.path == base_path {
                    rename_to.clone()
                } else {
                    let suffix = target
                        .path
                        .strip_prefix(&base_prefix)
                        .unwrap_or("");

                    format!("{}/{}", rename_to, suffix)
                };

                if !new_paths.insert(new_path.clone()) {
                    return Err(anyhow!(DbError::PageAlreadyExists));
                }

                if let Some(existing) = path_table.get(&new_path)? {
                    let existing_id = existing.value();
                    if !target_ids.contains(&existing_id) {
                        return Err(anyhow!(DbError::PageAlreadyExists));
                    }
                }

                mappings.push((
                    target.page_id.clone(),
                    target.path.clone(),
                    new_path,
                ));
            }

            /*
             * リネーム実行
             */
            for (target_id, src_path, new_path) in mappings {
                let mut index = match index_table.get(target_id.clone())? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!(DbError::PageNotFound)),
                };

                if index.is_draft() {
                    return Err(anyhow!(DbError::PageNotFound));
                }

                let latest = index.latest();
                let latest_source = match source_table.get(
                    (target_id.clone(), latest)
                )? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!("page source not found")),
                };

                let link_refs = {
                    let path_table = &path_table;
                    build_link_refs_with_table(
                        path_table,
                        &src_path,
                        &latest_source.source(),
                    )?
                };
                let rename_info = RenameInfo::new(
                    Some(src_path.clone()),
                    new_path.clone(),
                    link_refs,
                );
                let revision = latest + 1;
                let page_source = PageSource::new_revision(
                    revision,
                    latest_source.source(),
                    latest_source.user(),
                    Some(rename_info),
                );

                index.set_latest(revision);
                index.set_path(new_path.clone());
                index.push_rename_revision(revision);
                index_table.insert(target_id.clone(), index)?;
                source_table.insert((target_id.clone(), revision), page_source)?;

                path_table.remove(&src_path)?;
                path_table.insert(&new_path, target_id.clone())?;
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの削除
    ///
    /// # 概要
    /// ページをソフトデリートし、関連アセットも削除状態にする。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn delete_page_by_id(&self, page_id: &PageId) -> Result<()>
    {
        if let Some(index) = self.get_page_index_by_id(page_id)? {
            if index.is_draft() {
                return self.delete_draft_by_id(page_id);
            }
        }

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            Self::delete_page_soft_in_txn(
                page_id,
                &mut path_table,
                &mut deleted_path_table,
                &mut index_table,
                &mut lock_table,
                &mut asset_table,
                &group_table,
            )?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ロック解除トークン付きページ削除
    ///
    /// # 概要
    /// ロック検証とページ削除を単一トランザクション内で実行する。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    /// * `user_id` - 操作ユーザID
    /// * `token` - ロック解除トークン
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn delete_page_by_id_with_lock_token(
        &self,
        page_id: &PageId,
        user_id: &UserId,
        token: Option<&LockToken>,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids = Vec::new();

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let now = Local::now();

            /*
             * ページ情報取得
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            /*
             * ロック検証
             */
            if index.is_draft() {
                if let Some((lock_token, lock_info)) =
                    find_lock_by_page(&lock_table, page_id)?
                {
                    if lock_info.expire() > now {
                        let provided = match token {
                            Some(token) => token,
                            None => return Err(anyhow!(DbError::PageLocked)),
                        };
                        if *provided != lock_token {
                            return Err(anyhow!(DbError::LockForbidden));
                        }
                        if lock_info.user() != *user_id {
                            return Err(anyhow!(DbError::LockForbidden));
                        }
                    }
                    lock_table.remove(lock_token)?;
                }
            } else if let Some(lock_token) = index.lock_token() {
                let mut lock_user = None;
                let mut lock_expired = false;
                let mut lock_exists = false;
                if let Some(lock_info) = lock_table.get(lock_token.clone())? {
                    let lock_info = lock_info.value();
                    lock_exists = true;
                    lock_expired = lock_info.expire() <= now;
                    lock_user = Some(lock_info.user());
                }

                if lock_exists && !lock_expired {
                    let provided = match token {
                        Some(token) => token,
                        None => return Err(anyhow!(DbError::PageLocked)),
                    };
                    if *provided != lock_token {
                        return Err(anyhow!(DbError::LockForbidden));
                    }
                    if lock_user != Some(user_id.clone()) {
                        return Err(anyhow!(DbError::LockForbidden));
                    }
                } else if lock_exists && lock_expired {
                    lock_table.remove(lock_token.clone())?;
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index.clone())?;
                } else {
                    index.set_lock_token(None);
                    index_table.insert(page_id.clone(), index.clone())?;
                }
            }

            /*
             * ページ削除
             */
            if index.is_draft() {
                asset_ids = delete_draft_in_txn(&txn, page_id)?;
            } else {
                Self::delete_page_soft_in_txn(
                    page_id,
                    &mut path_table,
                    &mut deleted_path_table,
                    &mut index_table,
                    &mut lock_table,
                    &mut asset_table,
                    &group_table,
                )?;
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(())
    }

    ///
    /// ドラフトページの削除
    ///
    /// # 概要
    /// ドラフトページを削除し、関連アセットも削除する。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn delete_draft_by_id(&self, page_id: &PageId) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let asset_ids: Vec<AssetId>;

        {
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut tokens = Vec::new();
            for entry in lock_table.iter()? {
                let (token, info) = entry?;
                let info = info.value();
                if info.page() == *page_id {
                    tokens.push(token.value().clone());
                }
            }

            for token in tokens {
                let _ = lock_table.remove(token);
            }
        }

        asset_ids = delete_draft_in_txn(&txn, page_id)?;

        txn.commit()?;

        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(())
    }

    ///
    /// ページの復帰
    ///
    /// # 概要
    /// ページを削除状態から復帰し、必要であれば付随アセットを復帰する。
    ///
    /// # 引数
    /// * `page_id` - 復帰対象のページID
    /// * `with_assets` - 付随アセットの復帰を行う場合はtrue
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn undelete_page_by_id(
        &self,
        page_id: &PageId,
        restore_to: &str,
        with_assets: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * ページ情報取得と削除状態の判定
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if !index.deleted() {
                return Err(anyhow!("page not deleted"));
            }

            if path_table.get(&restore_to.to_string())?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            let restore_to = if
                let Some(suffix) = Self::get_suffix(index.path())
            {
                if restore_to.ends_with('/') {
                    format!("{}{}", restore_to, suffix)
                } else {
                    restore_to.to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            };

            /*
             * 削除フラグの更新
             */
            let deleted_path = match index.last_deleted_path() {
                Some(path) => path.to_string(),
                None => return Err(anyhow!("deleted path not found")),
            };
            index.set_path(restore_to.to_string());
            index_table.insert(page_id.clone(), index)?;
            let _ = deleted_path_table.remove(deleted_path, page_id.clone())?;
            let _ = path_table.insert(restore_to, page_id.clone())?;

            if with_assets {
                /*
                 * 付随アセットの復帰
                 */
                for entry in group_table.get(page_id.clone())? {
                    let asset_id = entry?.value();
                    let mut asset_info = match asset_table.get(asset_id.clone())? {
                        Some(info) => info.value(),
                        None => return Err(anyhow!("asset info not found")),
                    };

                    if !asset_info.deleted() || asset_info.page_id().is_some() {
                        continue;
                    }

                    let file_name = asset_info.file_name();
                    asset_info.set_deleted(false);
                    asset_info.set_page_id(page_id.clone());
                    asset_table.insert(asset_id.clone(), asset_info)?;
                    let _ = lookup_table.insert(
                        (page_id.clone(), file_name),
                        asset_id.clone(),
                    )?;
                }
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページの再帰的復帰
    ///
    /// # 概要
    /// 指定ページと配下ページを削除状態から復帰する。
    ///
    /// # 引数
    /// * `page_id` - 復帰対象のページID
    /// * `restore_to` - 復帰先のパス
    /// * `with_assets` - 付随アセットの復帰を行う場合はtrue
    ///
    /// # 戻り値
    /// 成功時は`Ok(())`を返す。
    ///
    pub(crate) fn undelete_pages_recursive_by_id(
        &self,
        page_id: &PageId,
        restore_to: &str,
        with_assets: bool,
    ) -> Result<()> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            /*
             * 起点ページの取得と検証
             */
            let base_index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if base_index.is_draft() {
                return Err(anyhow!(DbError::PageLocked));
            }

            if !base_index.deleted() {
                return Err(anyhow!("page not deleted"));
            }

            let restore_to = if
                let Some(suffix) = Self::get_suffix(base_index.path())
            {
                if restore_to.ends_with('/') {
                    format!("{}{}", restore_to, suffix)
                } else {
                    restore_to.to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            };

            let base_deleted_path = match base_index.last_deleted_path() {
                Some(path) => path.to_string(),
                None => return Err(anyhow!("deleted path not found")),
            };

            if path_table.get(&restore_to.to_string())?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * 再帰対象の収集
             */
            let targets = Self::collect_recursive_deleted_page_targets_in_txn(
                &mut deleted_path_table,
                &mut index_table,
                &mut lock_table,
                &base_deleted_path,
            )?;

            /*
             * 衝突チェックと復帰先生成
             */
            let base_prefix = Self::build_recursive_prefix(&base_deleted_path);
            let mut new_paths = HashSet::new();
            let mut mappings: Vec<(PageId, String, String)> = Vec::new();

            for target in &targets {
                let new_path = if target.path == base_deleted_path {
                    restore_to.clone()
                } else {
                    let suffix = target
                        .path
                        .strip_prefix(&base_prefix)
                        .unwrap_or("");

                    format!("{}/{}", restore_to, suffix)
                };

                if !new_paths.insert(new_path.clone()) {
                    return Err(anyhow!(DbError::PageAlreadyExists));
                }

                if path_table.get(&new_path)?.is_some() {
                    return Err(anyhow!(DbError::PageAlreadyExists));
                }

                mappings.push((
                    target.page_id.clone(),
                    target.path.clone(),
                    new_path,
                ));
            }

            /*
             * 復帰実行
             */
            for (target_id, deleted_path, new_path) in mappings {
                let mut index = match index_table.get(target_id.clone())? {
                    Some(entry) => entry.value(),
                    None => return Err(anyhow!(DbError::PageNotFound)),
                };

                if !index.deleted() {
                    return Err(anyhow!("page not deleted"));
                }

                index.set_path(new_path.to_string());
                index_table.insert(target_id.clone(), index)?;
                let _ = deleted_path_table.remove(deleted_path, target_id.clone())?;
                let _ = path_table.insert(new_path, target_id.clone())?;

                if with_assets {
                    /*
                     * 付随アセットの復帰
                     */
                    for entry in group_table.get(target_id.clone())? {
                        let asset_id = entry?.value();
                        let mut asset_info = match asset_table.get(asset_id.clone())? {
                            Some(info) => info.value(),
                            None => return Err(anyhow!("asset info not found")),
                        };

                        if !asset_info.deleted() || asset_info.page_id().is_some() {
                            continue;
                        }

                        let file_name = asset_info.file_name();
                        asset_info.set_deleted(false);
                        asset_info.set_page_id(target_id.clone());
                        asset_table.insert(asset_id.clone(), asset_info)?;
                        let _ = lookup_table.insert(
                            (target_id.clone(), file_name),
                            asset_id.clone(),
                        )?;
                    }
                }
            }
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }

    ///
    /// ページのハードデリート
    ///
    /// # 概要
    /// ページの全リビジョンとインデックスを削除し、関連アセットの参照を解消する。
    ///
    /// # 引数
    /// * `page_id` - 削除対象のページID
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn delete_page_by_id_hard(
        &self,
        page_id: &PageId,
    ) -> Result<()> {
        if let Some(index) = self.get_page_index_by_id(page_id)? {
            if index.is_draft() {
                return self.delete_draft_by_id(page_id);
            }
        }

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;
        let mut asset_ids = Vec::new();

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut deleted_path_table =
                txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;
            let mut lock_table = txn.open_table(LOCK_INFO_TABLE)?;
            let mut asset_table = txn.open_table(ASSET_INFO_TABLE)?;
            let mut lookup_table = txn.open_table(ASSET_LOOKUP_TABLE)?;
            let mut group_table = txn.open_multimap_table(ASSET_GROUP_TABLE)?;

            Self::delete_page_hard_in_txn(
                page_id,
                &mut path_table,
                &mut deleted_path_table,
                &mut index_table,
                &mut source_table,
                &mut lock_table,
                &mut asset_table,
                &mut lookup_table,
                &mut group_table,
                &mut asset_ids,
            )?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        /*
         * アセットファイルの削除
         */
        for asset_id in asset_ids {
            let asset_path = self.asset_file_path(&asset_id);
            let _ = fs::remove_file(asset_path);
        }

        Ok(())
    }

    ///
    /// ページの書き込み
    ///
    /// # 引数
    /// * `path` - 削除対象のパス
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn remove_page<S>(path: S) -> Result<()>
    where 
        S: AsRef<str>
    {
        if is_root_path(path.as_ref()) {
            return Err(anyhow!(DbError::RootPageProtected));
        }

        todo!()
    }

    ///
    /// パス文字列の終端ページ名(サフィックスエレメント)を取得する
    ///
    /// # 引数
    /// * `path` - 対象のパス
    ///
    /// # 戻り値
    /// 取得できた終端ページ名を`Some()`でラップして返す。引数`path`で渡された
    /// パスの終端がパスセパレータ("/")の場合は`None`を返す。
    ///
    fn get_suffix<S>(path: S) -> Option<String>
    where 
        S: AsRef<str>
    {
        path.as_ref().rsplit('/').find(|s| !s.is_empty()).map(|s| s.to_string())
    }

    ///
    /// ページのリネーム
    ///
    /// # 引数
    /// * `path` - リネーム対象のページのパス
    /// * `dst_path` - リネーム後のパス
    ///
    /// # 戻り値
    /// 処理が成功した場合は`Ok(())`を返す。失敗した場合はエラー情報を`Err()`で
    /// ラップして返す。
    ///
    pub(crate) fn rename_page<S>(&self, path: S, dst_path: S) -> Result<()>
    where 
        S: AsRef<str>
    {
        if is_root_path(path.as_ref()) {
            return Err(anyhow!(DbError::RootPageProtected));
        }

        let path = path.as_ref().to_string();

        let dst_path = {
            if let Some(suffix) = Self::get_suffix(&path) {
                if dst_path.as_ref().ends_with("/") {
                    format!("{}{}", dst_path.as_ref(), suffix)
                } else {
                    dst_path.as_ref().to_string()
                }
            } else {
                return Err(anyhow!(DbError::InvalidPath));
            }
        };

        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)?;
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)?;
            let mut source_table = txn.open_table(PAGE_SOURCE_TABLE)?;

            /*
             * リネーム対象の解決と競合チェック
             */
            let page_id = match path_table.get(&path)? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if path_table.get(&dst_path)?.is_some() {
                return Err(anyhow!(DbError::PageAlreadyExists));
            }

            /*
             * 最新リビジョンとソース取得
             */
            let mut index = match index_table.get(page_id.clone())? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!(DbError::PageNotFound)),
            };

            if index.is_draft() {
                return Err(anyhow!(DbError::PageNotFound));
            }

            let latest = index.latest();
            let latest_source = match source_table.get(
                (page_id.clone(), latest)
            )? {
                Some(entry) => entry.value(),
                None => return Err(anyhow!("page source not found")),
            };

            /*
             * リネーム情報と新リビジョン生成
             */
            let link_refs = build_link_refs_with_table(
                &path_table,
                &path,
                &latest_source.source(),
            )?;
            let rename_info = RenameInfo::new(
                Some(path.clone()),
                dst_path.clone(),
                link_refs,
            );

            let revision = latest + 1;
            let page_source = PageSource::new_revision(
                revision,
                latest_source.source(),
                latest_source.user(),
                Some(rename_info),
            );

            /*
             * インデックス更新とパスインデックス書き換え
             */
            index.set_latest(revision);
            index.set_path(dst_path.clone());
            index.push_rename_revision(revision);
            index_table.insert(page_id.clone(), index)?;
            source_table.insert((page_id.clone(), revision), page_source)?;

            path_table.remove(&path)?;
            path_table.insert(&dst_path, page_id)?;
        }

        /*
         * コミット
         */
        txn.commit()?;

        Ok(())
    }
}

///
/// ページソースからリンク参照情報を生成
///
/// # 概要
/// MarkdownリンクからWiki内部リンクを抽出して解決する
///
/// # 引数
/// * `txn` - 書き込みトランザクション
/// * `base_path` - 基準となるページパス
/// * `source` - ページソース
///
/// # 戻り値
/// リンク参照情報を返す。
///
/// # 注記
/// 正規表現では括弧の入れ子や`![]()`の除外が複雑になるため、
/// 字句走査による抽出を行っている。
///
/// 抽出ルールは以下の通り。
///  - 対象は`[]()`のリンクのみ
///  - `![]()`は除外
///  - スキーマ指定リンク（`xxx:`）は除外
///  - 相対パスは`base_path`基準で正規化し、`.`/`..`を解決する
///  - 未存在ページは`None`として記録する
///
fn build_link_refs(
    txn: &redb::WriteTransaction,
    base_path: &str,
    source: &str,
) -> Result<BTreeMap<String, Option<PageId>>> {
    let table = txn.open_table(PAGE_PATH_TABLE)?;
    build_link_refs_with_table(&table, base_path, source)
}

fn build_link_refs_with_table<'txn>(
    path_table: &Table<'txn, String, PageId>,
    base_path: &str,
    source: &str,
) -> Result<BTreeMap<String, Option<PageId>>> {
    build_link_refs_with_resolver(
        base_path,
        source,
        |path| resolve_page_id_with_table(path_table, path),
    )
}

fn build_link_refs_with_resolver<F>(
    base_path: &str,
    source: &str,
    mut resolve: F,
) -> Result<BTreeMap<String, Option<PageId>>>
where
    F: FnMut(&str) -> Result<Option<PageId>>,
{
    /*
     * 参照一覧の初期化
     */
    let mut refs = BTreeMap::new();
    let mut chars = source.chars().peekable();

    /*
     * Markdownリンクの抽出
     */
    while let Some(ch) = chars.next() {
        if ch == '!' {
            if matches!(chars.peek(), Some('[')) {
                // 画像リンクは対象外のため末尾まで読み飛ばす
                skip_until_link_end(&mut chars);
            }
            continue;
        }

        if ch != '[' {
            // リンク開始以外の文字は無視する
            continue;
        }

        if !skip_until_char(&mut chars, ']') {
            // ラベル終端が無い場合はリンクとして扱わない
            continue;
        }

        if !matches!(chars.peek(), Some('(')) {
            // 直後がURL部でないものは対象外とする
            continue;
        }
        let _ = chars.next();

        let raw_link = match read_until_paren(&mut chars) {
            Some(link) => link,
            // 閉じ括弧が無い場合は不正リンクとして除外する
            None => continue,
        };

        let raw_link = raw_link.trim();
        if raw_link.is_empty() {
            // URLが空のリンクは対象外とする
            continue;
        }

        if is_schema_link(raw_link) {
            // スキーマ付きリンクは外部参照として除外する
            continue;
        }

        if let Some(normalized) = normalize_page_path(base_path, raw_link) {
            let page_id = resolve(&normalized)?;
            refs.insert(normalized, page_id);
        }
    }

    Ok(refs)
}

///
/// 指定文字が現れるまで読み進める
///
/// # 引数
/// * `iter` - 文字列イテレータ
/// * `target` - 探索対象の文字
///
/// # 戻り値
/// 指定文字が見つかった場合は`true`を返す。
///
fn skip_until_char<I>(iter: &mut std::iter::Peekable<I>, target: char) -> bool
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = iter.next() {
        if ch == target {
            return true;
        }
    }

    false
}

///
/// 閉じ丸括弧までの文字列を取得
///
/// # 引数
/// * `iter` - 文字列イテレータ
///
/// # 戻り値
/// 取得した文字列を返す。閉じ丸括弧が見つからない場合は`None`を返す。
///
fn read_until_paren<I>(iter: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = char>,
{
    /*
     * 取得結果の初期化
     */
    let mut buf = String::new();
    let mut depth = 0usize;

    /*
     * 閉じ括弧までの読み込み
     */
    while let Some(ch) = iter.next() {
        match ch {
            '(' => {
                depth += 1;
                buf.push(ch);
            }
            ')' => {
                if depth == 0 {
                    return Some(buf);
                }
                depth -= 1;
                buf.push(ch);
            }
            _ => buf.push(ch),
        }
    }

    None
}

///
/// Markdownリンクの末尾まで読み飛ばす
///
/// # 引数
/// * `iter` - 文字列イテレータ
///
/// # 戻り値
/// なし
///
fn skip_until_link_end<I>(iter: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    if !skip_until_char(iter, ']') {
        return;
    }

    if !matches!(iter.peek(), Some('(')) {
        return;
    }
    let _ = iter.next();
    let _ = read_until_paren(iter);
}

///
/// スキーマ指定リンクかどうかの判定
///
/// # 引数
/// * `link` - 判定対象のリンク
///
/// # 戻り値
/// スキーマ指定リンクの場合は`true`を返す。
///
fn is_schema_link(link: &str) -> bool {
    let mut chars = link.chars().peekable();
    let mut had_char = false;

    while let Some(ch) = chars.next() {
        if ch == ':' {
            return had_char;
        }

        if ch == '/' || ch.is_whitespace() {
            return false;
        }

        if !is_schema_char(ch) {
            return false;
        }

        had_char = true;
    }

    false
}

///
/// スキーマ指定の文字として許可するかの判定
///
/// # 引数
/// * `ch` - 判定対象の文字
///
/// # 戻り値
/// 許可する文字の場合は`true`を返す。
///
fn is_schema_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.'
}

///
/// ページパスの正規化
///
/// # 引数
/// * `base_path` - 基準となるページパス
/// * `link` - 対象リンク
///
/// # 戻り値
/// 正規化したパスを返す。対象外の場合は`None`を返す。
///
fn normalize_page_path(base_path: &str, link: &str) -> Option<String> {
    /*
     * 事前の判定
     */
    if link.starts_with('/') {
        return Some(cleanup_path(link));
    }

    if link.starts_with('#') {
        return None;
    }

    if link.contains(' ') || link.contains('\t') || link.contains('\n') {
        return None;
    }

    /*
     * 相対パスの解決
     */
    let trimmed = base_path.trim_end_matches('/');
    let base = if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("{}/", trimmed)
    };

    Some(cleanup_path(&format!("{}{}", base, link)))
}

///
/// ページパスの正規化処理
///
/// # 引数
/// * `path` - 対象パス
///
/// # 戻り値
/// 正規化済みのパスを返す。
///
fn cleanup_path(path: &str) -> String {
    /*
     * パスセグメントの収集
     */
    let mut result = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => result.clear(),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.is_empty() {
                    result.pop();
                }
            }
            Component::Normal(name) => result.push(name.to_string_lossy()),
            _ => {}
        }
    }

    /*
     * 正規化パスの構築
     */
    let mut normalized = String::from("/");
    normalized.push_str(&result.join("/"));
    normalized
}

///
/// パスからページIDを解決
///
/// # 引数
/// * `txn` - 書き込みトランザクション
/// * `path` - ページパス
///
/// # 戻り値
/// 解決できたページIDを返す。存在しない場合は`None`を返す。
///
#[allow(dead_code)]
fn resolve_page_id(
    txn: &redb::WriteTransaction,
    path: &str,
) -> Result<Option<PageId>> {
    let table = txn.open_table(PAGE_PATH_TABLE)?;
    resolve_page_id_with_table(&table, path)
}

fn resolve_page_id_with_table<'txn>(
    table: &Table<'txn, String, PageId>,
    path: &str,
) -> Result<Option<PageId>> {
    /*
     * インデックス参照
     */
    let key = path.to_string();
    let entry = match table.get(&key)? {
        Some(entry) => entry.value(),
        None => return Ok(None),
    };

    Ok(Some(entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn build_link_refs_extracts_wiki_links() {
        let (base_dir, db_path) = prepare_test_dirs();
        let mut db = Database::create(&db_path).expect("create db failed");
        init_database(&mut db).expect("init db failed");

        let txn = db.begin_write().expect("begin write failed");
        {
            let mut path_table = txn.open_table(PAGE_PATH_TABLE)
                .expect("open table failed");
            let mut index_table = txn.open_table(PAGE_INDEX_TABLE)
                .expect("open table failed");
            let id_root = PageId::new();
            let id_page = PageId::new();
            path_table.insert(
                "/a".to_string(),
                id_root.clone(),
            ).expect("insert /a failed");
            path_table.insert(
                "/a/b".to_string(),
                id_page.clone(),
            ).expect("insert /a/b failed");
            index_table.insert(
                id_root.clone(),
                PageIndex::new_page(id_root.clone(), "/a".to_string()),
            ).expect("insert /a index failed");
            index_table.insert(
                id_page.clone(),
                PageIndex::new_page(id_page.clone(), "/a/b".to_string()),
            ).expect("insert /a/b index failed");
        }

        let source = concat!(
            "[abs](/a/b) ",
            "[child](child) ",
            "[cur](.) ",
            "[parent](..) ",
            "![img](/img/only) ",
            "[ext](https://example.com) ",
            "[mail](mailto:info@example.com)",
        );

        let refs = build_link_refs(&txn, "/a/b", source)
            .expect("build_link_refs failed");

        assert!(matches!(refs.get("/a/b"), Some(Some(_))));
        assert!(matches!(refs.get("/a"), Some(Some(_))));
        assert!(matches!(refs.get("/a/b/child"), Some(None)));
        assert!(!refs.contains_key("/img/only"));
        assert!(!refs.contains_key("https://example.com"));
        assert!(!refs.contains_key("mailto:info@example.com"));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    fn prepare_test_dirs() -> (PathBuf, PathBuf) {
        let base = Path::new("tests").join("tmp").join(unique_suffix());
        fs::create_dir_all(&base).expect("create test dir failed");
        let db_path = base.join("database.redb");
        (base, db_path)
    }

    #[test]
    fn ensure_default_root_creates_root_page() {
        let (base_dir, db_path) = prepare_test_dirs();
        let asset_path = base_dir.join("assets");

        let manager = DatabaseManager::open(&db_path, &asset_path)
            .expect("open manager failed");
        manager.add_user("user", "pass", None)
            .expect("add user failed");
        manager.ensure_default_root("user")
            .expect("ensure root failed");

        assert!(manager.page_exists(ROOT_PAGE_PATH)
            .expect("page exists failed"));

        fs::remove_dir_all(base_dir).expect("cleanup failed");
    }

    fn unique_suffix() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time failed")
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{}-{}-{}", pid, now, seq)
    }
}
