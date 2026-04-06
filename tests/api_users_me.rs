/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use reqwest::header::AUTHORIZATION;
use serde_json::Value;

use common::*;

#[test]
/// GET /api/users/me: Bearer read で自分自身の情報を取得できることを確認する。
fn get_users_me_allows_bearer_read_scope() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let read_token = run_create_token(&db_path, &assets_dir, "read");
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let url = format!("{}/users/me", api_base_url);

    let response = client
        .get(&url)
        .header(AUTHORIZATION, format!("Bearer {}", read_token))
        .send()
        .expect("get users/me with bearer read failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("Content-Type")
            .expect("missing content-type")
            .to_str()
            .expect("content-type to_str failed"),
        "application/json"
    );
    assert_eq!(
        response
            .headers()
            .get("Cache-Control")
            .expect("missing cache-control")
            .to_str()
            .expect("cache-control to_str failed"),
        "no-store"
    );

    let body = response.text().expect("read body failed");
    let value: Value =
        serde_json::from_str(&body).expect("parse users/me response failed");

    assert!(value["id"].as_str().is_some());
    assert_eq!(value["username"], TEST_USERNAME);
    assert!(value["display_name"].as_str().is_some());
    assert_eq!(
        value["attributes"].as_array().map(Vec::len),
        Some(0),
    );
    assert!(value["timestamp"].as_str().is_some());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET /api/users/me: Bearer write でも従来どおり取得できることを確認する。
fn get_users_me_allows_bearer_write_scope() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let write_token = run_create_token(&db_path, &assets_dir, "write");
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let url = format!("{}/users/me", api_base_url);

    let response = client
        .get(&url)
        .header(AUTHORIZATION, format!("Bearer {}", write_token))
        .send()
        .expect("get users/me with bearer write failed");

    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read body failed");
    let value: Value =
        serde_json::from_str(&body).expect("parse users/me response failed");
    assert_eq!(value["username"], TEST_USERNAME);
    assert_eq!(
        value["attributes"].as_array().map(Vec::len),
        Some(0),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET /api/users/me: ReadOnly ユーザでも参照系APIは利用できることを確認する。
fn get_users_me_allows_read_only_bearer_user() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials_and_attributes(
        &db_path,
        &assets_dir,
        "readonly_user",
        "readonly-pass",
        &["read_only"],
    );
    let read_token = run_create_token_for_user(
        &db_path,
        &assets_dir,
        "read",
        "readonly_user",
    );
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());
    let url = format!("{}/users/me", api_base_url);

    let response = client
        .get(&url)
        .header(AUTHORIZATION, format!("Bearer {}", read_token))
        .send()
        .expect("get users/me with readonly bearer failed");

    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().expect("read body failed");
    let value: Value =
        serde_json::from_str(&body).expect("parse users/me response failed");
    assert_eq!(value["username"], "readonly_user");
    assert_eq!(
        value["attributes"]
            .as_array()
            .and_then(|attributes| attributes.first())
            .and_then(Value::as_str),
        Some("ReadOnly"),
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}
