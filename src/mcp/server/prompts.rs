/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP promptsの公開型変換とprotocol error写像を定義するモジュール
//!

use std::net::IpAddr;

use rmcp::ErrorData as McpProtocolError;
use rmcp::model::{
    GetPromptResult,
    ListPromptsResult,
    Prompt,
    PromptArgument,
    PromptMessage,
    PromptMessageRole,
};

use crate::auth::AuthContext;
use crate::mcp::errors::{McpError, McpErrorCode};

use super::LuwikiMcpServer;

// 可読性確保のため、標準prompts操作のimplを子モジュールへ分離する。
impl LuwikiMcpServer {
    ///
    /// 認証済み文脈からrmcp prompt一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のprompt一覧結果を返す。
    ///
    pub(super) fn list_prompts_for_auth(
        &self,
        auth: &AuthContext,
        cursor: Option<&str>,
    ) -> Result<ListPromptsResult, McpProtocolError> {
        self.list_prompts_for_auth_at(auth, None, cursor)
    }

    ///
    /// 認証済み文脈と入力元からrmcp prompt一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のprompt一覧結果を返す。
    ///
    pub(super) fn list_prompts_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListPromptsResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok(handler.handle_list_prompts(
                auth,
                state.db(),
                address,
                cursor,
            ))
        })?;
        let result =
            result.map_err(Self::list_prompt_protocol_error)?;
        let prompts = result
            .items()
            .iter()
            .map(|item| {
                let arguments = item
                    .arguments()
                    .iter()
                    .map(|argument| {
                        let required = argument.required();
                        let mapped = PromptArgument::new(
                            argument.name(),
                        )
                        .with_description(argument.description());
                        match required {
                            Some(required) => {
                                mapped.with_required(required)
                            }
                            None => mapped,
                        }
                    })
                    .collect::<Vec<_>>();
                let arguments = if arguments.is_empty() {
                    None
                } else {
                    Some(arguments)
                };
                Prompt::new(
                    item.name(),
                    Some(item.description()),
                    arguments,
                )
            })
            .collect();
        let mut response =
            ListPromptsResult::with_all_items(prompts);
        response.next_cursor =
            result.next_cursor().map(str::to_string);

        Ok(response)
    }

    ///
    /// prompt論理エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - prompt一覧の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn list_prompt_protocol_error(
        error: McpError,
    ) -> McpProtocolError {
        match error.code() {
            McpErrorCode::Forbidden => {
                McpProtocolError::invalid_request(
                    "operation is not allowed",
                    Some(serde_json::json!({
                        "code": "forbidden",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    "cursor is invalid",
                    Some(serde_json::json!({
                        "code": "invalid_input",
                    })),
                )
            }
            _ => McpProtocolError::internal_error(
                "internal error",
                Some(serde_json::json!({
                    "code": "internal_error",
                })),
            ),
        }
    }

    ///
    /// 認証済み文脈からrmcp prompt取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// rmcp標準のprompt取得結果を返す。
    ///
    pub(super) fn get_prompt_for_auth(
        &self,
        auth: &AuthContext,
        name: &str,
        arguments: Option<
            &serde_json::Map<String, serde_json::Value>,
        >,
    ) -> Result<GetPromptResult, McpProtocolError> {
        self.get_prompt_for_auth_at(
            auth,
            None,
            name,
            arguments,
        )
    }

    ///
    /// 認証済み文脈と入力元から
    /// rmcp prompt取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// rmcp標準のprompt取得結果を返す。
    ///
    pub(super) fn get_prompt_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        name: &str,
        arguments: Option<
            &serde_json::Map<String, serde_json::Value>,
        >,
    ) -> Result<GetPromptResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok(handler.handle_get_prompt(
                auth,
                state.db(),
                address,
                name,
                arguments,
            ))
        })?;
        let result =
            result.map_err(Self::get_prompt_protocol_error)?;
        let message = PromptMessage::new_text(
            PromptMessageRole::User,
            result.message(),
        );

        Ok(GetPromptResult::new(vec![message])
            .with_description(result.description()))
    }

    ///
    /// prompt取得エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - prompt取得の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    pub(super) fn get_prompt_protocol_error(
        error: McpError,
    ) -> McpProtocolError {
        match error.code() {
            McpErrorCode::Forbidden => {
                McpProtocolError::invalid_request(
                    "operation is not allowed",
                    Some(serde_json::json!({
                        "code": "forbidden",
                    })),
                )
            }
            McpErrorCode::NotFound => {
                McpProtocolError::invalid_params(
                    "prompt not found",
                    Some(serde_json::json!({
                        "code": "not_found",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    error.message().to_string(),
                    Some(serde_json::json!({
                        "code": "invalid_input",
                    })),
                )
            }
            _ => McpProtocolError::internal_error(
                "internal error",
                Some(serde_json::json!({
                    "code": "internal_error",
                })),
            ),
        }
    }
}
