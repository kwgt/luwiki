/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP resourcesの公開型変換とprotocol error写像を定義するモジュール
//!

use std::net::IpAddr;

use rmcp::ErrorData as McpProtocolError;
use rmcp::model::{
    ListResourcesResult,
    RawResource,
    ReadResourceResult,
    Resource,
    ResourceContents,
};

use crate::auth::AuthContext;
use crate::mcp::errors::{McpError, McpErrorCode};

use super::LuwikiMcpServer;

// 可読性確保のため、標準resources操作のimplを子モジュールへ分離する。
impl LuwikiMcpServer {
    ///
    /// 認証済み文脈からrmcp resource一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のresource一覧結果を返す。
    ///
    pub(super) fn list_resources_for_auth(
        &self,
        auth: &AuthContext,
        cursor: Option<&str>,
    ) -> Result<ListResourcesResult, McpProtocolError> {
        self.list_resources_for_auth_at(auth, None, cursor)
    }

    ///
    /// 認証済み文脈と入力元からrmcp resource一覧を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// rmcp標準のresource一覧結果を返す。
    ///
    pub(super) fn list_resources_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListResourcesResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok(handler.handle_list_resources(
                auth,
                state.db(),
                address,
                cursor,
            ))
        })?;
        let result =
            result.map_err(Self::list_resource_protocol_error)?;
        let resources = result
            .items()
            .iter()
            .map(|item| {
                let raw = RawResource::new(item.uri(), item.name())
                    .with_description(item.description())
                    .with_mime_type(item.mime_type());

                Resource::new(raw, None)
            })
            .collect();
        let mut response =
            ListResourcesResult::default();
        response.resources = resources;
        response.next_cursor =
            result.next_cursor().map(str::to_string);

        Ok(response)
    }

    ///
    /// resource論理エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - resource一覧の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn list_resource_protocol_error(
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
    /// 認証済み文脈からrmcp resource取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// rmcp標準のresource取得結果を返す。
    ///
    pub(super) fn read_resource_for_auth(
        &self,
        auth: &AuthContext,
        uri: &str,
    ) -> Result<ReadResourceResult, McpProtocolError> {
        self.read_resource_for_auth_at(auth, None, uri)
    }

    ///
    /// 認証済み文脈と入力元からrmcp resource取得結果を生成する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `address` - 入力元IP address
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// rmcp標準のresource取得結果を返す。
    ///
    pub(super) fn read_resource_for_auth_at(
        &self,
        auth: &AuthContext,
        address: Option<IpAddr>,
        uri: &str,
    ) -> Result<ReadResourceResult, McpProtocolError> {
        let handler = self.create_handler();
        let result = self.with_state_read(|state| {
            Ok::<_, McpProtocolError>(handler.handle_read_resource(
                auth,
                state.db(),
                address,
                uri,
            ))
        })?;
        let result =
            result.map_err(Self::read_resource_protocol_error)?;
        let _revision = result.revision();
        let contents = ResourceContents::text(
            result.text(),
            result.uri(),
        )
        .with_mime_type(result.mime_type());

        Ok(ReadResourceResult::new(vec![contents]))
    }

    ///
    /// resource取得エラーをprotocol errorへ変換する
    ///
    /// # 引数
    /// * `error` - resource取得の論理エラー
    ///
    /// # 戻り値
    /// rmcpへ返すprotocol errorを返す。
    ///
    fn read_resource_protocol_error(
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
                    "resource not found",
                    Some(serde_json::json!({
                        "code": "not_found",
                    })),
                )
            }
            McpErrorCode::InvalidInput => {
                McpProtocolError::invalid_params(
                    "resource uri is invalid",
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
