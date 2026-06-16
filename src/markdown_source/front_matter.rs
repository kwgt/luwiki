/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! front matter 抽出処理を提供するモジュール
//!

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_yaml_ng::Value;

/// MCP prompt 名の最大文字数
const MAX_MCP_PROMPT_NAME_CHARS: usize = 128;

/// MCP prompt 説明の最大文字数
const MAX_MCP_PROMPT_DESCRIPTION_CHARS: usize = 1024;

/// MCP prompt system 情報の最大文字数
const MAX_MCP_PROMPT_SYSTEM_CHARS: usize = 8192;

/// MCP prompt 引数名の最大文字数
const MAX_MCP_PROMPT_ARGUMENT_NAME_CHARS: usize = 64;

/// MCP prompt 引数説明の最大文字数
const MAX_MCP_PROMPT_ARGUMENT_DESCRIPTION_CHARS: usize = 1024;

/// MCP resource 識別子の最大文字数
const MAX_MCP_RESOURCE_ID_CHARS: usize = 512;

/// MCP resource 名の最大文字数
const MAX_MCP_RESOURCE_NAME_CHARS: usize = 128;

/// MCP resource 説明の最大文字数
const MAX_MCP_RESOURCE_DESCRIPTION_CHARS: usize = 1024;

/// MCP resource MIME type の最大文字数
const MAX_MCP_RESOURCE_MIME_TYPE_CHARS: usize = 128;

///
/// front matter 抽出エラー種別
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtractFrontMatterError {
    /// 閉じ区切りが見つからない
    ClosingDelimiterNotFound,
}

impl std::fmt::Display for ExtractFrontMatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClosingDelimiterNotFound => {
                write!(f, "front matter closing delimiter not found")
            }
        }
    }
}

impl std::error::Error for ExtractFrontMatterError {}

///
/// front matter 検証エラー種別
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontMatterValidationError {
    /// YAML 構文不正
    Syntax {
        /// エラーメッセージ
        message: String,

        /// 行番号
        line: Option<usize>,

        /// 列番号
        column: Option<usize>,
    },

    /// スキーマ不正
    Validation {
        /// 不正箇所のプロパティパス
        property_path: String,

        /// エラーメッセージ
        message: String,
    },
}

impl std::fmt::Display for FrontMatterValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax {
                message,
                line,
                column,
            } => {
                if let Some(line) = line {
                    if let Some(column) = column {
                        return write!(
                            f,
                            "front matter syntax error at line {}, column {}: {}",
                            line,
                            column,
                            message,
                        );
                    }

                    return write!(
                        f,
                        "front matter syntax error at line {}: {}",
                        line,
                        message,
                    );
                }

                write!(f, "front matter syntax error: {}", message)
            }
            Self::Validation {
                property_path,
                message,
            } => write!(
                f,
                "front matter validation error at {}: {}",
                property_path,
                message,
            ),
        }
    }
}

impl std::error::Error for FrontMatterValidationError {}

///
/// front matter 抽出または検証エラー種別
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontMatterError {
    /// 抽出失敗
    Extract(ExtractFrontMatterError),

    /// 検証失敗
    Validate(FrontMatterValidationError),
}

impl std::fmt::Display for FrontMatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extract(err) => err.fmt(f),
            Self::Validate(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for FrontMatterError {}

impl From<ExtractFrontMatterError> for FrontMatterError {
    fn from(value: ExtractFrontMatterError) -> Self {
        Self::Extract(value)
    }
}

///
/// front matter 抽出結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedFrontMatter<'a> {
    /// front matter 全体
    raw_block: &'a str,

    /// YAML 本文
    front_matter: &'a str,

    /// front matter 除去後の本文
    body: &'a str,
}

impl<'a> ExtractedFrontMatter<'a> {
    ///
    /// front matter 全体へのアクセサ
    ///
    /// # 戻り値
    /// 先頭区切り行と終端区切り行を含む文字列を返す。
    ///
    pub fn raw_block(&self) -> &'a str {
        self.raw_block
    }

    ///
    /// YAML 本文へのアクセサ
    ///
    /// # 戻り値
    /// 区切り行を除いた front matter 本体を返す。
    ///
    pub fn front_matter(&self) -> &'a str {
        self.front_matter
    }

    ///
    /// 本文へのアクセサ
    ///
    /// # 戻り値
    /// front matter 除去後の本文を返す。
    ///
    pub fn body(&self) -> &'a str {
        self.body
    }
}

///
/// front matter 全体
///
#[derive(Clone, Debug, Deserialize)]
pub struct FrontMatter {
    /// LuWiki 名前空間
    wiki: Option<WikiFrontMatter>,

    /// MCP 名前空間
    mcp: Option<McpFrontMatter>,

    /// ユーザ定義名前空間
    custom_meta: Option<BTreeMap<String, Value>>,
}

///
/// テンプレートページ抽出結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplatePageFrontMatter {
    /// テンプレート表示名
    name: String,

    /// テンプレート説明
    description: Option<String>,

    /// マクロ即時展開可否
    macro_expand: Option<bool>,
}

impl TemplatePageFrontMatter {
    ///
    /// テンプレート表示名へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template.name` の値を返す。
    ///
    pub fn name(&self) -> &str {
        &self.name
    }

    ///
    /// テンプレート説明へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template.description` が存在する場合はその値を返す。
    ///
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    ///
    /// マクロ即時展開可否へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template.macro_expand` が存在する場合はその値を返す。
    ///
    pub fn macro_expand(&self) -> Option<bool> {
        self.macro_expand
    }
}

///
/// prompt 引数の front matter 抽出結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptArgumentFrontMatter {
    /// 引数名
    name: String,

    /// 引数説明
    description: String,

    /// 必須可否
    required: Option<bool>,
}

impl PromptArgumentFrontMatter {
    ///
    /// 引数名へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.arguments[].name` の値を返す。
    ///
    pub fn name(&self) -> &str {
        &self.name
    }

    ///
    /// 引数説明へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.arguments[].description` の値を返す。
    ///
    pub fn description(&self) -> &str {
        &self.description
    }

    ///
    /// 必須可否へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.arguments[].required` が存在する場合はその値を返す。
    ///
    pub fn required(&self) -> Option<bool> {
        self.required
    }
}

///
/// prompt ページの front matter 抽出結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptPageFrontMatter {
    /// prompt 名
    name: String,

    /// prompt 説明
    description: String,

    /// system 情報
    system: Option<String>,

    /// prompt 引数
    arguments: Vec<PromptArgumentFrontMatter>,
}

impl PromptPageFrontMatter {
    ///
    /// prompt 名へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.name` の値を返す。
    ///
    pub fn name(&self) -> &str {
        &self.name
    }

    ///
    /// prompt 説明へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.description` の値を返す。
    ///
    pub fn description(&self) -> &str {
        &self.description
    }

    ///
    /// system 情報へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.system` が存在する場合はその値を返す。
    ///
    pub fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    ///
    /// prompt 引数へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.arguments` の記載順を維持したスライスを返す。
    ///
    pub fn arguments(&self) -> &[PromptArgumentFrontMatter] {
        &self.arguments
    }
}

///
/// resource ページの front matter 抽出結果
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePageFrontMatter {
    /// resource 識別子
    resource_id: Option<String>,

    /// resource 名
    name: String,

    /// resource 説明
    description: String,

    /// MIME type
    mime_type: Option<String>,
}

impl ResourcePageFrontMatter {
    ///
    /// resource 識別子へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.resource_id` が存在する場合はその値を返す。
    ///
    pub fn resource_id(&self) -> Option<&str> {
        self.resource_id.as_deref()
    }

    ///
    /// resource 名へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.name` の値を返す。
    ///
    pub fn name(&self) -> &str {
        &self.name
    }

    ///
    /// resource 説明へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.description` の値を返す。
    ///
    pub fn description(&self) -> &str {
        &self.description
    }

    ///
    /// MIME type へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.mime_type` が存在する場合はその値を返す。
    ///
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
}

///
/// ページが持つ用途の front matter 分類結果
///
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageFrontMatterKind {
    /// テンプレートページ
    Template(TemplatePageFrontMatter),

    /// MCP prompt ページ
    McpPrompt(PromptPageFrontMatter),

    /// MCP resource ページ
    McpResource(ResourcePageFrontMatter),
}

impl PageFrontMatterKind {
    ///
    /// テンプレートページかどうかを判定する
    ///
    /// # 戻り値
    /// テンプレートページの場合は `true` を返す。
    ///
    pub fn is_template_page(&self) -> bool {
        matches!(self, Self::Template(_))
    }

    ///
    /// MCP prompt ページかどうかを判定する
    ///
    /// # 戻り値
    /// MCP prompt ページの場合は `true` を返す。
    ///
    pub fn is_mcp_prompt_page(&self) -> bool {
        matches!(self, Self::McpPrompt(_))
    }

    ///
    /// MCP resource ページかどうかを判定する
    ///
    /// # 戻り値
    /// MCP resource ページの場合は `true` を返す。
    ///
    pub fn is_mcp_resource_page(&self) -> bool {
        matches!(self, Self::McpResource(_))
    }

    ///
    /// テンプレートページ情報へのアクセサ
    ///
    /// # 戻り値
    /// テンプレートページの場合はその情報を返す。
    ///
    pub fn template_page(&self) -> Option<&TemplatePageFrontMatter> {
        match self {
            Self::Template(template) => Some(template),
            _ => None,
        }
    }

    ///
    /// prompt ページ情報へのアクセサ
    ///
    /// # 戻り値
    /// MCP prompt ページの場合はその情報を返す。
    ///
    pub fn prompt_page(&self) -> Option<&PromptPageFrontMatter> {
        match self {
            Self::McpPrompt(prompt) => Some(prompt),
            _ => None,
        }
    }

    ///
    /// resource ページ情報へのアクセサ
    ///
    /// # 戻り値
    /// MCP resource ページの場合はその情報を返す。
    ///
    pub fn resource_page(&self) -> Option<&ResourcePageFrontMatter> {
        match self {
            Self::McpResource(resource) => Some(resource),
            _ => None,
        }
    }
}

impl From<&WikiTemplateFrontMatter> for TemplatePageFrontMatter {
    fn from(value: &WikiTemplateFrontMatter) -> Self {
        Self {
            name: value.name.clone(),
            description: value.description.clone(),
            macro_expand: value.macro_expand,
        }
    }
}

impl FrontMatter {
    ///
    /// `wiki` 名前空間へのアクセサ
    ///
    /// # 戻り値
    /// `wiki` が存在する場合はその参照を返す。
    ///
    pub fn wiki(&self) -> Option<&WikiFrontMatter> {
        self.wiki.as_ref()
    }

    ///
    /// `mcp` 名前空間へのアクセサ
    ///
    /// # 戻り値
    /// `mcp` が存在する場合はその参照を返す。
    ///
    pub fn mcp(&self) -> Option<&McpFrontMatter> {
        self.mcp.as_ref()
    }

    ///
    /// `custom_meta` 名前空間へのアクセサ
    ///
    /// # 戻り値
    /// `custom_meta` が存在する場合はその参照を返す。
    ///
    pub fn custom_meta(&self) -> Option<&BTreeMap<String, Value>> {
        self.custom_meta.as_ref()
    }

    ///
    /// `wiki.template` 名前空間へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template` が存在する場合はその参照を返す。
    ///
    pub fn wiki_template(&self) -> Option<&WikiTemplateFrontMatter> {
        self.wiki.as_ref().and_then(WikiFrontMatter::template)
    }

    ///
    /// テンプレートページ情報へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template` を持つ場合はテンプレートページ情報を返す。
    ///
    pub fn template_page(&self) -> Option<TemplatePageFrontMatter> {
        self.wiki_template().map(TemplatePageFrontMatter::from)
    }

    ///
    /// テンプレートページかどうかを判定する
    ///
    /// # 戻り値
    /// `wiki.template` を持つ場合は `true` を返す。
    ///
    pub fn is_template_page(&self) -> bool {
        self.wiki_template().is_some()
    }

    ///
    /// prompt ページ情報へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.primitive = prompt` の場合は prompt ページ情報を返す。
    ///
    pub fn prompt_page(&self) -> Option<PromptPageFrontMatter> {
        self.mcp
            .as_ref()
            .and_then(PromptPageFrontMatter::from_mcp)
    }

    ///
    /// resource ページ情報へのアクセサ
    ///
    /// # 戻り値
    /// `mcp.primitive = resource` の場合は resource ページ情報を返す。
    ///
    pub fn resource_page(&self) -> Option<ResourcePageFrontMatter> {
        self.mcp
            .as_ref()
            .and_then(ResourcePageFrontMatter::from_mcp)
    }

    ///
    /// prompt ページかどうかを判定する
    ///
    /// # 戻り値
    /// `mcp.primitive = prompt` の場合は `true` を返す。
    ///
    pub fn is_prompt_page(&self) -> bool {
        self.mcp
            .as_ref()
            .is_some_and(|mcp| mcp.primitive() == "prompt")
    }

    ///
    /// resource ページかどうかを判定する
    ///
    /// # 戻り値
    /// `mcp.primitive = resource` の場合は `true` を返す。
    ///
    pub fn is_mcp_resource_page(&self) -> bool {
        self.mcp
            .as_ref()
            .is_some_and(|mcp| mcp.primitive() == "resource")
    }

    ///
    /// front matter で指定されたページ用途を返す
    ///
    /// # 戻り値
    /// 名前空間順にページ用途を返す。用途指定がない場合は空の配列を返す。
    ///
    pub fn page_kinds(&self) -> Vec<PageFrontMatterKind> {
        let mut kinds = Vec::new();

        if let Some(template) = self.template_page() {
            kinds.push(PageFrontMatterKind::Template(template));
        }
        if let Some(prompt) = self.prompt_page() {
            kinds.push(PageFrontMatterKind::McpPrompt(prompt));
        } else if let Some(resource) = self.resource_page() {
            kinds.push(PageFrontMatterKind::McpResource(resource));
        }

        kinds
    }

    ///
    /// front matter 全体の妥当性を検証する
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    pub fn validate(&self) -> Result<(), FrontMatterValidationError> {
        if let Some(wiki) = &self.wiki {
            wiki.validate()?;
        }

        if let Some(mcp) = &self.mcp {
            mcp.validate()?;
        }

        Ok(())
    }
}

///
/// `wiki` 名前空間
///
#[derive(Clone, Debug, Deserialize)]
pub struct WikiFrontMatter {
    /// テンプレート情報
    template: Option<WikiTemplateFrontMatter>,

    /// タグ一覧
    tags: Option<Vec<String>>,
}

impl WikiFrontMatter {
    ///
    /// テンプレート情報へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template` が存在する場合はその参照を返す。
    ///
    pub fn template(&self) -> Option<&WikiTemplateFrontMatter> {
        self.template.as_ref()
    }

    ///
    /// タグ一覧へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.tags` が存在する場合はその参照を返す。
    ///
    pub fn tags(&self) -> Option<&[String]> {
        self.tags.as_deref()
    }

    ///
    /// 妥当性を検証する
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    fn validate(&self) -> Result<(), FrontMatterValidationError> {
        if let Some(template) = &self.template {
            template.validate()?;
        }

        if let Some(tags) = &self.tags {
            if tags.is_empty() {
                return Err(validation_error(
                    "wiki.tags",
                    "wiki.tags must not be empty",
                ));
            }

            for (index, tag) in tags.iter().enumerate() {
                if tag.is_empty() {
                    return Err(validation_error(
                        &format!("wiki.tags[{}]", index),
                        "tag must not be empty",
                    ));
                }

                if tag.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
                    return Err(validation_error(
                        &format!("wiki.tags[{}]", index),
                        "tag must not contain whitespace or control characters",
                    ));
                }
            }
        }

        Ok(())
    }
}

///
/// `wiki.template` 名前空間
///
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WikiTemplateFrontMatter {
    /// テンプレート表示名
    name: String,

    /// テンプレート説明
    description: Option<String>,

    /// マクロ即時展開可否
    macro_expand: Option<bool>,
}

impl WikiTemplateFrontMatter {
    ///
    /// テンプレート表示名へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template.name` の値を返す。
    ///
    pub fn name(&self) -> &str {
        &self.name
    }

    ///
    /// テンプレート説明へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template.description` が存在する場合はその値を返す。
    ///
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    ///
    /// マクロ即時展開可否へのアクセサ
    ///
    /// # 戻り値
    /// `wiki.template.macro_expand` が存在する場合はその値を返す。
    ///
    pub fn macro_expand(&self) -> Option<bool> {
        self.macro_expand
    }

    ///
    /// 妥当性を検証する
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    fn validate(&self) -> Result<(), FrontMatterValidationError> {
        Ok(())
    }
}

///
/// `mcp` 名前空間
///
#[derive(Clone, Debug, Deserialize)]
pub struct McpFrontMatter {
    /// primitive 種別
    primitive: String,

    /// resource 識別子
    resource_id: Option<String>,

    /// 表示名
    name: Option<String>,

    /// 説明
    description: Option<String>,

    /// MIME type
    mime_type: Option<String>,

    /// system 情報
    system: Option<String>,

    /// prompt 引数
    arguments: Option<Vec<McpPromptArgumentFrontMatter>>,

    /// 未知プロパティ
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl McpFrontMatter {
    ///
    /// primitive 種別へのアクセサ
    ///
    /// # 戻り値
    /// primitive 種別文字列を返す。
    ///
    pub fn primitive(&self) -> &str {
        &self.primitive
    }

    ///
    /// 妥当性を検証する
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    fn validate(&self) -> Result<(), FrontMatterValidationError> {
        match self.primitive.as_str() {
            "prompt" => self.validate_prompt(),
            "resource" => self.validate_resource(),
            _ => Err(validation_error(
                "mcp.primitive",
                "unsupported mcp primitive",
            )),
        }
    }

    ///
    /// prompt primitive の妥当性を検証する
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    fn validate_prompt(&self) -> Result<(), FrontMatterValidationError> {
        let name = self.name.as_deref().ok_or_else(|| {
            validation_error(
                "mcp.name",
                "mcp.name is required for prompt primitive",
            )
        })?;
        validate_prompt_name(name)?;

        let description = self.description.as_deref().ok_or_else(|| {
            validation_error(
                "mcp.description",
                "mcp.description is required for prompt primitive",
            )
        })?;
        validate_prompt_description(description)?;

        if let Some(system) = self.system.as_deref() {
            validate_prompt_system(system)?;
        }

        if let Some(arguments) = &self.arguments {
            validate_prompt_arguments(arguments)?;
        }

        if self.resource_id.is_some() {
            return Err(validation_error(
                "mcp.resource_id",
                "mcp.resource_id is not allowed for prompt primitive",
            ));
        }

        if self.mime_type.is_some() {
            return Err(validation_error(
                "mcp.mime_type",
                "mcp.mime_type is not allowed for prompt primitive",
            ));
        }

        if let Some(property) = self.extra.keys().next() {
            return Err(validation_error(
                &format!("mcp.{}", property),
                "property is not allowed for prompt primitive",
            ));
        }

        Ok(())
    }

    ///
    /// resource primitive の妥当性を検証する
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    fn validate_resource(&self) -> Result<(), FrontMatterValidationError> {
        if let Some(resource_id) = self.resource_id.as_deref() {
            validate_resource_id(resource_id)?;
        }

        let name = self.name.as_deref().ok_or_else(|| {
            validation_error(
                "mcp.name",
                "mcp.name is required for resource primitive",
            )
        })?;
        validate_resource_name(name)?;

        let description = self.description.as_deref().ok_or_else(|| {
            validation_error(
                "mcp.description",
                "mcp.description is required for resource primitive",
            )
        })?;
        validate_resource_description(description)?;

        if let Some(mime_type) = self.mime_type.as_deref() {
            validate_resource_mime_type(mime_type)?;
        }

        if self.system.is_some() {
            return Err(validation_error(
                "mcp.system",
                "mcp.system is not allowed for resource primitive",
            ));
        }

        if self.arguments.is_some() {
            return Err(validation_error(
                "mcp.arguments",
                "mcp.arguments is not allowed for resource primitive",
            ));
        }

        if let Some(property) = self.extra.keys().next() {
            return Err(validation_error(
                &format!("mcp.{}", property),
                "property is not allowed for resource primitive",
            ));
        }

        Ok(())
    }
}

///
/// MCP resource 識別子の妥当性を検証する
///
/// # 引数
/// * `resource_id` - 検証対象の resource 識別子
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
pub(crate) fn validate_resource_id(
    resource_id: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と境界空白の検証
     */
    if resource_id.trim().is_empty() {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not be empty",
        ));
    }
    if resource_id.trim() != resource_id {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not have leading or trailing whitespace",
        ));
    }

    /*
     * 文字種と文字数の検証
     */
    if resource_id.chars().any(char::is_control) {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not contain control characters",
        ));
    }
    if resource_id.chars().count() > MAX_MCP_RESOURCE_ID_CHARS {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must be at most 512 characters",
        ));
    }

    /*
     * パス形式と予約 prefix の検証
     */
    if resource_id.starts_with("builtin/") {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not start with reserved prefix builtin/",
        ));
    }
    if resource_id.starts_with('/') || resource_id.ends_with('/') {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not start or end with /",
        ));
    }
    if resource_id.contains("//") {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not contain empty path segments",
        ));
    }
    if resource_id
        .split('/')
        .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(validation_error(
            "mcp.resource_id",
            "mcp.resource_id must not contain . or .. path segments",
        ));
    }

    Ok(())
}

///
/// MCP resource 名の妥当性を検証する
///
/// # 引数
/// * `name` - 検証対象の resource 名
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_resource_name(
    name: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と境界空白の検証
     */
    if name.trim().is_empty() {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must not be empty",
        ));
    }
    if name.trim() != name {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must not have leading or trailing whitespace",
        ));
    }

    /*
     * 文字種と文字数の検証
     */
    if name.chars().any(char::is_control) {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must not contain control characters",
        ));
    }
    if name.chars().count() > MAX_MCP_RESOURCE_NAME_CHARS {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must be at most 128 characters",
        ));
    }

    Ok(())
}

///
/// MCP resource 説明の妥当性を検証する
///
/// # 引数
/// * `description` - 検証対象の resource 説明
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_resource_description(
    description: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と制御文字の検証
     */
    if description.trim().is_empty() {
        return Err(validation_error(
            "mcp.description",
            "mcp.description must not be empty",
        ));
    }
    if description.chars().any(is_unsupported_text_control) {
        return Err(validation_error(
            "mcp.description",
            "mcp.description must not contain unsupported control characters",
        ));
    }

    /*
     * 文字数の検証
     */
    if description.chars().count() > MAX_MCP_RESOURCE_DESCRIPTION_CHARS {
        return Err(validation_error(
            "mcp.description",
            "mcp.description must be at most 1024 characters",
        ));
    }

    Ok(())
}

///
/// MCP resource MIME type の妥当性を検証する
///
/// # 引数
/// * `mime_type` - 検証対象の MIME type
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_resource_mime_type(
    mime_type: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値、文字数、ASCII範囲の検証
     */
    if mime_type.is_empty() {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must not be empty",
        ));
    }
    if mime_type.chars().count() > MAX_MCP_RESOURCE_MIME_TYPE_CHARS {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must be at most 128 characters",
        ));
    }
    if !mime_type.is_ascii() {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must contain ASCII characters only",
        ));
    }
    if mime_type.chars().any(char::is_whitespace)
        || mime_type.chars().any(char::is_control)
    {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must not contain whitespace or control characters",
        ));
    }

    /*
     * essence 形式の検証
     */
    let Some((type_part, subtype_part)) = mime_type.split_once('/') else {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must match type/subtype",
        ));
    };
    if subtype_part.contains('/') {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must match type/subtype",
        ));
    }
    if type_part.is_empty() || subtype_part.is_empty() {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must match type/subtype",
        ));
    }
    if !type_part.chars().all(is_mime_type_token_char)
        || !subtype_part.chars().all(is_mime_type_token_char)
    {
        return Err(validation_error(
            "mcp.mime_type",
            "mcp.mime_type must contain valid MIME type token characters",
        ));
    }

    Ok(())
}

///
/// MIME type token として許可する文字かを判定する
///
/// # 引数
/// * `character` - 判定対象文字
///
/// # 戻り値
/// MIME type token 文字の場合は `true` を返す。
///
fn is_mime_type_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

///
/// MCP prompt 引数定義一覧の妥当性を検証する
///
/// # 引数
/// * `arguments` - 検証対象の引数定義一覧
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_prompt_arguments(
    arguments: &[McpPromptArgumentFrontMatter],
) -> Result<(), FrontMatterValidationError> {
    if arguments.is_empty() {
        return Err(validation_error(
            "mcp.arguments",
            "mcp.arguments must not be empty",
        ));
    }

    /*
     * 各引数の値と名前の一意性を検証
     */
    let mut names = BTreeSet::new();
    for (index, argument) in arguments.iter().enumerate() {
        argument.validate(index)?;

        if !names.insert(argument.name.as_str()) {
            return Err(validation_error(
                &format!("mcp.arguments[{}].name", index),
                "argument name must be unique within prompt",
            ));
        }
    }

    Ok(())
}

///
/// MCP prompt 名の妥当性を検証する
///
/// # 引数
/// * `name` - 検証対象の prompt 名
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
pub(crate) fn validate_prompt_name(
    name: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と境界空白の検証
     */
    if name.trim().is_empty() {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must not be empty",
        ));
    }
    if name.trim() != name {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must not have leading or trailing whitespace",
        ));
    }

    /*
     * 文字種と文字数の検証
     */
    if name.chars().any(char::is_control) {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must not contain control characters",
        ));
    }
    if name.chars().count() > MAX_MCP_PROMPT_NAME_CHARS {
        return Err(validation_error(
            "mcp.name",
            "mcp.name must be at most 128 characters",
        ));
    }

    Ok(())
}

///
/// MCP prompt 説明の妥当性を検証する
///
/// # 引数
/// * `description` - 検証対象の prompt 説明
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_prompt_description(
    description: &str,
) -> Result<(), FrontMatterValidationError> {
    if description.trim().is_empty() {
        return Err(validation_error(
            "mcp.description",
            "mcp.description must not be empty",
        ));
    }
    if description.chars().count() > MAX_MCP_PROMPT_DESCRIPTION_CHARS {
        return Err(validation_error(
            "mcp.description",
            "mcp.description must be at most 1024 characters",
        ));
    }

    Ok(())
}

///
/// MCP prompt system 情報の妥当性を検証する
///
/// # 引数
/// * `system` - 検証対象の system 情報
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_prompt_system(
    system: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と制御文字の検証
     */
    if system.trim().is_empty() {
        return Err(validation_error(
            "mcp.system",
            "mcp.system must not be empty",
        ));
    }
    if system.chars().any(is_unsupported_text_control) {
        return Err(validation_error(
            "mcp.system",
            "mcp.system must not contain unsupported control characters",
        ));
    }

    /*
     * 文字数の検証
     */
    if system.chars().count() > MAX_MCP_PROMPT_SYSTEM_CHARS {
        return Err(validation_error(
            "mcp.system",
            "mcp.system must be at most 8192 characters",
        ));
    }

    Ok(())
}

///
/// prompt テキスト情報で許可しない制御文字かを判定する
///
/// # 引数
/// * `character` - 判定対象文字
///
/// # 戻り値
/// タブ、LF、CR以外の制御文字の場合は `true` を返す。
///
fn is_unsupported_text_control(character: char) -> bool {
    character.is_control()
        && !matches!(character, '\t' | '\n' | '\r')
}

///
/// prompt 引数定義
///
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpPromptArgumentFrontMatter {
    /// 引数名
    name: String,

    /// 引数説明
    description: String,

    /// 必須可否
    required: Option<bool>,
}

impl McpPromptArgumentFrontMatter {
    ///
    /// 妥当性を検証する
    ///
    /// # 引数
    /// * `index` - 引数インデックス
    ///
    /// # 戻り値
    /// 妥当な場合は `Ok(())` を返す。
    ///
    fn validate(
        &self,
        index: usize,
    ) -> Result<(), FrontMatterValidationError> {
        let name_path = format!("mcp.arguments[{}].name", index);
        validate_prompt_argument_name(&self.name, &name_path)?;

        let description_path =
            format!("mcp.arguments[{}].description", index);
        validate_prompt_argument_description(
            &self.description,
            &description_path,
        )?;

        Ok(())
    }
}

///
/// MCP prompt 引数名の妥当性を検証する
///
/// # 引数
/// * `name` - 検証対象の引数名
/// * `property_path` - エラーへ設定するプロパティパス
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_prompt_argument_name(
    name: &str,
    property_path: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と文字数の検証
     */
    if name.is_empty() {
        return Err(validation_error(
            property_path,
            "argument name must not be empty",
        ));
    }
    if name.chars().count() > MAX_MCP_PROMPT_ARGUMENT_NAME_CHARS {
        return Err(validation_error(
            property_path,
            "argument name must be at most 64 characters",
        ));
    }

    /*
     * 識別子形式の検証
     */
    if !is_valid_prompt_argument_name(name) {
        return Err(validation_error(
            property_path,
            "argument name must match ^[A-Za-z_][A-Za-z0-9_-]*$",
        ));
    }

    Ok(())
}

///
/// MCP prompt 引数名が許可形式かを判定する
///
/// # 引数
/// * `name` - 判定対象の引数名
///
/// # 戻り値
/// 許可形式の場合は `true` を返す。
///
pub(crate) fn is_valid_prompt_argument_name(name: &str) -> bool {
    if name.chars().count() > MAX_MCP_PROMPT_ARGUMENT_NAME_CHARS {
        return false;
    }
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }

    characters.all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-')
    })
}

///
/// MCP prompt 引数説明の妥当性を検証する
///
/// # 引数
/// * `description` - 検証対象の引数説明
/// * `property_path` - エラーへ設定するプロパティパス
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_prompt_argument_description(
    description: &str,
    property_path: &str,
) -> Result<(), FrontMatterValidationError> {
    /*
     * 空値と制御文字の検証
     */
    if description.trim().is_empty() {
        return Err(validation_error(
            property_path,
            "argument description must not be empty",
        ));
    }
    if description.chars().any(is_unsupported_text_control) {
        return Err(validation_error(
            property_path,
            "argument description must not contain unsupported control characters",
        ));
    }

    /*
     * 文字数の検証
     */
    if description.chars().count()
        > MAX_MCP_PROMPT_ARGUMENT_DESCRIPTION_CHARS
    {
        return Err(validation_error(
            property_path,
            "argument description must be at most 1024 characters",
        ));
    }

    Ok(())
}

impl From<&McpPromptArgumentFrontMatter> for PromptArgumentFrontMatter {
    fn from(value: &McpPromptArgumentFrontMatter) -> Self {
        Self {
            name: value.name.clone(),
            description: value.description.clone(),
            required: value.required,
        }
    }
}

impl PromptPageFrontMatter {
    ///
    /// MCP 名前空間から prompt ページ情報を生成する
    ///
    /// # 引数
    /// * `mcp` - MCP 名前空間
    ///
    /// # 戻り値
    /// `mcp.primitive = prompt` で必要項目が揃う場合は情報を返す。
    ///
    fn from_mcp(mcp: &McpFrontMatter) -> Option<Self> {
        if mcp.primitive != "prompt" {
            return None;
        }

        Some(Self {
            name: mcp.name.clone()?,
            description: mcp.description.clone()?,
            system: mcp.system.clone(),
            arguments: mcp
                .arguments
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(PromptArgumentFrontMatter::from)
                .collect(),
        })
    }
}

impl ResourcePageFrontMatter {
    ///
    /// MCP 名前空間から resource ページ情報を生成する
    ///
    /// # 引数
    /// * `mcp` - MCP 名前空間
    ///
    /// # 戻り値
    /// `mcp.primitive = resource` で必要項目が揃う場合は情報を返す。
    ///
    fn from_mcp(mcp: &McpFrontMatter) -> Option<Self> {
        if mcp.primitive != "resource" {
            return None;
        }

        Some(Self {
            resource_id: mcp.resource_id.clone(),
            name: mcp.name.clone()?,
            description: mcp.description.clone()?,
            mime_type: mcp.mime_type.clone(),
        })
    }
}

///
/// Markdown ソース先頭の front matter を抽出して検証する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// front matter が存在しない場合は `Ok(None)` を返す。
///
pub fn parse_document_front_matter(
    source: &str,
) -> Result<Option<FrontMatter>, FrontMatterError> {
    let extracted = match extract_front_matter(source)? {
        Some(extracted) => extracted,
        None => return Ok(None),
    };

    parse_front_matter(extracted.front_matter())
        .map(Some)
        .map_err(FrontMatterError::Validate)
}

///
/// Markdown ソースの front matter を保存前検証する
///
/// # 引数
/// * `source` - 検証対象の Markdown ソース
///
/// # 戻り値
/// front matter が存在しない、または妥当な場合は `Ok(())` を返す。
///
pub fn validate_document_front_matter(
    source: &str,
) -> Result<(), FrontMatterError> {
    parse_document_front_matter(source).map(|_| ())
}

///
/// Markdown ソースからテンプレートページ情報を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// front matter が存在しない、または `wiki.template` を持たない場合は `Ok(None)` を返す。
///
pub fn extract_template_page_front_matter(
    source: &str,
) -> Result<Option<TemplatePageFrontMatter>, FrontMatterError> {
    Ok(parse_document_front_matter(source)?
        .and_then(|front_matter| front_matter.template_page()))
}

///
/// Markdown ソースから prompt ページ情報を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// front matter が存在しない、または prompt ページでない場合は `Ok(None)` を返す。
///
pub fn extract_prompt_page_front_matter(
    source: &str,
) -> Result<Option<PromptPageFrontMatter>, FrontMatterError> {
    Ok(parse_document_front_matter(source)?
        .and_then(|front_matter| front_matter.prompt_page()))
}

///
/// Markdown ソースから resource ページ情報を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// front matter が存在しない、または resource ページでない場合は `Ok(None)` を返す。
///
pub fn extract_resource_page_front_matter(
    source: &str,
) -> Result<Option<ResourcePageFrontMatter>, FrontMatterError> {
    Ok(parse_document_front_matter(source)?
        .and_then(|front_matter| front_matter.resource_page()))
}

///
/// Markdown ソースを front matter 観点でページ分類する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// 名前空間順にページ用途を返す。用途指定がない場合は空の配列を返す。
///
pub fn classify_page_front_matter(
    source: &str,
) -> Result<Vec<PageFrontMatterKind>, FrontMatterError> {
    Ok(match parse_document_front_matter(source)? {
        Some(front_matter) => front_matter.page_kinds(),
        None => Vec::new(),
    })
}

///
/// Markdown ソースからテンプレート候補表示名を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// `wiki.template.name` を持つ場合はその値を返す。
///
pub fn extract_template_page_name(
    source: &str,
) -> Result<Option<String>, FrontMatterError> {
    Ok(extract_template_page_front_matter(source)?
        .map(|template| template.name().to_string()))
}

///
/// Markdown ソースからテンプレート候補説明文を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// `wiki.template.description` を持つ場合はその値を返す。
///
pub fn extract_template_page_description(
    source: &str,
) -> Result<Option<String>, FrontMatterError> {
    Ok(extract_template_page_front_matter(source)?
        .and_then(|template| template.description().map(str::to_string)))
}

///
/// Markdown ソースからテンプレート候補のマクロ即時展開可否を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// `wiki.template.macro_expand` を持つ場合はその値を返す。
///
pub fn extract_template_page_macro_expand(
    source: &str,
) -> Result<Option<bool>, FrontMatterError> {
    Ok(extract_template_page_front_matter(source)?
        .and_then(|template| template.macro_expand()))
}

///
/// front matter 文字列を YAML としてパースし、内部構造へ変換する
///
/// # 引数
/// * `front_matter` - front matter 本体
///
/// # 戻り値
/// パースと検証に成功した front matter を返す。
///
pub fn parse_front_matter(
    front_matter: &str,
) -> Result<FrontMatter, FrontMatterValidationError> {
    let value = serde_yaml_ng::from_str::<Value>(front_matter).map_err(|err| {
        let location = err.location();
        FrontMatterValidationError::Syntax {
            message: err.to_string(),
            line: location.as_ref().map(|location| location.line()),
            column: location.as_ref().map(|location| location.column()),
        }
    })?;

    let mapping = match value {
        Value::Mapping(mapping) => mapping,
        _ => {
            return Err(validation_error(
                "$",
                "front matter top-level must be object",
            ));
        }
    };

    validate_namespace_is_object(&mapping, "wiki")?;
    validate_namespace_is_object(&mapping, "mcp")?;
    validate_namespace_is_object(&mapping, "custom_meta")?;

    let parsed = serde_yaml_ng::from_value::<FrontMatter>(Value::Mapping(mapping))
        .map_err(|err| FrontMatterValidationError::Validation {
            property_path: "$".to_string(),
            message: err.to_string(),
        })?;
    parsed.validate()?;

    Ok(parsed)
}

///
/// Markdown ソース先頭から front matter を抽出する
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
///
/// # 戻り値
/// front matter が存在しない場合は `Ok(None)` を返す。
/// 先頭に開始区切りがあるのに終端区切りが存在しない場合はエラーを返す。
///
pub fn extract_front_matter(
    source: &str,
) -> Result<Option<ExtractedFrontMatter<'_>>, ExtractFrontMatterError> {
    /*
     * 先頭以外は front matter とみなさない
     */
    if source != "---"
        && !source.starts_with("---\n")
        && !source.starts_with("---\r\n")
    {
        return Ok(None);
    }

    let open_delimiter_len = if source == "---" { 3 } else if source.starts_with("---\r\n") {
        5
    } else {
        4
    };
    let front_matter_start = open_delimiter_len;
    let closing = find_closing_delimiter(source, front_matter_start)
        .ok_or(ExtractFrontMatterError::ClosingDelimiterNotFound)?;

    let closing_line_len = match source[closing..].find('\n') {
        Some(offset) => offset + 1,
        None => source.len() - closing,
    };
    let body_start = closing + closing_line_len;

    Ok(Some(ExtractedFrontMatter {
        raw_block: &source[..closing + closing_line_len],
        front_matter: &source[front_matter_start..closing],
        body: &source[body_start..],
    }))
}

///
/// front matter の終端区切り行を探す
///
/// # 引数
/// * `source` - 解析対象の Markdown ソース
/// * `search_start` - 探索開始位置
///
/// # 戻り値
/// 終端区切り行の先頭位置を返す。
///
fn find_closing_delimiter(source: &str, search_start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = search_start;
    while index < source.len() {
        let line_start = index;
        let line_end = match source[index..].find('\n') {
            Some(offset) => index + offset,
            None => source.len(),
        };
        let line = source[line_start..line_end].trim_end_matches('\r');
        if line == "---" {
            let is_line_start = line_start == 0 || bytes[line_start - 1] == b'\n';
            if is_line_start {
                return Some(line_start);
            }
        }
        if line_end == source.len() {
            break;
        }
        index = line_end + 1;
    }

    None
}

///
/// 名前空間値が object であることを検証する
///
/// # 引数
/// * `mapping` - トップレベル mapping
/// * `key` - 対象キー
///
/// # 戻り値
/// 妥当な場合は `Ok(())` を返す。
///
fn validate_namespace_is_object(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
) -> Result<(), FrontMatterValidationError> {
    let key_value = Value::String(key.to_string());
    let Some(value) = mapping.get(&key_value) else {
        return Ok(());
    };

    if matches!(value, Value::Mapping(_)) {
        return Ok(());
    }

    Err(validation_error(
        key,
        &format!("{} must be object", key),
    ))
}

///
/// 検証エラー生成の補助
///
/// # 引数
/// * `property_path` - 不正箇所
/// * `message` - メッセージ
///
/// # 戻り値
/// 生成した検証エラーを返す。
///
fn validation_error(
    property_path: &str,
    message: &str,
) -> FrontMatterValidationError {
    FrontMatterValidationError::Validation {
        property_path: property_path.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_yaml_ng::Value;

    use super::{
        FrontMatterError,
        FrontMatterValidationError,
        ExtractFrontMatterError,
        PageFrontMatterKind,
        PromptArgumentFrontMatter,
        PromptPageFrontMatter,
        ResourcePageFrontMatter,
        TemplatePageFrontMatter,
        classify_page_front_matter,
        extract_prompt_page_front_matter,
        extract_resource_page_front_matter,
        extract_template_page_description,
        extract_template_page_macro_expand,
        extract_template_page_name,
        extract_template_page_front_matter,
        extract_front_matter,
        parse_document_front_matter,
        parse_front_matter,
    };

    #[test]
    fn extract_front_matter_recognizes_leading_delimiter_block() {
        let source = "---\nkey: value\n---\n# title\n本文";

        let extracted = extract_front_matter(source)
            .expect("extract failed")
            .expect("front matter not found");

        assert_eq!(extracted.front_matter(), "key: value\n");
        assert_eq!(extracted.body(), "# title\n本文");
    }

    #[test]
    fn extract_front_matter_ignores_non_leading_delimiter_block() {
        let source = "# title\n\n---\nkey: value\n---\n本文";

        let extracted = extract_front_matter(source).expect("extract failed");

        assert_eq!(extracted, None);
    }

    #[test]
    fn extract_front_matter_separates_raw_block_and_body() {
        let source = "---\r\nkey: value\r\nlist:\r\n  - item\r\n---\r\n本文\r\n";

        let extracted = extract_front_matter(source)
            .expect("extract failed")
            .expect("front matter not found");

        assert_eq!(
            extracted.raw_block(),
            "---\r\nkey: value\r\nlist:\r\n  - item\r\n---\r\n",
        );
        assert_eq!(extracted.front_matter(), "key: value\r\nlist:\r\n  - item\r\n");
        assert_eq!(extracted.body(), "本文\r\n");
    }

    #[test]
    fn extract_front_matter_accepts_source_without_front_matter() {
        let source = "# title\n本文";

        let extracted = extract_front_matter(source).expect("extract failed");

        assert_eq!(extracted, None);
    }

    #[test]
    fn extract_front_matter_returns_error_when_closing_delimiter_is_missing() {
        let source = "---\nkey: value\n# title\n本文";

        let err = extract_front_matter(source).expect_err("missing error");

        assert_eq!(err, ExtractFrontMatterError::ClosingDelimiterNotFound);
    }

    #[test]
    fn parse_document_front_matter_accepts_source_without_front_matter() {
        let source = "# title\n本文";

        let parsed = parse_document_front_matter(source).expect("parse failed");

        assert!(parsed.is_none());
    }

    #[test]
    fn parse_document_front_matter_exposes_wiki_template() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n---\n# title\n本文";

        let parsed = parse_document_front_matter(source)
            .expect("parse failed")
            .expect("front matter missing");
        let template = parsed.wiki_template().expect("template missing");

        assert_eq!(template.name(), "議事録");
        assert_eq!(template.description(), Some("定例会議"));
        assert_eq!(template.macro_expand(), Some(true));
        assert!(parsed.is_template_page());
        assert_eq!(
            parsed.template_page().expect("template page missing"),
            TemplatePageFrontMatter {
                name: "議事録".to_string(),
                description: Some("定例会議".to_string()),
                macro_expand: Some(true),
            }
        );
        assert_eq!(
            parsed.page_kinds(),
            [PageFrontMatterKind::Template(TemplatePageFrontMatter {
                name: "議事録".to_string(),
                description: Some("定例会議".to_string()),
                macro_expand: Some(true),
            })]
        );
    }

    #[test]
    fn parse_document_front_matter_returns_none_for_missing_wiki_template() {
        let source = "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文";

        let parsed = parse_document_front_matter(source)
            .expect("parse failed")
            .expect("front matter missing");

        assert!(parsed.wiki_template().is_none());
        assert!(parsed
            .wiki()
            .expect("wiki missing")
            .template()
            .is_none());
        assert!(!parsed.is_template_page());
        assert!(parsed.template_page().is_none());
        assert!(parsed.page_kinds().is_empty());
    }

    #[test]
    fn parse_front_matter_exposes_template_and_tags_accessors() {
        let parsed = parse_front_matter(
            "wiki:\n  template:\n    name: 議事録\n  tags:\n    - rust\n    - wiki",
        )
        .expect("parse should succeed");
        let wiki = parsed.wiki().expect("wiki missing");
        let template = wiki.template().expect("template missing");

        assert_eq!(template.name(), "議事録");
        assert_eq!(template.description(), None);
        assert_eq!(template.macro_expand(), None);
        assert_eq!(
            wiki.tags().expect("tags missing"),
            ["rust".to_string(), "wiki".to_string()],
        );
    }

    #[test]
    fn extract_template_page_front_matter_returns_template_page_info() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n---\n# title\n本文";

        let template = extract_template_page_front_matter(source)
            .expect("extract failed")
            .expect("template page missing");

        assert_eq!(template.name(), "議事録");
        assert_eq!(template.description(), Some("定例会議"));
        assert_eq!(template.macro_expand(), Some(true));
    }

    #[test]
    fn extract_template_page_front_matter_returns_none_without_template() {
        let source = "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文";

        let template = extract_template_page_front_matter(source)
            .expect("extract failed");

        assert!(template.is_none());
    }

    #[test]
    fn extract_template_page_front_matter_returns_none_without_front_matter() {
        let source = "# title\n本文";

        let template = extract_template_page_front_matter(source)
            .expect("extract failed");

        assert!(template.is_none());
    }

    ///
    /// prompt ページ情報と引数順序・三状態を取得できることを確認する
    ///
    /// 注記: prompt の全フィールドを抽出し、各アクセサの値を比較する。
    ///
    #[test]
    fn extract_prompt_page_front_matter_returns_prompt_page_info() {
        let source = "---\nmcp:\n  primitive: prompt\n  name: summarize\n  description: 要約する\n  system: 簡潔に回答する\n  arguments:\n    - name: required_arg\n      description: 必須引数\n      required: true\n    - name: optional_arg\n      description: 任意引数\n      required: false\n    - name: unspecified_arg\n      description: 未指定引数\n---\n本文";

        let prompt = extract_prompt_page_front_matter(source)
            .expect("extract failed")
            .expect("prompt page missing");

        assert_eq!(prompt.name(), "summarize");
        assert_eq!(prompt.description(), "要約する");
        assert_eq!(prompt.system(), Some("簡潔に回答する"));
        assert_eq!(
            prompt.arguments(),
            [
                PromptArgumentFrontMatter {
                    name: "required_arg".to_string(),
                    description: "必須引数".to_string(),
                    required: Some(true),
                },
                PromptArgumentFrontMatter {
                    name: "optional_arg".to_string(),
                    description: "任意引数".to_string(),
                    required: Some(false),
                },
                PromptArgumentFrontMatter {
                    name: "unspecified_arg".to_string(),
                    description: "未指定引数".to_string(),
                    required: None,
                },
            ]
        );
        assert_eq!(prompt.arguments()[0].name(), "required_arg");
        assert_eq!(prompt.arguments()[0].description(), "必須引数");
        assert_eq!(prompt.arguments()[0].required(), Some(true));
        assert_eq!(prompt.arguments()[1].required(), Some(false));
        assert_eq!(prompt.arguments()[2].required(), None);
    }

    ///
    /// 引数未指定の prompt を空引数として取得できることを確認する
    ///
    /// 注記: arguments と system を省略した prompt を抽出する。
    ///
    #[test]
    fn extract_prompt_page_front_matter_uses_empty_arguments_when_omitted() {
        let source = "---\nmcp:\n  primitive: prompt\n  name: summarize\n  description: 要約する\n---\n本文";

        let prompt = extract_prompt_page_front_matter(source)
            .expect("extract failed")
            .expect("prompt page missing");

        assert!(prompt.arguments().is_empty());
        assert_eq!(prompt.system(), None);
    }

    ///
    /// resource ページを prompt として抽出しないことを確認する
    ///
    /// 注記: resource のソースに prompt 抽出関数を適用する。
    ///
    #[test]
    fn extract_prompt_page_front_matter_returns_none_for_resource_page() {
        let source = "---\nmcp:\n  primitive: resource\n  name: spec\n  description: 仕様\n---\n本文";

        let prompt = extract_prompt_page_front_matter(source)
            .expect("extract failed");

        assert!(prompt.is_none());
    }

    ///
    /// 通常ページを prompt として抽出しないことを確認する
    ///
    /// 注記: custom_meta のみを持つソースに prompt 抽出関数を適用する。
    ///
    #[test]
    fn extract_prompt_page_front_matter_returns_none_for_normal_page() {
        let source = "---\ncustom_meta:\n  project: alpha\n---\n本文";

        let prompt = extract_prompt_page_front_matter(source)
            .expect("extract failed");

        assert!(prompt.is_none());
    }

    ///
    /// resource ページ情報を取得できることを確認する
    ///
    /// 注記: resource の全フィールドを抽出し、各アクセサの値を比較する。
    ///
    #[test]
    fn extract_resource_page_front_matter_returns_resource_page_info() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_id: docs/spec\n",
            "  name: spec\n",
            "  description: 仕様\n",
            "  mime_type: text/markdown\n",
            "---\n",
            "本文",
        );

        let resource = extract_resource_page_front_matter(source)
            .expect("extract failed")
            .expect("resource page missing");

        assert_eq!(resource.resource_id(), Some("docs/spec"));
        assert_eq!(resource.name(), "spec");
        assert_eq!(resource.description(), "仕様");
        assert_eq!(resource.mime_type(), Some("text/markdown"));
    }

    ///
    /// 任意項目未指定の resource を取得できることを確認する
    ///
    /// 注記: resource_id と mime_type を省略した resource を抽出する。
    ///
    #[test]
    fn extract_resource_page_front_matter_uses_none_for_optional_fields() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  name: spec\n",
            "  description: 仕様\n",
            "---\n",
            "本文",
        );

        let resource = extract_resource_page_front_matter(source)
            .expect("extract failed")
            .expect("resource page missing");

        assert_eq!(resource.resource_id(), None);
        assert_eq!(resource.mime_type(), None);
    }

    ///
    /// prompt ページを resource として抽出しないことを確認する
    ///
    /// 注記: prompt のソースに resource 抽出関数を適用する。
    ///
    #[test]
    fn extract_resource_page_front_matter_returns_none_for_prompt_page() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: prompt\n",
            "  name: summarize\n",
            "  description: 要約する\n",
            "---\n",
            "本文",
        );

        let resource = extract_resource_page_front_matter(source)
            .expect("extract failed");

        assert!(resource.is_none());
    }

    #[test]
    fn classify_page_front_matter_returns_template_for_template_page() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n---\n# title\n本文";

        let kinds = classify_page_front_matter(source).expect("classify failed");
        let kind = kinds.first().expect("page kind missing");

        assert_eq!(kinds.len(), 1);
        assert!(kind.is_template_page());
        assert_eq!(
            kind.template_page().expect("template page missing").name(),
            "議事録",
        );
    }

    #[test]
    fn classify_page_front_matter_returns_empty_without_page_usage() {
        let source = "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文";

        let kinds = classify_page_front_matter(source).expect("classify failed");

        assert!(kinds.is_empty());
    }

    ///
    /// prompt ページを MCP prompt 用途へ分類できることを確認する
    ///
    /// 注記: prompt のソースを分類し、variant と抽出情報を検証する。
    ///
    #[test]
    fn classify_page_front_matter_returns_prompt_for_prompt_page() {
        let source = "---\nmcp:\n  primitive: prompt\n  name: summarize\n  description: 要約する\n---\n本文";

        let kinds = classify_page_front_matter(source).expect("classify failed");
        let kind = kinds.first().expect("page kind missing");

        assert_eq!(kinds.len(), 1);
        assert!(kind.is_mcp_prompt_page());
        assert!(!kind.is_template_page());
        assert_eq!(
            kind.prompt_page().expect("prompt page missing").name(),
            "summarize",
        );
    }

    ///
    /// resource ページを MCP resource 用途へ分類できることを確認する
    ///
    /// 注記: resource のソースを分類し、他用途へ分類されないことを検証する。
    ///
    #[test]
    fn classify_page_front_matter_returns_resource_for_resource_page() {
        let source = "---\nmcp:\n  primitive: resource\n  name: spec\n  description: 仕様\n---\n本文";

        let kinds = classify_page_front_matter(source).expect("classify failed");
        let kind = kinds.first().expect("page kind missing");

        assert_eq!(kinds.len(), 1);
        assert!(kind.is_mcp_resource_page());
        assert!(!kind.is_mcp_prompt_page());
        assert!(kind.prompt_page().is_none());
        assert_eq!(
            kind.resource_page().expect("resource page missing").name(),
            "spec",
        );
    }

    ///
    /// FrontMatter から resource 用途を識別できることを確認する
    ///
    /// 注記: primitive 判定と resource 情報取得 API の組み合わせを検証する。
    ///
    #[test]
    fn front_matter_identifies_resource_page_usage() {
        let source = "---\nmcp:\n  primitive: resource\n  name: spec\n  description: 仕様\n---\n本文";

        let front_matter = parse_document_front_matter(source)
            .expect("parse failed")
            .expect("front matter missing");
        let resource = front_matter
            .resource_page()
            .expect("resource page missing");

        assert!(front_matter.is_mcp_resource_page());
        assert!(!front_matter.is_prompt_page());
        assert_eq!(resource.name(), "spec");
        assert_eq!(resource.description(), "仕様");
    }

    ///
    /// FrontMatter が resource 以外を resource 用途として識別しないことを確認する
    ///
    /// 注記: prompt と mcp namespace 不在の両方で resource 判定が偽になることを検証する。
    ///
    #[test]
    fn front_matter_does_not_identify_non_resource_as_resource_page() {
        let prompt_source = "---\nmcp:\n  primitive: prompt\n  name: summarize\n  description: 要約する\n---\n本文";
        let normal_source = "---\nwiki:\n  tags:\n    - rust\n---\n本文";

        let prompt_front_matter = parse_document_front_matter(prompt_source)
            .expect("parse prompt failed")
            .expect("prompt front matter missing");
        let normal_front_matter = parse_document_front_matter(normal_source)
            .expect("parse normal failed")
            .expect("normal front matter missing");

        assert!(!prompt_front_matter.is_mcp_resource_page());
        assert!(prompt_front_matter.resource_page().is_none());
        assert!(!normal_front_matter.is_mcp_resource_page());
        assert!(normal_front_matter.resource_page().is_none());
    }

    ///
    /// template と resource の複合用途を保持できることを確認する
    ///
    /// 注記: wiki と mcp の用途順序を維持し、resource 情報も保持することを検証する。
    ///
    #[test]
    fn classify_page_front_matter_keeps_template_and_resource_usages() {
        let source = concat!(
            "---\n",
            "wiki:\n",
            "  template:\n",
            "    name: 仕様テンプレート\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_id: docs/spec\n",
            "  name: spec\n",
            "  description: 仕様\n",
            "---\n",
            "本文",
        );

        let kinds = classify_page_front_matter(source).expect("classify failed");

        assert_eq!(
            kinds,
            [
                PageFrontMatterKind::Template(TemplatePageFrontMatter {
                    name: "仕様テンプレート".to_string(),
                    description: None,
                    macro_expand: None,
                }),
                PageFrontMatterKind::McpResource(ResourcePageFrontMatter {
                    resource_id: Some("docs/spec".to_string()),
                    name: "spec".to_string(),
                    description: "仕様".to_string(),
                    mime_type: None,
                }),
            ]
        );
    }

    ///
    /// template と prompt の複合用途を保持できることを確認する
    ///
    /// 注記: 両名前空間を持つソースを分類し、固定順序と内容を比較する。
    ///
    #[test]
    fn classify_page_front_matter_keeps_template_and_prompt_usages() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\nmcp:\n  primitive: prompt\n  name: summarize\n  description: 要約する\n---\n本文";

        let kinds = classify_page_front_matter(source).expect("classify failed");

        assert_eq!(
            kinds,
            [
                PageFrontMatterKind::Template(TemplatePageFrontMatter {
                    name: "議事録".to_string(),
                    description: None,
                    macro_expand: None,
                }),
                PageFrontMatterKind::McpPrompt(PromptPageFrontMatter {
                    name: "summarize".to_string(),
                    description: "要約する".to_string(),
                    system: None,
                    arguments: Vec::new(),
                }),
            ]
        );
    }

    #[test]
    fn classify_page_front_matter_returns_empty_without_front_matter() {
        let source = "# title\n本文";

        let kinds = classify_page_front_matter(source).expect("classify failed");

        assert!(kinds.is_empty());
    }

    ///
    /// 既存ページ用途の分類回帰がないことを確認する
    ///
    /// 注記: template、prompt、resource、用途なし、front matterなしをまとめて検証する。
    ///
    #[test]
    fn classify_page_front_matter_keeps_existing_page_usage_regressions() {
        let cases = [
            (
                "---\nwiki:\n  template:\n    name: 議事録\n---\n本文",
                [true, false, false],
            ),
            (
                "---\nmcp:\n  primitive: prompt\n  name: summarize\n  description: 要約する\n---\n本文",
                [false, true, false],
            ),
            (
                "---\nmcp:\n  primitive: resource\n  name: spec\n  description: 仕様\n---\n本文",
                [false, false, true],
            ),
            (
                "---\nwiki:\n  tags:\n    - rust\n---\n本文",
                [false, false, false],
            ),
            ("# title\n本文", [false, false, false]),
        ];

        for (source, [has_template, has_prompt, has_resource]) in cases {
            let kinds = classify_page_front_matter(source)
                .expect("classify failed");

            assert_eq!(
                kinds.iter().any(PageFrontMatterKind::is_template_page),
                has_template,
            );
            assert_eq!(
                kinds.iter().any(PageFrontMatterKind::is_mcp_prompt_page),
                has_prompt,
            );
            assert_eq!(
                kinds.iter().any(PageFrontMatterKind::is_mcp_resource_page),
                has_resource,
            );
        }
    }

    ///
    /// 不正な resource を分類時に拒否することを確認する
    ///
    /// 注記: resource 値制約違反が通常ページや空分類に落ちないことを検証する。
    ///
    #[test]
    fn classify_page_front_matter_rejects_invalid_resource_before_classification() {
        let source = concat!(
            "---\n",
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_id: builtin/spec\n",
            "  name: spec\n",
            "  description: 仕様\n",
            "---\n",
            "本文",
        );

        let err = classify_page_front_matter(source)
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterError::Validate(
                FrontMatterValidationError::Validation {
                    property_path: "mcp.resource_id".to_string(),
                    message: concat!(
                        "mcp.resource_id must not start with ",
                        "reserved prefix builtin/",
                    )
                    .to_string(),
                },
            ),
        );
    }

    #[test]
    fn extract_template_page_name_returns_template_name() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n---\n# title\n本文";

        let name = extract_template_page_name(source).expect("extract failed");

        assert_eq!(name, Some("議事録".to_string()));
    }

    #[test]
    fn extract_template_page_name_returns_none_without_template() {
        let source = "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文";

        let name = extract_template_page_name(source).expect("extract failed");

        assert_eq!(name, None);
    }

    #[test]
    fn extract_template_page_name_returns_none_without_front_matter() {
        let source = "# title\n本文";

        let name = extract_template_page_name(source).expect("extract failed");

        assert_eq!(name, None);
    }

    #[test]
    fn extract_template_page_description_returns_template_description() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n    description: 定例会議\n---\n# title\n本文";

        let description = extract_template_page_description(source)
            .expect("extract failed");

        assert_eq!(description, Some("定例会議".to_string()));
    }

    #[test]
    fn extract_template_page_description_returns_none_without_description() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n---\n# title\n本文";

        let description = extract_template_page_description(source)
            .expect("extract failed");

        assert_eq!(description, None);
    }

    #[test]
    fn extract_template_page_description_returns_none_without_template() {
        let source = "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文";

        let description = extract_template_page_description(source)
            .expect("extract failed");

        assert_eq!(description, None);
    }

    #[test]
    fn extract_template_page_macro_expand_returns_true() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n    macro_expand: true\n---\n# title\n本文";

        let macro_expand = extract_template_page_macro_expand(source)
            .expect("extract failed");

        assert_eq!(macro_expand, Some(true));
    }

    #[test]
    fn extract_template_page_macro_expand_returns_none_without_macro_expand() {
        let source = "---\nwiki:\n  template:\n    name: 議事録\n---\n# title\n本文";

        let macro_expand = extract_template_page_macro_expand(source)
            .expect("extract failed");

        assert_eq!(macro_expand, None);
    }

    #[test]
    fn extract_template_page_macro_expand_returns_none_without_template() {
        let source = "---\nwiki:\n  tags:\n    - rust\n---\n# title\n本文";

        let macro_expand = extract_template_page_macro_expand(source)
            .expect("extract failed");

        assert_eq!(macro_expand, None);
    }

    #[test]
    fn parse_front_matter_rejects_yaml_syntax_error() {
        let err = parse_front_matter("wiki: [")
            .expect_err("syntax error expected");

        assert!(matches!(
            err,
            FrontMatterValidationError::Syntax { .. }
        ));
    }

    #[test]
    fn parse_front_matter_rejects_non_object_top_level() {
        let err = parse_front_matter("- item")
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "$".to_string(),
                message: "front matter top-level must be object".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_rejects_non_object_wiki_namespace() {
        let err = parse_front_matter("wiki: tagged")
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "wiki".to_string(),
                message: "wiki must be object".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_rejects_non_object_custom_meta_namespace() {
        let err = parse_front_matter("custom_meta: tagged")
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "custom_meta".to_string(),
                message: "custom_meta must be object".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_rejects_empty_tags() {
        let err = parse_front_matter("wiki:\n  tags: []")
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "wiki.tags".to_string(),
                message: "wiki.tags must not be empty".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_rejects_tag_with_whitespace() {
        let err = parse_front_matter("wiki:\n  tags:\n    - rust lang")
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "wiki.tags[0]".to_string(),
                message: "tag must not contain whitespace or control characters".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_keeps_wiki_validation_with_custom_meta() {
        let err = parse_front_matter(
            "wiki:\n  tags:\n    - rust lang\ncustom_meta:\n  project: alpha",
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "wiki.tags[0]".to_string(),
                message: "tag must not contain whitespace or control characters".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_rejects_prompt_without_required_fields() {
        let err = parse_front_matter("mcp:\n  primitive: prompt")
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.name".to_string(),
                message: "mcp.name is required for prompt primitive".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_keeps_prompt_validation_with_custom_meta() {
        let err = parse_front_matter(
            "mcp:\n  primitive: prompt\ncustom_meta:\n  project: alpha",
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.name".to_string(),
                message: "mcp.name is required for prompt primitive".to_string(),
            }
        );
    }

    ///
    /// prompt 名の空文字と空白だけの値を拒否することを確認する
    ///
    /// 注記: 2種類の無効値を順に解析し、同じ検証エラーを比較する。
    ///
    #[test]
    fn parse_front_matter_rejects_empty_prompt_name() {
        for name in ["", "   "] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: \"{}\"\n  description: desc",
                name,
            );
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: "mcp.name".to_string(),
                    message: "mcp.name must not be empty".to_string(),
                }
            );
        }
    }

    ///
    /// prompt 名の先頭・末尾空白を拒否することを確認する
    ///
    /// 注記: 前置空白と後置空白を持つ名前を順に解析する。
    ///
    #[test]
    fn parse_front_matter_rejects_prompt_name_edge_whitespace() {
        for name in [" summarize", "summarize "] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: \"{}\"\n  description: desc",
                name,
            );
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: "mcp.name".to_string(),
                    message: "mcp.name must not have leading or trailing whitespace"
                        .to_string(),
                }
            );
        }
    }

    ///
    /// prompt 名の内部空白とUnicodeを変更せず保持することを確認する
    ///
    /// 注記: 大文字・小文字、日本語、内部空白を含む名前を抽出する。
    ///
    #[test]
    fn parse_front_matter_keeps_prompt_name_without_normalization() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: Project ページ Summary\n  description: desc",
        )
        .expect("parse should succeed");
        let prompt = parsed.prompt_page().expect("prompt page missing");

        assert_eq!(prompt.name(), "Project ページ Summary");
    }

    ///
    /// prompt 名の制御文字を拒否することを確認する
    ///
    /// 注記: YAMLエスケープでBELを含む名前を解析する。
    ///
    #[test]
    fn parse_front_matter_rejects_prompt_name_control_character() {
        let err = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: \"bad\\u0007name\"\n  description: desc",
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.name".to_string(),
                message: "mcp.name must not contain control characters"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt 名の128文字を許可し129文字を拒否することを確認する
    ///
    /// 注記: 日本語1文字を繰り返し、Unicode scalar value単位の境界を検証する。
    ///
    #[test]
    fn parse_front_matter_validates_prompt_name_character_limit() {
        let valid_source = format!(
            "mcp:\n  primitive: prompt\n  name: {}\n  description: desc",
            "名".repeat(128),
        );
        parse_front_matter(&valid_source).expect("128 characters should pass");

        let invalid_source = format!(
            "mcp:\n  primitive: prompt\n  name: {}\n  description: desc",
            "名".repeat(129),
        );
        let err = parse_front_matter(&invalid_source)
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.name".to_string(),
                message: "mcp.name must be at most 128 characters".to_string(),
            }
        );
    }

    ///
    /// prompt 説明の空文字と空白だけの値を拒否することを確認する
    ///
    /// 注記: 2種類の無効値を順に解析し、同じ検証エラーを比較する。
    ///
    #[test]
    fn parse_front_matter_rejects_empty_prompt_description() {
        for description in ["", "   "] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: prompt\n  description: \"{}\"",
                description,
            );
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: "mcp.description".to_string(),
                    message: "mcp.description must not be empty".to_string(),
                }
            );
        }
    }

    ///
    /// prompt 説明の1024文字を許可し1025文字を拒否することを確認する
    ///
    /// 注記: 日本語1文字を繰り返し、Unicode scalar value単位の境界を検証する。
    ///
    #[test]
    fn parse_front_matter_validates_prompt_description_character_limit() {
        let valid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: {}",
            "説".repeat(1024),
        );
        parse_front_matter(&valid_source).expect("1024 characters should pass");

        let invalid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: {}",
            "説".repeat(1025),
        );
        let err = parse_front_matter(&invalid_source)
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.description".to_string(),
                message: "mcp.description must be at most 1024 characters"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt の複数行説明と末尾改行を保持することを確認する
    ///
    /// 注記: literal block scalarを解析し、抽出後の文字列を比較する。
    ///
    #[test]
    fn parse_front_matter_keeps_multiline_prompt_description() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: |\n    first\n    second\n",
        )
        .expect("parse should succeed");
        let prompt = parsed.prompt_page().expect("prompt page missing");

        assert_eq!(prompt.description(), "first\nsecond\n");
    }

    ///
    /// prompt system の未指定とnullを許可することを確認する
    ///
    /// 注記: systemなしと明示的nullの2種類を解析して抽出値を比較する。
    ///
    #[test]
    fn parse_front_matter_accepts_omitted_and_null_prompt_system() {
        for system_line in ["", "  system: null\n"] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n{}",
                system_line,
            );
            let parsed =
                parse_front_matter(&source).expect("parse should succeed");
            let prompt = parsed.prompt_page().expect("prompt page missing");

            assert_eq!(prompt.system(), None);
        }
    }

    ///
    /// prompt system の空文字と空白だけの値を拒否することを確認する
    ///
    /// 注記: 空文字、空白、改行だけの値を順に解析する。
    ///
    #[test]
    fn parse_front_matter_rejects_empty_prompt_system() {
        for system in ["\"\"", "\"   \"", "\"\\n\\t\\r\""] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: {}",
                system,
            );
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: "mcp.system".to_string(),
                    message: "mcp.system must not be empty".to_string(),
                }
            );
        }
    }

    ///
    /// prompt system の8192文字を許可し8193文字を拒否することを確認する
    ///
    /// 注記: 日本語1文字を繰り返し、Unicode scalar value単位の境界を検証する。
    ///
    #[test]
    fn parse_front_matter_validates_prompt_system_character_limit() {
        let valid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: {}",
            "指".repeat(8192),
        );
        parse_front_matter(&valid_source).expect("8192 characters should pass");

        let invalid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: {}",
            "指".repeat(8193),
        );
        let err = parse_front_matter(&invalid_source)
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.system".to_string(),
                message: "mcp.system must be at most 8192 characters"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt system でタブ、LF、CRを許可することを確認する
    ///
    /// 注記: YAMLエスケープで許可対象制御文字を含む値を解析する。
    ///
    #[test]
    fn parse_front_matter_accepts_prompt_system_text_controls() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: \"first\\tsecond\\nthird\\rfourth\"",
        )
        .expect("parse should succeed");
        let prompt = parsed.prompt_page().expect("prompt page missing");

        assert_eq!(
            prompt.system(),
            Some("first\tsecond\nthird\rfourth"),
        );
    }

    ///
    /// prompt system で許可外の制御文字を拒否することを確認する
    ///
    /// 注記: YAMLエスケープでBELを含む値を解析する。
    ///
    #[test]
    fn parse_front_matter_rejects_prompt_system_control_character() {
        let err = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: \"bad\\u0007system\"",
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.system".to_string(),
                message: "mcp.system must not contain unsupported control characters"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt system の複数行と末尾改行を保持することを確認する
    ///
    /// 注記: literal block scalarを解析し、抽出後の文字列を比較する。
    ///
    #[test]
    fn parse_front_matter_keeps_multiline_prompt_system() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: |\n    first\n    second\n",
        )
        .expect("parse should succeed");
        let prompt = parsed.prompt_page().expect("prompt page missing");

        assert_eq!(prompt.system(), Some("first\nsecond\n"));
    }

    ///
    /// prompt system の先頭・末尾空白を保持することを確認する
    ///
    /// 注記: 引用符付き文字列を解析し、trimされないことを比較する。
    ///
    #[test]
    fn parse_front_matter_keeps_prompt_system_edge_whitespace() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  system: \"  instruction  \"",
        )
        .expect("parse should succeed");
        let prompt = parsed.prompt_page().expect("prompt page missing");

        assert_eq!(prompt.system(), Some("  instruction  "));
    }

    ///
    /// prompt 引数名の許可形式と required 三状態を保持することを確認する
    ///
    /// 注記: 許可形式の引数をまとめて解析し、名前と required を比較する。
    ///
    #[test]
    fn parse_front_matter_accepts_prompt_argument_names_and_required_states() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: first\n    - name: _target\n      description: second\n      required: false\n    - name: target-1\n      description: third\n      required: true",
        )
        .expect("parse should succeed");
        let prompt = parsed.prompt_page().expect("prompt page missing");
        let arguments = prompt.arguments();

        assert_eq!(arguments[0].name(), "target");
        assert_eq!(arguments[0].required(), None);
        assert_eq!(arguments[1].name(), "_target");
        assert_eq!(arguments[1].required(), Some(false));
        assert_eq!(arguments[2].name(), "target-1");
        assert_eq!(arguments[2].required(), Some(true));
    }

    ///
    /// prompt 引数名の不正形式を拒否することを確認する
    ///
    /// 注記: 先頭数字、先頭ハイフン、空白、記号、Unicodeを順に解析する。
    ///
    #[test]
    fn parse_front_matter_rejects_invalid_prompt_argument_names() {
        for name in ["1target", "-target", "target name", "target.name", "対象"] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: \"{}\"\n      description: argument",
                name,
            );
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: "mcp.arguments[0].name".to_string(),
                    message: "argument name must match ^[A-Za-z_][A-Za-z0-9_-]*$"
                        .to_string(),
                }
            );
        }
    }

    ///
    /// prompt 引数名の文字数境界を検証する
    ///
    /// 注記: 64文字を許可し、65文字を拒否する。
    ///
    #[test]
    fn parse_front_matter_validates_prompt_argument_name_character_limit() {
        let valid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: {}\n      description: argument",
            "a".repeat(64),
        );
        parse_front_matter(&valid_source).expect("64 characters should pass");

        let invalid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: {}\n      description: argument",
            "a".repeat(65),
        );
        let err = parse_front_matter(&invalid_source)
            .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.arguments[0].name".to_string(),
                message: "argument name must be at most 64 characters"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt 引数説明の空値、制御文字、文字数境界を検証する
    ///
    /// 注記: 無効値を拒否し、1024文字の説明を許可する。
    ///
    #[test]
    fn parse_front_matter_validates_prompt_argument_description() {
        for description in ["\"\"", "\"   \""] {
            let source = format!(
                "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: {}",
                description,
            );
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");
            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: "mcp.arguments[0].description".to_string(),
                    message: "argument description must not be empty".to_string(),
                }
            );
        }

        let control_source = "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: \"bad\\u0007description\"";
        let control_err = parse_front_matter(control_source)
            .expect_err("control character error expected");
        assert_eq!(
            control_err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.arguments[0].description".to_string(),
                message: "argument description must not contain unsupported control characters"
                    .to_string(),
            }
        );

        let valid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: {}",
            "説".repeat(1024),
        );
        parse_front_matter(&valid_source).expect("1024 characters should pass");

        let invalid_source = format!(
            "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: {}",
            "説".repeat(1025),
        );
        let length_err = parse_front_matter(&invalid_source)
            .expect_err("length error expected");
        assert_eq!(
            length_err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.arguments[0].description".to_string(),
                message: "argument description must be at most 1024 characters"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt 引数名の重複を拒否し、大文字小文字違いを許可することを確認する
    ///
    /// 注記: 完全一致の重複とcase違いをそれぞれ解析する。
    ///
    #[test]
    fn parse_front_matter_validates_prompt_argument_name_uniqueness() {
        let duplicate = "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: first\n    - name: target\n      description: second";
        let err = parse_front_matter(duplicate)
            .expect_err("duplicate error expected");
        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.arguments[1].name".to_string(),
                message: "argument name must be unique within prompt".to_string(),
            }
        );

        let case_distinct = "mcp:\n  primitive: prompt\n  name: prompt\n  description: desc\n  arguments:\n    - name: target\n      description: first\n    - name: Target\n      description: second";
        parse_front_matter(case_distinct)
            .expect("case-distinct names should pass");
    }

    ///
    /// prompt 引数の明示的な空配列を拒否することを
    /// 確認する
    ///
    /// 注記: arguments を空配列として指定し、
    /// 検証エラーを比較する。
    ///
    #[test]
    fn parse_front_matter_rejects_empty_prompt_arguments() {
        let err = parse_front_matter(
            concat!(
                "mcp:\n  primitive: prompt\n  name: prompt\n",
                "  description: desc\n  arguments: []",
            ),
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.arguments".to_string(),
                message: "mcp.arguments must not be empty".to_string(),
            }
        );
    }

    ///
    /// prompt 直下の未知プロパティを拒否することを
    /// 確認する
    ///
    /// 注記: 未知プロパティを指定し、
    /// 対象パスを含む検証エラーを比較する。
    ///
    #[test]
    fn parse_front_matter_rejects_unknown_prompt_property() {
        let err = parse_front_matter(
            concat!(
                "mcp:\n  primitive: prompt\n  name: prompt\n",
                "  description: desc\n  unknown: value",
            ),
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.unknown".to_string(),
                message: "property is not allowed for prompt primitive"
                    .to_string(),
            }
        );
    }

    ///
    /// prompt 引数内の未知プロパティを拒否することを
    /// 確認する
    ///
    /// 注記: 引数へ未知プロパティを指定し、
    /// serde由来の検証エラーを確認する。
    ///
    #[test]
    fn parse_front_matter_rejects_unknown_prompt_argument_property() {
        let err = parse_front_matter(
            concat!(
                "mcp:\n  primitive: prompt\n  name: prompt\n",
                "  description: desc\n  arguments:\n",
                "    - name: target\n      description: argument\n",
                "      unknown: value",
            ),
        )
        .expect_err("validation error expected");

        match err {
            FrontMatterValidationError::Validation {
                property_path,
                message,
            } => {
                assert_eq!(property_path, "$");
                assert!(message.contains("unknown field `unknown`"));
            }
            other => panic!("unexpected error: {}", other),
        }
    }

    ///
    /// resource 専用項目を prompt で拒否することを確認する
    ///
    /// 注記: resource_id と mime_type を prompt primitive へ指定する。
    ///
    #[test]
    fn parse_front_matter_rejects_resource_properties_for_prompt() {
        let cases = [
            (
                concat!(
                    "mcp:\n  primitive: prompt\n  name: prompt\n",
                    "  description: desc\n  resource_id: docs/spec",
                ),
                "mcp.resource_id",
                "mcp.resource_id is not allowed for prompt primitive",
            ),
            (
                concat!(
                    "mcp:\n  primitive: prompt\n  name: prompt\n",
                    "  description: desc\n  mime_type: text/markdown",
                ),
                "mcp.mime_type",
                "mcp.mime_type is not allowed for prompt primitive",
            ),
        ];

        for (source, property_path, message) in cases {
            let err = parse_front_matter(source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: property_path.to_string(),
                    message: message.to_string(),
                }
            );
        }
    }

    #[test]
    fn parse_front_matter_accepts_valid_resource_values() {
        let source = concat!(
            "mcp:\n",
            "  primitive: resource\n",
            "  resource_id: docs/spec.v1\n",
            "  name: Resource Spec\n",
            "  description: \"仕様\\t説明\\n続き\"\n",
            "  mime_type: application/vnd.luwiki+json",
        );

        let parsed = parse_front_matter(source).expect("parse should succeed");
        let resource = parsed.resource_page().expect("resource missing");

        assert_eq!(resource.resource_id(), Some("docs/spec.v1"));
        assert_eq!(resource.name(), "Resource Spec");
        assert_eq!(resource.mime_type(), Some("application/vnd.luwiki+json"));
    }

    ///
    /// resource_id の不正値を拒否することを確認する
    ///
    /// 注記: M4 1.3で確定した明示resource_idの値制約を検証する。
    ///
    #[test]
    fn parse_front_matter_rejects_invalid_resource_ids() {
        let long_id = "a".repeat(513);
        let cases = vec![
            (
                "mcp:\n  primitive: resource\n  resource_id: \"\"\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not be empty",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: \" docs/spec\"\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not have leading or trailing whitespace",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: \"docs/spec\\u0007\"\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not contain control characters",
            ),
            (
                format!(
                    "mcp:\n  primitive: resource\n  resource_id: {}\n  name: spec\n  description: desc",
                    long_id,
                ),
                "mcp.resource_id",
                "mcp.resource_id must be at most 512 characters",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: builtin/spec\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not start with reserved prefix builtin/",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: /docs/spec\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not start or end with /",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: docs/spec/\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not start or end with /",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: docs//spec\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not contain empty path segments",
            ),
            (
                "mcp:\n  primitive: resource\n  resource_id: docs/../spec\n  name: spec\n  description: desc".to_string(),
                "mcp.resource_id",
                "mcp.resource_id must not contain . or .. path segments",
            ),
        ];

        for (source, property_path, message) in cases {
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: property_path.to_string(),
                    message: message.to_string(),
                }
            );
        }
    }

    ///
    /// resource の name / description 不正値を拒否することを確認する
    ///
    /// 注記: name と description の必須性に加えて詳細な値制約を検証する。
    ///
    #[test]
    fn parse_front_matter_rejects_invalid_resource_name_and_description() {
        let long_name = "a".repeat(129);
        let long_description = "a".repeat(1025);
        let cases = vec![
            (
                "mcp:\n  primitive: resource\n  name: \"\"\n  description: desc".to_string(),
                "mcp.name",
                "mcp.name must not be empty",
            ),
            (
                "mcp:\n  primitive: resource\n  name: \" spec\"\n  description: desc".to_string(),
                "mcp.name",
                "mcp.name must not have leading or trailing whitespace",
            ),
            (
                "mcp:\n  primitive: resource\n  name: \"spec\\u0007\"\n  description: desc".to_string(),
                "mcp.name",
                "mcp.name must not contain control characters",
            ),
            (
                format!(
                    "mcp:\n  primitive: resource\n  name: {}\n  description: desc",
                    long_name,
                ),
                "mcp.name",
                "mcp.name must be at most 128 characters",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: \"\"".to_string(),
                "mcp.description",
                "mcp.description must not be empty",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: \"desc\\u0007\"".to_string(),
                "mcp.description",
                "mcp.description must not contain unsupported control characters",
            ),
            (
                format!(
                    "mcp:\n  primitive: resource\n  name: spec\n  description: {}",
                    long_description,
                ),
                "mcp.description",
                "mcp.description must be at most 1024 characters",
            ),
        ];

        for (source, property_path, message) in cases {
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: property_path.to_string(),
                    message: message.to_string(),
                }
            );
        }
    }

    ///
    /// resource の MIME type 不正値を拒否することを確認する
    ///
    /// 注記: M4初期版で受け付ける MIME type essence の範囲を検証する。
    ///
    #[test]
    fn parse_front_matter_rejects_invalid_resource_mime_types() {
        let long_mime_type = format!("{}/{}", "a".repeat(64), "b".repeat(64));
        let cases = vec![
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: \"\"".to_string(),
                "mcp.mime_type",
                "mcp.mime_type must not be empty",
            ),
            (
                format!(
                    "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: {}",
                    long_mime_type,
                ),
                "mcp.mime_type",
                "mcp.mime_type must be at most 128 characters",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: text/プレーン".to_string(),
                "mcp.mime_type",
                "mcp.mime_type must contain ASCII characters only",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: \"text /markdown\"".to_string(),
                "mcp.mime_type",
                "mcp.mime_type must not contain whitespace or control characters",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: text".to_string(),
                "mcp.mime_type",
                "mcp.mime_type must match type/subtype",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: text/markdown/extra".to_string(),
                "mcp.mime_type",
                "mcp.mime_type must match type/subtype",
            ),
            (
                "mcp:\n  primitive: resource\n  name: spec\n  description: desc\n  mime_type: text/markdown;charset=utf-8".to_string(),
                "mcp.mime_type",
                "mcp.mime_type must contain valid MIME type token characters",
            ),
        ];

        for (source, property_path, message) in cases {
            let err = parse_front_matter(&source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: property_path.to_string(),
                    message: message.to_string(),
                }
            );
        }
    }

    ///
    /// resource の必須項目欠落を拒否することを確認する
    ///
    /// 注記: name と description は一覧表示用の必須公開情報として扱う。
    ///
    #[test]
    fn parse_front_matter_rejects_missing_resource_required_fields() {
        let cases = vec![
            (
                "mcp:\n  primitive: resource\n  description: Spec",
                "mcp.name",
                "mcp.name is required for resource primitive",
            ),
            (
                "mcp:\n  primitive: resource\n  name: Page",
                "mcp.description",
                "mcp.description is required for resource primitive",
            ),
        ];

        for (source, property_path, message) in cases {
            let err = parse_front_matter(source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: property_path.to_string(),
                    message: message.to_string(),
                }
            );
        }
    }

    #[test]
    fn parse_front_matter_rejects_resource_only_prompt_properties() {
        let cases = vec![
            (
                "mcp:\n  primitive: resource\n  name: Page\n  description: Spec\n  arguments:\n    - name: target\n      description: desc",
                "mcp.arguments",
                "mcp.arguments is not allowed for resource primitive",
            ),
            (
                "mcp:\n  primitive: resource\n  name: Page\n  description: Spec\n  system: prompt only",
                "mcp.system",
                "mcp.system is not allowed for resource primitive",
            ),
        ];

        for (source, property_path, message) in cases {
            let err = parse_front_matter(source)
                .expect_err("validation error expected");

            assert_eq!(
                err,
                FrontMatterValidationError::Validation {
                    property_path: property_path.to_string(),
                    message: message.to_string(),
                }
            );
        }
    }

    #[test]
    fn parse_front_matter_accepts_supported_m1_schema() {
        let parsed = parse_front_matter(
            "wiki:\n  template:\n    name: 議事録\n    description: 定例会議\n    macro_expand: true\n  tags:\n    - rust\n    - wiki\nmcp:\n  primitive: prompt\n  name: ページ要約\n  description: ページ内容を要約する\n  system: 補助情報\n  arguments:\n    - name: target\n      description: 対象ページ\n      required: true",
        )
        .expect("parse should succeed");

        assert_eq!(
            parsed.mcp().expect("mcp missing").primitive(),
            "prompt",
        );
        assert!(parsed.wiki().is_some());
    }

    #[test]
    fn parse_front_matter_accepts_custom_meta_namespace() {
        let parsed = parse_front_matter(
            "wiki:\n  tags:\n    - rust\ncustom_meta:\n  project: alpha\n  priority: 3\n  flags:\n    reviewed: true",
        )
        .expect("parse should succeed");

        let custom_meta = parsed.custom_meta().expect("custom_meta missing");
        assert_eq!(
            custom_meta.get("project"),
            Some(&Value::String("alpha".to_string())),
        );
        assert_eq!(
            custom_meta.get("priority"),
            Some(&Value::Number(3.into())),
        );
        assert!(parsed.wiki().is_some());
    }

    #[test]
    fn parse_front_matter_accepts_custom_meta_without_builtin_namespaces() {
        let parsed = parse_front_matter(
            "custom_meta:\n  project: alpha\n  flags:\n    reviewed: true",
        )
        .expect("parse should succeed");

        assert!(parsed.wiki().is_none());
        assert!(parsed.mcp().is_none());
        assert!(parsed.custom_meta().is_some());
    }

    #[test]
    fn parse_front_matter_accepts_mcp_with_custom_meta() {
        let parsed = parse_front_matter(
            "mcp:\n  primitive: resource\n  name: Page\n  description: Spec\ncustom_meta:\n  team: docs",
        )
        .expect("parse should succeed");

        assert_eq!(
            parsed.mcp().expect("mcp missing").primitive(),
            "resource",
        );
        assert_eq!(
            parsed
                .custom_meta()
                .expect("custom_meta missing")
                .get("team"),
            Some(&Value::String("docs".to_string())),
        );
    }

    #[test]
    fn parse_front_matter_keeps_existing_mcp_validation_with_custom_meta() {
        let err = parse_front_matter(
            "mcp:\n  primitive: prompt\ncustom_meta:\n  project: alpha",
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.name".to_string(),
                message: "mcp.name is required for prompt primitive".to_string(),
            }
        );
    }

    #[test]
    fn parse_front_matter_rejects_unsupported_mcp_primitive() {
        let err = parse_front_matter(
            "mcp:\n  primitive: tool\n  name: example\n  description: desc",
        )
        .expect_err("validation error expected");

        assert_eq!(
            err,
            FrontMatterValidationError::Validation {
                property_path: "mcp.primitive".to_string(),
                message: "unsupported mcp primitive".to_string(),
            }
        );
    }

    #[test]
    fn parse_document_front_matter_wraps_extract_error() {
        let err = parse_document_front_matter("---\nwiki:\n  tags:\n    - rust")
            .expect_err("extract error expected");

        assert_eq!(
            err,
            FrontMatterError::Extract(
                ExtractFrontMatterError::ClosingDelimiterNotFound,
            )
        );
    }
}
