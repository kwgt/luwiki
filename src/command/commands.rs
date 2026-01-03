/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"commands"の実装
//!

use anyhow::Result;
use clap::CommandFactory;

use crate::cmd_args::Options;
use super::CommandContext;

///
/// "commands"サブコマンドのコンテキスト情報をパックした構造体
///
struct CommandsCommandContext;

impl CommandsCommandContext {
    ///
    /// コマンド一覧の出力
    ///
    fn print_commands() {
        let root = Options::command();
        let mut entries = Vec::new();
        collect_commands(&root, "", &mut entries);
        for (path, description) in entries {
            println!("{:<16} {}", path, description);
        }
    }
}

// CommandContextの実装
impl CommandContext for CommandsCommandContext {
    fn exec(&self) -> Result<()> {
        Self::print_commands();
        Ok(())
    }
}

fn collect_commands(
    cmd: &clap::Command,
    prefix: &str,
    entries: &mut Vec<(String, String)>,
) {
    for sub in cmd.get_subcommands() {
        let name = sub.get_name();

        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{} {}", prefix, name)
        };

        let description = sub
            .get_long_about()
            .or(sub.get_about())
            .map(|value| value.to_string())
            .unwrap_or_default();

        entries.push((path.clone(), description));
        collect_commands(sub, &path, entries);
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    _opts: &Options,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(CommandsCommandContext))
}
