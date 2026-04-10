/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCP固有エラーの骨格を定義するモジュール
//!

use std::fmt;

use serde::Serialize;

///
/// MCP公開面で利用する論理エラーコード
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum McpErrorCode {
    /// 対象が存在しない
    NotFound,

    /// 認可失敗
    Forbidden,

    /// 競合
    Conflict,

    /// 入力不正
    InvalidInput,

    /// 更新要求の revision が最新ではない
    NotLatestRevision,

    /// 更新要求の instance_id が最新内容と一致しない
    InstanceIdNotMatch,

    /// 未対応
    Unsupported,

    /// 内部失敗
    InternalError,
}

///
/// MCP固有エラー情報
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct McpError {
    /// 外部向け論理エラーコード
    code: McpErrorCode,

    /// エラー説明
    message: String,
}

impl McpError {
    ///
    /// MCPエラー情報の生成
    ///
    /// # 引数
    /// * `code` - 論理エラーコード
    /// * `message` - エラー説明
    ///
    /// # 戻り値
    /// 生成したMCPエラー情報を返す。
    ///
    pub(crate) fn new<S>(code: McpErrorCode, message: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            code,
            message: message.into(),
        }
    }

    ///
    /// ReadOnly 属性による認可拒否エラーを生成する
    ///
    /// # 戻り値
    /// `forbidden` として公開する認可拒否エラーを返す。
    ///
    pub(crate) fn forbidden_read_only() -> Self {
        Self::new(
            McpErrorCode::Forbidden,
            "read only denied: write operation is not allowed",
        )
    }

    ///
    /// 論理エラーコードへのアクセサ
    ///
    /// # 戻り値
    /// 論理エラーコードを返す。
    ///
    pub(crate) fn code(&self) -> McpErrorCode {
        self.code
    }

    ///
    /// エラー説明へのアクセサ
    ///
    /// # 戻り値
    /// エラー説明文字列を返す。
    ///
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl McpErrorCode {
    ///
    /// 外部公開用のエラーコード文字列を返す
    ///
    /// # 戻り値
    /// 外部応答で使用するエラーコード文字列を返す。
    ///
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::InvalidInput => "invalid_input",
            Self::NotLatestRevision => "not_latest_revision",
            Self::InstanceIdNotMatch => "instance_id_not_match",
            Self::Unsupported => "unsupported",
            Self::InternalError => "internal_error",
        }
    }
}

///
/// MCP公開面で返すエラー応答
///
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct McpErrorResponse {
    /// 外部向け論理エラーコード
    code: &'static str,

    /// エラー説明
    message: String,
}

impl McpErrorResponse {
    ///
    /// MCPエラー応答を生成する
    ///
    /// # 引数
    /// * `code` - 外部向け論理エラーコード
    /// * `message` - エラー説明
    ///
    /// # 戻り値
    /// 生成したエラー応答を返す。
    ///
    pub(crate) fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    ///
    /// 外部向け論理エラーコードを返す
    ///
    /// # 戻り値
    /// 外部向け論理エラーコードを返す。
    ///
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    ///
    /// エラー説明を返す
    ///
    /// # 戻り値
    /// エラー説明を返す。
    ///
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl From<McpError> for McpErrorResponse {
    fn from(error: McpError) -> Self {
        Self::new(error.code().as_str(), error.message().to_string())
    }
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for McpError {}
