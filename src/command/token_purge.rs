/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! token purge コマンドの実装
//!

use std::cell::RefCell;

use anyhow::{anyhow, Result};
use chrono::Local;

use super::CommandContext;
use super::common::confirm_action;
use crate::cmd_args::{Options, TokenPurgeOpts};
use crate::database::types::TokenId;
use crate::database::DatabaseManager;

///
/// "token purge"サブコマンドのコンテキスト情報をパックした構造体
///
struct TokenPurgeCommandContext {
    /// データベースマネージャオブジェクト
    manager: RefCell<DatabaseManager>,

    /// 実行対象
    target: PurgeTarget,

    /// 確認プロンプト省略指定
    yes: bool,
}

///
/// 削除対象の指定方法
///
enum PurgeTarget {
    TokenId(TokenId),
    Filters {
        expired: bool,
        revoked: bool,
    },
}

impl TokenPurgeCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &TokenPurgeOpts) -> Result<Self> {
        let target = match (
            sub_opts.token_id(),
            sub_opts.is_expired(),
            sub_opts.is_revoked(),
        ) {
            (Some(token_id), false, false) => PurgeTarget::TokenId(
                TokenId::from_string(&token_id)
                    .map_err(|_| anyhow!("invalid token id: {}", token_id))?,
            ),
            (None, expired, revoked) if expired || revoked => {
                PurgeTarget::Filters { expired, revoked }
            }
            _ => return Err(anyhow!("invalid purge target")),
        };

        Ok(Self {
            manager: RefCell::new(opts.open_database()?),
            target,
            yes: sub_opts.is_yes(),
        })
    }

    ///
    /// 削除対象件数を取得
    ///
    fn target_count(&self, manager: &DatabaseManager) -> Result<usize> {
        match &self.target {
            PurgeTarget::TokenId(token_id) => {
                let exists = manager.get_bearer_token_info_by_id(token_id)?;
                if exists.is_none() {
                    return Err(anyhow!("token not found: {}", token_id));
                }
                Ok(1)
            }
            PurgeTarget::Filters { expired, revoked } => {
                let tokens = manager.filter_bearer_tokens(
                    None,
                    *revoked,
                    *expired,
                    Local::now(),
                )?;
                Ok(tokens.len())
            }
        }
    }

    ///
    /// 削除対象の説明文字列を返す
    ///
    fn target_description(&self) -> String {
        match &self.target {
            PurgeTarget::TokenId(token_id) => {
                format!("token_id={}", token_id)
            }
            PurgeTarget::Filters { expired, revoked } => match (*expired, *revoked) {
                (true, true) => "expired or revoked tokens".to_string(),
                (true, false) => "expired tokens".to_string(),
                (false, true) => "revoked tokens".to_string(),
                (false, false) => "tokens".to_string(),
            },
        }
    }

    ///
    /// 削除処理の実行
    ///
    fn purge(&self, manager: &DatabaseManager) -> Result<usize> {
        match &self.target {
            PurgeTarget::TokenId(token_id) => {
                manager.purge_bearer_token_by_id(token_id)
            }
            PurgeTarget::Filters { expired, revoked } => {
                manager.purge_bearer_tokens(*expired, *revoked, Local::now())
            }
        }
    }
}

// CommandContextの実装
impl CommandContext for TokenPurgeCommandContext {
    fn exec(&self) -> Result<()> {
        let manager = self.manager.borrow_mut();
        let target_count = self.target_count(&manager)?;

        if !self.yes {
            let prompt = format!(
                "{} を削除します。対象件数: {}。続行しますか？",
                self.target_description(),
                target_count
            );
            if !confirm_action(&prompt)? {
                println!("canceled");
                return Ok(());
            }
        }

        let deleted_count = self.purge(&manager)?;
        println!("deleted_count: {}", deleted_count);
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &TokenPurgeOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(TokenPurgeCommandContext::new(opts, sub_opts)?))
}
