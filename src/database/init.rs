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
    ASSET_GROUP_TABLE,
    ASSET_INFO_TABLE,
    ASSET_LOOKUP_TABLE,
    BEARER_TOKEN_ID_TABLE,
    BEARER_TOKEN_TABLE,
    DELETED_PAGE_PATH_TABLE,
    LOCK_INFO_TABLE,
    MCP_PRIMITIVE_NAME_STATE_TABLE,
    MCP_PRIMITIVE_NAME_TABLE,
    PAGE_INDEX_TABLE,
    PAGE_PATH_TABLE,
    PAGE_SOURCE_TABLE,
    PROMPT_CANDIDATE_TABLE,
    RESOURCE_CANDIDATE_TABLE,
    RESOURCE_URI_INDEX_STATE_TABLE,
    RESOURCE_URI_INDEX_TABLE,
    TEMPLATE_CANDIDATE_TABLE,
    USER_ID_TABLE,
    USER_INFO_TABLE,
};
use super::primitive_names::initialize_mcp_primitive_names_in_txn;

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
///  - TEMPLATE_CANDIDATE_TABLE: テンプレート候補テーブル
///  - PROMPT_CANDIDATE_TABLE: prompt候補テーブル
///  - RESOURCE_CANDIDATE_TABLE: resource候補テーブル
///  - MCP_PRIMITIVE_NAME_TABLE: MCP primitive共通名前索引テーブル
///  - MCP_PRIMITIVE_NAME_STATE_TABLE: MCP primitive名前索引構築状態
///  - RESOURCE_URI_INDEX_TABLE: resource URI逆引き索引テーブル
///  - RESOURCE_URI_INDEX_STATE_TABLE: resource URI逆引き索引構築状態
///  - ASSET_GROUP_TABLE: アセット情報テーブル
///  - ASSET_LOOKUP_TABLE: アセットID特定テーブル
///  - ASSET_GROUP_TABLE: ページ所属アセット群取得テーブル
///  - USER_ID_TABLE: ユーザIDテーブル
///  - USER_INFO_TABLE: ユーザ情報テーブル
///  - BEARER_TOKEN_TABLE: Bearerトークン主テーブル
///  - BEARER_TOKEN_ID_TABLE: BearerトークンID変換テーブル
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
        let _ = txn
            .open_table(PAGE_PATH_TABLE)
            .context("create PAGE_PATH_TABLE")?;

        // 削除済みページパスインデックステーブル
        let _ = txn
            .open_multimap_table(DELETED_PAGE_PATH_TABLE)
            .context("create DELETED_PAGE_PATH_TABLE")?;

        // ページインデックステーブル
        let _ = txn
            .open_table(PAGE_INDEX_TABLE)
            .context("create PAGE_INDEX_TABLE")?;

        // ページソーステーブル
        let _ = txn
            .open_table(PAGE_SOURCE_TABLE)
            .context("create PAGE_SOURCE_TABLE")?;

        // テンプレート候補テーブル
        let _ = txn
            .open_table(TEMPLATE_CANDIDATE_TABLE)
            .context("create TEMPLATE_CANDIDATE_TABLE")?;

        // prompt候補テーブル
        let _ = txn
            .open_table(PROMPT_CANDIDATE_TABLE)
            .context("create PROMPT_CANDIDATE_TABLE")?;

        // resource候補テーブル
        let _ = txn
            .open_table(RESOURCE_CANDIDATE_TABLE)
            .context("create RESOURCE_CANDIDATE_TABLE")?;

        // MCP primitive共通名前索引テーブル
        let _ = txn
            .open_table(MCP_PRIMITIVE_NAME_TABLE)
            .context("create MCP_PRIMITIVE_NAME_TABLE")?;

        // MCP primitive名前索引構築状態テーブル
        let _ = txn
            .open_table(MCP_PRIMITIVE_NAME_STATE_TABLE)
            .context("create MCP_PRIMITIVE_NAME_STATE_TABLE")?;

        initialize_mcp_primitive_names_in_txn(&txn)
            .context("initialize MCP primitive names")?;

        // resource URI逆引き索引テーブル
        let _ = txn
            .open_table(RESOURCE_URI_INDEX_TABLE)
            .context("create RESOURCE_URI_INDEX_TABLE")?;

        // resource URI逆引き索引構築状態テーブル
        let _ = txn
            .open_table(RESOURCE_URI_INDEX_STATE_TABLE)
            .context("create RESOURCE_URI_INDEX_STATE_TABLE")?;

        /*
         * ロック・アセット関連テーブル作成
         */
        // ロック情報テーブル
        let _ = txn
            .open_table(LOCK_INFO_TABLE)
            .context("create LOCK_INFO_TABLE")?;

        // アセット情報テーブル
        let _ = txn
            .open_table(ASSET_INFO_TABLE)
            .context("create ASSET_INFO_TABLE")?;

        // アセットID特定テーブル
        let _ = txn
            .open_table(ASSET_LOOKUP_TABLE)
            .context("create ASSET_LOOKUP_TABLE")?;

        // ページ所属アセット群取得テーブル
        let _ = txn
            .open_multimap_table(ASSET_GROUP_TABLE)
            .context("create ASSET_GROUP_TABLE")?;

        /*
         * ユーザ関連テーブル作成
         */
        // ユーザIDテーブル
        let _ = txn
            .open_table(USER_ID_TABLE)
            .context("create USER_ID_TABLE")?;

        // ユーザ情報テーブル
        let _ = txn
            .open_table(USER_INFO_TABLE)
            .context("create USER_INFO_TABLE")?;

        /*
         * Bearer関連テーブル作成
         */
        // Bearerトークン主テーブル
        let _ = txn
            .open_table(BEARER_TOKEN_TABLE)
            .context("create BEARER_TOKEN_TABLE")?;

        // BearerトークンID変換テーブル
        let _ = txn
            .open_table(BEARER_TOKEN_ID_TABLE)
            .context("create BEARER_TOKEN_ID_TABLE")?;
    }

    /*
     * コミット
     */
    txn.commit()?;

    Ok(())
}
