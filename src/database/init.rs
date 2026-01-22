/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! データベース初期化処理を提供するモジュール
//!

use anyhow::{Context, Result};
use redb::Database;

use super::schema::{
    ASSET_GROUP_TABLE, ASSET_INFO_TABLE, ASSET_LOOKUP_TABLE,
    DELETED_PAGE_PATH_TABLE, LOCK_INFO_TABLE, PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE, PAGE_SOURCE_TABLE, USER_ID_TABLE, USER_INFO_TABLE,
};

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
pub(in crate::database) fn init_database(db: &mut Database) -> Result<()> {
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
