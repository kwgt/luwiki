/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! MCPハンドラのresourcesを定義するモジュール
//!

use super::{
    AuthContext,
    DatabaseManager,
    IpAddr,
    ListResourcesServiceResult,
    McpError,
    McpHandler,
    ReadResourceServiceResult,
    build_list_resources_audit_record,
    build_list_resources_error_audit_record,
    build_read_resource_audit_record,
    build_read_resource_error_audit_record,
    resolve_audit_user_id,
};

// 可読性確保のため、操作別のimplを子モジュールへ分離する。
impl McpHandler {
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
