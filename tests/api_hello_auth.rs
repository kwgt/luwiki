/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use common::{
    build_client, prepare_test_dirs, reserve_port, run_add_user,
    wait_for_server, ServerGuard, TEST_PASSWORD, TEST_USERNAME,
};

#[test]
fn api_hello_requires_basic_auth() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let _server = ServerGuard::start(port, &db_path, &assets_dir);

    let base_url = format!("http://127.0.0.1:{}/api/hello", port);
    wait_for_server(&base_url);

    let client = build_client();

    let response = client.get(&base_url).send().expect("request failed");
    assert_eq!(response.status().as_u16(), 401);

    let response = client
        .get(&base_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("authorized request failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), "hello");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}
