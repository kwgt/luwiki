/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! アセット本文の復号と公開schemaを定義するモジュール
//!

use rmcp::schemars::{Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer};
use serde_json::json;

///
/// 存在する本文をStringとして復号し、値をエラーへ含めない
///
/// # 引数
/// * `deserializer` - 本文値の復号器
///
/// # 戻り値
/// Stringまたは固定の型不正エラー。未指定はdefaultで扱う。
///
pub(super) fn string_input<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)
        .map(Some)
        .map_err(|_| serde::de::Error::custom("asset input must be a string"))
}

///
/// nullを含まない本文schemaを返す
///
/// # 引数
/// * `generator` - schema生成器
///
/// # 戻り値
/// Stringのschema。
///
pub(super) fn string_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<String>()
}

///
/// 本文の排他条件を公開schemaへ追加する
///
/// # 引数
/// * `schema` - 更新する引数object schema
///
/// # 戻り値
/// なし。
///
pub(super) fn exclusive_schema(schema: &mut Schema) {
    /*
     * 本文の既定値を除去し、ちょうど一方の指定を要求する
     */
    if let Some(properties) = schema.get_mut("properties") {
        for name in ["text", "data_base64"] {
            if let Some(field) = properties[name].as_object_mut() {
                field.remove("default");
            }
        }
    }
    schema.insert("oneOf".into(), json!([
        {"required": ["text"], "not": {"required": ["data_base64"]}},
        {"required": ["data_base64"], "not": {"required": ["text"]}}
    ]));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::UploadPageAssetToolArgs;

    ///
    /// 本文のnull・型不正と非nullableな排他schemaを検証する
    /// # 概要
    /// 本文のnull・型不正と非nullableな排他schemaを検証する
    ///
    ///
    /// # 注記
    /// 復号エラーにsentinelが含まれず、空textが保持されることも確認。
    ///
    /// # 戻り値
    /// なし。
    ///
    #[test]
    fn upload_asset_input_schema_and_deserialization() {
        /*
         * 存在する本文はStringだけ受理する
         */
        for field in ["text", "data_base64"] {
            for value in [json!(null), json!(1), json!(false),
                json!(["private-sentinel"]),
                json!({"private-sentinel": "private-sentinel"})]
            {
                let mut args = json!({"path":"/x", "file_name":"a",
                    "mime_type":"text/plain"});
                args[field] = value;
                let error = serde_json::from_value::<UploadPageAssetToolArgs>(
                    args,
                ).unwrap_err().to_string();
                assert!(error.contains("asset input must be a string"));
                assert!(!error.contains("private-sentinel"));
            }
        }
        let args: UploadPageAssetToolArgs = serde_json::from_value(json!({
            "path":"/x", "file_name":"a", "mime_type":"text/plain",
            "text":""
        })).unwrap();
        assert_eq!(args.text.as_deref(), Some(""));
        assert!(args.data_base64.is_none());

        /*
         * schemaは共通3引数を必須とし本文を排他にする
         */
        let schema = serde_json::to_value(
            rmcp::schemars::schema_for!(UploadPageAssetToolArgs),
        ).unwrap();
        assert_eq!(schema["required"], json!([
            "path", "file_name", "mime_type"
        ]));
        for field in ["text", "data_base64"] {
            assert_eq!(schema["properties"][field]["type"], "string");
            assert!(schema["properties"][field].get("default").is_none());
        }
        assert_eq!(schema["oneOf"].as_array().unwrap().len(), 2);
        assert_eq!(schema["oneOf"][0]["not"]["required"],
            json!(["data_base64"]));
        assert_eq!(schema["oneOf"][1]["not"]["required"], json!(["text"]));
    }
}
