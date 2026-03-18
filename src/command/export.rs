/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"export"の実装
//!

use anyhow::Result;

use super::CommandContext;
use crate::cmd_args::{ExportOpts, Options};
use crate::database::DatabaseManager;
use crate::export_import::{
    self, ExportImportPolicy, ExportRequest,
};

///
/// "export"サブコマンドのコンテキスト情報をパックした構造体
///
struct ExportCommandContext {
    manager: DatabaseManager,
    request: ExportRequest,
    _yes: bool,
    _strict_mode: bool,
}

impl ExportCommandContext {
    ///
    /// オブジェクトの生成
    ///
    fn new(opts: &Options, sub_opts: &ExportOpts) -> Result<Self> {
        let policy = match sub_opts.subtree() {
            Some(subtree) => ExportImportPolicy::migrate(&subtree)?,
            None => ExportImportPolicy::backup(),
        };

        Ok(Self {
            manager: opts.open_database()?,
            request: ExportRequest {
                policy,
                dry_run: sub_opts.is_dry_run(),
                output_path: sub_opts.output(),
                password: sub_opts.password(),
            },
            _yes: sub_opts.is_yes(),
            _strict_mode: sub_opts.is_strict_mode(),
        })
    }
}

impl CommandContext for ExportCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// export に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        let result = export_import::export(&self.manager, self.request.clone())?;

        println!(
            "export completed: type={} dry_run={} pages={} revisions={} assets={}",
            result.export_type.as_str(),
            self.request.dry_run,
            result.bundle.manifest.page_count,
            result.bundle.manifest.revision_count,
            result.bundle.manifest.asset_count,
        );
        Ok(())
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    opts: &Options,
    sub_opts: &ExportOpts,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(ExportCommandContext::new(opts, sub_opts)?))
}
