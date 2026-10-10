/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 同一トランザクションでページ認可と参照対象を確定する
//!

use anyhow::{Result, anyhow};
use redb::{
    ReadTransaction, ReadableDatabase, ReadableTable, WriteTransaction,
};

use super::DatabaseManager;
use super::pages_read::PagePathResolveState;
use crate::auth::PageAccessPolicy;
use crate::database::DbError;
use crate::database::schema::{
    DELETED_PAGE_PATH_TABLE, PAGE_INDEX_TABLE, PAGE_PATH_TABLE,
    PAGE_SOURCE_TABLE, USER_INFO_TABLE,
};
use crate::database::types::{PageId, PageIndex, PageSource, UserId, UserInfo};

///
/// 公開層で区別するパス認可拒否
///
#[derive(Clone, Debug)]
pub(crate) struct PathPrefixDenied {
    /// 実際に判定した正規形パス
    pub(crate) path: String,
    /// 通常探索の親候補に対する拒否
    pub(crate) parent_candidate: bool,
}

impl std::fmt::Display for PathPrefixDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "path prefix denied: {}", self.path)
    }
}

impl std::error::Error for PathPrefixDenied {}

///
/// 通常探索に親候補が存在しない
///
#[derive(Debug)]
pub(crate) struct ParentNotFound;

impl std::fmt::Display for ParentNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("parent not found")
    }
}

impl std::error::Error for ParentNotFound {}

///
/// 指定した履歴ソースが存在しない
///
#[derive(Debug)]
pub(crate) struct PageSourceNotFound;

impl std::fmt::Display for PageSourceNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("page source not found")
    }
}

impl std::error::Error for PageSourceNotFound {}

///
/// 同じsnapshotから取得したページ参照値
///
pub(crate) struct AuthorizedPage {
    /// 認可済みindex
    pub(crate) index: PageIndex,
    /// 要求revision（未指定時は最新）
    pub(crate) revision: u64,
    /// 要求した場合に限り取得する履歴ソース
    pub(crate) source: Option<PageSource>,
    /// ソース更新者名
    pub(crate) user_name: Option<String>,
}

///
/// 同snapshot/txnで確定した最小参照対象
///
#[derive(Clone, Debug)]
pub(crate) struct PageAccessTarget {
    /// 解決・認可した正規形path
    pub(crate) path: crate::wiki_path::NormalizedWikiPath,
    /// 実際に取得したsourceのrevision（未取得ならNone）
    pub(crate) revision: Option<u64>,
}

impl PageAccessTarget {
    ///
    /// 同snapshotの参照値から確定対象を作る
    ///
    /// # 引数
    /// * `page` - 認可済み参照値
    ///
    /// # 戻り値
    /// 実source取得時だけrevisionを持つ対象。
    ///
    pub(crate) fn from_page(page: &AuthorizedPage) -> Result<Self> {
        Ok(Self {
            path: page.index.authorization_path()?,
            revision: page.source.as_ref().map(|_| page.revision),
        })
    }

    ///
    /// 同txnの認可済みindexからrevision未指定対象を作る
    ///
    /// # 引数
    /// * `index` - 認可済みindex
    ///
    /// # 戻り値
    /// sourceを未取得の対象。
    ///
    pub(crate) fn from_index(index: &PageIndex) -> Result<Self> {
        Ok(Self { path: index.authorization_path()?, revision: None })
    }
}

impl std::fmt::Display for PageAccessTarget {
    ///
    /// 対象値を漏らさず内部contextを表示する
    ///
    /// # 引数
    /// * `formatter` - 診断出力先
    ///
    /// # 戻り値
    /// 固定診断の書込結果。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>)
        -> std::fmt::Result
    {
        formatter.write_str("authorized page target")
    }
}

impl std::error::Error for PageAccessTarget {}

///
/// indexの状態別パスを検証し認可する
///
/// # 引数
/// * `index` - 同一txnから得たindex
/// * `policy` - 保存prefix制約
///
/// # 戻り値
/// 許可時は成功、不整合または実pathを持つ拒否を返す。
///
pub(super) fn authorize_index(
    index: &PageIndex,
    policy: &PageAccessPolicy<'_>,
) -> Result<()> {
    let path = index.authorization_path()?;
    if !policy.allows(path.as_str()) {
        return Err(PathPrefixDenied {
            path: path.as_str().to_string(),
            parent_candidate: false,
        }
        .into());
    }
    Ok(())
}

///
/// 同じtxnのindexと履歴ソースを取得する
///
/// # 引数
/// * `txn` - 読み取りまたは書き込みtxn
/// * `page_id` - 固定の対象ID
/// * `revision` - 要求revision
/// * `with_source` - ソースを取得するか
/// * `policy` - パス制約
///
/// # 戻り値
/// 同一時点の参照値、または不存在・認可・不整合エラー。
///
/// # 概要
/// 状態判定や履歴取得より先に最新indexを認可する。
///
pub(super) fn page_in_txn(
    txn: &WriteTransaction,
    page_id: &PageId,
    revision: Option<u64>,
    with_source: bool,
    policy: &PageAccessPolicy<'_>,
) -> Result<AuthorizedPage> {
    let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
    let sources = txn.open_table(PAGE_SOURCE_TABLE)?;
    let users = txn.open_table(USER_INFO_TABLE)?;
    page_in_tables(
        &indexes,
        &sources,
        &users,
        page_id,
        revision,
        with_source,
        policy,
    )
}

///
/// read snapshotのページ情報を共有table処理へ渡す
///
/// # 引数
/// * `txn` - read snapshot
/// * `page_id` - 対象ID
/// * `revision` - 要求revision
/// * `with_source` - ソース取得指定
/// * `policy` - パス制約
///
/// # 戻り値
/// 同一snapshotの認可済み参照値。
///
fn page_in_read_txn(
    txn: &ReadTransaction,
    page_id: &PageId,
    revision: Option<u64>,
    with_source: bool,
    policy: &PageAccessPolicy<'_>,
) -> Result<AuthorizedPage> {
    let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
    let sources = txn.open_table(PAGE_SOURCE_TABLE)?;
    let users = txn.open_table(USER_INFO_TABLE)?;
    page_in_tables(
        &indexes,
        &sources,
        &users,
        page_id,
        revision,
        with_source,
        policy,
    )
}

///
/// 同じtxnで開いたtableから対象を認可し取得する
///
/// # 引数
/// * `indexes` - index table
/// * `sources` - source table
/// * `users` - user table
/// * `page_id` - 対象ID
/// * `revision` - 要求revision
/// * `with_source` - ソース取得指定
/// * `policy` - パス制約
///
/// # 戻り値
/// 認可済み参照値または状態/認可エラー。
///
/// # 概要
/// 同じtxnのindex認可後に履歴ソースと更新者を取得する。
///
fn page_in_tables(
    indexes: &impl ReadableTable<PageId, PageIndex>,
    sources: &impl ReadableTable<(PageId, u64), PageSource>,
    users: &impl ReadableTable<UserId, UserInfo>,
    page_id: &PageId,
    revision: Option<u64>,
    with_source: bool,
    policy: &PageAccessPolicy<'_>,
) -> Result<AuthorizedPage> {
    /*
     * 最新indexから対象を確定し認可する
     */
    let index = indexes
        .get(page_id.clone())?
        .ok_or_else(|| anyhow!(DbError::PageNotFound))?
        .value();
    authorize_index(&index, policy)?;
    let revision = revision.unwrap_or(index.latest());
    let mut source = None;
    let mut user_name = None;

    let mut target = PageAccessTarget::from_index(&index)?;
    let result = (|| -> Result<_> {
        /*
         * 履歴値は認可済み対象の同じtxnから取得する
         */
        if with_source && !index.is_draft() {
            source = sources
                .get((page_id.clone(), revision))?
                .map(|entry| entry.value());
            if let Some(source) = &source {
                target.revision = Some(revision);
                user_name = users
                    .get(source.user())?
                    .map(|entry| entry.value().username());
            }
        }
        Ok(AuthorizedPage {
            index,
            revision,
            source,
            user_name,
        })
    })();
    result.map_err(|err| err.context(target))
}

impl DatabaseManager {
    ///
    /// ID指定ページを同一read snapshotで認可・取得する
    ///
    /// # 引数
    /// * `page_id` - 対象ID
    /// * `revision` - 履歴revision
    /// * `with_source` - 履歴ソースを取得するか
    /// * `policy` - prefix制約
    ///
    /// # 戻り値
    /// 認可済み参照値または型付き拒否。
    ///
    pub(crate) fn read_authorized_page(
        &self,
        page_id: &PageId,
        revision: Option<u64>,
        with_source: bool,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<AuthorizedPage> {
        let txn = self.db.begin_read()?;
        page_in_read_txn(&txn, page_id, revision, with_source, policy)
    }

    ///
    /// path指定参照を同一snapshotで解決し認可する
    ///
    /// # 引数
    /// * `path` - 入力解決済み正規形パス
    /// * `policy` - prefix制約
    ///
    /// # 戻り値
    /// 現在・削除・不存在の状態、または拒否/不整合。
    ///
    /// # 概要
    /// 要求pathを先に認可し、indexの実pathとの一致を確認する。
    ///
    pub(crate) fn resolve_authorized_page_path(
        &self,
        path: &str,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<PagePathResolveState> {
        /*
         * 最終入力を認可し同一snapshotのpath indexを読む
         */
        if !policy.allows(path) {
            return Err(PathPrefixDenied {
                path: path.to_string(),
                parent_candidate: false,
            }
            .into());
        }
        let txn = self.db.begin_read()?;
        let paths = txn.open_table(PAGE_PATH_TABLE)?;
        if let Some(entry) = paths.get(path.to_string())? {
            let page_id = entry.value();
            let page = page_in_read_txn(&txn, &page_id, None, false, policy)?;
            if page.index.authorization_path()?.as_str() != path
                || page.index.deleted()
            {
                return Err(anyhow!("page path index is inconsistent"));
            }
            return Ok(PagePathResolveState::Current {
                page_id,
                draft: page.index.is_draft(),
            });
        }

        /*
         * 同じsnapshotの削除indexを確認する
         */
        let deleted = txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
        for entry in deleted.get(path.to_string())? {
            let page_id = entry?.value();
            let page = page_in_read_txn(&txn, &page_id, None, false, policy)?;
            if !page.index.deleted()
                || page.index.authorization_path()?.as_str() != path
            {
                return Err(anyhow!("deleted path index is inconsistent"));
            }
            return Ok(PagePathResolveState::Deleted);
        }
        Ok(PagePathResolveState::NotFound)
    }

    ///
    /// 通常の親探索と候補認可を同一snapshotで行う
    ///
    /// # 引数
    /// * `page_id` - 指定ページID
    /// * `recursive` - 存在する祖先まで探索するか
    /// * `policy` - prefix制約
    ///
    /// # 戻り値
    /// 親ID/path、または指定拒否・親候補拒否・状態エラー。
    ///
    /// # 概要
    /// 親候補非認可で停止し、別祖先への探索を続けない。
    ///
    // 従来tuple返却の内部API互換を保持する。
    #[allow(dead_code)]
    pub(crate) fn read_authorized_parent(
        &self,
        page_id: &PageId,
        recursive: bool,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<(PageId, String)> {
        self.read_authorized_parent_target(page_id, recursive, policy)
            .map(|(id, path, _)| (id, path))
    }

    ///
    /// 指定対象を保持して同snapshotから親候補を取得する
    ///
    /// # 引数
    /// * `page_id` - 指定ページID
    /// * `recursive` - 親の再帰探索指定
    /// * `policy` - prefix制約
    ///
    /// # 戻り値
    /// 親ID/親pathと指定対象、又は対象contextを持つエラー。
    ///
    /// # 概要
    /// 成功の監査対象は指定ページ、親拒否では拒否候補を区別する。
    ///
    pub(crate) fn read_authorized_parent_target(
        &self,
        page_id: &PageId,
        recursive: bool,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<(PageId, String, PageAccessTarget)> {
        /*
         * 指定対象の認可と既存状態条件
         */
        let txn = self.db.begin_read()?;
        let page = page_in_read_txn(&txn, page_id, None, false, policy)?;
        let target = PageAccessTarget::from_page(&page)?;

        let result = (|| -> Result<_> {
            if page.index.deleted() {
                return Err(anyhow!(DbError::PageDeleted));
            }
            if page.index.is_draft() {
                return Err(anyhow!(DbError::PageNotFound));
            }
            let mut path =
                parent_path(page.index.authorization_path()?.as_str());
            let paths = txn.open_table(PAGE_PATH_TABLE)?;

            /*
             * 通常探索で最初に得た候補だけを認可する
             */
            loop {
                if let Some(entry) = paths.get(path.clone())? {
                    let parent_id = entry.value();
                    let parent =
                        page_in_read_txn(&txn, &parent_id, None, false, policy)
                            .map_err(|err| {
                                if err.downcast_ref::<PathPrefixDenied>()
                                    .is_some()
                                {
                                    PathPrefixDenied {
                                        path: path.clone(),
                                        parent_candidate: true,
                                    }
                                    .into()
                                } else {
                                    err
                                }
                            })?;
                    if parent.index.authorization_path()?.as_str() != path {
                        return Err(anyhow!(
                            "parent path index is inconsistent",
                        ));
                    }
                    return Ok((parent_id, path, target.clone()));
                }
                if !recursive || path == "/" {
                    break;
                }
                path = parent_path(&path);
            }
            Err(ParentNotFound.into())
        })();
        result.map_err(|err| err.context(target))
    }
}

///
/// rootを維持して親pathを求める
///
/// # 引数
/// * `path` - 正規形パス
///
/// # 戻り値
/// 親パスを返す。
///
fn parent_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, Local};
    use redb::{ReadableDatabase, ReadableTable};

    use super::{DatabaseManager, PathPrefixDenied, page_in_read_txn};
    use crate::auth::PageAccessPolicy;
    use crate::database::schema::{LOCK_INFO_TABLE, PAGE_INDEX_TABLE};
    use crate::database::types::{LockInfo, PathPrefixSet};

    ///
    /// DBとassetを隔離する
    ///
    /// # 戻り値
    /// ディレクトリとDB。
    ///
    /// # 概要
    /// 一意な隔離領域にDBとテストユーザを初期化する。
    ///
    fn fixture() -> (PathBuf, DatabaseManager) {
        /*
         * 隔離DB/assetsとユーザを初期化する
         */
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = PathBuf::from(format!(
            "tests/tmp/db-ms3-{}-{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&base).unwrap();
        let db = DatabaseManager::open(base.join("db"), base.join("assets"))
            .unwrap();
        db.add_user("user", "pass", None).unwrap();
        (base, db)
    }

    ///
    /// 認可拒否時の期限切れdraft/lock/asset保全と許可時cleanupを検証する
    ///
    /// # 戻り値
    /// テスト結果。
    ///
    /// # 注記
    /// 時刻を保存値へ明示設定しsleepに依存せず副作用を照合する。
    ///
    #[test]
    fn denied_cleanup_and_lock_delete_preserve_draft_and_asset() {
        /*
         * draftとassetを作成しlockを期限切れへ書き換える
         */
        let (base, db) = fixture();
        let (page, lock) =
            db.create_draft_page("/outside/draft", "user").unwrap();
        let asset = db
            .create_asset(&page, "sample", "text/plain", "user", b"data")
            .unwrap();
        let mut value = serde_json::to_value(&lock).unwrap();
        value["expire"] = serde_json::json!(
            (Local::now() - Duration::seconds(1)).to_rfc3339()
        );
        let expired: LockInfo = serde_json::from_value(value).unwrap();
        let txn = db.db.begin_write().unwrap();
        txn.open_table(LOCK_INFO_TABLE)
            .unwrap()
            .insert(lock.token(), expired)
            .unwrap();
        txn.commit().unwrap();
        let prefixes = PathPrefixSet::from_iter(["/allowed"]);
        let denied = PageAccessPolicy::new(&prefixes);

        /*
         * meta/lock GETおよびlock DELETE/PUT/POSTの拒否は無更新
         */
        for with_source in [false, true] {
            let err = db
                .read_authorized_page_meta(
                    &page,
                    None,
                    with_source,
                    true,
                    &denied,
                )
                .err()
                .unwrap();
            assert!(err.downcast_ref::<PathPrefixDenied>().is_some());
        }
        assert!(
            db.release_page_lock_authorized(
                &page,
                "user",
                Ok(&lock.token()),
                &denied
            )
            .unwrap_err()
            .downcast_ref::<PathPrefixDenied>()
            .is_some()
        );
        assert!(
            db.renew_page_lock_authorized(
                &page,
                "user",
                Ok(&lock.token()),
                &denied
            )
            .unwrap_err()
            .downcast_ref::<PathPrefixDenied>()
            .is_some()
        );
        assert!(
            db.acquire_page_lock_authorized(&page, "user", &denied)
                .unwrap_err()
                .downcast_ref::<PathPrefixDenied>()
                .is_some()
        );
        assert!(db.get_page_index_by_id(&page).unwrap().is_some());
        let txn = db.db.begin_read().unwrap();
        assert!(
            txn.open_table(LOCK_INFO_TABLE)
                .unwrap()
                .get(lock.token())
                .unwrap()
                .is_some()
        );
        drop(txn);
        assert!(db.get_asset_info_by_id(&asset).unwrap().is_some());
        assert_eq!(fs::read(db.asset_file_path(&asset)).unwrap(), b"data");

        /*
         * 許可時cleanupは同txnでdraft/lock/assetを除去する
         */
        let unrestricted = PathPrefixSet::new();
        let policy = PageAccessPolicy::new(&unrestricted);
        assert!(
            db.read_authorized_page_meta(&page, None, true, true, &policy)
                .is_err()
        );
        assert!(db.get_page_index_by_id(&page).unwrap().is_none());
        assert!(db.get_asset_info_by_id(&asset).unwrap().is_none());
        assert!(!db.asset_file_path(&asset).exists());
        /*
         * lock成功結果は同じtxnで認可したpath/indexを保持する
         */
        let page = db
            .create_page("/allowed/page", "user", "source".into())
            .unwrap();
        let acquired = db
            .acquire_page_lock_authorized(&page, "user", &policy)
            .unwrap();
        assert_eq!(
            acquired.target.authorization_path().unwrap().as_str(),
            "/allowed/page"
        );
        let renewed = db
            .renew_page_lock_authorized(
                &page,
                "user",
                Ok(&acquired.value.token()),
                &policy,
            )
            .unwrap();
        assert_eq!(renewed.target.latest(), 1);
        let released = db
            .release_page_lock_authorized(
                &page,
                "user",
                Ok(&renewed.value.token()),
                &policy,
            )
            .unwrap();
        assert_eq!(
            released.target.authorization_path().unwrap().as_str(),
            "/allowed/page"
        );
        drop(db);
        fs::remove_dir_all(base).unwrap();
    }

    ///
    /// 並行writerのcommit後もread snapshotのpath/source一致を保つ
    ///
    /// # 戻り値
    /// テスト結果。
    ///
    /// # 注記
    /// barrierでsnapshot取得後のrenameを同期させる。sleepは使わない。
    ///
    #[test]
    fn read_snapshot_keeps_authorized_path_and_source_together() {
        let (base, db) = fixture();
        let page = db
            .create_page("/allowed/page", "user", "original".into())
            .unwrap();
        let db = Arc::new(db);
        let txn = db.db.begin_read().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let writer = {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.rename_page("/allowed/page", "/outside/page").unwrap();
                barrier.wait();
            })
        };
        /*
         * snapshot確定後に並行renameをcommitする
         */
        barrier.wait();
        barrier.wait();
        writer.join().unwrap();
        let prefixes = PathPrefixSet::from_iter(["/allowed"]);
        let policy = PageAccessPolicy::new(&prefixes);
        let old = page_in_read_txn(&txn, &page, None, true, &policy).unwrap();
        assert_eq!(
            old.index.authorization_path().unwrap().as_str(),
            "/allowed/page"
        );
        assert_eq!(old.source.unwrap().source(), "original");
        assert!(
            db.read_authorized_page(&page, None, true, &policy)
                .err()
                .unwrap()
                .downcast_ref::<PathPrefixDenied>()
                .is_some()
        );
        drop(txn);
        drop(db);
        fs::remove_dir_all(base).unwrap();
    }

    ///
    /// 不正保存pathを自動変換せず内部不整合として返す
    ///
    /// # 戻り値
    /// テスト結果。
    ///
    /// # 注記
    /// 正常indexのpath表記だけを書き換え、無制約policyでも拒否する。
    ///
    #[test]
    fn malformed_stored_path_is_internal_error() {
        let (base, db) = fixture();
        let page = db
            .create_page("/allowed/page", "user", "source".into())
            .unwrap();
        let txn = db.db.begin_write().unwrap();
        {
            let mut table = txn.open_table(PAGE_INDEX_TABLE).unwrap();
            let mut index = table.get(page.clone()).unwrap().unwrap().value();
            index.set_path("/allowed/../outside".to_string());
            table.insert(page.clone(), index).unwrap();
        }
        txn.commit().unwrap();
        let prefixes = PathPrefixSet::new();
        let err = db
            .read_authorized_page(
                &page,
                None,
                false,
                &PageAccessPolicy::new(&prefixes),
            )
            .err()
            .unwrap();
        assert!(err.downcast_ref::<PathPrefixDenied>().is_none());
        drop(db);
        fs::remove_dir_all(base).unwrap();
    }
    ///
    /// 検索snapshotのpath/deleted/履歴可否をDB正本から確定する
    ///
    /// # 戻り値
    /// テスト結果。
    ///
    /// # 概要
    /// writerを同期させ、検索snapshotの対象状態と履歴存在を照合する。
    ///
    /// # 注記
    /// snapshot後のrename/delete/compactionで旧結果を再解決しない。
    ///
    #[test]
    fn search_snapshot_keeps_state_and_checks_history() {
        /*
         * snapshot後にwriterのrenameをcommitする
         */
        let (base, db) = fixture();
        let page = db
            .create_page("/allow/page", "user", "source".into())
            .unwrap();
        let prefixes = PathPrefixSet::from_iter(["/allow"]);
        let policy = PageAccessPolicy::new(&prefixes);
        let old = db.authorized_search_snapshot(false, &policy).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let barrier_writer = barrier.clone();
            let db = &db;
            scope.spawn(move || {
                barrier_writer.wait();
                db.rename_page("/allow/page", "/outside/page").unwrap();
                barrier_writer.wait();
            });
            barrier.wait(); barrier.wait();
        });
        assert_eq!(old.resolve_result(&page, 1, false).unwrap(),
            ("/allow/page".into(), false));
        assert!(db.authorized_search_snapshot(false, &policy).unwrap()
            .page_ids().is_empty());
        drop(old);

        /*
         * 履歴sourceの存在と削除状態も同snapshotを正本にする
         */
        let page = db
            .create_page("/allow/history", "user", "old".into())
            .unwrap();
        db.put_page(&page, "user", "new".into(), false).unwrap();
        let history = db.authorized_search_snapshot(false, &policy).unwrap();
        assert!(history.resolve_result(&page, 1, false).is_err());
        assert!(history.resolve_result(&page, 1, true).is_ok());
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let barrier_writer = barrier.clone();
            let db = &db;
            let page = &page;
            scope.spawn(move || {
                barrier_writer.wait();
                db.compact_page_source(&page, 2).unwrap();
                db.put_page(&page, "user", "third".into(), false).unwrap();
                db.delete_page_by_id(&page).unwrap();
                barrier_writer.wait();
            });
            barrier.wait(); barrier.wait();
        });
        assert_eq!(history.resolve_result(&page, 2, false).unwrap(),
            ("/allow/history".into(), false));
        assert!(history.resolve_result(&page, 1, true).is_ok());
        let latest = db.authorized_search_snapshot(true, &policy).unwrap();
        assert!(latest.resolve_result(&page, 1, true).is_err());
        drop(history); drop(latest);
        assert!(db.authorized_search_snapshot(false, &policy).unwrap()
            .page_ids().is_empty());
        let deleted = db.authorized_search_snapshot(true, &policy).unwrap();
        assert_eq!(deleted.resolve_result(&page, 3, false).unwrap(),
            ("/allow/history".into(), true));
        drop(deleted); drop(db); fs::remove_dir_all(base).unwrap();
    }

}

///
/// 検索候補の認可と返却状態を決めるread snapshot
///
pub(crate) struct SearchPageSnapshot {
    /// 結果の履歴存在確認まで保持する同じread txn
    txn: ReadTransaction,
    /// 状態条件とprefixを満たすID/index
    pages: std::collections::HashMap<PageId, PageIndex>,
}

impl SearchPageSnapshot {
    ///
    /// 認可済みID集合を取得する
    ///
    /// # 戻り値
    /// 型付きFTSフィルタへ渡す許可ID。
    ///
    pub(crate) fn page_ids(&self) -> Vec<PageId> {
        self.pages.keys().cloned().collect()
    }

    ///
    /// FTS候補を同snapshotの状態・履歴に照合する
    ///
    /// # 引数
    /// * `page_id` - FTS候補ID
    /// * `revision` - 候補revision
    /// * `all_revision` - 履歴も返すか
    ///
    /// # 戻り値
    /// DB正本のpath/deleted、または候補不整合。
    ///
    /// # 概要
    /// FTSのpath/deleted/latestを認可や返却状態の根拠にしない。
    ///
    pub(crate) fn resolve_result(
        &self,
        page_id: &PageId,
        revision: u64,
        all_revision: bool,
    ) -> Result<(String, bool)> {
        /*
         * 許可集合とDBの履歴条件に候補を照合する
         */
        let index = self.pages.get(page_id)
            .ok_or_else(|| {
                anyhow!("search page is not in authorized snapshot")
            })?;
        if (!all_revision && revision != index.latest())
            || revision < index.earliest() || revision > index.latest()
        {
            return Err(anyhow!("search revision is inconsistent"));
        }
        let sources = self.txn.open_table(PAGE_SOURCE_TABLE)?;
        if sources.get((page_id.clone(), revision))?.is_none() {
            return Err(anyhow!("search source is missing"));
        }
        Ok((index.authorization_path()?.as_str().to_string(), index.deleted()))
    }
}

impl DatabaseManager {
    ///
    /// 検索用の許可IDと返却状態を同snapshotで確定する
    ///
    /// # 引数
    /// * `with_deleted` - 削除済みページも対象にするか
    /// * `policy` - 許可prefix
    ///
    /// # 戻り値
    /// 同期検索区間で保持するsnapshot。
    ///
    /// # 概要
    /// 非draftのcurrent/last_deletedから許可IDを選ぶ。awaitを跨がない。
    ///
    pub(crate) fn authorized_search_snapshot(
        &self,
        with_deleted: bool,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<SearchPageSnapshot> {
        /*
         * 状態と保存pathから許可集合を生成する
         */
        let txn = self.db.begin_read()?;
        let mut pages = std::collections::HashMap::new();
        {
            let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
            for entry in indexes.iter()? {
                let (page_id, index) = entry?;
                let index = index.value();
                if index.is_draft() || (index.deleted() && !with_deleted) {
                    continue;
                }
                let path = index.authorization_path()?;
                if policy.allows(path.as_str()) {
                    pages.insert(page_id.value(), index);
                }
            }
        }
        Ok(SearchPageSnapshot { txn, pages })
    }

    ///
    /// path指定の削除候補を同snapshotで認可して取得する
    ///
    /// # 引数
    /// * `path` - 入力解決済み対象path
    /// * `policy` - 許可prefix
    ///
    /// # 戻り値
    /// 同じ削除時pathを持つID一覧、または拒否/内部不整合。
    ///
    /// # 概要
    /// 要求pathを認可しmultimapの各indexとlast_deletedを照合する。
    ///
    pub(crate) fn get_authorized_deleted_page_ids(
        &self,
        path: &str,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<Vec<PageId>> {
        /*
         * 要求最終pathを先に認可する
         */
        if !policy.allows(path) {
            return Err(PathPrefixDenied {
                path: path.to_string(), parent_candidate: false,
            }.into());
        }
        let txn = self.db.begin_read()?;
        let deleted = txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?;
        let mut ids = Vec::new();

        /*
         * 同snapshotの全候補indexを確認する
         */
        for entry in deleted.get(path.to_string())? {
            let page_id = entry?.value();
            let page = page_in_read_txn(&txn, &page_id, None, false, policy)?;
            if !page.index.deleted()
                || page.index.authorization_path()?.as_str() != path
            {
                return Err(anyhow!("deleted path index is inconsistent"));
            }
            ids.push(page_id);
        }
        Ok(ids)
    }
}
