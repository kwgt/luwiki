/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user info"の実装
//!

use anyhow::{anyhow, Result};

use super::CommandContext;
use super::common::format_cli_timestamp;
use crate::cmd_args::{Options, UserInfoOpts};
use crate::database::types::UserAttributeSet;
use crate::database::DatabaseManager;

///
/// "user info"サブコマンドのコンテキスト情報をパックした構造体
///
struct UserInfoCommandContext {
    manager: DatabaseManager,
    username: String,
}

impl UserInfoCommandContext {
    ///
    /// オブジェクトの生成
    ///
    /// # 引数
    /// * `opts` - グローバルオプション
    /// * `sub_opts` - user info オプション
    ///
    /// # 戻り値
    /// 生成したコマンドコンテキストを返す。
    ///
    fn new(opts: &Options, sub_opts: &UserInfoOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            username: sub_opts.user_name(),
        })
    }
}

impl CommandContext for UserInfoCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// 詳細表示に成功した場合は `Ok(())` を返す。
    ///
    fn exec(&self) -> Result<()> {
        /*
         * ユーザ情報を解決する
         */
        let user = self
            .manager
            .get_user_info_by_name(&self.username)?
            .ok_or_else(|| anyhow!("user not found: {}", self.username))?;

        /*
         * 詳細情報を表示する
         */
        print_field("USER ID", &user.id().to_string());
        print_field("USERNAME", &user.username());
        print_field("DISPLAY NAME", &user.display_name());
        print_field(
            "BASIC AUTH",
            if user.allows_basic_auth() {
                "allowed"
            } else {
                "denied"
            },
        );
        print_attributes(&user.attributes());
        print_timestamps(&[("update", user.timestamp())]);

        Ok(())
    }
}

///
/// 属性集合の表示文字列を生成する
///
/// # 引数
/// * `attributes` - 表示対象の属性集合
///
/// # 戻り値
/// 属性なしが識別できる表示文字列を返す。
///
#[cfg_attr(not(test), allow(dead_code))]
fn format_attributes(attributes: &UserAttributeSet) -> String {
    if attributes.is_empty() {
        return "none".to_string();
    }

    attributes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

///
/// 単一値フィールドを整形出力する
///
/// # 引数
/// * `label` - 表示ラベル
/// * `value` - 表示値
///
/// # 戻り値
/// なし
///
fn print_field(label: &str, value: &str) {
    println!("{:<13} {}", format!("{}:", label), value);
}

///
/// 属性集合を整形出力する
///
/// # 引数
/// * `attributes` - 表示対象の属性集合
///
/// # 戻り値
/// なし
///
fn print_attributes(attributes: &UserAttributeSet) {
    println!("ATTRIBUTES:");
    if attributes.is_empty() {
        println!("    - none");
        return;
    }

    for attribute in attributes.iter() {
        println!("    - {}", attribute);
    }
}

///
/// タイムスタンプ群を整形出力する
///
/// # 引数
/// * `timestamps` - ラベル付きタイムスタンプ一覧
///
/// # 戻り値
/// なし
///
fn print_timestamps(timestamps: &[(&str, chrono::DateTime<chrono::Local>)]) {
    println!("TIMESTAMPS:");
    for (label, timestamp) in timestamps {
        println!("    {}: {}", label, format_cli_timestamp(*timestamp));
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &UserInfoOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(UserInfoCommandContext::new(opts, sub_opts)?))
}

#[cfg(test)]
mod tests {
    use super::format_attributes;
    use crate::database::types::{UserAttribute, UserAttributeSet};

    #[test]
    fn format_attributes_shows_empty_set() {
        assert_eq!(format_attributes(&UserAttributeSet::new()), "none");
    }

    #[test]
    fn format_attributes_uses_formal_attribute_name() {
        let attributes =
            UserAttributeSet::from_iter([UserAttribute::NoBasicAuth]);
        assert_eq!(format_attributes(&attributes), "NoBasicAuth");
    }
}
