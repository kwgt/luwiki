/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! HTTPサーバが共有する状態をまとめたモジュール
//!

use std::sync::{Arc, RwLock};

use crate::audit::AuditSink;
use crate::cmd_args::FrontendConfig;
use crate::database::DatabaseManager;
use crate::fts::FtsIndexConfig;

///
/// HTTPサーバの共有状態
///
pub(crate) struct AppState {
    /// データベースマネージャ
    db: DatabaseManager,

    /// frontend設定
    frontend_config: FrontendConfig,

    /// FTS設定
    fts_config: FtsIndexConfig,

    /// テンプレートルート
    template_root: Option<String>,

    /// Wikiタイトル
    wiki_title: String,

    /// アセットサイズ上限
    asset_limit_size: u64,

    /// 監査ログ投入入口
    audit_sink: Option<Arc<RwLock<AuditSink>>>,
}

impl AppState {
    ///
    /// 共有状態オブジェクトの生成
    ///
    /// # 引数
    /// * `db` - 所有させるデータベースマネージャオブジェクト
    /// * `frontend_config` - frontend設定
    /// * `fts_config` - FTS設定
    /// * `template_root` - テンプレートルート
    /// * `wiki_title` - Wikiタイトル
    /// * `asset_limit_size` - アセットサイズ上限(バイト単位)
    /// * `audit_sink` - 監査ログ投入入口
    ///
    /// # 戻り値
    /// 生成したオブジェクトを返す。
    ///
    pub(crate) fn new(
        db: DatabaseManager,
        frontend_config: FrontendConfig,
        fts_config: FtsIndexConfig,
        template_root: Option<String>,
        wiki_title: String,
        asset_limit_size: u64,
        audit_sink: Option<Arc<RwLock<AuditSink>>>,
    ) -> Self {
        Self {
            db,
            frontend_config,
            fts_config,
            template_root,
            wiki_title,
            asset_limit_size,
            audit_sink,
        }
    }

    ///
    /// データベースマネージャオブジェクトへのアクセサ
    ///
    /// # 戻り値
    /// データベースマネージャオブジェクトへの参照を返す。
    ///
    pub(crate) fn db<'a>(&'a self) -> &'a DatabaseManager {
        &self.db
    }

    ///
    /// frontend設定へのアクセサ
    ///
    pub(crate) fn frontend_config<'a>(&'a self) -> &'a FrontendConfig {
        &self.frontend_config
    }

    ///
    /// FTS設定へのアクセサ
    ///
    /// # 戻り値
    /// FTS設定への参照を返す。
    ///
    pub(crate) fn fts_config<'a>(&'a self) -> &'a FtsIndexConfig {
        &self.fts_config
    }

    ///
    /// テンプレートルートへのアクセサ
    ///
    /// # 戻り値
    /// テンプレートルートが設定されている場合は参照を返す。
    ///
    pub(crate) fn template_root<'a>(&'a self) -> Option<&'a str> {
        self.template_root.as_deref()
    }

    ///
    /// Wikiタイトルへのアクセサ
    ///
    /// # 戻り値
    /// Wikiタイトルへの参照を返す。
    ///
    pub(crate) fn wiki_title<'a>(&'a self) -> &'a str {
        &self.wiki_title
    }

    ///
    /// アセットサイズ上限へのアクセサ
    ///
    /// # 戻り値
    /// アップロード可能なアセットサイズ上限(バイト単位)を返す。
    ///
    pub(crate) fn asset_limit_size(&self) -> u64 {
        self.asset_limit_size
    }

    ///
    /// 監査ログ投入入口へのアクセサ
    ///
    /// # 戻り値
    /// 監査ログ投入入口が存在する場合は共有参照を返す。
    ///
    pub(crate) fn audit_sink(&self) -> Option<Arc<RwLock<AuditSink>>> {
        self.audit_sink.clone()
    }

    ///
    /// データベースマネージャオブジェクトへのアクセサ
    ///
    /// # 戻り値
    /// データベースマネージャオブジェクトへの参照を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn db_mut<'a>(&'a mut self) -> &'a mut DatabaseManager {
        &mut self.db
    }
}
