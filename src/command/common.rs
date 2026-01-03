/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! サブコマンド共通処理を提供するモジュール
//!

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{anyhow, Result};
#[cfg(not(target_family = "windows"))]
use rpassword::prompt_password;

#[cfg(target_family = "windows")]
use windows_sys::Win32::Foundation::HANDLE;
#[cfg(target_family = "windows")]
use windows_sys::Win32::System::Console::{
    GetConsoleMode, SetConsoleMode, CONSOLE_MODE, ENABLE_LINE_INPUT,
    ENABLE_PROCESSED_INPUT,
};

/// パスワードの最小長
const MIN_PASSWORD_LENGTH: usize = 8;

///
/// パスワード入力とバリデーション
///
/// # 戻り値
/// 入力されたパスワードを返す。
///
pub(crate) fn read_password_with_confirm() -> Result<String> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();

    let use_terminal_input = stdin.is_terminal();
    read_password_with_confirm_from(
        &mut input,
        &mut output,
        use_terminal_input,
    )
}

///
/// パスワード入力とバリデーション
///
/// # 概要
/// 入力されたパスワードと確認入力を検証し、最小長を満たすか確認する。
///
/// # 引数
/// * `input` - パスワードの入力ストリーム
/// * `output` - プロンプトの出力ストリーム
///
/// # 戻り値
/// 入力されたパスワードを返す。
///
fn read_password_with_confirm_from<R, W>(
    input: &mut R,
    output: &mut W,
    use_terminal_input: bool,
) -> Result<String>
where
    R: BufRead,
    W: Write,
{
    let password = read_password_prompt(
        input,
        output,
        "password: ",
        use_terminal_input,
    )?;

    let confirm = read_password_prompt(
        input,
        output,
        "confirm: ",
        use_terminal_input,
    )?;

    if password != confirm {
        return Err(anyhow!("password mismatch"));
    }

    if password.chars().count() < MIN_PASSWORD_LENGTH {
        return Err(anyhow!(
            "password must be at least {} characters",
            MIN_PASSWORD_LENGTH
        ));
    }

    Ok(password)
}

///
/// パスワード入力
///
/// # 引数
/// * `input` - パスワードの入力ストリーム
/// * `output` - プロンプトの出力ストリーム
/// * `prompt` - 表示するプロンプト文字列
///
/// # 戻り値
/// 入力されたパスワードを返す。
///
fn read_password_prompt<R, W>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
    use_terminal_input: bool,
) -> Result<String>
where
    R: BufRead,
    W: Write,
{
    if use_terminal_input {
        return read_password_from_terminal(prompt);
    }

    write!(output, "{}", prompt)?;
    output.flush()?;

    let mut buf = String::new();
    input.read_line(&mut buf)?;

    Ok(buf.trim_end_matches(&['\r', '\n'][..]).to_string())
}

#[cfg(target_family = "windows")]
fn read_password_from_terminal(prompt: &str) -> Result<String> {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::FromRawHandle;

    use windows_sys::core::PCSTR;
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileA, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    /*
     * プロンプト出力
     */
    let stdout = io::stdout();
    let mut output = stdout.lock();
    write!(output, "{}", prompt)?;
    output.flush()?;

    /*
     * コンソール入力ハンドルの取得
     */
    let handle = unsafe {
        CreateFileA(
            b"CONIN$\x00".as_ptr() as PCSTR,
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            INVALID_HANDLE_VALUE,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }

    let mut reader = BufReader::new(unsafe {
        std::fs::File::from_raw_handle(handle as _)
    });

    /*
     * エコー無効化
     */
    let _guard = HiddenInput::new(handle)?;
    let mut buf = String::new();
    let result = reader.read_line(&mut buf);
    result?;

    Ok(buf.trim_end_matches(&['\r', '\n'][..]).to_string())
}

#[cfg(target_family = "windows")]
struct HiddenInput {
    mode: u32,
    handle: HANDLE,
}

#[cfg(target_family = "windows")]
impl HiddenInput {
    fn new(handle: HANDLE) -> Result<Self> {
        let mut mode = 0;
        if unsafe { GetConsoleMode(handle, &mut mode as *mut CONSOLE_MODE) } == 0 {
            return Err(io::Error::last_os_error().into());
        }

        if unsafe { SetConsoleMode(handle, ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT) } == 0 {
            return Err(io::Error::last_os_error().into());
        }

        Ok(Self { mode, handle })
    }

    fn restore(&mut self) {
        unsafe {
            SetConsoleMode(self.handle, self.mode);
        }
    }
}

#[cfg(target_family = "windows")]
impl Drop for HiddenInput {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(not(target_family = "windows"))]
fn read_password_from_terminal(prompt: &str) -> Result<String> {
    Ok(prompt_password(prompt)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_password_with_confirm_fails_on_mismatch() {
        let input_data = "password123\npassword124\n";
        let mut input = Cursor::new(input_data.as_bytes());
        let mut output = Vec::new();

        let result = read_password_with_confirm_from(
            &mut input,
            &mut output,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn read_password_with_confirm_fails_on_short_password() {
        let input_data = "short\nshort\n";
        let mut input = Cursor::new(input_data.as_bytes());
        let mut output = Vec::new();

        let result = read_password_with_confirm_from(
            &mut input,
            &mut output,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn read_password_with_confirm_succeeds() {
        let input_data = "password123\npassword123\n";
        let mut input = Cursor::new(input_data.as_bytes());
        let mut output = Vec::new();

        let result = read_password_with_confirm_from(
            &mut input,
            &mut output,
            false,
        );
        assert_eq!(result.expect("password read failed"), "password123");
    }
}
