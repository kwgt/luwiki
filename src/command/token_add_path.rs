/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! token add_path コマンドの実装
//!

use anyhow::{anyhow, Result};

use super::CommandContext;
use crate::cmd_args::{Options, TokenPathUpdateOpts};
use crate::command::common::format_cli_timestamp;
use crate::command::token_list::format_path_prefixes_detail;
use crate::database::types::TokenId;
use crate::database::DatabaseManager;

///
/// "token add_path"サブコマンドのコンテキスト情報をパックした構造体
///
struct TokenAddPathCommandContext {
    manager: DatabaseManager,
    token_id: TokenId,
    path_prefix: String,
}

impl TokenAddPathCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &TokenPathUpdateOpts) -> Result<Self> {
        let token_id = sub_opts.token_id();
        Ok(Self {
            manager: opts.open_database()?,
            token_id: TokenId::from_string(&token_id)
                .map_err(|_| anyhow!("invalid token id: {}", token_id))?,
            path_prefix: sub_opts.normalized_path_prefix(),
        })
    }
}

impl CommandContext for TokenAddPathCommandContext {
    fn exec(&self) -> Result<()> {
        /*
         * path prefix 制約を追加する
         */
        let info = self
            .manager
            .add_path_prefix_to_bearer_token(&self.token_id, &self.path_prefix)?;

        /*
         * 更新結果を表示する
         */
        println!("token_id: {}", info.token_id());
        println!(
            "path_prefixes: {}",
            format_path_prefixes_detail(info.path_prefixes())
        );
        println!("updated_at: {}", format_cli_timestamp(info.updated_at()));
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &TokenPathUpdateOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(TokenAddPathCommandContext::new(opts, sub_opts)?))
}
