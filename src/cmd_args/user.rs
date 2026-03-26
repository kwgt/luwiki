/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"user"のコマンドライン定義
//!

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use super::{ApplyConfig, ShowOptions, Validate};
use crate::cmd_args::config::Config;
use crate::database::types::{UserAttribute, UserAttributeSet};

#[derive(Clone, Args, Debug)]
pub(crate) struct UserCommand {
    #[command(subcommand)]
    pub(crate) subcommand: UserSubCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum UserSubCommand {
    /// ユーザ追加コマンド
    #[command(name = "add", alias = "a")]
    Add(UserAddOpts),

    /// ユーザ情報の削除
    #[command(name = "delete", alias = "d", alias = "del")]
    Delete(UserDeleteOpts),

    /// ユーザ情報の変更
    #[command(name = "edit", alias = "e", alias = "ed")]
    Edit(UserEditOpts),

    /// ユーザ情報の一覧表示
    #[command(name = "list", alias = "l", alias = "ls")]
    List(UserListOpts),

    /// ユーザ情報の詳細表示
    #[command(name = "info")]
    Info(UserInfoOpts),
}

///
/// サブコマンドuser_addのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserAddOpts {
    /// 表示名の指定
    #[arg(short = 'd', long = "display-name", value_name = "NAME")]
    display_name: Option<String>,

    /// 初期ユーザ属性の指定
    #[arg(long = "attribute", value_name = "ATTRIBUTE")]
    attributes: Vec<String>,

    /// 登録するユーザ名
    #[arg()]
    user_name: String,
}

impl UserAddOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 表示名へのアクセサ
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }

    ///
    /// 指定されたユーザ属性集合へのアクセサ
    ///
    /// # 戻り値
    /// 検証済みのユーザ属性集合を返す。
    ///
    pub(crate) fn attributes(&self) -> Result<UserAttributeSet> {
        parse_user_attributes(&self.attributes)
    }

    ///
    /// パスワード入力要否を返す
    ///
    /// # 戻り値
    /// `NoBasicAuth` を含まない場合は `true` を返す。
    ///
    pub(crate) fn requires_password(&self) -> Result<bool> {
        Ok(!self.attributes()?.contains(UserAttribute::NoBasicAuth))
    }
}

// Validateトレイトの実装
impl Validate for UserAddOpts {
    fn validate(&mut self) -> Result<()> {
        self.attributes()?;
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserAddOpts {
    fn show_options(&self) {
        println!("user add command options");
        println!("   user_name:    {}", self.user_name());
        println!("   display_name: {:?}", self.display_name());
        println!("   attributes:   {:?}", self.attributes);
    }
}

///
/// サブコマンドuser_deleteのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserDeleteOpts {
    /// 削除するユーザ名
    #[arg()]
    user_name: String,
}

impl UserDeleteOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }
}

// Validateトレイトの実装
impl Validate for UserDeleteOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserDeleteOpts {
    fn show_options(&self) {
        println!("user delete command options");
        println!("   user_name: {}", self.user_name());
    }
}

///
/// サブコマンドuser_editのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserEditOpts {
    /// 表示名の指定
    #[arg(short = 'd', long = "display-name", value_name = "NEW-NAME")]
    display_name: Option<String>,

    /// パスワードの指定
    #[arg(short = 'p', long = "password")]
    password: bool,

    /// 追加する属性
    #[arg(long = "add-attribute", value_name = "ATTRIBUTE")]
    add_attributes: Vec<String>,

    /// 削除する属性
    #[arg(long = "remove-attribute", value_name = "ATTRIBUTE")]
    remove_attributes: Vec<String>,

    /// 属性を全消去する
    #[arg(long = "clear-attributes")]
    clear_attributes: bool,

    /// 変更対象のユーザ名
    #[arg()]
    user_name: String,
}

impl UserEditOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// ユーザ名を返す
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }

    ///
    /// 表示名へのアクセサ
    ///
    /// # 戻り値
    /// 表示名を返す
    ///
    pub(crate) fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }

    ///
    /// パスワード変更指定へのアクセサ
    ///
    /// # 戻り値
    /// パスワード変更が指定されている場合はtrue
    ///
    pub(crate) fn is_password_change(&self) -> bool {
        self.password
    }

    ///
    /// 追加対象の属性集合へのアクセサ
    ///
    /// # 戻り値
    /// 追加対象の属性集合を返す。
    ///
    pub(crate) fn add_attributes(&self) -> Result<UserAttributeSet> {
        parse_user_attributes(&self.add_attributes)
    }

    ///
    /// 削除対象の属性集合へのアクセサ
    ///
    /// # 戻り値
    /// 削除対象の属性集合を返す。
    ///
    pub(crate) fn remove_attributes(&self) -> Result<UserAttributeSet> {
        parse_user_attributes(&self.remove_attributes)
    }

    ///
    /// 属性全消去指定へのアクセサ
    ///
    /// # 戻り値
    /// 属性全消去が指定されている場合は `true` を返す。
    ///
    pub(crate) fn clear_attributes(&self) -> bool {
        self.clear_attributes
    }

    ///
    /// 属性更新指定の有無を返す
    ///
    /// # 戻り値
    /// いずれかの属性操作が指定されている場合は `true` を返す。
    ///
    pub(crate) fn has_attribute_changes(&self) -> bool {
        self.clear_attributes
            || !self.add_attributes.is_empty()
            || !self.remove_attributes.is_empty()
    }
}

// Validateトレイトの実装
impl Validate for UserEditOpts {
    fn validate(&mut self) -> Result<()> {
        self.add_attributes()?;
        self.remove_attributes()?;

        if self.display_name.is_none()
            && !self.password
            && !self.has_attribute_changes()
        {
            return Err(anyhow!("no update options specified"));
        }

        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserEditOpts {
    fn show_options(&self) {
        println!("user edit command options");
        println!("   user_name:    {}", self.user_name());
        println!("   display_name: {:?}", self.display_name());
        println!("   password:     {:?}", self.is_password_change());
        println!("   add_attrs:    {:?}", self.add_attributes);
        println!("   remove_attrs: {:?}", self.remove_attributes);
        println!("   clear_attrs:  {:?}", self.clear_attributes());
    }
}

///
/// サブコマンドuser_infoのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserInfoOpts {
    /// 詳細表示対象のユーザ名
    #[arg()]
    user_name: String,
}

impl UserInfoOpts {
    ///
    /// ユーザ名へのアクセサ
    ///
    /// # 戻り値
    /// 指定されたユーザ名を返す。
    ///
    pub(crate) fn user_name(&self) -> String {
        self.user_name.clone()
    }
}

// Validateトレイトの実装
impl Validate for UserInfoOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for UserInfoOpts {
    fn apply_config(&mut self, _config: &Config) {}
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserInfoOpts {
    fn show_options(&self) {
        println!("user info command options");
        println!("   user_name: {}", self.user_name());
    }
}

///
/// user_listサブコマンドのソート順
///
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ValueEnum)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum UserListSortMode {
    /// デフォルト（ユーザID順）
    Default,

    /// ユーザ名でソート
    UserName,

    /// 表示名でソート
    DisplayName,

    /// 更新日時でソート
    LastUpdate,
}

///
/// サブコマンドuser_listのオプション
///
#[derive(Clone, Args, Debug)]
pub(crate) struct UserListOpts {
    /// 一覧のソート方法
    #[arg(long = "sort-by", value_name = "MODE")]
    sort_by: Option<UserListSortMode>,

    /// ソートを逆順で行う
    #[arg(short = 'r', long = "reverse-sort")]
    reverse_sort: bool,
}

impl UserListOpts {
    ///
    /// ソートモードへのアクセサ
    ///
    /// # 戻り値
    /// ソートモードを返す
    ///
    pub(crate) fn sort_mode(&self) -> UserListSortMode {
        self.sort_by.unwrap_or(UserListSortMode::Default)
    }

    ///
    /// 逆順ソート指定へのアクセサ
    ///
    /// # 戻り値
    /// 逆順ソートが指定されている場合はtrue
    ///
    pub(crate) fn is_reverse_sort(&self) -> bool {
        self.reverse_sort
    }
}

// ApplyConfigトレイトの実装
impl ApplyConfig for UserListOpts {
    ///
    /// user list サブコマンドへ設定ファイルの値を反映
    ///
    /// # 引数
    /// * `config` - 読み込み済み設定
    ///
    fn apply_config(&mut self, config: &Config) {
        /*
         * ソート設定を未指定項目へ補完
         */
        if self.sort_by.is_none() {
            if let Some(mode) = config.user_list_sort_mode() {
                self.sort_by = Some(mode);
            }
        }

        if !self.reverse_sort {
            if let Some(reverse) = config.user_list_reverse_sort() {
                self.reverse_sort = reverse;
            }
        }
    }
}

// Validateトレイトの実装
impl Validate for UserListOpts {
    fn validate(&mut self) -> Result<()> {
        Ok(())
    }
}

// ShowOptionsトレイトの実装
impl ShowOptions for UserListOpts {
    fn show_options(&self) {
        println!("user list command options");
        println!("   sort_by:      {:?}", self.sort_mode());
        println!("   reverse_sort: {:?}", self.is_reverse_sort());
    }
}

///
/// ユーザ属性指定群の解析
///
/// # 引数
/// * `raw_attributes` - CLI から受け取った属性指定群
///
/// # 戻り値
/// 解析済みのユーザ属性集合を返す。
///
fn parse_user_attributes(raw_attributes: &[String]) -> Result<UserAttributeSet> {
    let mut attributes = UserAttributeSet::new();

    /*
     * 属性指定を順に解析する
     */
    for raw_attribute in raw_attributes {
        let attribute = parse_user_attribute(raw_attribute.trim())?;
        attributes.insert(attribute);
    }

    Ok(attributes)
}

///
/// 単一ユーザ属性指定の解析
///
/// # 引数
/// * `raw_attribute` - CLI 属性指定
///
/// # 戻り値
/// 解析済みのユーザ属性を返す。
///
fn parse_user_attribute(raw_attribute: &str) -> Result<UserAttribute> {
    match raw_attribute {
        "no_basic_auth" => Ok(UserAttribute::NoBasicAuth),
        "" => Err(anyhow!("attribute must not be empty")),
        _ => Err(anyhow!("invalid user attribute: {}", raw_attribute)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    ///
    /// `user add` の属性指定が `NoBasicAuth` へ変換されることを確認
    ///
    /// # 注記
    /// 複数指定と重複除去の両方を一度に検証する。
    ///
    fn user_add_attributes_parse_no_basic_auth() {
        let opts = UserAddOpts {
            display_name: None,
            attributes: vec![
                "no_basic_auth".to_string(),
                "no_basic_auth".to_string(),
            ],
            user_name: "alice".to_string(),
        };

        let attributes = opts.attributes().expect("attributes parse failed");
        assert!(attributes.contains(UserAttribute::NoBasicAuth));
    }

    #[test]
    ///
    /// `user add` で `NoBasicAuth` 指定時にパスワード不要になることを確認
    ///
    /// # 注記
    /// 属性の有無に応じてパスワード入力要否が切り替わることを検証する。
    ///
    fn user_add_requires_password_depends_on_attributes() {
        let plain = UserAddOpts {
            display_name: None,
            attributes: Vec::new(),
            user_name: "alice".to_string(),
        };
        let no_basic = UserAddOpts {
            display_name: None,
            attributes: vec!["no_basic_auth".to_string()],
            user_name: "alice".to_string(),
        };

        assert!(plain.requires_password().expect("plain check failed"));
        assert!(!no_basic.requires_password().expect("attr check failed"));
    }

    #[test]
    ///
    /// `user edit` で属性操作のみを更新対象として受理することを確認
    ///
    /// # 注記
    /// 表示名やパスワードが無くても属性追加があれば検証を通す。
    ///
    fn user_edit_validate_accepts_attribute_only_update() {
        let mut opts = UserEditOpts {
            display_name: None,
            password: false,
            add_attributes: vec!["no_basic_auth".to_string()],
            remove_attributes: Vec::new(),
            clear_attributes: false,
            user_name: "alice".to_string(),
        };

        opts.validate().expect("validate must pass");
    }

    #[test]
    ///
    /// 未定義属性が検証で拒否されることを確認
    ///
    /// # 注記
    /// `user add` と `user edit` の両方で不正属性名が弾かれることを確認する。
    ///
    fn invalid_user_attribute_is_rejected() {
        let mut add_opts = UserAddOpts {
            display_name: None,
            attributes: vec!["unknown".to_string()],
            user_name: "alice".to_string(),
        };
        let mut edit_opts = UserEditOpts {
            display_name: None,
            password: false,
            add_attributes: vec!["unknown".to_string()],
            remove_attributes: Vec::new(),
            clear_attributes: false,
            user_name: "alice".to_string(),
        };

        assert!(add_opts.validate().is_err());
        assert!(edit_opts.validate().is_err());
    }
}
