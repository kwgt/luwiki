/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use chrono::{Duration as ChronoDuration, Local};
use luwiki::database::{
    create_bearer_token_for_test,
    delete_user_only_for_bearer_test,
    rewrite_bearer_token_timestamps_for_test,
    revoke_bearer_token_for_test,
};

use common::{
    ServerGuard, build_client, prepare_test_dirs, reserve_port, run_add_user,
    run_add_user_with_credentials, run_create_token,
    wait_for_server_with_scheme, TEST_PASSWORD, TEST_USERNAME,
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

const TEST_BASIC_AUTHORIZATION: &str =
    "Basic dGVzdF91c2VyOnBhc3N3b3JkMTIz";
const TEST_BASIC_AUTH_CHALLENGE: &str =
    "Basic realm=\"LuWiki REST API\"";

fn assert_bad_request_for_authorization_header(header_value: &str) {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client
        .get(&base_url)
        .header(AUTHORIZATION, header_value)
        .send()
        .expect("request failed");

    assert_eq!(response.status().as_u16(), 400);
    assert!(response.headers().get("WWW-Authenticate").is_none());
    assert_eq!(
        response.text().expect("read body failed"),
        r#"{"reason":"bad request"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn assert_basic_authorization_success(header_value: &str) {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client
        .get(&base_url)
        .header(AUTHORIZATION, header_value)
        .send()
        .expect("authorized request failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), "hello");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn assert_unauthorized_for_bearer_token(token: &str) {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client
        .get(&base_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .send()
        .expect("request failed");

    assert_eq!(response.status().as_u16(), 401);
    assert_eq!(
        response
            .headers()
            .get("WWW-Authenticate")
            .expect("missing WWW-Authenticate header")
            .to_str()
            .expect("WWW-Authenticate to_str failed"),
        TEST_BASIC_AUTH_CHALLENGE,
    );
    assert_eq!(
        response.text().expect("read body failed"),
        r#"{"reason":"unauthorized"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

fn wait_for_server_without_auth(port: u16, stderr_path: &Path) -> (String, reqwest::blocking::Client) {
    let http_api_base_url = format!("http://127.0.0.1:{}/api", port);
    let https_api_base_url = format!("https://127.0.0.1:{}/api", port);
    let http_url = format!("{}/hello", http_api_base_url);
    let https_url = format!("{}/hello", https_api_base_url);
    let client = build_client();
    let https_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(7000))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("https client build failed");
    let mut last_error = String::new();

    for _ in 0..300 {
        match client.get(&http_url).send() {
            Ok(response) if response.status().as_u16() == 401 => {
                return (http_api_base_url, client);
            }
            Ok(response) => {
                last_error = format!("status {}", response.status().as_u16());
            }
            Err(err) => {
                let message = err.to_string();
                if message.contains("invalid HTTP version") {
                    match https_client.get(&https_url).send() {
                        Ok(response) if response.status().as_u16() == 401 => {
                            return (https_api_base_url, https_client);
                        }
                        Ok(response) => {
                            last_error =
                                format!("https status {}", response.status().as_u16());
                        }
                        Err(err) => {
                            last_error = format!("https error: {}", err);
                        }
                    }
                } else {
                    last_error = message;
                }
            }
        }

        thread::sleep(Duration::from_millis(100));
    }

    let stderr_log = fs::read_to_string(stderr_path)
        .unwrap_or_else(|_| "<stderr log not available>".to_string());
    panic!(
        "server did not start for unauthorized probe\nstderr:\n{}\nlast error: {}",
        stderr_log, last_error
    );
}

#[test]
fn api_hello_returns_401_without_authorization_header() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client.get(&base_url).send().expect("request failed");
    assert_eq!(response.status().as_u16(), 401);
    assert_eq!(
        response
            .headers()
            .get("WWW-Authenticate")
            .expect("missing WWW-Authenticate header")
            .to_str()
            .expect("WWW-Authenticate to_str failed"),
        TEST_BASIC_AUTH_CHALLENGE,
    );
    assert_eq!(
        response.text().expect("read body failed"),
        r#"{"reason":"unauthorized"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn api_hello_returns_401_with_www_authenticate_on_basic_auth_failure() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client
        .get(&base_url)
        .basic_auth(TEST_USERNAME, Some("wrong-password"))
        .send()
        .expect("request failed");

    assert_eq!(response.status().as_u16(), 401);
    assert_eq!(
        response
            .headers()
            .get("WWW-Authenticate")
            .expect("missing WWW-Authenticate header")
            .to_str()
            .expect("WWW-Authenticate to_str failed"),
        TEST_BASIC_AUTH_CHALLENGE,
    );
    assert_eq!(
        response.text().expect("read body failed"),
        r#"{"reason":"unauthorized"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn api_hello_accepts_basic_authorization_via_common_auth_entry() {
    assert_basic_authorization_success(TEST_BASIC_AUTHORIZATION);
}

#[test]
fn api_hello_accepts_bearer_authorization() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let token = run_create_token(&db_path, &assets_dir, "read");
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client
        .get(&base_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .send()
        .expect("authorized request failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), "hello");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn api_hello_returns_400_with_multiple_authorization_headers() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);
    let mut headers = HeaderMap::new();

    headers.append(
        AUTHORIZATION,
        HeaderValue::from_static(TEST_BASIC_AUTHORIZATION),
    );
    headers.append(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer duplicate-token"),
    );

    let response = client
        .get(&base_url)
        .headers(headers)
        .send()
        .expect("request failed");

    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(
        response.text().expect("read body failed"),
        r#"{"reason":"bad request"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn api_hello_returns_400_with_unsupported_authorization_scheme() {
    assert_bad_request_for_authorization_header("Digest token-value");
}

#[test]
fn api_hello_returns_400_with_malformed_authorization_header() {
    assert_bad_request_for_authorization_header("Bearer");
    assert_bad_request_for_authorization_header("Bearer token extra");
}

#[test]
fn api_hello_returns_401_for_bearer_auth_failure_reasons() {
    assert_unauthorized_for_bearer_token("unissued-token");

    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials(
        &db_path,
        &assets_dir,
        "survivor_user",
        "password456",
    );

    let (revoked_token_id, revoked_token) = create_bearer_token_for_test(
        &db_path,
        &assets_dir,
        "test_user",
        3600,
    )
    .expect("create revoked token failed");
    revoke_bearer_token_for_test(&db_path, &assets_dir, &revoked_token_id)
        .expect("revoke token failed");

    let (_, expired_token) = create_bearer_token_for_test(
        &db_path,
        &assets_dir,
        "test_user",
        -60,
    )
    .expect("create expired token failed");

    let (_, orphan_token) = create_bearer_token_for_test(
        &db_path,
        &assets_dir,
        "test_user",
        3600,
    )
    .expect("create orphan token failed");
    delete_user_only_for_bearer_test(&db_path, &assets_dir, "test_user")
        .expect("delete user only failed");

    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_without_auth(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    for token in [revoked_token, expired_token, orphan_token] {
        let response = client
            .get(&base_url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .send()
            .expect("request failed");

        assert_eq!(response.status().as_u16(), 401);
        assert_eq!(
            response.text().expect("read body failed"),
            r#"{"reason":"unauthorized"}"#
        );
    }

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
fn api_hello_sets_x_bearer_expire_only_when_bearer_ttl_is_extended() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let (extend_token_id, extend_token) = create_bearer_token_for_test(
        &db_path,
        &assets_dir,
        "test_user",
        3600,
    )
    .expect("create extending token failed");
    let now = Local::now();
    rewrite_bearer_token_timestamps_for_test(
        &db_path,
        &assets_dir,
        &extend_token_id,
        now - ChronoDuration::minutes(40),
        now - ChronoDuration::minutes(40),
        now + ChronoDuration::minutes(20),
    )
    .expect("rewrite extending token timestamps failed");
    let steady_token = run_create_token(&db_path, &assets_dir, "read");

    let _server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, _server.stderr_path());
    let base_url = format!("{}/hello", api_base_url);

    let response = client
        .get(&base_url)
        .header(AUTHORIZATION, format!("Bearer {}", extend_token))
        .send()
        .expect("request with extending bearer failed");
    assert_eq!(response.status().as_u16(), 200);
    let expire_header = response
        .headers()
        .get("X-Bearer-Expire")
        .expect("missing X-Bearer-Expire header")
        .to_str()
        .expect("X-Bearer-Expire to_str failed")
        .to_string();
    assert_eq!(expire_header.len(), 19);
    assert_eq!(&expire_header[4..5], "-");
    assert_eq!(&expire_header[7..8], "-");
    assert_eq!(&expire_header[10..11], "T");
    assert_eq!(&expire_header[13..14], ":");
    assert_eq!(&expire_header[16..17], ":");

    let response = client
        .get(&base_url)
        .header(AUTHORIZATION, format!("Bearer {}", steady_token))
        .send()
        .expect("request with steady bearer failed");
    assert_eq!(response.status().as_u16(), 200);
    assert!(response.headers().get("X-Bearer-Expire").is_none());

    let response = client
        .get(&base_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("request with basic auth failed");
    assert_eq!(response.status().as_u16(), 200);
    assert!(response.headers().get("X-Bearer-Expire").is_none());

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}
