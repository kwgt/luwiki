/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPサービスのprompts処理を定義するモジュール
//!

use super::{
    AuthContext,
    DEFAULT_PROMPT_LIST_LIMIT,
    DatabaseManager,
    GetPromptServiceResult,
    HashMap,
    JsonObject,
    JsonValue,
    ListPromptsServiceResult,
    McpError,
    McpErrorCode,
    McpOperation,
    McpService,
    PromptListArgument,
    PromptListItem,
    PromptPageFrontMatter,
    extract_front_matter,
    is_valid_prompt_argument_name,
    parse_front_matter,
    validate_prompt_name,
};

// 可読性確保のため、業務別のimplを子モジュールへ分離する。
impl McpService {
    ///
    /// `prompts/list`を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// prompt一覧取得結果を返す。
    ///
    pub(crate) fn list_prompts(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        cursor: Option<&str>,
    ) -> Result<ListPromptsServiceResult, McpError> {
        /*
         * read scopeとcursorを検証する
         */
        self.ensure_operation_scope(auth, McpOperation::ListPrompts)?;
        if let Some(cursor) = cursor {
            validate_prompt_name(cursor).map_err(|_| {
                McpError::new(
                    McpErrorCode::InvalidInput,
                    "cursor is invalid",
                )
            })?;
        }

        /*
         * 公開可能な候補を取得する
         */
        let mut entries = db.list_prompt_candidates().map_err(|_| {
            McpError::new(
                McpErrorCode::InternalError,
                "internal error",
            )
        })?;
        entries.sort_by(|left, right| left.name().cmp(right.name()));

        /*
         * cursor境界と既定件数を適用する
         */
        let filtered = entries
            .into_iter()
            .filter(|entry| match cursor {
                Some(cursor) => entry.name() > cursor,
                None => true,
            })
            .take(DEFAULT_PROMPT_LIST_LIMIT + 1)
            .collect::<Vec<_>>();
        let has_more = filtered.len() > DEFAULT_PROMPT_LIST_LIMIT;
        let selected = filtered
            .into_iter()
            .take(DEFAULT_PROMPT_LIST_LIMIT)
            .collect::<Vec<_>>();
        let next_cursor = if has_more {
            selected.last().map(|entry| entry.name().to_string())
        } else {
            None
        };

        /*
         * database候補をMCP内部モデルへ変換する
         */
        let items = selected
            .into_iter()
            .map(|entry| {
                let arguments = entry
                    .arguments()
                    .iter()
                    .map(|argument| {
                        PromptListArgument::new(
                            argument.name().to_string(),
                            argument.description().to_string(),
                            argument.required(),
                        )
                    })
                    .collect();
                PromptListItem::new(
                    entry.name().to_string(),
                    entry.description().to_string(),
                    arguments,
                )
            })
            .collect();

        Ok(ListPromptsServiceResult::new(items, next_cursor))
    }

    ///
    /// `prompts/get`を実行する
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// prompt取得結果を返す。
    ///
    pub(crate) fn get_prompt(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        name: &str,
        arguments: Option<&JsonObject<String, JsonValue>>,
    ) -> Result<GetPromptServiceResult, McpError> {
        /*
         * read scopeとprompt名を検証する
         */
        self.ensure_operation_scope(auth, McpOperation::GetPrompt)?;
        validate_prompt_name(name).map_err(|_| {
            McpError::new(
                McpErrorCode::NotFound,
                "prompt not found",
            )
        })?;

        /*
         * 名前索引から最新ページソースを解決する
         */
        let entry = db
            .get_prompt_source_by_name(name)
            .map_err(|_| prompt_internal_error())?
            .ok_or_else(prompt_not_found)?;
        let extracted = extract_front_matter(entry.source())
            .map_err(|_| prompt_internal_error())?
            .ok_or_else(prompt_internal_error)?;
        let front_matter = parse_front_matter(
            extracted.front_matter(),
        )
        .map_err(|_| prompt_internal_error())?;
        let prompt = front_matter
            .prompt_page()
            .ok_or_else(prompt_internal_error)?;
        if prompt.name() != name {
            return Err(prompt_internal_error());
        }

        /*
         * 引数を検証・展開して単一message本文を生成する
         */
        let values = validate_prompt_arguments(&prompt, arguments)?;
        let body = expand_prompt_text(extracted.body(), &values)?;
        let message = match prompt.system() {
            Some(system) => {
                let system = expand_prompt_text(system, &values)?;
                format!("{}\n\n{}", system, body)
            }
            None => body,
        };

        Ok(GetPromptServiceResult::new(
            prompt.description().to_string(),
            message,
            entry.revision(),
        ))
    }

}

///
/// prompt取得の不存在エラーを生成する
///
/// # 戻り値
/// 情報を秘匿したprompt不存在エラーを返す。
///
fn prompt_not_found() -> McpError {
    McpError::new(McpErrorCode::NotFound, "prompt not found")
}

///
/// prompt取得の内部エラーを生成する
///
/// # 戻り値
/// 内部情報を秘匿したエラーを返す。
///
fn prompt_internal_error() -> McpError {
    McpError::new(McpErrorCode::InternalError, "internal error")
}

///
/// prompt要求引数を検証して展開値を構築する
///
/// # 引数
/// * `prompt` - prompt定義
/// * `arguments` - 要求引数
///
/// # 戻り値
/// 宣言済み引数ごとの展開値を返す。
///
fn validate_prompt_arguments(
    prompt: &PromptPageFrontMatter,
    arguments: Option<&JsonObject<String, JsonValue>>,
) -> Result<HashMap<String, String>, McpError> {
    let definitions = prompt
        .arguments()
        .iter()
        .map(|argument| (argument.name(), argument))
        .collect::<HashMap<_, _>>();

    /*
     * 未知引数と値型を検証する
     */
    if let Some(arguments) = arguments {
        for (name, value) in arguments {
            if !definitions.contains_key(name.as_str()) {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    format!("unknown prompt argument: {}", name),
                ));
            }
            if !value.is_string() {
                return Err(McpError::new(
                    McpErrorCode::InvalidInput,
                    format!(
                        "prompt argument must be a string: {}",
                        name,
                    ),
                ));
            }
        }
    }

    /*
     * 必須判定とoptional既定値を適用する
     */
    let mut values = HashMap::new();
    for definition in prompt.arguments() {
        let value = arguments
            .and_then(|arguments| arguments.get(definition.name()))
            .and_then(JsonValue::as_str);
        if definition.required() == Some(true) && value.is_none() {
            return Err(McpError::new(
                McpErrorCode::InvalidInput,
                format!(
                    "required prompt argument is missing: {}",
                    definition.name(),
                ),
            ));
        }
        values.insert(
            definition.name().to_string(),
            value.unwrap_or_default().to_string(),
        );
    }

    Ok(values)
}

///
/// prompt専用placeholderを一回だけ展開する
///
/// # 引数
/// * `text` - 展開対象文字列
/// * `values` - 宣言済み引数ごとの展開値
///
/// # 戻り値
/// 展開済み文字列を返す。
///
fn expand_prompt_text(
    text: &str,
    values: &HashMap<String, String>,
) -> Result<String, McpError> {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    /*
     * placeholder候補を左から一回だけ走査する
     */
    while let Some(start) = rest.find("{{@") {
        result.push_str(&rest[..start]);
        let placeholder = &rest[start..];
        let Some(end) = placeholder.find("}}") else {
            result.push_str(placeholder);
            return Ok(result);
        };
        let token = &placeholder[..end + 2];
        let inner = &placeholder[3..end];

        if let Some(escaped_name) = inner.strip_prefix('@') {
            if is_valid_prompt_argument_name(escaped_name) {
                result.push_str("{{@");
                result.push_str(escaped_name);
                result.push_str("}}");
            } else {
                result.push_str(token);
            }
        } else if is_valid_prompt_argument_name(inner) {
            let value = values
                .get(inner)
                .ok_or_else(prompt_internal_error)?;
            result.push_str(value);
        } else {
            result.push_str(token);
        }
        rest = &placeholder[end + 2..];
    }
    result.push_str(rest);

    Ok(result)
}

