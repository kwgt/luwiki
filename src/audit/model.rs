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
    Deserialize,
    Eq,
    PartialEq,
    Serialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditOperation {
    /// ページ取得
    Get,

    /// セクション取得
    GetSection,

    /// ページ一覧取得
    List,

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
}

impl AuditOperation {
    ///
    /// 永続化時に利用する安定した操作名を返す
    ///
    /// # 戻り値
    /// 小文字スネークケースの操作名を返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::GetSection => "get_section",
            Self::List => "list",
            Self::Search => "search",
            Self::Create => "create",
            Self::Update => "update",
            Self::Append => "append",
            Self::Rename => "rename",
        }
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
