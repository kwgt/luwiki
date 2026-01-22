/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベースのスキーマ定義と共通定数を集約するモジュール
//!

use redb::{MultimapTableDefinition, TableDefinition};

use crate::database::types::{
    AssetId, AssetInfo, LockInfo, LockToken, PageId, PageIndex, PageSource,
    UserId, UserInfo,
};

/// ページパスインデックステーブル (ページパス => ページID)
pub(in crate::database) static PAGE_PATH_TABLE:
    TableDefinition<String, PageId> = TableDefinition::new("page_path_table");

/// 削除済みページパスインデックステーブル (ページパス => ページID)
pub(in crate::database) static DELETED_PAGE_PATH_TABLE:
    MultimapTableDefinition<String, PageId> =
    MultimapTableDefinition::new("deleted_page_path_table");

/// ページインデックステーブル (ページID => ページインデックス情報)
pub(in crate::database) static PAGE_INDEX_TABLE:
    TableDefinition<PageId, PageIndex> =
    TableDefinition::new("page_index_table");

/// ページソーステーブル (ページID,リビジョン番号 => ページソース情報)
pub(in crate::database) static PAGE_SOURCE_TABLE:
    TableDefinition<(PageId, u64), PageSource> =
    TableDefinition::new("page_source_table");

/// ロック情報テーブル (ロック解除トークン => ロック情報)
pub(in crate::database) static LOCK_INFO_TABLE:
    TableDefinition<LockToken, LockInfo> =
    TableDefinition::new("lock_info_table");

/// アセット情報テーブル (アセットID => アセット情報)
pub(in crate::database) static ASSET_INFO_TABLE:
    TableDefinition<AssetId, AssetInfo> =
    TableDefinition::new("asset_info_table");

/// アセットID特定テーブル (ページID,ファイル名 => アセットID)
pub(in crate::database) static ASSET_LOOKUP_TABLE:
    TableDefinition<(PageId, String), AssetId> =
    TableDefinition::new("asset_lookup_table");

/// ページ所属アセット群取得テーブル (ページID => [アセットID])
pub(in crate::database) static ASSET_GROUP_TABLE:
    MultimapTableDefinition<PageId, AssetId> =
    MultimapTableDefinition::new("asset_group_table");

/// ユーザIDテーブル (ユーザ名 => ユーザID)
pub(in crate::database) static USER_ID_TABLE:
    TableDefinition<String, UserId> =
    TableDefinition::new("user_id_table");

/// ユーザ情報テーブル (ユーザID => ユーザ情報)
pub(in crate::database) static USER_INFO_TABLE:
    TableDefinition<UserId, UserInfo> =
    TableDefinition::new("user_info_table");

/// ルートページのパス
pub(in crate::database) const ROOT_PAGE_PATH: &str = "/";

/// ルートページ雛形ソース
pub(in crate::database) static DEFAULT_ROOT_SOURCE: &str =
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

    /// ページが削除済み
    PageDeleted,

    /// リビジョン指定が不正
    InvalidRevision,

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
            DbError::PageDeleted => write!(f, "page deleted"),
            DbError::InvalidRevision => write!(f, "invalid revision"),
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
pub(in crate::database) fn is_root_path(path: &str) -> bool {
    path == ROOT_PAGE_PATH
}
