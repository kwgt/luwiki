/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! Bearerトークン管理操作を提供するモジュール
//!

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Local};
use rand_core::{OsRng, RngCore};
use redb::{ReadableDatabase, ReadableTable};

use super::DatabaseManager;
use crate::database::schema::{
    BEARER_TOKEN_ID_TABLE,
    BEARER_TOKEN_TABLE,
    DbError,
    USER_ID_TABLE,
    USER_INFO_TABLE,
};
use crate::database::types::{
    BearerScopeSet,
    BearerTokenInfo,
    BearerTokenPlaintext,
    TokenHash,
    TokenId,
    UserId,
    UserInfo,
};

/// Bearerトークン平文生成に利用する乱数バイト長
#[allow(dead_code)]
const BEARER_TOKEN_RANDOM_BYTES: usize = 32;

/// Base64 エンコードで利用する文字集合
#[allow(dead_code)]
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

///
/// Bearerトークン失効処理の集計結果
///
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RevokeBearerTokensResult {
    /// 実際に失効状態へ更新した件数
    updated_count: usize,

    /// 既失効または期限切れで警告対象となった件数
    warning_count: usize,
}

#[allow(dead_code)]
impl RevokeBearerTokensResult {
    ///
    /// 失効処理結果を生成する
    ///
    /// # 引数
    /// * `updated_count` - 実際に更新した件数
    /// * `warning_count` - 警告対象件数
    ///
    /// # 戻り値
    /// 失効処理結果を返す。
    ///
    pub(crate) fn new(
        updated_count: usize,
        warning_count: usize,
    ) -> Self {
        Self {
            updated_count,
            warning_count,
        }
    }

    ///
    /// 実際に更新した件数を返す
    ///
    /// # 戻り値
    /// 実際に失効状態へ更新した件数を返す。
    ///
    pub(crate) fn updated_count(&self) -> usize {
        self.updated_count
    }

    ///
    /// 警告対象件数を返す
    ///
    /// # 戻り値
    /// 既失効または期限切れで警告対象となった件数を返す。
    ///
    pub(crate) fn warning_count(&self) -> usize {
        self.warning_count
    }
}

///
/// Bearer認証失敗理由
///
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VerifyBearerTokenFailureReason {
    /// 未発行または照合不一致
    Unissued,

    /// 失効済み
    Revoked(TokenId),

    /// 期限切れ
    Expired(TokenId),

    /// 紐付けユーザ未解決
    UserNotFound(TokenId),
}

#[allow(dead_code)]
impl VerifyBearerTokenFailureReason {
    ///
    /// ログ出力用の理由文字列を返す
    ///
    /// # 戻り値
    /// Bearer認証失敗理由の識別子を返す。
    ///
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Unissued => "unissued",
            Self::Revoked(_) => "revoked",
            Self::Expired(_) => "expired",
            Self::UserNotFound(_) => "user_not_found",
        }
    }

    ///
    /// 特定できた BearerトークンID を返す
    ///
    /// # 戻り値
    /// `token_id` を特定できた場合はそれを返す。
    ///
    pub(crate) fn token_id(&self) -> Option<&TokenId> {
        match self {
            Self::Unissued => None,
            Self::Revoked(token_id) => Some(token_id),
            Self::Expired(token_id) => Some(token_id),
            Self::UserNotFound(token_id) => Some(token_id),
        }
    }
}

///
/// Bearer認証照合結果
///
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct VerifyBearerTokenResult {
    /// 照合に成功した Bearerトークン管理情報
    token_info: BearerTokenInfo,

    /// トークンから解決したユーザ情報
    user_info: UserInfo,
}

#[allow(dead_code)]
impl VerifyBearerTokenResult {
    ///
    /// Bearer認証照合結果を生成する
    ///
    /// # 引数
    /// * `token_info` - 照合に成功した Bearerトークン管理情報
    /// * `user_info` - トークンから解決したユーザ情報
    ///
    /// # 戻り値
    /// Bearer認証照合結果を返す。
    ///
    pub(crate) fn new(
        token_info: BearerTokenInfo,
        user_info: UserInfo,
    ) -> Self {
        Self {
            token_info,
            user_info,
        }
    }

    ///
    /// Bearerトークン管理情報を返す
    ///
    /// # 戻り値
    /// 照合に成功した Bearerトークン管理情報を返す。
    ///
    pub(crate) fn token_info(&self) -> BearerTokenInfo {
        self.token_info.clone()
    }

    ///
    /// ユーザ情報を返す
    ///
    /// # 戻り値
    /// トークンから解決したユーザ情報を返す。
    ///
    pub(crate) fn user_info(&self) -> UserInfo {
        self.user_info.clone()
    }
}

impl DatabaseManager {
    ///
    /// Bearerトークン平文を生成する
    ///
    /// # 戻り値
    /// 256bit 相当の乱数から生成した Base64 文字列を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn generate_bearer_token_plaintext(
        &self,
    ) -> BearerTokenPlaintext {
        /*
         * トークン用乱数の生成
         */
        let mut random = [0u8; BEARER_TOKEN_RANDOM_BYTES];
        OsRng.fill_bytes(&mut random);

        /*
         * Base64 文字列への変換
         */
        BearerTokenPlaintext::new(encode_base64(&random))
    }

    ///
    /// Bearerトークン平文から照合用ハッシュ値を生成する
    ///
    /// # 引数
    /// * `token` - Bearerトークン平文
    ///
    /// # 戻り値
    /// 認証照合および保存で共通利用する SHA-256 ハッシュ値を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn calculate_bearer_token_hash(
        token: &BearerTokenPlaintext,
    ) -> TokenHash {
        TokenHash::from_token(token.expose())
    }

    ///
    /// Bearerトークンを作成する
    ///
    /// # 引数
    /// * `user_name` - 発行対象のユーザ名
    /// * `scopes` - 付与スコープ集合
    /// * `ttl` - トークンTTL
    /// * `name` - 任意のトークン名
    ///
    /// # 戻り値
    /// 作成した Bearerトークン平文と管理情報を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn create_bearer_token(
        &self,
        user_name: &str,
        scopes: BearerScopeSet,
        ttl: Duration,
        name: Option<String>,
    ) -> Result<(BearerTokenPlaintext, BearerTokenInfo)> {
        /*
         * 書き込みトランザクション開始
         */
        let txn = self.db.begin_write()?;

        /*
         * 対象ユーザを解決する
         */
        let key = user_name.to_string();
        let user_id = {
            let id_table = txn.open_table(USER_ID_TABLE)?;
            match id_table.get(&key)? {
                Some(id) => id.value(),
                None => return Err(anyhow!(DbError::UserNotFound)),
            }
        };

        /*
         * Bearerトークンを生成して2テーブルへ登録する
         */
        let created = {
            let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
            let mut token_id_table = txn.open_table(BEARER_TOKEN_ID_TABLE)?;

            loop {
                let plaintext = self.generate_bearer_token_plaintext();
                let token_hash =
                    Self::calculate_bearer_token_hash(&plaintext);

                if token_table.get(token_hash)?.is_some() {
                    continue;
                }

                let info = BearerTokenInfo::new(
                    user_id.clone(),
                    scopes.clone(),
                    ttl,
                    name.clone(),
                );
                let token_id = info.token_id();

                if token_id_table.get(token_id.clone())?.is_some() {
                    continue;
                }

                token_table.insert(token_hash, info.clone())?;
                token_id_table.insert(token_id, token_hash)?;

                break (plaintext, info);
            }
        };

        /*
         * コミット
         */
        txn.commit()?;

        Ok(created)
    }

    ///
    /// BearerトークンIDから管理情報を取得する
    ///
    /// # 引数
    /// * `token_id` - 取得対象の BearerトークンID
    ///
    /// # 戻り値
    /// 対象が存在する場合は管理情報を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn get_bearer_token_info_by_id(
        &self,
        token_id: &TokenId,
    ) -> Result<Option<BearerTokenInfo>> {
        /*
         * BearerトークンIDから照合用ハッシュ値を解決する
         */
        let token_hash =
            match self.get_bearer_token_hash_by_id(token_id)? {
                Some(token_hash) => token_hash,
                None => return Ok(None),
            };

        /*
         * 主テーブルから管理情報を取得する
         */
        let txn = self.db.begin_read()?;
        let token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
        match token_table.get(token_hash)? {
            Some(entry) => Ok(Some(entry.value())),
            None => Ok(None),
        }
    }

    ///
    /// BearerトークンIDから照合用ハッシュ値を取得する
    ///
    /// # 引数
    /// * `token_id` - 取得対象の BearerトークンID
    ///
    /// # 戻り値
    /// 対象が存在する場合は照合用ハッシュ値を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn get_bearer_token_hash_by_id(
        &self,
        token_id: &TokenId,
    ) -> Result<Option<TokenHash>> {
        let txn = self.db.begin_read()?;
        let token_id_table = txn.open_table(BEARER_TOKEN_ID_TABLE)?;
        match token_id_table.get(token_id.clone())? {
            Some(entry) => Ok(Some(entry.value())),
            None => Ok(None),
        }
    }

    ///
    /// Bearerトークン一覧を取得する
    ///
    /// # 戻り値
    /// 主テーブルへ保存されている Bearerトークン管理情報の一覧を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn list_bearer_tokens(&self) -> Result<Vec<BearerTokenInfo>> {
        /*
         * 主テーブルを走査して管理情報を収集する
         */
        let txn = self.db.begin_read()?;
        let token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
        let mut tokens = Vec::new();

        for entry in token_table.iter()? {
            let (_, info) = entry?;
            tokens.push(info.value());
        }

        Ok(tokens)
    }

    ///
    /// Bearerトークン一覧を条件で抽出する
    ///
    /// # 引数
    /// * `user_id` - 対象ユーザIDによるフィルタ
    /// * `revoked_only` - 失効済みトークンのみを対象とするか
    /// * `expired_only` - 期限切れトークンのみを対象とするか
    /// * `now` - 期限切れ判定に用いる現在時刻
    ///
    /// # 戻り値
    /// 条件に一致した Bearerトークン管理情報の一覧を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn filter_bearer_tokens(
        &self,
        user_id: Option<&UserId>,
        revoked_only: bool,
        expired_only: bool,
        now: DateTime<Local>,
    ) -> Result<Vec<BearerTokenInfo>> {
        /*
         * 全件取得後に条件を評価する
         */
        let tokens = self.list_bearer_tokens()?;
        let filtered = tokens
            .into_iter()
            .filter(|info| {
                bearer_token_matches_filters(
                    info,
                    user_id,
                    revoked_only,
                    expired_only,
                    now,
                )
            })
            .collect();

        Ok(filtered)
    }

    ///
    /// Bearerトークンを単体指定で失効する
    ///
    /// # 引数
    /// * `token_id` - 失効対象の BearerトークンID
    ///
    /// # 戻り値
    /// 失効処理の集計結果を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn revoke_bearer_token_by_id(
        &self,
        token_id: &TokenId,
    ) -> Result<RevokeBearerTokensResult> {
        /*
         * BearerトークンIDから照合用ハッシュ値を解決する
         */
        let token_hash = self
            .get_bearer_token_hash_by_id(token_id)?
            .ok_or_else(|| anyhow!("token not found: {}", token_id))?;

        /*
         * 主テーブル上の状態を更新する
         */
        let txn = self.db.begin_write()?;
        let result = {
            let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
            let mut info = token_table
                .get(token_hash)?
                .ok_or_else(|| anyhow!("token not found: {}", token_id))?
                .value();

            let result = revoke_bearer_token_info(&mut info, Local::now());
            if result.updated_count() > 0 {
                token_table.insert(token_hash, info)?;
            }
            result
        };

        txn.commit()?;
        Ok(result)
    }

    ///
    /// Bearerトークンをユーザ指定で失効する
    ///
    /// # 引数
    /// * `user_name` - 失効対象ユーザ名
    ///
    /// # 戻り値
    /// 失効処理の集計結果を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn revoke_bearer_tokens_by_user(
        &self,
        user_name: &str,
    ) -> Result<RevokeBearerTokensResult> {
        /*
         * 対象ユーザを解決する
         */
        let user_id = self
            .get_user_id_by_name(user_name)?
            .ok_or_else(|| anyhow!(DbError::UserNotFound))?;

        self.revoke_bearer_tokens_with_filter(Some(&user_id))
    }

    ///
    /// 全 Bearerトークンを失効する
    ///
    /// # 戻り値
    /// 失効処理の集計結果を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn revoke_all_bearer_tokens(
        &self,
    ) -> Result<RevokeBearerTokensResult> {
        self.revoke_bearer_tokens_with_filter(None)
    }

    ///
    /// 条件に一致する Bearerトークンを失効する
    ///
    /// # 引数
    /// * `user_id` - 対象ユーザIDによるフィルタ
    ///
    /// # 戻り値
    /// 失効処理の集計結果を返す。
    ///
    #[allow(dead_code)]
    fn revoke_bearer_tokens_with_filter(
        &self,
        user_id: Option<&UserId>,
    ) -> Result<RevokeBearerTokensResult> {
        /*
         * 書き込みトランザクション内で全件走査し、必要なものだけ更新する
         */
        let txn = self.db.begin_write()?;
        let result = {
            let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
            let mut updates = Vec::new();
            let mut updated_count = 0;
            let mut warning_count = 0;
            let now = Local::now();

            for entry in token_table.iter()? {
                let (token_hash, info) = entry?;
                let mut info = info.value();

                if let Some(user_id) = user_id {
                    if info.user_id() != *user_id {
                        continue;
                    }
                }

                let result = revoke_bearer_token_info(&mut info, now);
                updated_count += result.updated_count();
                warning_count += result.warning_count();

                if result.updated_count() > 0 {
                    updates.push((token_hash.value(), info));
                }
            }

            for (token_hash, info) in updates {
                token_table.insert(token_hash, info)?;
            }

            RevokeBearerTokensResult::new(updated_count, warning_count)
        };

        txn.commit()?;
        Ok(result)
    }

    ///
    /// Bearerトークンを単体指定で物理削除する
    ///
    /// # 引数
    /// * `token_id` - 削除対象の BearerトークンID
    ///
    /// # 戻り値
    /// 削除件数を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn purge_bearer_token_by_id(
        &self,
        token_id: &TokenId,
    ) -> Result<usize> {
        /*
         * BearerトークンIDから照合用ハッシュ値を解決する
         */
        let token_hash = self
            .get_bearer_token_hash_by_id(token_id)?
            .ok_or_else(|| anyhow!("token not found: {}", token_id))?;

        /*
         * 2テーブルから同一トランザクションで削除する
         */
        let txn = self.db.begin_write()?;
        {
            let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
            let mut token_id_table = txn.open_table(BEARER_TOKEN_ID_TABLE)?;

            let _ = token_table
                .remove(token_hash)?
                .ok_or_else(|| anyhow!("token not found: {}", token_id))?;
            let _ = token_id_table.remove(token_id.clone())?;
        }

        txn.commit()?;
        Ok(1)
    }

    ///
    /// 条件に一致する Bearerトークンを物理削除する
    ///
    /// # 引数
    /// * `expired_only` - 期限切れトークンを削除対象に含めるか
    /// * `revoked_only` - 失効済みトークンを削除対象に含めるか
    /// * `now` - 期限切れ判定に用いる現在時刻
    ///
    /// # 戻り値
    /// 削除件数を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn purge_bearer_tokens(
        &self,
        expired_only: bool,
        revoked_only: bool,
        now: DateTime<Local>,
    ) -> Result<usize> {
        /*
         * 書き込みトランザクション内で削除対象を収集する
         */
        let txn = self.db.begin_write()?;
        let deleted_count = {
            let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
            let mut token_id_table = txn.open_table(BEARER_TOKEN_ID_TABLE)?;
            let mut targets = Vec::new();

            for entry in token_table.iter()? {
                let (token_hash, info) = entry?;
                let info = info.value();

                if !bearer_token_matches_filters(
                    &info,
                    None,
                    revoked_only,
                    expired_only,
                    now,
                ) {
                    continue;
                }

                targets.push((token_hash.value(), info.token_id()));
            }

            for (token_hash, token_id) in &targets {
                let _ = token_table.remove(*token_hash)?;
                let _ = token_id_table.remove(token_id.clone())?;
            }

            targets.len()
        };

        txn.commit()?;
        Ok(deleted_count)
    }

    ///
    /// Bearerトークン平文を照合して認証対象ユーザを解決する
    ///
    /// # 引数
    /// * `token` - Bearerトークン平文
    ///
    /// # 戻り値
    /// 認証に成功した場合は Bearerトークン管理情報とユーザ情報を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn verify_bearer_token(
        &self,
        token: &BearerTokenPlaintext,
    ) -> Result<
        std::result::Result<
            VerifyBearerTokenResult,
            VerifyBearerTokenFailureReason,
        >,
    > {
        /*
         * トークン平文から照合用ハッシュ値を算出する
         */
        let token_hash = Self::calculate_bearer_token_hash(token);

        /*
         * 主テーブルから管理情報を取得し、状態を検証する
         */
        let txn = self.db.begin_read()?;
        let token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
        let token_info = match token_table.get(token_hash)? {
            Some(entry) => entry.value(),
            None => {
                return Ok(Err(VerifyBearerTokenFailureReason::Unissued));
            }
        };

        if token_info.revoked() {
            return Ok(Err(VerifyBearerTokenFailureReason::Revoked(
                token_info.token_id(),
            )));
        }

        if token_info.expire_at() <= Local::now() {
            return Ok(Err(VerifyBearerTokenFailureReason::Expired(
                token_info.token_id(),
            )));
        }

        /*
         * 対象ユーザを解決する
         */
        let user_table = txn.open_table(USER_INFO_TABLE)?;
        let user_info = match user_table.get(token_info.user_id())? {
            Some(entry) => entry.value(),
            None => {
                return Ok(Err(VerifyBearerTokenFailureReason::UserNotFound(
                    token_info.token_id(),
                )));
            }
        };

        Ok(Ok(VerifyBearerTokenResult::new(token_info, user_info)))
    }

    ///
    /// Bearerトークンの TTL 延長要否を判定し、必要時のみ更新する
    ///
    /// # 引数
    /// * `token_id` - 更新対象の BearerトークンID
    /// * `now` - 判定および更新に用いる現在時刻
    ///
    /// # 戻り値
    /// 延長が発生した場合は更新後の有効期限を返す。
    ///
    #[allow(dead_code)]
    pub(crate) fn extend_bearer_token_ttl_if_needed(
        &self,
        token_id: &TokenId,
        now: DateTime<Local>,
    ) -> Result<Option<DateTime<Local>>> {
        /*
         * BearerトークンIDから照合用ハッシュ値を解決する
         */
        let token_hash = match self.get_bearer_token_hash_by_id(token_id)? {
            Some(token_hash) => token_hash,
            None => return Ok(None),
        };

        /*
         * 主テーブル上の管理情報を必要時のみ更新する
         */
        let txn = self.db.begin_write()?;
        let updated_expire_at = {
            let mut token_table = txn.open_table(BEARER_TOKEN_TABLE)?;
            let mut token_info = match token_table.get(token_hash)? {
                Some(entry) => entry.value(),
                None => return Ok(None),
            };

            if !should_extend_bearer_token_ttl(&token_info, now) {
                None
            } else {
                token_info.extend_expire_at(now);
                let updated_expire_at = token_info.expire_at();
                token_table.insert(token_hash, token_info)?;
                Some(updated_expire_at)
            }
        };

        txn.commit()?;
        Ok(updated_expire_at)
    }
}

///
/// バイト列を Base64 文字列へ変換する
///
/// # 引数
/// * `input` - 変換対象のバイト列
///
/// # 戻り値
/// RFC 4648 の標準 alphabet を用いた Base64 文字列を返す。
///
#[allow(dead_code)]
fn encode_base64(input: &[u8]) -> String {
    /*
     * 出力領域の事前確保
     */
    let capacity = input.len().div_ceil(3) * 4;
    let mut encoded = String::with_capacity(capacity);

    /*
     * 3 バイト単位で Base64 へ変換
     */
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let block =
            ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);

        encoded.push(BASE64_ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        encoded.push(BASE64_ALPHABET[((block >> 12) & 0x3f) as usize] as char);

        match chunk.len() {
            3 => {
                encoded.push(
                    BASE64_ALPHABET[((block >> 6) & 0x3f) as usize] as char,
                );
                encoded.push(BASE64_ALPHABET[(block & 0x3f) as usize] as char);
            }
            2 => {
                encoded.push(
                    BASE64_ALPHABET[((block >> 6) & 0x3f) as usize] as char,
                );
                encoded.push('=');
            }
            1 => {
                encoded.push('=');
                encoded.push('=');
            }
            _ => unreachable!("chunk length must be between 1 and 3"),
        }
    }

    encoded
}

///
/// Bearerトークン管理情報が抽出条件に一致するかを判定する
///
/// # 引数
/// * `info` - 判定対象の Bearerトークン管理情報
/// * `user_id` - 対象ユーザIDによるフィルタ
/// * `revoked_only` - 失効済みトークンのみを対象とするか
/// * `expired_only` - 期限切れトークンのみを対象とするか
/// * `now` - 期限切れ判定に用いる現在時刻
///
/// # 戻り値
/// 条件に一致する場合は `true` を返す。
///
#[allow(dead_code)]
fn bearer_token_matches_filters(
    info: &BearerTokenInfo,
    user_id: Option<&UserId>,
    revoked_only: bool,
    expired_only: bool,
    now: DateTime<Local>,
) -> bool {
    /*
     * ユーザ条件を評価する
     */
    if let Some(user_id) = user_id {
        if info.user_id() != *user_id {
            return false;
        }
    }

    /*
     * 状態条件を評価する
     */
    if !revoked_only && !expired_only {
        return true;
    }

    let is_revoked = info.revoked();
    let is_expired = info.expire_at() <= now;
    (revoked_only && is_revoked) || (expired_only && is_expired)
}

///
/// Bearerトークン管理情報へ失効状態を反映する
///
/// # 引数
/// * `info` - 更新対象の Bearerトークン管理情報
/// * `now` - 更新時刻
///
/// # 戻り値
/// 失効処理の集計結果を返す。
///
#[allow(dead_code)]
fn revoke_bearer_token_info(
    info: &mut BearerTokenInfo,
    now: DateTime<Local>,
) -> RevokeBearerTokensResult {
    if info.revoked() || info.expire_at() <= now {
        return RevokeBearerTokensResult::new(0, 1);
    }

    info.revoke(now);
    RevokeBearerTokensResult::new(1, 0)
}

///
/// Bearerトークンの TTL 延長要否を判定する
///
/// # 引数
/// * `info` - 判定対象の Bearerトークン管理情報
/// * `now` - 判定に用いる現在時刻
///
/// # 戻り値
/// TTL 延長が必要な場合は `true` を返す。
///
#[allow(dead_code)]
fn should_extend_bearer_token_ttl(
    info: &BearerTokenInfo,
    now: DateTime<Local>,
) -> bool {
    /*
     * 基準時刻と経過時間を導出する
     */
    let ttl = info.ttl();
    let base_time = info.expire_at() - ttl;
    let elapsed = now - base_time;
    let threshold = ttl / 2;

    elapsed >= threshold
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::should_extend_bearer_token_ttl;
    use crate::database::types::{
        BearerScope,
        BearerScopeSet,
        BearerTokenInfo,
        UserId,
    };

    ///
    /// TTL延長判定が閾値境界と期限切れ後を含めて
    /// 設計どおりに動作することを確認する。
    ///
    /// # 注記
    /// TTLの半分未満、ちょうど半分、半分超過、
    /// 期限切れ後の各ケースを検証する。
    ///
    #[test]
    fn should_extend_bearer_token_ttl_covers_threshold_and_expired_cases() {
        /*
         * 判定対象トークンを生成する
         */
        let ttl = Duration::days(30);
        let token_info = BearerTokenInfo::new(
            UserId::new(),
            BearerScopeSet::from_iter([BearerScope::Read]),
            ttl,
            Some("ttl boundary test".to_string()),
        );
        let base_time = token_info.created_at();

        /*
         * TTLの半分未満では延長しないことを検証する
         */
        assert!(!should_extend_bearer_token_ttl(
            &token_info,
            base_time + Duration::days(14),
        ));

        /*
         * TTLのちょうど半分で延長対象になることを検証する
         */
        assert!(should_extend_bearer_token_ttl(
            &token_info,
            base_time + Duration::days(15),
        ));

        /*
         * TTLの半分超過でも延長対象になることを検証する
         */
        assert!(should_extend_bearer_token_ttl(
            &token_info,
            base_time + Duration::days(20),
        ));

        /*
         * 期限切れ後でも判定自体は延長対象を返すことを検証する
         */
        assert!(should_extend_bearer_token_ttl(
            &token_info,
            base_time + Duration::days(31),
        ));
    }
}
