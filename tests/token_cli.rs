/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use luwiki::database::{
    create_bearer_token_for_test,
    get_bearer_token_snapshot_for_test,
};
use common::{
    TEST_PASSWORD,
    TEST_USERNAME,
    reserve_port,
    prepare_test_dirs,
    run_add_user,
    run_add_user_with_credentials,
    test_binary_path,
    wait_for_server_with_scheme,
};

fn build_base_command(db_path: &Path, assets_dir: &Path) -> Command {
    let mut command = Command::new(test_binary_path());
    let base_dir = db_path.parent().expect("db_path parent missing");
    command
        .env("XDG_CONFIG_HOME", base_dir)
        .env("XDG_DATA_HOME", base_dir)
        .arg("--db-path")
        .arg(db_path)
        .arg("--assets-path")
        .arg(assets_dir)
        .arg("--fts-index")
        .arg(common::fts_index_path(db_path));
    command
}

fn assert_cli_timestamp(value: &str) {
    assert_eq!(value.len(), 19, "unexpected timestamp length: {}", value);
    assert_eq!(&value[4..5], "-");
    assert_eq!(&value[7..8], "-");
    assert_eq!(&value[10..11], "T");
    assert_eq!(&value[13..14], ":");
    assert_eq!(&value[16..17], ":");
    assert!(
        value.chars().enumerate().all(|(idx, ch)| match idx {
            4 | 7 | 10 | 13 | 16 => true,
            _ => ch.is_ascii_digit(),
        }),
        "timestamp contains unexpected characters: {}",
        value
    );
}

fn assert_cli_error(output: std::process::Output, expected: &str) {
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr)
            .expect("stderr decode failed")
            .trim(),
        expected
    );
}

fn assert_cli_success_counts(
    output: std::process::Output,
    revoked_count: usize,
    warning_count: usize,
) {
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    assert_eq!(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("revoked_count: "))
            .expect("revoked_count missing"),
        revoked_count.to_string()
    );
    assert_eq!(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("warning_count: "))
            .expect("warning_count missing"),
        warning_count.to_string()
    );
}

fn assert_cli_deleted_count(
    output: std::process::Output,
    deleted_count: usize,
) {
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    assert_eq!(
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("deleted_count: "))
            .expect("deleted_count missing"),
        deleted_count.to_string()
    );
}

fn create_token_and_get_id(
    db_path: &Path,
    assets_dir: &Path,
    user_name: &str,
) -> String {
    let output = build_base_command(db_path, assets_dir)
        .arg("token")
        .arg("create")
        .arg(user_name)
        .output()
        .expect("token create failed");

    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("stdout decode failed")
        .lines()
        .find_map(|line| line.strip_prefix("TOKEN ID:     "))
        .map(str::to_string)
        .expect("token_id missing")
}

fn find_label_value<'a>(stdout: &'a str, label: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .expect("label missing")
}

fn list_token_rows(
    db_path: &Path,
    assets_dir: &Path,
    args: &[&str],
) -> Vec<Vec<String>> {
    let mut command = build_base_command(db_path, assets_dir);
    command.arg("token").arg("list");
    for arg in args {
        command.arg(arg);
    }

    let output = command.output().expect("token list failed");
    assert!(output.status.success());

    String::from_utf8(output.stdout)
        .expect("stdout decode failed")
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().map(str::to_string).collect())
        .collect()
}

fn run_user_info(
    db_path: &Path,
    assets_dir: &Path,
    user_name: &str,
) -> std::process::Output {
    build_base_command(db_path, assets_dir)
        .arg("user")
        .arg("info")
        .arg(user_name)
        .output()
        .expect("user info failed")
}

fn run_user_edit_with_input(
    db_path: &Path,
    assets_dir: &Path,
    args: &[&str],
    input: Option<&str>,
) -> std::process::Output {
    let mut command = build_base_command(db_path, assets_dir);
    command.arg("user").arg("edit");
    for arg in args {
        command.arg(arg);
    }

    if let Some(input) = input {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn user edit failed");
        {
            let stdin = child.stdin.as_mut().expect("stdin missing");
            use std::io::Write;
            write!(stdin, "{}", input).expect("write user edit stdin failed");
        }
        child.wait_with_output().expect("wait user edit failed")
    } else {
        command.output().expect("user edit failed")
    }
}

struct CliServerGuard {
    child: Child,
    stderr_path: std::path::PathBuf,
}

impl CliServerGuard {
    fn start(
        db_path: &Path,
        assets_dir: &Path,
        port: u16,
        enable_mcp: bool,
        config_use_mcp: bool,
    ) -> Self {
        let exe = test_binary_path();
        let base_dir = db_path.parent().expect("db_path parent missing");
        let fts_index = common::fts_index_path(db_path);
        let config_dir = base_dir.join(env!("CARGO_PKG_NAME"));
        fs::create_dir_all(&config_dir).expect("create config dir failed");
        let config_path = config_dir.join("config.toml");
        let config_body = if config_use_mcp {
            "[run]\nuse_tls = false\nuse_mcp = true\n"
        } else {
            "[run]\nuse_tls = false\n"
        };
        fs::write(&config_path, config_body).expect("write test config failed");
        let stdout_path = base_dir.join("server.stdout.log");
        let stdout =
            std::fs::File::create(&stdout_path).expect("create stdout failed");
        let stderr_path = base_dir.join("server.stderr.log");
        let stderr =
            std::fs::File::create(&stderr_path).expect("create stderr failed");
        let mut command = Command::new(exe);
        command
            .env("XDG_CONFIG_HOME", base_dir)
            .env("XDG_DATA_HOME", base_dir)
            .arg("--db-path")
            .arg(db_path)
            .arg("--assets-path")
            .arg(assets_dir)
            .arg("--fts-index")
            .arg(fts_index)
            .arg("run");
        if enable_mcp {
            command.arg("--mcp");
        }
        let child = command
            .arg(format!("127.0.0.1:{}", port))
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn server failed");

        Self { child, stderr_path }
    }

    fn stderr_path(&self) -> &Path {
        &self.stderr_path
    }
}

impl Drop for CliServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn token_revoke_requires_yes_in_non_interactive_mode() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--all")
        .stdin(Stdio::null())
        .output()
        .expect("token revoke failed");

    assert_cli_error(
        output,
        "error: confirmation required in non-interactive mode; use --yes",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_requires_yes_in_non_interactive_mode() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--expired")
        .stdin(Stdio::null())
        .output()
        .expect("token purge failed");

    assert_cli_error(
        output,
        "error: confirmation required in non-interactive mode; use --yes",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_create_outputs_expected_fields_and_persists_to_db() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let expected_name = "api-client";
    let expected_scope = "write";
    let expected_effective = "read, create, delete, update, append";
    let expected_ttl_seconds = 12 * 60 * 60;
    let expected_path_prefix = "/docs";
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--scope")
        .arg(expected_scope)
        .arg("--ttl")
        .arg("12h")
        .arg("--name")
        .arg(expected_name)
        .arg("--path-prefix")
        .arg(expected_path_prefix)
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    let token_id = find_label_value(&stdout, "TOKEN ID:     ");
    let token_name = find_label_value(&stdout, "TOKEN NAME:   ");
    let user_name = find_label_value(&stdout, "USERNAME:     ");
    let scopes = find_label_value(&stdout, "SCOPES:       ");
    let effective_permissions = find_label_value(&stdout, "PERMISSIONS:  ");
    let ttl = find_label_value(&stdout, "TTL:          ");
    let created_at = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("create: "))
        .expect("create timestamp missing");
    let expire_at = stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("expire: "))
        .expect("expire timestamp missing");
    let token = stdout
        .split_once("TOKEN VALUE:\n")
        .map(|(_, value)| value.trim())
        .expect("token missing");

    assert_eq!(token_name, expected_name);
    assert_eq!(user_name, TEST_USERNAME);
    assert_eq!(scopes, expected_scope);
    assert_eq!(effective_permissions, expected_effective);
    assert_eq!(ttl, "12h");
    assert!(stdout.contains("PATH PREFIXES:\n"));
    assert!(stdout.contains(&format!("    - {}", expected_path_prefix)));
    assert!(stdout.contains("TIMESTAMPS:\n"));
    assert_cli_timestamp(created_at);
    assert_cli_timestamp(expire_at);
    assert_eq!(token_id.len(), 26, "unexpected token_id: {}", token_id);
    assert!(
        token_id.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()),
        "token_id contains unexpected characters: {}",
        token_id
    );
    assert_eq!(token.len(), 44, "unexpected token length: {}", token);
    assert!(
        token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '/' || ch == '='),
        "token contains unexpected characters: {}",
        token
    );

    let snapshot = get_bearer_token_snapshot_for_test(&db_path, &assets_dir, token_id)
        .expect("get bearer token snapshot failed")
        .expect("created token not found in db");
    assert_eq!(snapshot.token_id, token_id);
    assert_eq!(snapshot.scopes, vec![expected_scope.to_string()]);
    assert_eq!(snapshot.ttl_seconds, expected_ttl_seconds);
    assert_eq!(
        snapshot.path_prefixes,
        vec![expected_path_prefix.to_string()]
    );
    assert_eq!(snapshot.revoked, false);
    assert_eq!(snapshot.name, Some(expected_name.to_string()));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_create_rejects_invalid_scope() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--scope")
        .arg("admin")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");

    assert_cli_error(output, "error: invalid bearer scope: admin");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_create_rejects_invalid_ttl_format() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--ttl")
        .arg("30x")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");

    assert_cli_error(output, "error: ttl format is invalid");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_create_rejects_non_normalized_path_prefix() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--path-prefix")
        .arg("/docs/")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");

    assert_cli_error(
        output,
        "error: invalid path prefix: path must be normalized",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_create_rejects_non_positive_ttl() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--ttl=-1h")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");

    assert_cli_error(output, "error: ttl must be greater than zero");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_info_outputs_full_token_details() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let expected_name = "api-client";
    let create_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--scope")
        .arg("read,append")
        .arg("--name")
        .arg(expected_name)
        .arg("--path-prefix")
        .arg("/docs")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");
    assert!(create_output.status.success());

    let token_id = String::from_utf8(create_output.stdout)
        .expect("create stdout decode failed")
        .lines()
        .find_map(|line| line.strip_prefix("TOKEN ID:     "))
        .map(str::to_string)
        .expect("token_id missing");

    let info_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("info")
        .arg(&token_id)
        .output()
        .expect("token info failed");

    assert!(info_output.status.success());
    let stdout = String::from_utf8(info_output.stdout).expect("stdout decode failed");
    assert!(stdout.contains(&format!("TOKEN ID:     {}", token_id)));
    assert!(stdout.contains(&format!("USERNAME:     {}", TEST_USERNAME)));
    assert!(stdout.contains(&format!("TOKEN NAME:   {}", expected_name)));
    assert!(stdout.contains("SCOPES:       read,append"));
    assert!(stdout.contains("PERMISSIONS:  read, append"));
    assert!(stdout.contains("PATH PREFIXES:\n    - /docs"));
    assert!(stdout.contains("STATUS:       alive"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_add_path_updates_token_path_prefixes() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("add_path")
        .arg(&token_id)
        .arg("/docs")
        .output()
        .expect("token add_path failed");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    assert!(stdout.contains(&format!("token_id: {}", token_id)));
    assert!(stdout.contains("path_prefixes: /docs"));

    let snapshot = get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &token_id)
        .expect("get bearer token snapshot failed")
        .expect("token not found");
    assert_eq!(snapshot.path_prefixes, vec!["/docs".to_string()]);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_remove_path_restores_unrestricted_access_when_last_prefix_is_removed() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let create_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--path-prefix")
        .arg("/docs")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");
    assert!(create_output.status.success());

    let token_id = String::from_utf8(create_output.stdout)
        .expect("create stdout decode failed")
        .lines()
        .find_map(|line| line.strip_prefix("TOKEN ID:     "))
        .map(str::to_string)
        .expect("token_id missing");
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("remove_path")
        .arg(&token_id)
        .arg("/docs")
        .output()
        .expect("token remove_path failed");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    assert!(stdout.contains(&format!("token_id: {}", token_id)));
    assert!(stdout.contains("path_prefixes: all"));
    assert!(stdout.contains("warning: token allows access to all paths"));

    let snapshot = get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &token_id)
        .expect("get bearer token snapshot failed")
        .expect("token not found");
    assert!(snapshot.path_prefixes.is_empty());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_remove_path_rejects_missing_prefix() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("remove_path")
        .arg(&token_id)
        .arg("/docs")
        .output()
        .expect("token remove_path failed");

    assert_cli_error(output, "error: path prefix not found: /docs");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_create_rejects_unknown_user() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("missing_user")
        .output()
        .expect("token create failed");

    assert_cli_error(output, "error: user not found");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_revoke_by_token_id_marks_only_target_as_revoked() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let target_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let other_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&target_token_id)
        .output()
        .expect("token revoke failed");

    assert_cli_success_counts(output, 1, 0);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &target_token_id)
            .expect("get target token snapshot failed")
            .expect("target token not found")
            .revoked
    );
    assert!(
        !get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &other_token_id)
            .expect("get other token snapshot failed")
            .expect("other token not found")
            .revoked
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_revoke_by_user_marks_only_that_users_tokens_as_revoked() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let second_user = "other_user";
    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials(
        &db_path,
        &assets_dir,
        second_user,
        TEST_PASSWORD,
    );

    let first_user_token1 = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let first_user_token2 = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let second_user_token = create_token_and_get_id(&db_path, &assets_dir, second_user);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg("--user")
        .arg(TEST_USERNAME)
        .output()
        .expect("token revoke failed");

    assert_cli_success_counts(output, 2, 0);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &first_user_token1)
            .expect("get first user token1 snapshot failed")
            .expect("first user token1 not found")
            .revoked
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &first_user_token2)
            .expect("get first user token2 snapshot failed")
            .expect("first user token2 not found")
            .revoked
    );
    assert!(
        !get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &second_user_token)
            .expect("get second user token snapshot failed")
            .expect("second user token not found")
            .revoked
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_revoke_all_marks_all_tokens_as_revoked() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let second_user = "other_user";
    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials(
        &db_path,
        &assets_dir,
        second_user,
        TEST_PASSWORD,
    );

    let first_token = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let second_token = create_token_and_get_id(&db_path, &assets_dir, second_user);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg("--all")
        .output()
        .expect("token revoke failed");

    assert_cli_success_counts(output, 2, 0);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &first_token)
            .expect("get first token snapshot failed")
            .expect("first token not found")
            .revoked
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &second_token)
            .expect("get second token snapshot failed")
            .expect("second token not found")
            .revoked
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_revoke_reports_warning_for_already_revoked_token() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let first_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&token_id)
        .output()
        .expect("first token revoke failed");
    assert_cli_success_counts(first_output, 1, 0);

    let second_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&token_id)
        .output()
        .expect("second token revoke failed");

    assert_cli_success_counts(second_output, 0, 1);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &token_id)
            .expect("get token snapshot failed")
            .expect("token not found")
            .revoked
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_revoke_reports_warning_for_expired_token() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let (token_id, _) =
        create_bearer_token_for_test(&db_path, &assets_dir, TEST_USERNAME, -60)
            .expect("create expired token failed");

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&token_id)
        .output()
        .expect("token revoke failed");

    assert_cli_success_counts(output, 0, 1);
    assert!(
        !get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &token_id)
            .expect("get expired token snapshot failed")
            .expect("expired token not found")
            .revoked
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_revoke_rejects_mutually_exclusive_target_options() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg("--user")
        .arg(TEST_USERNAME)
        .arg(&token_id)
        .output()
        .expect("token revoke failed");

    assert_cli_error(
        output,
        "error: invalid revoke target",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_by_token_id_removes_only_target_token() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let target_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let other_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--yes")
        .arg(&target_token_id)
        .output()
        .expect("token purge failed");

    assert_cli_deleted_count(output, 1);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &target_token_id)
            .expect("get target token snapshot failed")
            .is_none()
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &other_token_id)
            .expect("get other token snapshot failed")
            .is_some()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_expired_removes_only_expired_tokens() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let (expired_token_id, _) =
        create_bearer_token_for_test(&db_path, &assets_dir, TEST_USERNAME, -60)
            .expect("create expired token failed");
    let active_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--yes")
        .arg("--expired")
        .output()
        .expect("token purge failed");

    assert_cli_deleted_count(output, 1);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &expired_token_id)
            .expect("get expired token snapshot failed")
            .is_none()
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &active_token_id)
            .expect("get active token snapshot failed")
            .is_some()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_revoked_removes_only_revoked_tokens() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let revoked_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let active_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let revoke_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&revoked_token_id)
        .output()
        .expect("token revoke failed");
    assert_cli_success_counts(revoke_output, 1, 0);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--yes")
        .arg("--revoked")
        .output()
        .expect("token purge failed");

    assert_cli_deleted_count(output, 1);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &revoked_token_id)
            .expect("get revoked token snapshot failed")
            .is_none()
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &active_token_id)
            .expect("get active token snapshot failed")
            .is_some()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_expired_and_revoked_removes_union_of_targets() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let (expired_token_id, _) =
        create_bearer_token_for_test(&db_path, &assets_dir, TEST_USERNAME, -60)
            .expect("create expired token failed");
    let revoked_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let active_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let revoke_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&revoked_token_id)
        .output()
        .expect("token revoke failed");
    assert_cli_success_counts(revoke_output, 1, 0);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--yes")
        .arg("--expired")
        .arg("--revoked")
        .output()
        .expect("token purge failed");

    assert_cli_deleted_count(output, 2);
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &expired_token_id)
            .expect("get expired token snapshot failed")
            .is_none()
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &revoked_token_id)
            .expect("get revoked token snapshot failed")
            .is_none()
    );
    assert!(
        get_bearer_token_snapshot_for_test(&db_path, &assets_dir, &active_token_id)
            .expect("get active token snapshot failed")
            .is_some()
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_rejects_mutually_exclusive_target_options() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--yes")
        .arg("--expired")
        .arg(&token_id)
        .output()
        .expect("token purge failed");

    assert_cli_error(
        output,
        "error: invalid purge target",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_purge_requires_target_specification() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("purge")
        .arg("--yes")
        .output()
        .expect("token purge failed");

    assert_cli_error(
        output,
        "error: invalid purge target",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_default_output_shows_scope_path_token_user_name_and_expire_at() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);
    let expected_name = "default-client";

    let create_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--name")
        .arg(expected_name)
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");
    assert!(create_output.status.success());

    let list_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("list")
        .output()
        .expect("token list failed");

    assert!(list_output.status.success());
    let stdout = String::from_utf8(list_output.stdout).expect("stdout decode failed");
    let mut lines = stdout.lines();
    let header = lines.next().expect("header missing");
    assert!(header.contains("SCOPE"));
    assert!(header.contains("PATH"));
    assert!(header.contains("ID"));
    assert!(header.contains("USER"));
    assert!(header.contains("NAME"));
    assert!(header.contains("EXPIRES"));

    let row = lines.next().expect("row missing");
    let fields: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 6, "unexpected row: {}", row);
    assert_eq!(fields[0], "rcdua");
    assert_eq!(fields[1], "*");
    assert_eq!(fields[2].len(), 26, "unexpected token_id: {}", fields[2]);
    assert_eq!(fields[3], TEST_USERNAME);
    assert_eq!(fields[4], expected_name);
    assert_cli_timestamp(fields[5]);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_long_info_outputs_detail_columns_and_values() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);
    let expected_name = "api-client";

    let create_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--name")
        .arg(expected_name)
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");
    assert!(create_output.status.success());

    let list_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("list")
        .arg("--long-info")
        .output()
        .expect("token list failed");

    assert!(list_output.status.success());
    let stdout = String::from_utf8(list_output.stdout).expect("stdout decode failed");
    let mut lines = stdout.lines();
    let header = lines.next().expect("header missing");
    assert!(header.contains("SCOPE"));
    assert!(header.contains("PATH"));
    assert!(header.contains("ID"));
    assert!(header.contains("USER"));
    assert!(header.contains("NAME"));
    assert!(header.contains("EXPIRES"));
    assert!(header.contains("CREATE"));
    assert!(header.contains("STATUS"));

    let row = lines.next().expect("row missing");
    let fields: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 8, "unexpected row: {}", row);
    assert_eq!(fields[0], "rcdua");
    assert_eq!(fields[1], "*");
    assert_eq!(fields[2].len(), 26, "unexpected token_id: {}", fields[2]);
    assert_eq!(fields[3], TEST_USERNAME);
    assert_eq!(fields[4], expected_name);
    assert_cli_timestamp(fields[5]);
    assert_cli_timestamp(fields[6]);
    assert_eq!(fields[7], "alive");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_filters_by_user() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let second_user = "other_user";
    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials(
        &db_path,
        &assets_dir,
        second_user,
        TEST_PASSWORD,
    );

    let target_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let _other_token_id = create_token_and_get_id(&db_path, &assets_dir, second_user);

    let rows = list_token_rows(
        &db_path,
        &assets_dir,
        &["--user", TEST_USERNAME],
    );

    assert_eq!(rows.len(), 1, "unexpected rows: {:?}", rows);
    assert_eq!(rows[0][2], target_token_id);
    assert_eq!(rows[0][3], TEST_USERNAME);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_filters_revoked_tokens() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let revoked_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let _active_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let revoke_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&revoked_token_id)
        .output()
        .expect("token revoke failed");
    assert_cli_success_counts(revoke_output, 1, 0);

    let rows = list_token_rows(&db_path, &assets_dir, &["--revoked"]);

    assert_eq!(rows.len(), 1, "unexpected rows: {:?}", rows);
    assert_eq!(rows[0][0], "rcdua");
    assert_eq!(rows[0][2], revoked_token_id);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_filters_expired_tokens() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let (expired_token_id, _) =
        create_bearer_token_for_test(&db_path, &assets_dir, TEST_USERNAME, -60)
            .expect("create expired token failed");
    let _active_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);

    let rows = list_token_rows(&db_path, &assets_dir, &["--expired"]);

    assert_eq!(rows.len(), 1, "unexpected rows: {:?}", rows);
    assert_eq!(rows[0][0], "r----");
    assert_eq!(rows[0][2], expired_token_id);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_filters_union_of_revoked_and_expired_tokens() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let (expired_token_id, _) =
        create_bearer_token_for_test(&db_path, &assets_dir, TEST_USERNAME, -60)
            .expect("create expired token failed");
    let revoked_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let _active_token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let revoke_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("revoke")
        .arg("--yes")
        .arg(&revoked_token_id)
        .output()
        .expect("token revoke failed");
    assert_cli_success_counts(revoke_output, 1, 0);

    let rows = list_token_rows(
        &db_path,
        &assets_dir,
        &["--revoked", "--expired"],
    );

    assert_eq!(rows.len(), 2, "unexpected rows: {:?}", rows);
    let listed_ids: Vec<&str> = rows.iter().map(|row| row[2].as_str()).collect();
    assert!(listed_ids.contains(&expired_token_id.as_str()));
    assert!(listed_ids.contains(&revoked_token_id.as_str()));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_list_marks_limited_path_tokens_with_l() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--scope")
        .arg("read")
        .arg("--path-prefix")
        .arg("/docs")
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");
    assert!(output.status.success());

    let rows = list_token_rows(&db_path, &assets_dir, &[]);
    assert_eq!(rows.len(), 1, "unexpected rows: {:?}", rows);
    assert_eq!(rows[0][0], "r----");
    assert_eq!(rows[0][1], "L");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn token_info_shows_all_for_unrestricted_path_prefixes() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let token_id = create_token_and_get_id(&db_path, &assets_dir, TEST_USERNAME);
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("info")
        .arg(&token_id)
        .output()
        .expect("token info failed");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    assert!(stdout.contains("PATH PREFIXES:\n    - all"));
    assert!(stdout.contains("PERMISSIONS:  read, create, delete, update, append"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_add_with_no_basic_auth_succeeds_without_password_prompt() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let output = build_base_command(&db_path, &assets_dir)
        .arg("user")
        .arg("add")
        .arg("--attribute")
        .arg("no_basic_auth")
        .arg("nobasic_user")
        .stdin(Stdio::null())
        .output()
        .expect("user add failed");

    assert!(output.status.success());
    let info_output = run_user_info(&db_path, &assets_dir, "nobasic_user");
    assert!(info_output.status.success());
    let stdout =
        String::from_utf8(info_output.stdout).expect("stdout decode failed");
    assert!(stdout.contains("USERNAME:     nobasic_user"));
    assert!(stdout.contains("ATTRIBUTES:\n    - NoBasicAuth"));
    assert!(stdout.contains("BASIC AUTH:   denied"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_add_with_read_only_persists_attribute() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let output = build_base_command(&db_path, &assets_dir)
        .arg("user")
        .arg("add")
        .arg("--attribute")
        .arg("read_only")
        .arg("readonly_user")
        .output()
        .expect("user add failed");

    assert!(output.status.success());
    let info_output = run_user_info(&db_path, &assets_dir, "readonly_user");
    assert!(info_output.status.success());
    let stdout =
        String::from_utf8(info_output.stdout).expect("stdout decode failed");
    assert!(stdout.contains("USERNAME:     readonly_user"));
    assert!(stdout.contains("ATTRIBUTES:\n    - ReadOnly"));
    assert!(stdout.contains("BASIC AUTH:   allowed"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_add_rejects_invalid_attribute() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let output = build_base_command(&db_path, &assets_dir)
        .arg("user")
        .arg("add")
        .arg("--attribute")
        .arg("unknown")
        .arg("invalid_user")
        .stdin(Stdio::null())
        .output()
        .expect("user add failed");

    assert_cli_error(output, "error: invalid user attribute: unknown");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_edit_can_add_no_basic_auth_and_user_info_reflects_it() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = run_user_edit_with_input(
        &db_path,
        &assets_dir,
        &[
            "--display-name",
            "updated-user",
            "--add-attribute",
            "no_basic_auth",
            TEST_USERNAME,
        ],
        None,
    );
    assert!(output.status.success());

    let info_output = run_user_info(&db_path, &assets_dir, TEST_USERNAME);
    assert!(info_output.status.success());
    let stdout =
        String::from_utf8(info_output.stdout).expect("stdout decode failed");
    assert!(stdout.contains("DISPLAY NAME: updated-user"));
    assert!(stdout.contains("ATTRIBUTES:\n    - NoBasicAuth"));
    assert!(stdout.contains("BASIC AUTH:   denied"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_edit_can_add_and_remove_read_only() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let add_output = run_user_edit_with_input(
        &db_path,
        &assets_dir,
        &["--add-attribute", "read_only", TEST_USERNAME],
        None,
    );
    assert!(add_output.status.success());

    let add_info_output = run_user_info(&db_path, &assets_dir, TEST_USERNAME);
    assert!(add_info_output.status.success());
    let add_stdout = String::from_utf8(add_info_output.stdout)
        .expect("stdout decode failed");
    assert!(add_stdout.contains("ATTRIBUTES:\n    - ReadOnly"));

    let remove_output = run_user_edit_with_input(
        &db_path,
        &assets_dir,
        &["--remove-attribute", "read_only", TEST_USERNAME],
        None,
    );
    assert!(remove_output.status.success());

    let remove_info_output =
        run_user_info(&db_path, &assets_dir, TEST_USERNAME);
    assert!(remove_info_output.status.success());
    let remove_stdout = String::from_utf8(remove_info_output.stdout)
        .expect("stdout decode failed");
    assert!(remove_stdout.contains("ATTRIBUTES:\n    - none"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_edit_clear_attributes_removes_read_only() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let add_output = build_base_command(&db_path, &assets_dir)
        .arg("user")
        .arg("add")
        .arg("--attribute")
        .arg("read_only")
        .arg(TEST_USERNAME)
        .output()
        .expect("user add failed");
    assert!(add_output.status.success());

    let clear_output = run_user_edit_with_input(
        &db_path,
        &assets_dir,
        &["--clear-attributes", TEST_USERNAME],
        None,
    );
    assert!(clear_output.status.success());

    let info_output = run_user_info(&db_path, &assets_dir, TEST_USERNAME);
    assert!(info_output.status.success());
    let stdout =
        String::from_utf8(info_output.stdout).expect("stdout decode failed");
    assert!(stdout.contains("ATTRIBUTES:\n    - none"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_edit_removing_no_basic_auth_requires_password() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let add_output = build_base_command(&db_path, &assets_dir)
        .arg("user")
        .arg("add")
        .arg("--attribute")
        .arg("no_basic_auth")
        .arg(TEST_USERNAME)
        .stdin(Stdio::null())
        .output()
        .expect("user add failed");
    assert!(add_output.status.success());

    let output = run_user_edit_with_input(
        &db_path,
        &assets_dir,
        &["--remove-attribute", "no_basic_auth", TEST_USERNAME],
        None,
    );
    assert_cli_error(
        output,
        "error: password must be specified when removing NoBasicAuth",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_info_rejects_missing_user() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let output = run_user_info(&db_path, &assets_dir, "missing_user");

    assert_cli_error(output, "error: user not found: missing_user");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn user_list_keeps_existing_columns_without_attribute_details() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let output = build_base_command(&db_path, &assets_dir)
        .arg("user")
        .arg("list")
        .output()
        .expect("user list failed");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    let mut lines = stdout.lines();
    let header = lines.next().expect("header missing");
    assert!(header.contains("USER_ID"));
    assert!(header.contains("TIMESTAMP"));
    assert!(header.contains("USER_NAME"));
    assert!(header.contains("DISPLAY_NAME"));
    assert!(!header.contains("ATTRIBUTE"));

    let row = lines.next().expect("row missing");
    assert!(row.contains(TEST_USERNAME));
    assert!(!row.contains("NoBasicAuth"));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn run_without_mcp_does_not_publish_mcp_endpoint() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let port = reserve_port();
    let server = CliServerGuard::start(
        &db_path,
        &assets_dir,
        port,
        false,
        false,
    );
    let (base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let response = client
        .get(format!(
            "{}/mcp",
            base_url.trim_end_matches("/api")
        ))
        .send()
        .expect("request /mcp failed");
    assert_eq!(response.status().as_u16(), 404);

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn run_with_mcp_publishes_mcp_endpoint() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let port = reserve_port();
    let server = CliServerGuard::start(
        &db_path,
        &assets_dir,
        port,
        true,
        false,
    );
    let (base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let response = client
        .get(format!(
            "{}/mcp",
            base_url.trim_end_matches("/api")
        ))
        .send()
        .expect("request /mcp failed");
    assert_eq!(response.status().as_u16(), 401);

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn run_with_mcp_enabled_in_config_publishes_mcp_endpoint() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let port = reserve_port();
    let server = CliServerGuard::start(
        &db_path,
        &assets_dir,
        port,
        false,
        true,
    );
    let (base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let response = client
        .get(format!(
            "{}/mcp",
            base_url.trim_end_matches("/api")
        ))
        .send()
        .expect("request /mcp failed");
    assert_eq!(response.status().as_u16(), 401);

    drop(server);
    fs::remove_dir_all(base_dir).expect("cleanup failed");
}
