/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use reqwest::header::AUTHORIZATION;
use reqwest::blocking::Client;
use serde_json::Value;

use common::*;

#[test]
/// GET: 最新ソースとヘッダが取得できることを確認する。
fn get_page_source_returns_latest_markdown() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/test", "source body");

    let url = format!("{}/{}/source", base_url, page_id);
    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source failed");

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("Content-Type")
            .expect("missing content-type")
            .to_str()
            .expect("content-type to_str failed"),
        "text/markdown"
    );
    assert_eq!(
        response
            .headers()
            .get("Cache-Control")
            .expect("missing cache-control")
            .to_str()
            .expect("cache-control to_str failed"),
        "private, max-age=3600, no-cache"
    );
    let etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'));
    assert_eq!(response.text().expect("read body failed"), "source body");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: rev パラメータの妥当性検証を確認する。
fn get_page_source_with_rev_validates_revision() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/test", "source body");

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .get(&url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source rev=1 failed");
    assert_eq!(response.status().as_u16(), 200);

    let response = client
        .get(&url)
        .query(&[("rev", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source rev=2 failed");
    assert_eq!(response.status().as_u16(), 404);

    let response = client
        .get(&url)
        .query(&[("rev", "abc")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source rev=abc failed");
    assert_eq!(response.status().as_u16(), 400);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: If-None-Match 一致時に304を返すことを確認する。
fn get_page_source_returns_not_modified_when_etag_matches() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/etag-test", "source body");
    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source failed");
    let etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();

    let response = client
        .get(&url)
        .header("If-None-Match", etag.clone())
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("conditional get source failed");

    assert_eq!(response.status().as_u16(), 304);
    assert_eq!(
        response
            .headers()
            .get("Cache-Control")
            .expect("missing cache-control")
            .to_str()
            .expect("cache-control to_str failed"),
        "private, max-age=3600, no-cache"
    );
    assert_eq!(
        response
            .headers()
            .get("ETag")
            .expect("missing etag")
            .to_str()
            .expect("etag to_str failed"),
        etag
    );
    assert_eq!(response.text().expect("read body failed"), "");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// GET: 認証必須と不正ID時の挙動を確認する。
fn get_page_source_requires_auth_and_valid_id() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let url = format!("{}/pages/not-a-ulid/source", api_base_url);

    let response = client
        .get(&url)
        .send()
        .expect("get source without auth failed");
    assert_eq!(response.status().as_u16(), 401);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get source invalid id failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: 通常更新で新リビジョンが作成されることを確認する。
fn put_page_source_creates_new_revision() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-test", "original body");

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("updated body".to_string())
        .send()
        .expect("put source failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get latest after put failed");
    assert_eq!(response.status().as_u16(), 200);
    let latest_etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    assert!(latest_etag.starts_with('"') && latest_etag.ends_with('"'));
    assert_eq!(response.text().expect("read body failed"), "updated body");

    let response = client
        .get(&url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev=1 etag after put failed");
    let rev1_etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    assert_ne!(latest_etag, rev1_etag);
    assert_eq!(response.text().expect("read body failed"), "original body");

    let response = client
        .get(&url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev=1 after put failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), "original body");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: front matter の YAML 構文不正時に詳細付き 400 を返すことを確認する。
fn put_page_source_rejects_invalid_front_matter_syntax() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-front-matter-syntax", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let invalid_source = "---\nwiki: [\n---\n# title\n本文";

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(invalid_source.to_string())
        .send()
        .expect("put invalid front matter syntax failed");

    assert_eq!(response.status().as_u16(), 400);

    let value = read_json_body(response);

    assert_eq!(
        value["error"].as_str().expect("missing error"),
        "front matter invalid"
    );
    assert_eq!(
        value["kind"].as_str().expect("missing kind"),
        "front_matter"
    );
    assert_eq!(
        value["detail"]["type"].as_str().expect("missing detail.type"),
        "syntax"
    );
    assert!(
        value["detail"]["line"]
            .as_u64()
            .expect("missing detail.line")
            > 0
    );
    assert!(
        value["detail"]["column"]
            .as_u64()
            .expect("missing detail.column")
            > 0
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: front matter のトップレベル構造不正時に詳細付き 400 を返すことを確認する。
fn put_page_source_rejects_invalid_front_matter_top_level() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-front-matter-top-level", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let invalid_source = "---\n- item\n---\n# title\n本文";

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(invalid_source.to_string())
        .send()
        .expect("put invalid front matter top-level failed");

    assert_eq!(response.status().as_u16(), 400);

    let value = read_json_body(response);

    assert_eq!(
        value["error"].as_str().expect("missing error"),
        "front matter invalid"
    );
    assert_eq!(
        value["kind"].as_str().expect("missing kind"),
        "front_matter"
    );
    assert_eq!(
        value["detail"]["type"].as_str().expect("missing detail.type"),
        "validation"
    );
    assert_eq!(
        value["detail"]["property_path"]
            .as_str()
            .expect("missing detail.property_path"),
        "$"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: custom_meta があっても既存 wiki validation の property_path を維持することを確認する。
fn put_page_source_keeps_wiki_validation_with_custom_meta() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-front-matter-custom-meta-wiki-error", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let invalid_source = [
        "---",
        "wiki:",
        "  tags:",
        "    - rust lang",
        "custom_meta:",
        "  project: alpha",
        "---",
        "# title",
        "本文",
    ]
    .join("\n");

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(invalid_source)
        .send()
        .expect("put invalid custom_meta wiki source failed");

    assert_eq!(response.status().as_u16(), 400);

    let value = read_json_body(response);
    assert_eq!(
        value["detail"]["property_path"]
            .as_str()
            .expect("missing detail.property_path"),
        "wiki.tags[0]"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: custom_meta 自身の validation error が既存 front matter 応答形式と競合しないことを確認する。
fn put_page_source_reports_custom_meta_validation_without_conflict() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-front-matter-custom-meta-error", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let invalid_source = [
        "---",
        "custom_meta: tagged",
        "---",
        "# title",
        "本文",
    ]
    .join("\n");

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(invalid_source)
        .send()
        .expect("put invalid custom_meta source failed");

    assert_eq!(response.status().as_u16(), 400);

    let value = read_json_body(response);
    assert_eq!(
        value["kind"].as_str().expect("missing kind"),
        "front_matter"
    );
    assert_eq!(
        value["detail"]["type"].as_str().expect("missing detail.type"),
        "validation"
    );
    assert_eq!(
        value["detail"]["property_path"]
            .as_str()
            .expect("missing detail.property_path"),
        "custom_meta"
    );
    assert_eq!(
        value["detail"]["message"]
            .as_str()
            .expect("missing detail.message"),
        "custom_meta must be object"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT + GET: custom_meta を含む front matter を保存し再読込できることを確認する。
fn put_page_source_round_trips_front_matter_with_custom_meta() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-front-matter-custom-meta", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let source = [
        "---",
        "wiki:",
        "  tags:",
        "    - rust",
        "custom_meta:",
        "  project: alpha",
        "  priority: 3",
        "  flags:",
        "    reviewed: true",
        "---",
        "# title",
        "本文",
    ]
    .join("\n");

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(source.clone())
        .send()
        .expect("put custom_meta source failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get custom_meta source failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), source);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT + GET: wiki / mcp / custom_meta を同時に含む front matter を保存し再読込できることを確認する。
fn put_page_source_round_trips_front_matter_with_builtin_namespaces_and_custom_meta() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(
        &client,
        &base_url,
        "/put-front-matter-builtin-and-custom-meta",
        "body",
    );
    let url = format!("{}/{}/source", base_url, page_id);

    let source = [
        "---",
        "wiki:",
        "  tags:",
        "    - rust",
        "    - search",
        "mcp:",
        "  primitive: prompt",
        "  name: summarize",
        "  description: summarize current page",
        "  system: keep concise",
        "  arguments:",
        "    - name: target",
        "      description: page path",
        "      required: true",
        "custom_meta:",
        "  project: alpha",
        "  owner: docs",
        "---",
        "# title",
        "本文",
    ]
    .join("\n");

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(source.clone())
        .send()
        .expect("put builtin namespaces with custom_meta source failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get builtin namespaces with custom_meta source failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().expect("read body failed"), source);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: Bearer read は拒否され、write は更新できることを確認する。
fn put_page_source_enforces_bearer_read_and_write_scopes() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let read_token = run_create_token(&db_path, &assets_dir, "read");
    let write_token = run_create_token(&db_path, &assets_dir, "write");
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id =
        create_page(&client, &base_url, "/put-bearer-scope", "original body");
    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .header(AUTHORIZATION, format!("Bearer {}", read_token))
        .header("Content-Type", "text/markdown")
        .body("read should fail".to_string())
        .send()
        .expect("put source with read bearer failed");
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        response.text().expect("read forbidden body failed"),
        r#"{"reason":"forbidden"}"#
    );

    let response = client
        .put(&url)
        .header(AUTHORIZATION, format!("Bearer {}", write_token))
        .header("Content-Type", "text/markdown")
        .body("write should pass".to_string())
        .send()
        .expect("put source with write bearer failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get latest after bearer put failed");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response.text().expect("read updated body failed"),
        "write should pass"
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: ReadOnly な Basic ユーザは更新できないことを確認する。
fn put_page_source_rejects_read_only_basic_user() {
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
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-readonly-basic", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .basic_auth("readonly_user", Some("readonly-pass"))
        .header("Content-Type", "text/markdown")
        .body("readonly should fail".to_string())
        .send()
        .expect("put source with readonly basic failed");

    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        response.text().expect("read forbidden body failed"),
        r#"{"reason":"forbidden"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: ReadOnly な Bearer ユーザは write スコープを持っていても更新できないことを確認する。
fn put_page_source_rejects_read_only_bearer_user() {
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
    let readonly_write_token = run_create_token_for_user(
        &db_path,
        &assets_dir,
        "write",
        "readonly_user",
    );
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-readonly-bearer", "body");
    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .header(AUTHORIZATION, format!("Bearer {}", readonly_write_token))
        .header("Content-Type", "text/markdown")
        .body("readonly should fail".to_string())
        .send()
        .expect("put source with readonly bearer failed");

    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        response.text().expect("read forbidden body failed"),
        r#"{"reason":"forbidden"}"#
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: amend=true で同一リビジョン更新になることを確認する。
fn put_page_source_amend_updates_latest_without_revision() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);

    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/amend-test", "before amend");

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .query(&[("amend", "true")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("after amend".to_string())
        .send()
        .expect("put amend failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get latest after amend failed");
    assert_eq!(response.status().as_u16(), 200);
    let latest_etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    assert!(latest_etag.starts_with('"') && latest_etag.ends_with('"'));
    assert_eq!(response.text().expect("read body failed"), "after amend");

    let response = client
        .get(&url)
        .query(&[("rev", "1")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev=1 after amend failed");
    let rev1_etag = response
        .headers()
        .get("ETag")
        .expect("missing etag")
        .to_str()
        .expect("etag to_str failed")
        .to_string();
    assert_eq!(latest_etag, rev1_etag);

    let response = client
        .get(&url)
        .query(&[("rev", "2")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("get rev=2 after amend failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: ロック中はトークン必須で、成功時にロック解除されることを確認する。
fn put_page_source_requires_lock_token_when_locked() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/lock-put", "initial");

    let source_url = format!("{}/{}/source", base_url, page_id);
    let lock_url = format!("{}/{}/lock", base_url, page_id);

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);
    let token = parse_lock_header(&response).expect("missing lock token");

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("blocked".to_string())
        .send()
        .expect("put without lock token failed");
    assert_eq!(response.status().as_u16(), 423);

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", token))
        .body("allowed".to_string())
        .send()
        .expect("put with lock token failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .get(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock get after put failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: 不正クエリ/ヘッダ/ボディで 400 になることを確認する。
fn put_page_source_rejects_invalid_query_and_headers() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/put-invalid", "body");

    let url = format!("{}/{}/source", base_url, page_id);

    let response = client
        .put(&url)
        .query(&[("amend", "maybe")])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("update".to_string())
        .send()
        .expect("put amend=maybe failed");
    assert_eq!(response.status().as_u16(), 400);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .body("update".to_string())
        .send()
        .expect("put without content-type failed");
    assert_eq!(response.status().as_u16(), 400);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "application/json")
        .body("update".to_string())
        .send()
        .expect("put invalid content-type failed");
    assert_eq!(response.status().as_u16(), 400);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(vec![0xff, 0xfe, 0xfd])
        .send()
        .expect("put invalid utf8 failed");
    assert_eq!(response.status().as_u16(), 400);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: 不正なページIDで 404 になることを確認する。
fn put_page_source_rejects_invalid_page_id() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let url = format!("{}/pages/not-a-ulid/source", api_base_url);

    let response = client
        .put(&url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("update".to_string())
        .send()
        .expect("put invalid page id failed");
    assert_eq!(response.status().as_u16(), 404);

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

#[test]
/// PUT: ロック関連の 403/423 が理由付きで返ることを確認する。
fn put_page_source_lock_errors_include_reason() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) = wait_for_server_with_scheme(port, server.stderr_path());

    let base_url = format!("{}/pages", api_base_url);
    let page_id = create_page(&client, &base_url, "/lock-reason", "body");

    let source_url = format!("{}/{}/source", base_url, page_id);
    let lock_url = format!("{}/{}/lock", base_url, page_id);

    let response = client
        .post(&lock_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("lock post failed");
    assert_eq!(response.status().as_u16(), 204);

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body("update".to_string())
        .send()
        .expect("put without token failed");
    assert_eq!(response.status().as_u16(), 423);
    assert_eq!(read_error_reason(response), "page locked");

    let response = client
        .put(&source_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", "token=invalid")
        .body("update".to_string())
        .send()
        .expect("put invalid token failed");
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(read_error_reason(response), "lock token invalid");

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// テスト用ページを作成する。
///
/// # 引数
/// * `client` - HTTPクライアント
/// * `base_url` - APIベースURL
/// * `path` - ページパス
/// * `body` - ページ本文
///
/// # 戻り値
/// 作成したページID
///
fn create_page(client: &Client, base_url: &str, path: &str, body: &str) -> String {
    /*
     * ドラフト作成
     */
    let pages_url = if base_url.ends_with("/pages") {
        base_url.to_string()
    } else {
        format!("{}/pages", base_url)
    };
    let response = client
        .post(&pages_url)
        .query(&[("path", path)])
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("create page failed");

    assert_eq!(response.status().as_u16(), 201);

    /*
     * ロックトークンの取得
     */
    let lock_header = response
        .headers()
        .get("X-Page-Lock")
        .expect("missing lock header")
        .to_str()
        .expect("lock header to_str failed");
    let lock_token = lock_header
        .split_whitespace()
        .find_map(|part| part.strip_prefix("token="))
        .map(str::to_string)
        .expect("missing lock token");

    let response_body = response.text().expect("read response body failed");
    let value: Value =
        serde_json::from_str(&response_body).expect("parse create page response failed");
    let page_id = value["id"].as_str().expect("missing page id").to_string();

    /*
     * ページソースの登録
     */
    let response = client
        .put(&format!("{}/{}/source", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", lock_token))
        .body(body.to_string())
        .send()
        .expect("update page failed");

    assert_eq!(response.status().as_u16(), 204);

    page_id
}

///
/// ロックレスポンスのヘッダからトークンを抽出する。
///
/// # 引数
/// * `response` - レスポンス
///
/// # 戻り値
/// トークン文字列(存在しない場合はNone)
///
fn parse_lock_header(response: &reqwest::blocking::Response) -> Option<String> {
    let raw = response.headers().get("X-Page-Lock")?.to_str().ok()?;
    for part in raw.split_whitespace() {
        if let Some(value) = part.strip_prefix("token=") {
            return Some(value.to_string());
        }
    }
    None
}

///
/// エラーレスポンスから理由メッセージを取得する
///
/// # 引数
/// * `response` - エラーレスポンス
///
/// # 戻り値
/// 理由メッセージ
///
fn read_error_reason(response: reqwest::blocking::Response) -> String {
    let value = read_json_body(response);
    value["reason"]
        .as_str()
        .expect("missing reason")
        .to_string()
}

///
/// JSON レスポンス本文をパースする
///
/// # 引数
/// * `response` - レスポンス
///
/// # 戻り値
/// JSON 値
///
fn read_json_body(response: reqwest::blocking::Response) -> Value {
    let body = response.text().expect("read error body failed");
    serde_json::from_str(&body).expect("parse json body failed")
}
