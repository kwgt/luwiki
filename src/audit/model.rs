/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 監査ログで利用するモデルの骨格を定義するモジュール
//!

use std::net::IpAddr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::types::{TokenId, UserId};

///
/// 監査対象の操作種別
///
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub(crate) enum AuditOperation {
    /// 固定REST操作
    Rest(RestAuditOperation),

    /// ページ取得
    Get,

    /// セクション取得
    GetSection,

    /// ページ一覧取得
    List,

    /// prompt一覧取得
    ListPrompts,

    /// prompt取得
    GetPrompt,

    /// resource一覧取得
    ListResources,

    /// resource取得
    ReadResource,

    /// ページ検索
    Search,

    /// ページ作成
    Create,

    /// ページ更新
    Update,

    /// ページ追記
    Append,

    /// ページリネーム
    Rename,

    /// ページアセット一覧取得
    ListPageAssets,

    /// ページアセット解決
    ResolvePageAsset,

    /// ページアセットメタ情報取得
    GetPageAssetMetadata,

    /// ページアセット本体取得
    GetPageAssetData,

    /// ページアセットアップロード
    UploadPageAsset,

    /// ページアセット削除
    DeletePageAsset,
}

impl AuditOperation {
    ///
    /// 永続化時に利用する安定した操作名を返す
    ///
    /// # 戻り値
    /// MCPはsnake_case、RESTは固定methodとroute templateを返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Rest(operation) => operation.as_str(),
            Self::Get => "get",
            Self::GetSection => "get_section",
            Self::List => "list",
            Self::ListPrompts => "list_prompts",
            Self::GetPrompt => "get_prompt",
            Self::ListResources => "list_resources",
            Self::ReadResource => "read_resource",
            Self::Search => "search",
            Self::Create => "create",
            Self::Update => "update",
            Self::Append => "append",
            Self::Rename => "rename",
            Self::ListPageAssets => "list_page_assets",
            Self::ResolvePageAsset => "resolve_page_asset",
            Self::GetPageAssetMetadata => "get_page_asset_metadata",
            Self::GetPageAssetData => "get_page_asset_data",
            Self::UploadPageAsset => "upload_page_asset",
            Self::DeletePageAsset => "delete_page_asset",
        }
    }
}

///
/// 登録済みHTTPメソッドとroute templateの閉じた監査操作集合
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestAuditOperation {
    /// GET /api/hello
    Hello,
    /// GET /api/users/me
    Me,
    /// POST /api/pages
    CreatePage,
    /// GET /api/pages
    ListPages,
    /// GET /api/pages/deleted
    DeletedPages,
    /// GET /api/pages/id
    PageId,
    /// GET /api/pages/short
    PageShortByPath,
    /// GET /api/pages/template
    Templates,
    /// GET /api/pages/search
    Search,
    /// GET /api/pages/{page_id}/source
    PageSource,
    /// PUT /api/pages/{page_id}/source
    PutPageSource,
    /// GET /api/pages/{page_id}/meta
    PageMeta,
    /// GET /api/pages/{page_id}/parent
    PageParent,
    /// GET /api/pages/{page_id}/short
    PageShort,
    /// GET /api/pages/{page_id}/path
    PagePath,
    /// POST /api/pages/{page_id}/path
    MovePage,
    /// POST /api/pages/{page_id}/revision
    RevisePage,
    /// GET /api/pages/{page_id}/assets
    PageAssets,
    /// POST /api/pages/{page_id}/assets/{file_name}
    UploadPageAsset,
    /// GET /api/pages/{page_id}/assets/{file_name}
    ResolvePageAsset,
    /// POST /api/pages/{page_id}/lock
    AcquireLock,
    /// PUT /api/pages/{page_id}/lock
    RenewLock,
    /// GET /api/pages/{page_id}/lock
    ReadLock,
    /// DELETE /api/pages/{page_id}/lock
    ReleaseLock,
    /// DELETE /api/pages/{page_id}
    DeletePage,
    /// POST /api/assets
    UploadAsset,
    /// GET /api/assets
    ResolveAsset,
    /// GET /api/assets/{asset_id}/data
    AssetData,
    /// GET /api/assets/{asset_id}/meta
    AssetMeta,
    /// DELETE /api/assets/{asset_id}
    DeleteAsset,
}

impl RestAuditOperation {
    ///
    /// 定義済み30操作を返す
    ///
    /// # 戻り値
    /// 固定操作の一覧。
    ///
    pub(crate) const ALL: [Self; 30] = [
        Self::Hello,
        Self::Me,
        Self::CreatePage,
        Self::ListPages,
        Self::DeletedPages,
        Self::PageId,
        Self::PageShortByPath,
        Self::Templates,
        Self::Search,
        Self::PageSource,
        Self::PutPageSource,
        Self::PageMeta,
        Self::PageParent,
        Self::PageShort,
        Self::PagePath,
        Self::MovePage,
        Self::RevisePage,
        Self::PageAssets,
        Self::UploadPageAsset,
        Self::ResolvePageAsset,
        Self::AcquireLock,
        Self::RenewLock,
        Self::ReadLock,
        Self::ReleaseLock,
        Self::DeletePage,
        Self::UploadAsset,
        Self::ResolveAsset,
        Self::AssetData,
        Self::AssetMeta,
        Self::DeleteAsset,
    ];

    ///
    /// 永続化用の固定HTTP操作名を返す
    ///
    /// # 戻り値
    /// methodとroute templateの固定文字列。
    ///
    /// # 概要
    /// 閉じた30組の操作を同じ固定文字列で保存する。
    ///
    pub(crate) fn as_str(self) -> &'static str {
        /*
         * 可変IDを含まない登録済みroute templateを選ぶ
         */
        match self {
            Self::Hello => "GET /api/hello",
            Self::Me => "GET /api/users/me",
            Self::CreatePage => "POST /api/pages",
            Self::ListPages => "GET /api/pages",
            Self::DeletedPages => "GET /api/pages/deleted",
            Self::PageId => "GET /api/pages/id",
            Self::PageShortByPath => "GET /api/pages/short",
            Self::Templates => "GET /api/pages/template",
            Self::Search => "GET /api/pages/search",
            Self::PageSource => "GET /api/pages/{page_id}/source",
            Self::PutPageSource => "PUT /api/pages/{page_id}/source",
            Self::PageMeta => "GET /api/pages/{page_id}/meta",
            Self::PageParent => "GET /api/pages/{page_id}/parent",
            Self::PageShort => "GET /api/pages/{page_id}/short",
            Self::PagePath => "GET /api/pages/{page_id}/path",
            Self::MovePage => "POST /api/pages/{page_id}/path",
            Self::RevisePage => "POST /api/pages/{page_id}/revision",
            Self::PageAssets => "GET /api/pages/{page_id}/assets",
            Self::UploadPageAsset => {
                "POST /api/pages/{page_id}/assets/{file_name}"
            }
            Self::ResolvePageAsset => {
                "GET /api/pages/{page_id}/assets/{file_name}"
            }
            Self::AcquireLock => "POST /api/pages/{page_id}/lock",
            Self::RenewLock => "PUT /api/pages/{page_id}/lock",
            Self::ReadLock => "GET /api/pages/{page_id}/lock",
            Self::ReleaseLock => "DELETE /api/pages/{page_id}/lock",
            Self::DeletePage => "DELETE /api/pages/{page_id}",
            Self::UploadAsset => "POST /api/assets",
            Self::ResolveAsset => "GET /api/assets",
            Self::AssetData => "GET /api/assets/{asset_id}/data",
            Self::AssetMeta => "GET /api/assets/{asset_id}/meta",
            Self::DeleteAsset => "DELETE /api/assets/{asset_id}",
        }
    }

    ///
    /// 固定文字列からREST操作を解決する
    ///
    /// # 引数
    /// * `value` - 永続化された操作名
    ///
    /// # 戻り値
    /// 一致した固定操作。未知の操作はNone。
    ///
    fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.as_str() == value)
    }
}

impl Serialize for AuditOperation {
    ///
    /// MCPとRESTの操作を安定した文字列へ直列化する
    ///
    /// # 引数
    /// * `serializer` - serde直列化器
    ///
    /// # 戻り値
    /// 文字列の直列化結果。
    ///
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AuditOperation {
    ///
    /// 既存MCP文字列又は定義済みREST文字列を読み取る
    ///
    /// # 引数
    /// * `deserializer` - serde逆直列化器
    ///
    /// # 戻り値
    /// 固定操作、又は未知値の拒否。
    ///
    /// # 概要
    /// 既存MCPと固定RESTの文字列だけを受け付ける。
    ///
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /*
         * 固定MCP操作を優先して既存JSONLを読み取る
         */
        let value = String::deserialize(deserializer)?;
        let operation = match value.as_str() {
            "get" => Some(Self::Get),
            "get_section" => Some(Self::GetSection),
            "list" => Some(Self::List),
            "list_prompts" => Some(Self::ListPrompts),
            "get_prompt" => Some(Self::GetPrompt),
            "list_resources" => Some(Self::ListResources),
            "read_resource" => Some(Self::ReadResource),
            "search" => Some(Self::Search),
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "append" => Some(Self::Append),
            "rename" => Some(Self::Rename),
            "list_page_assets" => Some(Self::ListPageAssets),
            "resolve_page_asset" => Some(Self::ResolvePageAsset),
            "get_page_asset_metadata" => Some(Self::GetPageAssetMetadata),
            "get_page_asset_data" => Some(Self::GetPageAssetData),
            "upload_page_asset" => Some(Self::UploadPageAsset),
            "delete_page_asset" => Some(Self::DeletePageAsset),
            _ => RestAuditOperation::from_str(&value).map(Self::Rest),
        };

        /*
         * 未定義操作の保存値を受け付けない
         */
        operation.ok_or_else(|| serde::de::Error::custom(
            "unknown audit operation",
        ))
    }
}

///
/// 監査ログ向け結果分類
///
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    PartialEq,
    Serialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditResult {
    /// 成功
    Success,

    /// スコープ不足
    ScopeDenied,

    /// path prefix 制約違反
    PathPrefixDenied,

    /// ReadOnly 属性による拒否
    ReadOnlyDenied,

    /// 対象未発見
    NotFound,

    /// 競合
    Conflict,

    /// 削除済み対象
    Gone,

    /// ロックによる拒否
    Locked,

    /// 一般操作拒否
    OperationDenied,

    /// 入力不正
    InvalidInput,

    /// 未対応
    Unsupported,

    /// 内部失敗
    InternalError,
}

impl AuditResult {
    ///
    /// 永続化時に利用する安定した結果名を返す
    ///
    /// # 戻り値
    /// 小文字スネークケースの結果名を返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ScopeDenied => "scope_denied",
            Self::PathPrefixDenied => "path_prefix_denied",
            Self::ReadOnlyDenied => "read_only_denied",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Gone => "gone",
            Self::Locked => "locked",
            Self::OperationDenied => "operation_denied",
            Self::InvalidInput => "invalid_input",
            Self::Unsupported => "unsupported",
            Self::InternalError => "internal_error",
        }
    }
}

///
/// 監査ログ1件分のレコード骨格
///
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AuditRecord {
    /// 操作種別
    pub(crate) operation: AuditOperation,

    /// 操作主体のユーザID
    pub(crate) user_id: UserId,

    /// Bearer トークンID
    pub(crate) token_id: Option<TokenId>,

    /// 入力元アドレス
    pub(crate) address: Option<IpAddr>,

    /// 正規化済み対象 path
    pub(crate) target_path: Option<String>,

    /// 操作結果分類
    pub(crate) result: AuditResult,

    /// 記録時刻
    pub(crate) timestamp: DateTime<Utc>,

    /// 補足要約
    pub(crate) summary: Option<String>,

    /// 対象 revision
    pub(crate) revision: Option<u64>,
}

impl AuditRecord {
    ///
    /// 監査レコードを生成する
    ///
    /// # 引数
    /// * `operation` - 操作種別
    /// * `user_id` - 操作主体のユーザID
    /// * `token_id` - Bearer トークンID
    /// * `address` - 入力元アドレス
    /// * `target_path` - 正規化済み対象 path
    /// * `result` - 操作結果分類
    /// * `timestamp` - 記録時刻
    /// * `summary` - 補足要約
    /// * `revision` - 対象 revision
    ///
    /// # 戻り値
    /// 生成した監査レコードを返す。
    ///
    pub(crate) fn new(
        operation: AuditOperation,
        user_id: UserId,
        token_id: Option<TokenId>,
        address: Option<IpAddr>,
        target_path: Option<String>,
        result: AuditResult,
        timestamp: DateTime<Utc>,
        summary: Option<String>,
        revision: Option<u64>,
    ) -> Self {
        Self {
            operation,
            user_id,
            token_id,
            address,
            target_path,
            result,
            timestamp,
            summary,
            revision,
        }
    }

    ///
    /// `append` 集約キーを生成する
    ///
    /// # 戻り値
    /// `append` 成功で key 構成要素が揃う場合は、集約キーを返す。
    ///
    pub(crate) fn append_audit_key(&self) -> Option<AppendAuditKey> {
        if self.operation != AuditOperation::Append
            || self.result != AuditResult::Success
        {
            return None;
        }

        Some(AppendAuditKey {
            user_id: self.user_id.clone(),
            token_id: self.token_id.clone()?,
            target_path: self.target_path.clone()?,
        })
    }
}

///
/// `append` 集約キー
///
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct AppendAuditKey {
    /// 操作主体のユーザID
    pub(crate) user_id: UserId,

    /// Bearer トークンID
    pub(crate) token_id: TokenId,

    /// 集約対象 path
    pub(crate) target_path: String,
}

///
/// `append` 集約時の内部結果分類
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppendAuditOutcome {
    /// amend 相当保存
    Amended,

    /// 新規 revision 保存
    NewRevision,
}

///
/// `append` 集約サマリ生成用の中間情報
///
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AppendAuditSummarySeed {
    /// amend 相当で処理した件数
    pub(crate) amend_count: u64,

    /// 新規 revision として確定した件数
    pub(crate) new_revision_count: u64,

    /// 補足要約の雛形
    pub(crate) last_summary: Option<String>,
}

impl AppendAuditSummarySeed {
    ///
    /// `append` 集約結果を取り込む
    ///
    /// # 引数
    /// * `outcome` - 集約対象の内部結果分類
    /// * `summary` - 補足要約
    ///
    /// # 戻り値
    /// なし
    ///
    pub(crate) fn record(
        &mut self,
        outcome: AppendAuditOutcome,
        summary: Option<&str>,
    ) {
        match outcome {
            AppendAuditOutcome::Amended => {
                self.amend_count += 1;
            }
            AppendAuditOutcome::NewRevision => {
                self.new_revision_count += 1;
            }
        }

        self.last_summary = summary.map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    ///
    /// 既存MCPの18操作とRESTの30操作を固定文字列で往復する
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// 全固定操作のJSON値と逆直列化を比較し、未知・構造化値を拒否する。
    ///
    #[test]
    fn audit_operation_strings_preserve_mcp_and_rest_contract() {
        /*
         * 過去のMCP固定文字列を一つずつ往復する
         */
        let mcp = [
            "get", "get_section", "list", "list_prompts", "get_prompt",
            "list_resources", "read_resource", "search", "create", "update",
            "append", "rename", "list_page_assets", "resolve_page_asset",
            "get_page_asset_metadata", "get_page_asset_data",
            "upload_page_asset", "delete_page_asset",
        ];
        for name in mcp {
            let value = serde_json::Value::String(name.to_string());
            let operation: AuditOperation =
                serde_json::from_value(value.clone()).unwrap();
            assert_eq!(operation.as_str(), name);
            assert_eq!(serde_json::to_value(operation).unwrap(), value);
        }

        /*
         * RESTはタグ付きobjectではなく30個の固定文字列とする
         */
        let expected_rest = [
            "GET /api/hello",
            "GET /api/users/me",
            "POST /api/pages",
            "GET /api/pages",
            "GET /api/pages/deleted",
            "GET /api/pages/id",
            "GET /api/pages/short",
            "GET /api/pages/template",
            "GET /api/pages/search",
            "GET /api/pages/{page_id}/source",
            "PUT /api/pages/{page_id}/source",
            "GET /api/pages/{page_id}/meta",
            "GET /api/pages/{page_id}/parent",
            "GET /api/pages/{page_id}/short",
            "GET /api/pages/{page_id}/path",
            "POST /api/pages/{page_id}/path",
            "POST /api/pages/{page_id}/revision",
            "GET /api/pages/{page_id}/assets",
            "POST /api/pages/{page_id}/assets/{file_name}",
            "GET /api/pages/{page_id}/assets/{file_name}",
            "POST /api/pages/{page_id}/lock",
            "PUT /api/pages/{page_id}/lock",
            "GET /api/pages/{page_id}/lock",
            "DELETE /api/pages/{page_id}/lock",
            "DELETE /api/pages/{page_id}",
            "POST /api/assets",
            "GET /api/assets",
            "GET /api/assets/{asset_id}/data",
            "GET /api/assets/{asset_id}/meta",
            "DELETE /api/assets/{asset_id}",
        ];
        let mut names = std::collections::BTreeSet::new();
        for (rest, expected) in RestAuditOperation::ALL
            .into_iter().zip(expected_rest)
        {
            assert_eq!(rest.as_str(), expected);
            let operation = AuditOperation::Rest(rest);
            let value = serde_json::to_value(operation).unwrap();
            assert_eq!(value, rest.as_str());
            assert_eq!(
                serde_json::from_value::<AuditOperation>(value).unwrap(),
                operation,
            );
            assert!(names.insert(rest.as_str()));
        }
        assert_eq!(names.len(), 30);

        /*
         * 可変ID・未知route・object・型不正を拒否する
         */
        for value in [
            serde_json::json!("GET /api/pages/secret/source"),
            serde_json::json!("GET /api/unknown"),
            serde_json::json!({"rest": "GET /api/hello"}),
            serde_json::json!(null),
        ] {
            assert!(serde_json::from_value::<AuditOperation>(value).is_err());
        }
    }

    ///
    /// 既存結果名と追加結果名を維持しJSONLの項目を増やさない
    ///
    /// # 概要
    /// 確定した型付き情報と既存公開契約をfixtureで照合する。
    ///
    /// # 注記
    /// result往復とRESTレコードのトップレベルfield集合を比較する。
    ///
    #[test]
    fn audit_rest_result_and_record_schema_remain_compatible() {
        /*
         * 既存名と新しいREST失敗分類を往復する
         */
        for result in [
            AuditResult::Success, AuditResult::ScopeDenied,
            AuditResult::PathPrefixDenied, AuditResult::ReadOnlyDenied,
            AuditResult::NotFound, AuditResult::Conflict,
            AuditResult::InvalidInput, AuditResult::Unsupported,
            AuditResult::InternalError, AuditResult::Gone,
            AuditResult::Locked, AuditResult::OperationDenied,
        ] {
            let value = serde_json::to_value(result).unwrap();
            assert_eq!(value, result.as_str());
            assert_eq!(
                serde_json::from_value::<AuditResult>(value).unwrap(),
                result,
            );
        }

        /*
         * REST追加でも9項目の既存JSONLレコード形式を維持する
         */
        let record = AuditRecord::new(
            AuditOperation::Rest(RestAuditOperation::Hello),
            UserId::new(), None, None, None, AuditResult::Success,
            Utc::now(), None, None,
        );
        let value = serde_json::to_value(&record).unwrap();
        let keys = value.as_object().unwrap().keys().map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys, [
            "operation", "user_id", "token_id", "address", "target_path",
            "result", "timestamp", "summary", "revision",
        ].into_iter().collect());
        assert_eq!(
            serde_json::from_value::<AuditRecord>(value).unwrap(), record,
        );
    }

    ///
    /// `append` 成功時だけ集約キーを生成できることを確認する。
    ///
    /// 注記:
    /// 成功時と失敗時で `append_audit_key()` の戻り値を比較する。
    ///
    #[test]
    fn append_audit_key_is_built_only_for_append_success() {
        let user_id = UserId::new();
        let token_id = TokenId::new();
        let success = AuditRecord::new(
            AuditOperation::Append,
            user_id.clone(),
            Some(token_id.clone()),
            None,
            Some("/audit/page".to_string()),
            AuditResult::Success,
            Utc::now(),
            Some("page appended".to_string()),
            Some(3),
        );
        let failure = AuditRecord::new(
            AuditOperation::Append,
            user_id,
            Some(token_id),
            None,
            Some("/audit/page".to_string()),
            AuditResult::Conflict,
            Utc::now(),
            None,
            None,
        );

        let key = success.append_audit_key().expect("key missing");
        assert_eq!(key.target_path, "/audit/page");
        assert!(failure.append_audit_key().is_none());
    }
}
