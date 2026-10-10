/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセットの所属ページと返却値を同じsnapshotで認可する
//! DatabaseManagerの操作別実装を読みやすく分離する。
//!

use anyhow::{Result, anyhow};
use redb::{
    ReadTransaction, ReadableDatabase, ReadableMultimapTable, ReadableTable,
    WriteTransaction,
};

use super::DatabaseManager;
use super::authorized_pages::authorize_index;
use super::mutation::authorize_create_path;
use crate::auth::PageAccessPolicy;
use crate::database::DbError;
use crate::database::schema::{
    ASSET_GROUP_TABLE, ASSET_INFO_TABLE, ASSET_LOOKUP_TABLE,
    DELETED_PAGE_PATH_TABLE, PAGE_INDEX_TABLE, PAGE_PATH_TABLE,
    USER_INFO_TABLE,
};
use crate::database::types::{AssetId, AssetInfo, PageId, PageIndex};

///
/// 期待ID固定と同txnのpath解決を区別するページ指定
///
#[derive(Clone, Copy)]
pub(crate) enum AssetPageSelector<'a> {
    /// REST ID/MCP read時点の期待ID
    Id(&'a PageId),
    /// 入力解決済み要求path
    Path(&'a str),
}

///
/// アセット参照指定
///
#[derive(Clone, Copy)]
pub(crate) enum AssetSelector<'a> {
    /// 直接ID指定
    Id(&'a AssetId),
    /// 同snapshotのpage/file lookup
    File {
        page: AssetPageSelector<'a>,
        file_name: &'a str,
    },
}

///
/// bytes読取の必要性と既存MCP上限を指定する
///
#[derive(Clone, Copy)]
pub(crate) enum AssetReadMode {
    /// metadata/list/redirectではbytesを読まない
    Metadata,
    /// 認可済みsnapshotを保持したbytes読取
    Data { max_bytes: Option<u64> },
}

///
/// 同snapshotで確定したアセットmetadata
///
#[derive(Clone, Debug)]
pub(crate) struct AssetRecord {
    /// 正常な所属を確認したmetadata
    pub(crate) info: AssetInfo,
    /// 同snapshotのユーザ名。孤立IDは既存unknown
    pub(crate) user_name: String,
}

///
/// 所属認可とmetadata/bytesを同snapshotにまとめた値
///
pub(crate) struct AuthorizedAssetRead {
    /// 所属先index
    pub(crate) owner: PageIndex,
    /// アセットmetadata
    pub(crate) record: AssetRecord,
    /// data要求時だけ取得するbytes
    pub(crate) data: Option<Vec<u8>>,
}

///
/// 既存MCPのdata上限拒否
///
#[derive(Debug)]
pub(crate) struct AssetDataTooLarge;

impl std::fmt::Display for AssetDataTooLarge {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("asset data too large")
    }
}

impl std::error::Error for AssetDataTooLarge {}

///
/// 同snapshotのpage selectorを解決し認可する
///
/// # 引数
/// * `paths` - current path table
/// * `deleted` - deleted path table
/// * `indexes` - index table
/// * `selector` - 固定IDまたは最終path
/// * `policy` - 保存prefix制約
/// * `owner_required` - asset FK先の不在を内部不整合とするか
///
/// # 戻り値
/// 認可済みID/index。
///
/// # 概要
/// pathを先に認可し、ID不在とowner FK不整合を区別する。
///
fn page_from_tables(
    paths: &impl ReadableTable<String, PageId>,
    deleted: &impl ReadableMultimapTable<String, PageId>,
    indexes: &impl ReadableTable<PageId, PageIndex>,
    selector: AssetPageSelector<'_>,
    policy: &PageAccessPolicy<'_>,
    owner_required: bool,
) -> Result<(PageId, PageIndex)> {
    let page_id = match selector {
        AssetPageSelector::Id(page_id) => page_id.clone(),
        AssetPageSelector::Path(path) => {
            /*
             * 要求最終pathの認可が存在/削除lookupより先
             */
            authorize_create_path(path, policy)?;
            match paths.get(path.to_string())? {
                Some(entry) => entry.value(),
                None => {
                    for entry in deleted.get(path.to_string())? {
                        let id = entry?.value();
                        let index = indexes
                            .get(id)?
                            .ok_or_else(|| {
                                anyhow!("deleted owner index missing")
                            })?
                            .value();
                        authorize_index(&index, policy)?;
                        if !index.deleted()
                            || index.authorization_path()?.as_str() != path
                        {
                            return Err(anyhow!(
                                "deleted owner path is inconsistent"
                            ));
                        }
                        return Err(anyhow!(DbError::PageDeleted));
                    }
                    return Err(anyhow!(DbError::PageNotFound));
                }
            }
        }
    };
    let index = match indexes.get(page_id.clone())? {
        Some(entry) => entry.value(),
        None if owner_required => {
            return Err(anyhow!("asset owner index missing"));
        }
        None => return Err(anyhow!(DbError::PageNotFound)),
    };
    if index.id() != page_id {
        return Err(anyhow!("owner embedded ID mismatch"));
    }
    authorize_index(&index, policy)?;
    if let AssetPageSelector::Path(path) = selector {
        if index.authorization_path()?.as_str() != path || index.deleted() {
            return Err(anyhow!("owner path index is inconsistent"));
        }
    }
    Ok((page_id, index))
}

///
/// read txnから所属ページを解決する
///
/// # 引数
/// * `txn` - snapshot
/// * `selector` - ページ指定
/// * `policy` - 保存prefix制約
/// * `owner_required` - FK不整合の区別
///
/// # 戻り値
/// 認可済みID/index。
///
fn page_in_read_txn(
    txn: &ReadTransaction,
    selector: AssetPageSelector<'_>,
    policy: &PageAccessPolicy<'_>,
    owner_required: bool,
) -> Result<(PageId, PageIndex)> {
    page_from_tables(
        &txn.open_table(PAGE_PATH_TABLE)?,
        &txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?,
        &txn.open_table(PAGE_INDEX_TABLE)?,
        selector,
        policy,
        owner_required,
    )
}

///
/// write txnから所属ページを解決する
///
/// # 引数
/// * `txn` - 操作txn
/// * `selector` - 固定IDまたは最終path
/// * `policy` - 保存prefix制約
/// * `owner_required` - FK不整合の区別
///
/// # 戻り値
/// 認可済みID/index。
///
pub(super) fn page_in_write_txn(
    txn: &WriteTransaction,
    selector: AssetPageSelector<'_>,
    policy: &PageAccessPolicy<'_>,
    owner_required: bool,
) -> Result<(PageId, PageIndex)> {
    page_from_tables(
        &txn.open_table(PAGE_PATH_TABLE)?,
        &txn.open_multimap_table(DELETED_PAGE_PATH_TABLE)?,
        &txn.open_table(PAGE_INDEX_TABLE)?,
        selector,
        policy,
        owner_required,
    )
}

///
/// 要求page/fileとasset ownerを一致させて名前を解決する
///
/// # 引数
/// * `lookup` - 名前lookup table
/// * `groups` - 所属table
/// * `infos` - metadata table
/// * `page_id` - 認可した要求owner
/// * `file_name` - 既存file名規則の入力
///
/// # 戻り値
/// 正常lookup metadata、または削除/不在/内部不整合。
///
/// # 概要
/// lookupの一致を確認し、履歴groupは現在owner候補だけを扱う。
///
fn file_from_tables(
    lookup: &impl ReadableTable<(PageId, String), AssetId>,
    groups: &impl ReadableMultimapTable<PageId, AssetId>,
    infos: &impl ReadableTable<AssetId, AssetInfo>,
    page_id: &PageId,
    file_name: &str,
) -> Result<AssetInfo> {
    /*
     * 正常lookupのmetadataと要求owner/nameを照合する
     */
    if let Some(entry) = lookup.get((page_id.clone(), file_name.to_string()))? {
        let asset_id = entry.value();
        let info = infos
            .get(asset_id.clone())?
            .ok_or_else(|| anyhow!(AssetLookupMetadataMissing))?
            .value();
        if info.id() != asset_id {
            return Err(anyhow!("asset metadata ID mismatch"));
        }
        if info.page_id().is_none() {
            return Err(anyhow!(DbError::AssetDeleted));
        }
        if info.page_id() != Some(page_id.clone())
            || info.file_name() != file_name
        {
            return Err(AssetLookupMismatch.into());
        }
        return Ok(info);
    }
    /*
     * 履歴groupから現在ownerの削除候補だけを解決する
     */
    for entry in groups.get(page_id.clone())? {
        let asset_id = entry?.value();
        let info = infos
            .get(asset_id.clone())?
            .ok_or_else(|| anyhow!(AssetLookupMetadataMissing))?
            .value();
        if info.id() != asset_id {
            return Err(anyhow!("asset metadata ID mismatch"));
        }
        // groupは削除/移動履歴を保持するため、別ownerへ追跡しない。
        if info.page_id().is_some_and(|owner| owner != *page_id) {
            continue;
        }
        if info.file_name() == file_name && (info.deleted() || info.is_zombie())
        {
            return Err(anyhow!(DbError::AssetDeleted));
        }
    }
    Err(anyhow!(DbError::AssetNotFound))
}

impl DatabaseManager {
    ///
    /// 所属認可とmetadata/bytesを同じread snapshotで取得する
    ///
    /// # 引数
    /// * `selector` - IDまたはpage/file指定
    /// * `policy` - 保存prefix制約
    /// * `mode` - bytesの必要性/既存上限
    ///
    /// # 戻り値
    /// 認可済み所属・metadata・必要bytes。
    ///
    /// # 概要
    /// zombieは一律410相当、owner欠損は内部不整合、状態より先に認可する。
    ///
    pub(crate) fn read_asset_authorized(
        &self,
        selector: AssetSelector<'_>,
        policy: &PageAccessPolicy<'_>,
        mode: AssetReadMode,
    ) -> Result<AuthorizedAssetRead> {
        let txn = self.db.begin_read()?;
        let infos = txn.open_table(ASSET_INFO_TABLE)?;
        /*
         * 同じsnapshotのownerを解決し、返却状態より先に認可する
         */
        let (owner, info) = match selector {
            AssetSelector::Id(asset_id) => {
                let info = infos
                    .get(asset_id.clone())?
                    .ok_or_else(|| anyhow!(DbError::AssetNotFound))?
                    .value();
                if info.id() != *asset_id {
                    return Err(anyhow!("asset metadata ID mismatch"));
                }
                let owner_id = info
                    .page_id()
                    .ok_or_else(|| anyhow!(DbError::AssetDeleted))?;
                let (_, owner) = page_in_read_txn(
                    &txn,
                    AssetPageSelector::Id(&owner_id),
                    policy,
                    true,
                )?;
                (owner, info)
            }
            AssetSelector::File { page, file_name } => {
                let (page_id, owner) =
                    page_in_read_txn(&txn, page, policy, false)?;
                let known = asset_read_target(&owner, None)?;
                if owner.deleted() {
                    return Err(anyhow!(DbError::PageDeleted).context(known));
                }
                let info = file_from_tables(
                    &txn.open_table(ASSET_LOOKUP_TABLE)?,
                    &txn.open_multimap_table(ASSET_GROUP_TABLE)?,
                    &infos,
                    &page_id,
                    file_name,
                )
                .map_err(|err| err.context(known))?;
                (owner, info)
            }
        };
        let known = asset_read_target(&owner, Some(&info))?;
        let result = (|| -> Result<AuthorizedAssetRead> {
            if owner.deleted() || info.deleted() || info.is_zombie() {
                return Err(anyhow!(DbError::AssetDeleted));
            }
            let users = txn.open_table(USER_INFO_TABLE)?;
            let user_name = users
                .get(info.user())?
                .map(|entry| entry.value().username())
                .unwrap_or_else(|| "unknown".into());
            /*
             * data要求だけ、認可済みsnapshotを保持してbytesを読む
             */
            let data = match mode {
                AssetReadMode::Metadata => None,
                AssetReadMode::Data { max_bytes } => {
                    if max_bytes.is_some_and(|limit| info.size() > limit) {
                        return Err(AssetDataTooLarge.into());
                    }
                    let bytes = self
                        .read_asset_data(&info.id())
                        .map_err(|err| err.context(AssetBodyReadFailure))?;
                    if max_bytes.is_some_and(|limit| bytes.len() as u64 > limit)
                    {
                        return Err(AssetDataTooLarge.into());
                    }
                    Some(bytes)
                }
            };
            Ok(AuthorizedAssetRead {
                owner,
                record: AssetRecord { info, user_name },
                data,
            })
        })();
        result.map_err(|err| err.context(known))
    }

    ///
    /// 同snapshotのページ認可後に一覧DTOを取得する
    ///
    /// # 引数
    /// * `page` - 固定IDまたは最終path
    /// * `policy` - 保存prefix制約
    ///
    /// # 戻り値
    /// ownerと正常非削除asset DTO一覧。
    ///
    /// # 概要
    /// groupの別ownerを追跡せず、既存user名fallbackを同snapshotで維持する。
    ///
    pub(crate) fn list_assets_authorized(
        &self,
        page: AssetPageSelector<'_>,
        policy: &PageAccessPolicy<'_>,
    ) -> Result<(PageIndex, Vec<AssetRecord>)> {
        /*
         * 同snapshotのowner認可後に候補を取得する
         */
        let txn = self.db.begin_read()?;
        let (page_id, owner) = page_in_read_txn(&txn, page, policy, false)?;
        let known = asset_read_target(&owner, None)?;
        let result = (|| -> Result<_> {
            if owner.deleted() {
                return Err(anyhow!(DbError::PageDeleted));
            }
            let groups = txn.open_multimap_table(ASSET_GROUP_TABLE)?;
            let infos = txn.open_table(ASSET_INFO_TABLE)?;
            let users = txn.open_table(USER_INFO_TABLE)?;
            let mut assets = Vec::new();
            /*
             * 候補のID/所属を照合して返却DTOを構築する
             */
            for entry in groups.get(page_id.clone())? {
                let asset_id = entry?.value();
                let info = infos
                    .get(asset_id.clone())?
                    .ok_or_else(|| anyhow!("asset group metadata missing"))?
                    .value();
                if info.id() != asset_id {
                    return Err(anyhow!("asset metadata ID mismatch"));
                }
                if info.deleted() || info.is_zombie() {
                    continue;
                }
                // 旧groupの履歴関係から他pageのmetadataを返さない。
                if info.page_id() != Some(page_id.clone()) {
                    continue;
                }
                let user_name = users
                    .get(info.user())?
                    .map(|entry| entry.value().username())
                    .unwrap_or_else(|| "unknown".into());
                assets.push(AssetRecord { info, user_name });
            }
            Ok((owner, assets))
        })();
        result.map_err(|err| err.context(known))
    }
}

///
/// 回収に失敗した新規fileの診断
///
#[derive(Debug)]
pub(crate) struct AssetFileRecoveryError {
    /// body/token/storage pathを含めないIO診断
    pub(crate) reason: String,
}

impl std::fmt::Display for AssetFileRecoveryError {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "new asset file recovery failed: {}", self.reason)
    }
}

///
/// commitを試行する前に確定したupload失敗
///
#[derive(Debug)]
pub(crate) struct AssetUploadPreCommit;

impl std::fmt::Display for AssetUploadPreCommit {
    ///
    /// 固定失敗段階の診断を返す
    ///
    /// # 引数
    /// * `formatter` - 診断出力先
    ///
    /// # 戻り値
    /// 書込結果。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>)
        -> std::fmt::Result
    {
        formatter.write_str("asset upload failed before commit")
    }
}

impl std::error::Error for AssetUploadPreCommit {}

///
/// 自分がcreate_newで作ったfileだけを回収するguard
///
struct NewAssetFile {
    /// 自分が作成した新規file
    path: std::path::PathBuf,
    /// commit成功まで回収対象とする
    armed: bool,
}

impl NewAssetFile {
    ///
    /// 新規fileを排他的に作成する
    ///
    /// # 引数
    /// * `path` - 新AssetIDに対応する保存先
    ///
    /// # 戻り値
    /// 新規file handleとguard。既存fileがある場合は所有しない。
    ///
    fn create(path: &std::path::Path) -> Result<(std::fs::File, Self)> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        Ok((
            file,
            Self {
                path: path.to_path_buf(),
                armed: true,
            },
        ))
    }

    ///
    /// commit成功で所有fileを回収対象から外す
    ///
    fn committed(&mut self) {
        self.armed = false;
    }

    ///
    /// 元失敗と回収診断を保持する
    ///
    /// # 引数
    /// * `err` - 登録/保存/commitの失敗
    ///
    /// # 戻り値
    /// 元エラー、回収失敗時は型付き診断contextを付けたエラー。
    ///
    fn recover(mut self, err: anyhow::Error) -> anyhow::Error {
        self.armed = false;
        match std::fs::remove_file(&self.path) {
            Ok(()) => err,
            Err(io) if io.kind() == std::io::ErrorKind::NotFound => err,
            Err(io) => {
                let diagnostic = AssetFileRecoveryError {
                    reason: io.to_string(),
                };
                log::warn!("{}", diagnostic);
                err.context(diagnostic)
            }
        }
    }
}

impl Drop for NewAssetFile {
    fn drop(&mut self) {
        if self.armed {
            if let Err(err) = std::fs::remove_file(&self.path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("new asset file recovery failed: {}", err);
                }
            }
        }
    }
}

///
/// 同txnで直接asset IDまたは期待page/fileを解決する
///
/// # 引数
/// * `txn` - 操作txn
/// * `selector` - 固定ID/owner指定
/// * `policy` - 保存prefix制約
///
/// # 戻り値
/// ownerとmetadata。zombieにはowner lock条件を仮定しない。
///
/// # 概要
/// 同write txnで固定IDまたは要求ownerを認可してmetadataを返す。
///
fn asset_in_write_txn(
    txn: &WriteTransaction,
    selector: AssetSelector<'_>,
    policy: &PageAccessPolicy<'_>,
) -> Result<(PageIndex, AssetInfo)> {
    /*
     * 同txnのasset IDとownerを確定して認可する
     */
    let infos = txn.open_table(ASSET_INFO_TABLE)?;
    match selector {
        AssetSelector::Id(asset_id) => {
            let info = infos
                .get(asset_id.clone())?
                .ok_or_else(|| anyhow!(DbError::AssetNotFound))?
                .value();
            if info.id() != *asset_id {
                return Err(anyhow!("asset metadata ID mismatch"));
            }
            let page_id = info
                .page_id()
                .ok_or_else(|| anyhow!(DbError::AssetDeleted))?;
            let (_, owner) = page_in_write_txn(
                &txn,
                AssetPageSelector::Id(&page_id),
                policy,
                true,
            )?;
            Ok((owner, info))
        }
        AssetSelector::File { page, file_name } => {
            let (page_id, owner) =
                page_in_write_txn(&txn, page, policy, false)?;
            let known = asset_read_target(&owner, None)?;
            if owner.deleted() {
                return Err(anyhow!(DbError::PageDeleted).context(known));
            }
            let info = file_from_tables(
                &txn.open_table(ASSET_LOOKUP_TABLE)?,
                &txn.open_multimap_table(ASSET_GROUP_TABLE)?,
                &infos,
                &page_id,
                file_name,
            )
            .map_err(|err| err.context(known))?;
            Ok((owner, info))
        }
    }
}

impl DatabaseManager {
    ///
    /// 最新owner認可後にFS保存とmetadata登録を行う
    ///
    /// # 引数
    /// * `page` - 固定期待IDまたは要求最終path
    /// * `file_name` - 既存file名入力
    /// * `mime` - 既存MIME入力
    /// * `user_name` - 登録ユーザ
    /// * `data` - payload読取済みbytes
    /// * `access` - policy/既存lock条件
    ///
    /// # 戻り値
    /// commit時metadataと確定owner/commit情報。
    ///
    /// # 概要
    /// 拒否前にdirectory/fileを作らず、新規fileだけを失敗時に回収する。
    ///
    pub(crate) fn create_asset_authorized(
        &self,
        page: AssetPageSelector<'_>,
        file_name: &str,
        mime: &str,
        user_name: &str,
        data: &[u8],
        access: &super::mutation::MutationAccess<'_>,
    ) -> Result<super::mutation::AuthorizedMutation<AssetRecord>> {
        use super::mutation::{AuthorizedMutation, mutation_outcome_in_txn};
        use std::io::Write;
        /*
         * 同write txnでownerを確定し全条件をFSより前に処理する
         */
        let txn = self.db.begin_write()?;
        let (page_id, owner) =
            page_in_write_txn(&txn, page, &access.policy, false)?;
        let known = asset_read_target(&owner, None)?;
        let result = (|| -> Result<AuthorizedMutation<AssetRecord>> {
            let prepared = self
                .prepare_mutation_in_txn(&txn, &page_id, access)
                .map_err(|err| err.context(known.clone()))?;
            if prepared.index.is_none() {
                return self.complete_expired_mutation(txn, &page_id, prepared);
            }
            self.validate_asset_create_in_txn(
                &txn, &page_id, file_name, user_name,
            )
            .map_err(|err| err.context(known.clone()))?;
            let asset_id = AssetId::new();
            let mut known = known.clone();
            known.asset_id = Some(asset_id.clone());
            let asset_path = self.asset_file_path(&asset_id);
            if let Some(parent) = asset_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| anyhow!(err).context(known.clone()))?;
            }
            let (mut file, mut guard) = NewAssetFile::create(&asset_path)
                .map_err(|err| err.context(known.clone()))?;
            let saved = file.write_all(data);
            drop(file);
            if let Err(err) = saved {
                return Err(guard.recover(anyhow!(err).context(known.clone())
                    .context(AssetUploadPreCommit)));
            }

            /*
             * 登録/commit失敗では当該新fileだけを回収する
             */
            #[cfg(test)]
            if self
                .fail_asset_upload_for_test
                .load(std::sync::atomic::Ordering::SeqCst)
                == 1
            {
                return Err(guard.recover(
                    anyhow!("injected asset registration failure")
                        .context(known.clone()).context(AssetUploadPreCommit),
                ));
            }
            #[cfg(test)]
            if self.fail_asset_upload_for_test.load(
                std::sync::atomic::Ordering::SeqCst,
            ) == 3 {
                // 自分の新fileだけをdirectoryへ替え、回収失敗を注入する。
                std::fs::remove_file(&asset_path)?;
                std::fs::create_dir(&asset_path)?;
                return Err(guard.recover(
                    anyhow!("injected asset recovery failure")
                        .context(known.clone()).context(AssetUploadPreCommit),
                ));
            }
            let info = match self.register_asset_in_txn(
                &txn,
                &asset_id,
                &page_id,
                file_name,
                mime,
                user_name,
                data.len() as u64,
            ) {
                Ok(info) => info,
                Err(err) => {
                    return Err(guard.recover(err.context(known.clone())
                        .context(AssetUploadPreCommit)));
                }
            };
            let users = txn.open_table(USER_INFO_TABLE)?;
            let user_name = users
                .get(info.user())?
                .map(|entry| entry.value().username())
                .unwrap_or_else(|| "unknown".into());
            drop(users);
            let mut outcome = mutation_outcome_in_txn(
                &txn,
                &page_id,
                &prepared.source_path,
                vec![page_id.clone()],
            )?;
            outcome.asset_id = Some(info.id());
            #[cfg(test)]
            if self
                .fail_asset_upload_for_test
                .load(std::sync::atomic::Ordering::SeqCst)
                == 2
            {
                return Err(guard.recover(
                    anyhow!("injected asset commit failure")
                        .context(outcome.clone())
                        .context(known.clone()).context(AssetUploadPreCommit),
                ));
            }
            if let Err(err) = txn.commit() {
                return Err(guard.recover(
                    anyhow!(err)
                        .context(outcome.clone())
                        .context(known.clone()),
                ));
            }
            guard.committed();
            outcome.committed = true;
            self.finish_mutation_assets(&prepared.asset_cleanup, &outcome)?;
            Ok(AuthorizedMutation {
                value: AssetRecord { info, user_name },
                outcome,
            })
        })();
        result.map_err(|err| {
            if err.downcast_ref::<AssetAccessTarget>().is_some() {
                err
            } else {
                err.context(known)
            }
        })
    }

    ///
    /// 最新owner認可とsoft deleteを同write txnで行う
    ///
    /// # 引数
    /// * `selector` - 固定asset IDまたは期待page/file
    /// * `access` - policy/操作固有lock条件
    ///
    /// # 戻り値
    /// 削除metadataと確定owner/commit。
    ///
    /// # 概要
    /// zombie/owner欠損を区別し、拒否時にcleanupやmetadataを変更しない。
    ///
    pub(crate) fn delete_asset_authorized(
        &self,
        selector: AssetSelector<'_>,
        access: &super::mutation::MutationAccess<'_>,
    ) -> Result<super::mutation::AuthorizedMutation<AssetRecord>> {
        use super::mutation::{AuthorizedMutation, mutation_outcome_in_txn};
        let txn = self.db.begin_write()?;
        let (owner, mut info) =
            asset_in_write_txn(&txn, selector, &access.policy)?;
        let known = asset_read_target(&owner, Some(&info))?;
        let result = (|| -> Result<_> {
            let page_id = info
                .page_id()
                .ok_or_else(|| anyhow!(DbError::AssetDeleted))?;
            /*
             * asset状態410より先に、成立するowner lock403を判定する
             */
            let state_access = super::mutation::MutationAccess {
                policy: access.policy,
                lock: match &access.lock {
                    super::mutation::MutationLock::Token {
                        user_name,
                        token,
                        validate_unlocked,
                        invalid_token_reason,
                    } => super::mutation::MutationLock::Token {
                        user_name,
                        token: token.clone(),
                        validate_unlocked: *validate_unlocked,
                        invalid_token_reason,
                    },
                    super::mutation::MutationLock::Unlocked => {
                        super::mutation::MutationLock::Unlocked
                    }
                    super::mutation::MutationLock::Unchecked => {
                        super::mutation::MutationLock::Unchecked
                    }
                },
                allow_draft: access.allow_draft,
                allow_deleted: true,
                cleanup_draft: false,
                expected_version: None,
            };
            // 状態拒否はcleanupより前。準備段階に余計な期限切れ整理をしない。
            if owner.deleted() || info.deleted() {
                self.check_asset_forbidden_lock_in_txn(
                    &txn,
                    &page_id,
                    &state_access,
                )?;
                return Err(anyhow!(DbError::AssetDeleted));
            }
            let prepared =
                self.prepare_mutation_in_txn(&txn, &page_id, access)?;
            if prepared.index.is_none() {
                return self
                    .complete_expired_mutation(txn, &page_id, prepared)
                    .map_err(|err| {
                        if let Some(outcome) = err
                            .downcast_ref::<super::mutation::MutationOutcome>(
                        ) {
                            if outcome.committed
                                && !outcome.post_processing_failure
                            {
                                return anyhow!(DbError::AssetNotFound)
                                    .context(outcome.clone());
                            }
                        }
                        err
                    });
            }
            let attempted = mutation_outcome_in_txn(
                &txn,
                &page_id,
                &prepared.source_path,
                vec![page_id.clone()],
            )?;
            self.delete_asset_in_txn(&txn, &info.id())
                .map_err(|err| err.context(attempted))?;
            info.set_deleted(true);
            let users = txn.open_table(USER_INFO_TABLE)?;
            let user_name = users
                .get(info.user())?
                .map(|entry| entry.value().username())
                .unwrap_or_else(|| "unknown".into());
            drop(users);
            let mut outcome = mutation_outcome_in_txn(
                &txn,
                &page_id,
                &prepared.source_path,
                vec![page_id.clone()],
            )?;
            outcome.asset_id = Some(info.id());
            txn.commit()
                .map_err(|err| anyhow!(err).context(outcome.clone()))?;
            outcome.committed = true;
            self.finish_mutation_assets(&prepared.asset_cleanup, &outcome)?;
            Ok(AuthorizedMutation {
                value: AssetRecord { info, user_name },
                outcome,
            })
        })();
        result.map_err(|err| err.context(known))
    }

    ///
    /// 410状態で実在active lockの403条件だけを確認する
    ///
    /// # 引数
    /// * `txn` - 操作txn
    /// * `page_id` - 認可済みowner
    /// * `access` - 既存token条件
    ///
    /// # 戻り値
    /// 成立する403のみ拒否。cleanup/423を行わない。
    ///
    /// # 概要
    /// cleanupせず、実在する有効lockのtoken/ownerだけを確認する。
    ///
    fn check_asset_forbidden_lock_in_txn(
        &self,
        txn: &WriteTransaction,
        page_id: &PageId,
        access: &super::mutation::MutationAccess<'_>,
    ) -> Result<()> {
        let super::mutation::MutationLock::Token {
            user_name,
            token,
            invalid_token_reason,
            ..
        } = &access.lock
        else {
            return Ok(());
        };
        /*
         * ownerの実在lockをcleanup無しで取得する
         */
        let indexes = txn.open_table(PAGE_INDEX_TABLE)?;
        let index = indexes
            .get(page_id.clone())?
            .ok_or_else(|| anyhow!("asset owner index missing"))?
            .value();
        drop(indexes);
        let locks = txn.open_table(crate::database::schema::LOCK_INFO_TABLE)?;
        let lock = if index.is_draft() {
            crate::database::txn_helpers::find_lock_by_page(&locks, page_id)?
                .map(|(_, lock)| lock)
        } else if let Some(lock_token) = index.lock_token() {
            locks.get(lock_token)?.map(|entry| entry.value())
        } else {
            None
        };
        /*
         * 有効lockに成立するtoken/owner拒否だけを適用する
         */
        if let Some(lock) =
            lock.filter(|lock| lock.expire() > chrono::Local::now())
        {
            if let Some(token) = token.as_ref().map_err(|err| *err)? {
                if *token != &lock.token() {
                    return Err(super::locks::LockTokenInputError {
                        reason: invalid_token_reason,
                    }
                    .into());
                }
                let users =
                    txn.open_table(crate::database::schema::USER_ID_TABLE)?;
                let user = users
                    .get(user_name.to_string())?
                    .ok_or_else(|| anyhow!(DbError::UserNotFound))?
                    .value();
                if user != lock.user() {
                    return Err(anyhow!(DbError::LockForbidden));
                }
            }
        }
        Ok(())
    }
}

///
/// MS4の応答後処理へ渡す認可済みread対象（bytesを含めない）
///
#[derive(Clone, Debug)]
pub(crate) struct AssetAccessTarget {
    /// 実owner ID
    pub(crate) page_id: PageId,
    /// snapshotで認可したowner path
    pub(crate) path: String,
    /// 単独asset時のID。一覧はNone
    pub(crate) asset_id: Option<AssetId>,
    /// ownerのsnapshot revision
    pub(crate) revision: Option<u64>,
    /// assetのキャッシュ識別子
    pub(crate) instance_id: Option<String>,
}

impl std::fmt::Display for AssetAccessTarget {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "asset owner resolved at {}", self.path)
    }
}

///
/// 認可済みsnapshot値からread対象を作る
///
/// # 引数
/// * `owner` - 同snapshotのindex
/// * `asset` - 単独asset metadata、一覧はNone
///
/// # 戻り値
/// body/token/storage pathを含めないread対象。
///
pub(crate) fn asset_read_target(
    owner: &PageIndex,
    asset: Option<&AssetInfo>,
) -> Result<AssetAccessTarget> {
    Ok(AssetAccessTarget {
        page_id: owner.id(),
        path: owner.authorization_path()?.as_str().to_string(),
        asset_id: asset.map(|info| info.id()),
        revision: if owner.is_draft() {
            None
        } else {
            Some(owner.latest())
        },
        instance_id: asset
            .and_then(|info| info.instance_id())
            .map(|id| id.to_string()),
    })
}

///
/// MCPの既存秘匿NotFoundとREST内部不整合を区別するlookup不一致
///
#[derive(Debug)]
pub(crate) struct AssetLookupMismatch;

impl std::fmt::Display for AssetLookupMismatch {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("asset lookup owner/name is inconsistent")
    }
}

impl std::error::Error for AssetLookupMismatch {}

///
/// lookupが指したmetadataの欠損
///
#[derive(Debug)]
pub(crate) struct AssetLookupMetadataMissing;

impl std::fmt::Display for AssetLookupMetadataMissing {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("asset lookup metadata missing")
    }
}

impl std::error::Error for AssetLookupMetadataMissing {}

///
/// 認可後のbytes読取失敗
///
#[derive(Debug)]
pub(crate) struct AssetBodyReadFailure;

impl std::fmt::Display for AssetBodyReadFailure {
    ///
    /// 内部境界情報の診断文字列を生成する
    ///
    /// # 引数
    /// * `f` - 出力先formatter
    ///
    /// # 戻り値
    /// 出力結果。
    ///
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("asset read failed")
    }
}

#[cfg(test)]
mod file_guard_tests {
    use super::{AssetFileRecoveryError, NewAssetFile};

    ///
    /// create_new失敗では既存fileを回収しない
    ///
    /// # 戻り値
    /// テスト結果。
    ///
    /// # 注記
    /// 同じ保存先へ作成を試み、元bytesを確認する。
    ///
    #[test]
    fn existing_file_is_never_owned_by_new_file_guard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asset");
        std::fs::write(&path, b"existing").unwrap();
        assert!(NewAssetFile::create(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"existing");
    }

    ///
    /// file回収失敗の診断を元エラーと保持する
    ///
    /// # 戻り値
    /// テスト結果。
    ///
    /// # 注記
    /// 新fileをdirectoryへ置き換えてremove_file失敗を固定的に起こす。
    ///
    #[test]
    fn failed_recovery_keeps_typed_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asset");
        let (file, guard) = NewAssetFile::create(&path).unwrap();
        drop(file);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let err = guard.recover(anyhow::anyhow!("registration failed"));
        assert!(err.downcast_ref::<AssetFileRecoveryError>().is_some());
        assert!(
            err.chain()
                .any(|cause| cause.to_string() == "registration failed")
        );
        assert!(path.is_dir());
    }
}
