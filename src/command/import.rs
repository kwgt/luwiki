/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"import"の実装
//!

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};

use super::CommandContext;
use crate::cmd_args::{ImportOpts, Options};
use crate::database::DatabaseManager;
use crate::export_import::{
    self, ExportBundle, ExportImportPolicy, ExportType,
};

///
/// "import"サブコマンドのコンテキスト情報をパックした構造体
///
struct ImportCommandContext {
    manager: DatabaseManager,
    migrate_prefix: Option<String>,
    user_map: Vec<(String, String)>,
    user_list: bool,
    dry_run: bool,
    fix_broken_link: bool,
    _yes: bool,
    password: Option<String>,
    strict_mode: bool,
    input_path: String,
}

impl ImportCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &ImportOpts) -> Result<Self> {
        Ok(Self {
            manager: opts.open_database()?,
            migrate_prefix: sub_opts.migrate(),
            user_map: parse_user_map(&sub_opts.user_map())?,
            user_list: sub_opts.is_user_list(),
            dry_run: sub_opts.is_dry_run(),
            fix_broken_link: sub_opts.is_fix_broken_link(),
            _yes: sub_opts.is_yes(),
            password: sub_opts.password(),
            strict_mode: sub_opts.is_strict_mode(),
            input_path: sub_opts.input(),
        })
    }
}

impl CommandContext for ImportCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// import に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        let bundle = export_import::read_bundle_from_input(
            &self.input_path,
            self.password.as_deref(),
        )?;

        if self.user_list {
            print_users(&bundle);
            return Ok(());
        }

        let export_type = bundle.manifest.export_type;
        let policy = build_policy(&bundle, self.migrate_prefix.clone())?;
        let validated = export_import::validate_import(
            &self.manager,
            &policy,
            &self.user_map,
            self.strict_mode,
            self.fix_broken_link,
            bundle,
        )?;

        if !self.dry_run {
            export_import::apply_import(
                &self.manager,
                &policy,
                &self.user_map,
                validated,
            )?;
        }

        println!(
            "import completed: type={} dry_run={}",
            export_type.as_str(),
            self.dry_run,
        );
        Ok(())
    }
}

///
/// import 対応ポリシーを構築する
///
fn build_policy(
    bundle: &ExportBundle,
    migrate_prefix: Option<String>,
) -> Result<ExportImportPolicy> {
    match bundle.manifest.export_type {
        ExportType::Backup => {
            if migrate_prefix.is_some() {
                return Err(anyhow!(
                    "backup import does not accept --migrate"
                ));
            }
            Ok(ExportImportPolicy::backup())
        }
        ExportType::Migrate => {
            let prefix = migrate_prefix.ok_or_else(|| {
                anyhow!("migrate import requires --migrate")
            })?;
            Ok(
                ExportImportPolicy::migrate(&bundle.manifest.export_root)?
                    .with_relocate_prefix(prefix),
            )
        }
    }
}

///
/// `--user-map` 指定を解析する
///
fn parse_user_map(entries: &[String]) -> Result<Vec<(String, String)>> {
    let mut mappings = Vec::new();

    for entry in entries {
        let (src, dst) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid user mapping: {}", entry))?;
        mappings.push((src.trim().to_string(), dst.trim().to_string()));
    }

    Ok(mappings)
}

///
/// アーカイブ内の編集者一覧を表示する
///
fn print_users(bundle: &ExportBundle) {
    let users: BTreeMap<&str, &str> = bundle
        .users
        .iter()
        .map(|user| (user.username.as_str(), user.display_name.as_str()))
        .collect();

    for (username, display_name) in users {
        println!("{}\t{}", username, display_name);
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &ImportOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(ImportCommandContext::new(opts, sub_opts)?))
}
