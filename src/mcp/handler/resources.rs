/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPハンドラのresourcesを定義するモジュール
//!

use crate::database::resource_list::BuiltinResourceContents;

use super::{
    AuditOperation,
    AuditRecord,
    AuditResult,
    AuthContext,
    DatabaseManager,
    IpAddr,
    ListResourcesServiceResult,
    McpError,
    McpHandler,
    ReadResourceServiceResult,
    Utc,
    audit_result_from_error,
    build_list_resources_audit_record,
    build_list_resources_error_audit_record,
    build_read_resource_audit_record,
    build_read_resource_error_audit_record,
    resolve_audit_user_id,
};

// 可読性確保のため、操作別のimplを子モジュールへ分離する。
impl McpHandler {
    ///
    /// 代替ツールの読取処理と監査を行う。
    ///
    /// # 引数
    /// * `auth` - Bearer認証文脈
    /// * `db` - 監査主体を確認するDB
    /// * `address` - 接続元IPアドレス
    ///
    /// # 戻り値
    /// 固定の運用指示本文、または認可エラー。
    ///
    /// # 概要
    /// ページに依存しない取得結果を、本文を含めずに監査する。
    ///
    pub(crate) fn handle_get_agent_instructions(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
    ) -> Result<BuiltinResourceContents, McpError> {
        /*
         * read認可付きで固定本文を取得する
         */
        let result = self.service.get_agent_instructions(auth);

        /*
         * 本文やパスを記録せず結果を監査する
         */
        if let Some(user_id) = resolve_audit_user_id(db, auth) {
            let audit_result = match &result {
                Ok(_) => AuditResult::Success,
                Err(error) => audit_result_from_error(error),
            };
            self.try_record_audit(AuditRecord::new(
                AuditOperation::GetAgentInstructions,
                user_id,
                auth.token_id().cloned(),
                address,
                None,
                audit_result,
                Utc::now(),
                None,
                None,
            ));
        }
        result
    }

    ///
    /// `resources/list`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `cursor` - 次ページcursor
    ///
    /// # 戻り値
    /// resource一覧サービス結果を返す。
    ///
    pub(crate) fn handle_list_resources(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        cursor: Option<&str>,
    ) -> Result<ListResourcesServiceResult, McpError> {
        match self.service.list_resources(auth, db, cursor) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_list_resources_audit_record(
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
                        build_list_resources_error_audit_record(
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
    /// `resources/read`をサービス層へ橋渡しする
    ///
    /// # 引数
    /// * `auth` - 認証文脈
    /// * `db` - データベースマネージャ
    /// * `address` - 入力元IP address
    /// * `uri` - resource URI
    ///
    /// # 戻り値
    /// resource取得サービス結果を返す。
    ///
    pub(crate) fn handle_read_resource(
        &self,
        auth: &AuthContext,
        db: &DatabaseManager,
        address: Option<IpAddr>,
        uri: &str,
    ) -> Result<ReadResourceServiceResult, McpError> {
        match self.service.read_resource(auth, db, uri) {
            Ok(result) => {
                if let Some(user_id) =
                    resolve_audit_user_id(db, auth)
                {
                    self.try_record_audit(
                        build_read_resource_audit_record(
                            auth,
                            address,
                            uri,
                            &result,
                            user_id,
                            self.service.resource_authority(),
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
                        build_read_resource_error_audit_record(
                            auth,
                            address,
                            uri,
                            &error,
                            user_id,
                            self.service.resource_authority(),
                        ),
                    );
                }
                Err(error)
            }
        }
    }

}
