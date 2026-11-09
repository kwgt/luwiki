/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPハンドラのpromptsを定義するモジュール
//!

use super::{
    AuthContext,
    DatabaseManager,
    GetPromptServiceResult,
    IpAddr,
    ListPromptsServiceResult,
    McpError,
    McpHandler,
    build_get_prompt_audit_record,
    build_get_prompt_error_audit_record,
    build_list_prompts_audit_record,
    build_list_prompts_error_audit_record,
    resolve_audit_user_id,
};

// 可読性確保のため、操作別のimplを子モジュールへ分離する。
impl McpHandler {
    ///
    /// `prompts/list`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// prompt一覧サービス結果を返す。
    ///
    pub(crate) fn handle_list_prompts(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListPromptsServiceResult, McpError> {
        match self.service.list_prompts(auth, db, cursor) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_prompts_audit_record(
                            auth,
                            address,
                            &result,
                            user_id,
                        ),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_prompts_error_audit_record(
                            auth,
                            address,
                            &error,
                            user_id,
                        ),
                    );
                }
                Err(error)
            }
        }
    }

    ///
    /// `prompts/get`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `name` - prompt名
    /// * `arguments` - prompt引数
    ///
    /// # 戻り値
    /// prompt取得サービス結果を返す。
    ///
    pub(crate) fn handle_get_prompt(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        name: &str,
        arguments: Option<
            &serde_json::Map<String, serde_json::Value>,
        >,
    ) -> Result<GetPromptServiceResult, McpError> {
        match self.service.get_prompt(auth, db, name, arguments) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_get_prompt_audit_record(
                            auth,
                            address,
                            name,
                            &result,
                            user_id,
                        ),
                    );
                }
                Ok(result)
            }
            Err(error) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_get_prompt_error_audit_record(
                            auth,
                            address,
                            name,
                            &error,
                            user_id,
                        ),
                    );
                }
                Err(error)
            }
        }
    }


}
