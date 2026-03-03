/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド"help-all"の実装
//!

use anyhow::Result;
use clap::CommandFactory;

use super::CommandContext;
use crate::cmd_args::Options;

///
/// "help-all"サブコマンドのコンテキスト情報をパックした構造体
///
struct HelpAllCommandContext;

impl HelpAllCommandContext {
    ///
    /// ヘルプ情報の出力
    ///
    fn print_help_all() {
        let root = Options::command();
        let mut entries = Vec::new();
        collect_commands(&root, "", true, &mut entries);
        for (path, _description, mut command) in entries {
            println!("\n----------------------------------------------");
            println!("{}\n", path);
            let help = command.render_long_help().to_string();
            for line in help.lines() {
                println!("  {}", line);
            }
        }
    }
}

impl CommandContext for HelpAllCommandContext {
    ///
    /// サブコマンドを実行
    ///
    /// # 戻り値
    /// 全ヘルプの出力に成功した場合は`Ok(())`を返す。
    ///
    fn exec(&self) -> Result<()> {
        Self::print_help_all();
        Ok(())
    }
}

///
/// 再帰的にヘルプ対象コマンドを収集
///
/// # 引数
/// * `cmd` - 収集対象のコマンド定義
/// * `prefix` - 親コマンド名
/// * `include_self` - 自コマンドを含めるか否か
/// * `entries` - 収集結果の格納先
///
/// # 戻り値
/// 収集処理を行うため戻り値はない。
///
fn collect_commands(
    cmd: &clap::Command,
    prefix: &str,
    include_self: bool,
    entries: &mut Vec<(String, String, clap::Command)>,
) {
    /*
     * 自コマンド情報の追加
     */
    if include_self {
        let path = if prefix.is_empty() {
            cmd.get_name().to_string()
        } else {
            prefix.to_string()
        };
        let description = cmd
            .get_long_about()
            .or(cmd.get_about())
            .map(|value| value.to_string())
            .unwrap_or_default();
        entries.push((path, description, cmd.clone()));
    }

    /*
     * サブコマンド情報の再帰収集
     */
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
        entries.push((path.clone(), description, sub.clone()));
        collect_commands(sub, &path, false, entries);
    }
}

///
/// コマンドコンテキストの生成
///
pub(crate) fn build_context(
    _opts: &Options,
) -> Result<Box<dyn CommandContext>> {
    Ok(Box::new(HelpAllCommandContext))
}
