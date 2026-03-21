/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! token revoke コマンドの実装
//!

use std::cell::RefCell;

use anyhow::{anyhow, Result};
use chrono::Local;

use super::CommandContext;
use super::common::confirm_action;
use crate::cmd_args::{Options, TokenRevokeOpts};
use crate::database::types::TokenId;
use crate::database::{DatabaseManager, DbError};

///
/// "token revoke"サブコマンドのコンテキスト情報をパックした構造体
///
struct TokenRevokeCommandContext {
    /// データベースマネージャオブジェクト
    manager: RefCell<DatabaseManager>,

    /// 実行対象
    target: RevokeTarget,

    /// 確認プロンプト省略指定
    yes: bool,
}

///
/// 失効対象の指定方法
///
enum RevokeTarget {
    TokenId(TokenId),
    User(String),
    All,
}

impl TokenRevokeCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &TokenRevokeOpts) -> Result<Self> {
        let target = match (
            sub_opts.token_id(),
            sub_opts.user_name(),
            sub_opts.is_all(),
        ) {
            (Some(token_id), None, false) => RevokeTarget::TokenId(
                TokenId::from_string(&token_id)
                    .map_err(|_| anyhow!("invalid token id: {}", token_id))?,
            ),
            (None, Some(user_name), false) => RevokeTarget::User(user_name),
            (None, None, true) => RevokeTarget::All,
            _ => return Err(anyhow!("invalid revoke target")),
        };

        Ok(Self {
            manager: RefCell::new(opts.open_database()?),
            target,
            yes: sub_opts.is_yes(),
        })
    }

    ///
    /// 失効対象件数を取得
    ///
    fn target_count(&self, manager: &DatabaseManager) -> Result<usize> {
        match &self.target {
            RevokeTarget::TokenId(token_id) => {
                let exists = manager.get_bearer_token_info_by_id(token_id)?;
                if exists.is_none() {
                    return Err(anyhow!("token not found: {}", token_id));
                }
                Ok(1)
            }
            RevokeTarget::User(user_name) => {
                let user_id = manager
                    .get_user_id_by_name(user_name)?
                    .ok_or_else(|| anyhow!(DbError::UserNotFound))?;
                let tokens = manager.filter_bearer_tokens(
                    Some(&user_id),
                    false,
                    false,
                    Local::now(),
                )?;
                Ok(tokens.len())
            }
            RevokeTarget::All => Ok(manager.list_bearer_tokens()?.len()),
        }
    }

    ///
    /// 失効対象の説明文字列を返す
    ///
    fn target_description(&self) -> String {
        match &self.target {
            RevokeTarget::TokenId(token_id) => {
                format!("token_id={}", token_id)
            }
            RevokeTarget::User(user_name) => {
                format!("user={}", user_name)
            }
            RevokeTarget::All => "all tokens".to_string(),
        }
    }

    ///
    /// 失効処理の実行
    ///
    fn revoke(
        &self,
        manager: &DatabaseManager,
    ) -> Result<(usize, usize)> {
        let result = match &self.target {
            RevokeTarget::TokenId(token_id) => {
                manager.revoke_bearer_token_by_id(token_id)?
            }
            RevokeTarget::User(user_name) => {
                manager.revoke_bearer_tokens_by_user(user_name)?
            }
            RevokeTarget::All => manager.revoke_all_bearer_tokens()?,
        };

        Ok((result.updated_count(), result.warning_count()))
    }
}

// CommandContextの実装
impl CommandContext for TokenRevokeCommandContext {
    fn exec(&self) -> Result<()> {
        let manager = self.manager.borrow_mut();
        let target_count = self.target_count(&manager)?;

        if !self.yes {
            let prompt = format!(
                "{} を失効します。対象件数: {}。続行しますか？",
                self.target_description(),
                target_count
            );
            if !confirm_action(&prompt)? {
                println!("canceled");
                return Ok(());
            }
        }

        let (updated_count, warning_count) = self.revoke(&manager)?;
        println!("revoked_count: {}", updated_count);
        println!("warning_count: {}", warning_count);
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &TokenRevokeOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(TokenRevokeCommandContext::new(opts, sub_opts)?))
}
