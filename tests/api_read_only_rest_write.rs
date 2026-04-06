/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

mod common;

use std::fs;

use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::AUTHORIZATION;
use serde_json::Value;

use common::*;

const READ_ONLY_USERNAME: &str = "readonly_user";
const READ_ONLY_PASSWORD: &str = "readonly-pass";

///
/// ReadOnly な Basic ユーザでは write 系 REST API が 403 になることを確認する。
///
/// # 注記
/// 通常ユーザで事前準備したページとアセットに対して、
/// `ReadOnly` ユーザの create / rename / restore / lock / revision /
/// delete / asset upload / asset delete がすべて拒否されることを確認する。
///
#[test]
fn read_only_basic_user_is_forbidden_on_write_rest_endpoints() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials_and_attributes(
        &db_path,
        &assets_dir,
        READ_ONLY_USERNAME,
        READ_ONLY_PASSWORD,
        &["read_only"],
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let fixture = prepare_fixture(&client, &api_base_url);
    let auth = ReadOnlyAuth::Basic;

    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/pages?path={}",
                    api_base_url, fixture.create_path
                ))
                .body(Vec::<u8>::new()),
            &auth,
        ),
        "create page (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/pages/{}/path?rename_to={}",
                    api_base_url, fixture.rename_page_id, fixture.rename_target
                )),
            &auth,
        ),
        "rename page (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/path?restore_to={}",
                api_base_url, fixture.restore_page_id, fixture.restore_target
            )),
            &auth,
        ),
        "restore page (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/lock",
                api_base_url, fixture.lock_page_id
            )),
            &auth,
        ),
        "lock acquire (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.put(format!(
                "{}/pages/{}/lock",
                api_base_url, fixture.lock_page_id
            )),
            &auth,
        ),
        "lock update (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.delete(format!(
                "{}/pages/{}/lock",
                api_base_url, fixture.lock_page_id
            )),
            &auth,
        ),
        "lock delete (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/revision?rollback_to=1",
                api_base_url, fixture.revision_page_id
            )),
            &auth,
        ),
        "revision rollback (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/revision?keep_from=2",
                api_base_url, fixture.revision_page_id
            )),
            &auth,
        ),
        "revision compaction (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.delete(format!(
                "{}/pages/{}",
                api_base_url, fixture.delete_page_id
            )),
            &auth,
        ),
        "delete page (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/pages/{}/assets/readonly-page.bin",
                    api_base_url, fixture.page_asset_upload_page_id
                ))
                .header("Content-Type", "application/octet-stream")
                .body(b"readonly page asset".to_vec()),
            &auth,
        ),
        "page asset upload (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/assets?path={}&file=readonly-global.bin",
                    api_base_url, fixture.global_asset_upload_path
                ))
                .header("Content-Type", "application/octet-stream")
                .body(b"readonly global asset".to_vec()),
            &auth,
        ),
        "global asset upload (basic)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.delete(format!(
                "{}/assets/{}",
                api_base_url, fixture.delete_asset_id
            )),
            &auth,
        ),
        "asset delete (basic)",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// ReadOnly な Bearer ユーザでは write 系 REST API が 403 になることを確認する。
///
/// # 注記
/// Bearer `write` トークンを与えても `ReadOnly` が優先されることを、
/// Basic のときと同じ write 系 endpoint 群で確認する。
///
#[test]
fn read_only_bearer_user_is_forbidden_on_write_rest_endpoints() {
    let (base_dir, db_path, assets_dir) = prepare_test_dirs();
    let port = reserve_port();

    run_add_user(&db_path, &assets_dir);
    run_add_user_with_credentials_and_attributes(
        &db_path,
        &assets_dir,
        READ_ONLY_USERNAME,
        READ_ONLY_PASSWORD,
        &["read_only"],
    );
    let read_only_token = run_create_token_for_user(
        &db_path,
        &assets_dir,
        "write",
        READ_ONLY_USERNAME,
    );

    let server = ServerGuard::start(port, &db_path, &assets_dir);
    let (api_base_url, client) =
        wait_for_server_with_scheme(port, server.stderr_path());

    let fixture = prepare_fixture(&client, &api_base_url);
    let auth = ReadOnlyAuth::Bearer(read_only_token);

    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/pages?path={}",
                    api_base_url, fixture.create_path
                ))
                .body(Vec::<u8>::new()),
            &auth,
        ),
        "create page (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/pages/{}/path?rename_to={}",
                    api_base_url, fixture.rename_page_id, fixture.rename_target
                )),
            &auth,
        ),
        "rename page (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/path?restore_to={}",
                api_base_url, fixture.restore_page_id, fixture.restore_target
            )),
            &auth,
        ),
        "restore page (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/lock",
                api_base_url, fixture.lock_page_id
            )),
            &auth,
        ),
        "lock acquire (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.put(format!(
                "{}/pages/{}/lock",
                api_base_url, fixture.lock_page_id
            )),
            &auth,
        ),
        "lock update (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.delete(format!(
                "{}/pages/{}/lock",
                api_base_url, fixture.lock_page_id
            )),
            &auth,
        ),
        "lock delete (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/revision?rollback_to=1",
                api_base_url, fixture.revision_page_id
            )),
            &auth,
        ),
        "revision rollback (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.post(format!(
                "{}/pages/{}/revision?keep_from=2",
                api_base_url, fixture.revision_page_id
            )),
            &auth,
        ),
        "revision compaction (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.delete(format!(
                "{}/pages/{}",
                api_base_url, fixture.delete_page_id
            )),
            &auth,
        ),
        "delete page (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/pages/{}/assets/readonly-page.bin",
                    api_base_url, fixture.page_asset_upload_page_id
                ))
                .header("Content-Type", "application/octet-stream")
                .body(b"readonly page asset".to_vec()),
            &auth,
        ),
        "page asset upload (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client
                .post(format!(
                    "{}/assets?path={}&file=readonly-global.bin",
                    api_base_url, fixture.global_asset_upload_path
                ))
                .header("Content-Type", "application/octet-stream")
                .body(b"readonly global asset".to_vec()),
            &auth,
        ),
        "global asset upload (bearer)",
    );
    assert_read_only_forbidden(
        apply_read_only_auth(
            client.delete(format!(
                "{}/assets/{}",
                api_base_url, fixture.delete_asset_id
            )),
            &auth,
        ),
        "asset delete (bearer)",
    );

    fs::remove_dir_all(base_dir).expect("cleanup failed");
}

///
/// ReadOnly 拒否確認用の事前準備済みリソース群。
///
struct Fixture {
    create_path: String,
    rename_page_id: String,
    rename_target: String,
    restore_page_id: String,
    restore_target: String,
    lock_page_id: String,
    revision_page_id: String,
    delete_page_id: String,
    page_asset_upload_page_id: String,
    global_asset_upload_path: String,
    delete_asset_id: String,
}

enum ReadOnlyAuth {
    Basic,
    Bearer(String),
}

///
/// ReadOnly 拒否確認に必要なページとアセットを準備する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_base_url` - API ベース URL
///
/// # 戻り値
/// 各 write 系 endpoint の入力に使う ID / path 群を返す。
///
fn prepare_fixture(client: &Client, api_base_url: &str) -> Fixture {
    /*
     * ページ群を準備する
     */
    let rename_path = format!("/ro-rename-{}", unique_suffix());
    let rename_target = format!("/ro-renamed-{}", unique_suffix());
    let restore_path = format!("/ro-restore-{}", unique_suffix());
    let restore_target = format!("/ro-restored-{}", unique_suffix());
    let lock_path = format!("/ro-lock-{}", unique_suffix());
    let revision_path = format!("/ro-revision-{}", unique_suffix());
    let delete_path = format!("/ro-delete-{}", unique_suffix());
    let page_asset_upload_path =
        format!("/ro-page-asset-{}", unique_suffix());
    let global_asset_upload_path =
        format!("/ro-global-asset-{}", unique_suffix());
    let asset_delete_path = format!("/ro-asset-delete-{}", unique_suffix());

    let rename_page_id =
        create_page(client, api_base_url, &rename_path, "rename body");
    let restore_page_id =
        create_page(client, api_base_url, &restore_path, "restore body");
    let lock_page_id =
        create_page(client, api_base_url, &lock_path, "lock body");
    let revision_page_id =
        create_page(client, api_base_url, &revision_path, "rev1");
    let delete_page_id =
        create_page(client, api_base_url, &delete_path, "delete body");
    let page_asset_upload_page_id = create_page(
        client,
        api_base_url,
        &page_asset_upload_path,
        "page asset body",
    );
    create_page(
        client,
        api_base_url,
        &global_asset_upload_path,
        "global asset body",
    );
    let asset_delete_page_id =
        create_page(client, api_base_url, &asset_delete_path, "asset body");

    /*
     * restore / revision / asset delete 用の前処理を行う
     */
    delete_page(client, api_base_url, &restore_page_id);
    update_page_source(client, api_base_url, &revision_page_id, "rev2");
    let delete_asset_id = upload_page_asset(
        client,
        api_base_url,
        &asset_delete_page_id,
        "delete-target.bin",
        b"delete target asset",
    );

    Fixture {
        create_path: format!("/ro-create-{}", unique_suffix()),
        rename_page_id,
        rename_target,
        restore_page_id,
        restore_target,
        lock_page_id,
        revision_page_id,
        delete_page_id,
        page_asset_upload_page_id,
        global_asset_upload_path,
        delete_asset_id,
    }
}

///
/// ReadOnly 用の認証情報をリクエストへ付与する。
///
/// # 引数
/// * `builder` - リクエストビルダ
/// * `auth` - 認証方式
///
/// # 戻り値
/// 認証情報を付与したリクエストビルダを返す。
///
fn apply_read_only_auth(
    builder: RequestBuilder,
    auth: &ReadOnlyAuth,
) -> RequestBuilder {
    match auth {
        ReadOnlyAuth::Basic => {
            builder.basic_auth(READ_ONLY_USERNAME, Some(READ_ONLY_PASSWORD))
        }
        ReadOnlyAuth::Bearer(token) => {
            builder.header(AUTHORIZATION, format!("Bearer {}", token))
        }
    }
}

///
/// ReadOnly により 403 Forbidden が返ることを確認する。
///
/// # 引数
/// * `builder` - 送信対象のリクエスト
/// * `label` - 失敗時メッセージ用のラベル
///
/// # 戻り値
/// なし
///
fn assert_read_only_forbidden(builder: RequestBuilder, label: &str) {
    let response = builder
        .send()
        .unwrap_or_else(|err| panic!("{} request failed: {}", label, err));
    assert_eq!(
        response.status().as_u16(),
        403,
        "{} must return 403",
        label
    );
    assert_eq!(
        response
            .text()
            .unwrap_or_else(|err| panic!("{} body read failed: {}", label, err)),
        r#"{"reason":"forbidden"}"#,
        "{} must return forbidden body",
        label
    );
}

///
/// 通常ユーザでページを作成して本文を保存する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_base_url` - API ベース URL
/// * `path` - 作成するページパス
/// * `body` - 保存する本文
///
/// # 戻り値
/// 作成したページ ID を返す。
///
fn create_page(
    client: &Client,
    api_base_url: &str,
    path: &str,
    body: &str,
) -> String {
    let pages_url = format!("{}/pages", api_base_url);

    /*
     * 下書きページを作成する
     */
    let response = client
        .post(&pages_url)
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .query(&[("path", path)])
        .send()
        .expect("create draft page failed");
    assert_eq!(response.status().as_u16(), 201);

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

    let response_body = response.text().expect("read create page body failed");
    let value: Value = serde_json::from_str(&response_body)
        .expect("parse create page response failed");
    let page_id = value["id"]
        .as_str()
        .expect("page id missing")
        .to_string();

    /*
     * 本文を保存する
     */
    let response = client
        .put(format!("{}/{}/source", pages_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .header("X-Lock-Authentication", format!("token={}", lock_token))
        .body(body.to_string())
        .send()
        .expect("update page source failed");
    assert_eq!(response.status().as_u16(), 204);

    page_id
}

///
/// 通常ユーザでページ本文を更新する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_base_url` - API ベース URL
/// * `page_id` - 対象ページ ID
/// * `body` - 保存する本文
///
/// # 戻り値
/// なし
///
fn update_page_source(
    client: &Client,
    api_base_url: &str,
    page_id: &str,
    body: &str,
) {
    let response = client
        .put(format!("{}/pages/{}/source", api_base_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "text/markdown")
        .body(body.to_string())
        .send()
        .expect("update page source failed");
    assert_eq!(response.status().as_u16(), 204);
}

///
/// 通常ユーザでページを削除する。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_base_url` - API ベース URL
/// * `page_id` - 対象ページ ID
///
/// # 戻り値
/// なし
///
fn delete_page(client: &Client, api_base_url: &str, page_id: &str) {
    let response = client
        .delete(format!("{}/pages/{}", api_base_url, page_id))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .send()
        .expect("delete page failed");
    assert_eq!(response.status().as_u16(), 204);
}

///
/// 通常ユーザでページアセットをアップロードする。
///
/// # 引数
/// * `client` - HTTP クライアント
/// * `api_base_url` - API ベース URL
/// * `page_id` - 対象ページ ID
/// * `file_name` - ファイル名
/// * `body` - ファイル内容
///
/// # 戻り値
/// 作成したアセット ID を返す。
///
fn upload_page_asset(
    client: &Client,
    api_base_url: &str,
    page_id: &str,
    file_name: &str,
    body: &[u8],
) -> String {
    let response = client
        .post(format!(
            "{}/pages/{}/assets/{}",
            api_base_url, page_id, file_name
        ))
        .basic_auth(TEST_USERNAME, Some(TEST_PASSWORD))
        .header("Content-Type", "application/octet-stream")
        .body(body.to_vec())
        .send()
        .expect("upload page asset failed");
    assert_eq!(response.status().as_u16(), 201);
    let body = response.text().expect("read asset response body failed");
    let value: Value =
        serde_json::from_str(&body).expect("parse asset response failed");
    value["id"]
        .as_str()
        .expect("asset id missing")
        .to_string()
}
