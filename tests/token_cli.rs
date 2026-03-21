/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use luwiki::database::{
    create_bearer_token_for_test,
    get_bearer_token_snapshot_for_test,
};

use common::{
    TEST_PASSWORD,
    TEST_USERNAME,
    prepare_test_dirs,
    run_add_user,
    run_add_user_with_credentials,
    test_binary_path,
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
        .find_map(|line| line.strip_prefix("token_id: "))
        .map(str::to_string)
        .expect("token_id missing")
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
    let expected_ttl_seconds = 12 * 60 * 60;
    let output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
        .arg("--scope")
        .arg(expected_scope)
        .arg("--ttl")
        .arg("12h")
        .arg("--name")
        .arg(expected_name)
        .arg(TEST_USERNAME)
        .output()
        .expect("token create failed");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout decode failed");
    let token_id = stdout
        .lines()
        .find_map(|line| line.strip_prefix("token_id: "))
        .expect("token_id missing");
    let user_name = stdout
        .lines()
        .find_map(|line| line.strip_prefix("user_name: "))
        .expect("user_name missing");
    let scopes = stdout
        .lines()
        .find_map(|line| line.strip_prefix("scopes: "))
        .expect("scopes missing");
    let created_at = stdout
        .lines()
        .find_map(|line| line.strip_prefix("created_at: "))
        .expect("created_at missing");
    let expire_at = stdout
        .lines()
        .find_map(|line| line.strip_prefix("expire_at: "))
        .expect("expire_at missing");
    let token = stdout
        .lines()
        .find_map(|line| line.strip_prefix("token: "))
        .expect("token missing");

    assert_eq!(user_name, TEST_USERNAME);
    assert_eq!(scopes, expected_scope);
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
fn token_list_default_output_shows_state_token_user_and_expire_at() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    run_add_user(&db_path, &assets_dir);

    let create_output = build_base_command(&db_path, &assets_dir)
        .arg("token")
        .arg("create")
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
    assert!(header.contains("STAT"));
    assert!(header.contains("TOKEN_ID"));
    assert!(header.contains("USER"));
    assert!(header.contains("EXPIRE_AT"));

    let row = lines.next().expect("row missing");
    let fields: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 4, "unexpected row: {}", row);
    assert_eq!(fields[0], "rw--");
    assert_eq!(fields[1].len(), 26, "unexpected token_id: {}", fields[1]);
    assert_eq!(fields[2], TEST_USERNAME);
    assert_cli_timestamp(fields[3]);

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
    assert!(header.contains("STAT"));
    assert!(header.contains("TOKEN_ID"));
    assert!(header.contains("USER"));
    assert!(header.contains("EXPIRE_AT"));
    assert!(header.contains("CREATED_AT"));
    assert!(header.contains("UPDATED_AT"));
    assert!(header.contains("REVOKED"));
    assert!(header.contains("NAME"));

    let row = lines.next().expect("row missing");
    let fields: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 8, "unexpected row: {}", row);
    assert_eq!(fields[0], "rw--");
    assert_eq!(fields[1].len(), 26, "unexpected token_id: {}", fields[1]);
    assert_eq!(fields[2], TEST_USERNAME);
    assert_cli_timestamp(fields[3]);
    assert_cli_timestamp(fields[4]);
    assert_cli_timestamp(fields[5]);
    assert_eq!(fields[6], "false");
    assert_eq!(fields[7], expected_name);

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
    assert_eq!(rows[0][1], target_token_id);
    assert_eq!(rows[0][2], TEST_USERNAME);

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
    assert_eq!(rows[0][0], "rwv-");
    assert_eq!(rows[0][1], revoked_token_id);

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
    assert_eq!(rows[0][0], "r--e");
    assert_eq!(rows[0][1], expired_token_id);

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
    let listed_ids: Vec<&str> = rows.iter().map(|row| row[1].as_str()).collect();
    assert!(listed_ids.contains(&expired_token_id.as_str()));
    assert!(listed_ids.contains(&revoked_token_id.as_str()));

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}
