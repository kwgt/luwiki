/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! 一覧・削除候補・テンプレート・検索のREST認可を検証する
//!

use std::fs;
use std::sync::{Arc, RwLock};

use actix_web::{http::header, test, web};
use serde_json::Value;

use super::authorization_tests::{fixture, token};
use crate::database::types::BearerScope;
use crate::fts;

///
/// prefix自身の認可、比較専用cursor、削除候補、templateのfilterを確認する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離fixtureの許可/非許可・状態・順序を比較し、公開契約を確認する。
///
/// # 注記
/// 境界外のページ/候補を同じfixtureへ置いて返却情報を照合する。
///
#[actix_web::test]
async fn listing_deleted_and_templates_guard_snapshot_candidates() {
    /*
     * 許可/境界外のページとtemplate候補を準備する
     */
    let (base, state) = fixture();
    let db = state.db();
    for path in [
        "/allow/a",
        "/allow/b",
        "/allow/c",
        "/allow2/x",
        "/outside/x",
    ] {
        db.create_page(path, "user", "body".into()).unwrap();
    }
    let deleted = db
        .create_page("/allow/deleted", "user", "deleted".into())
        .unwrap();
    db.delete_page_by_id(&deleted).unwrap();
    let hidden = db
        .create_page("/outside/deleted", "user", "secret".into())
        .unwrap();
    db.delete_page_by_id(&hidden).unwrap();
    let allowed_template = db.create_page("/allow/template", "user",
        concat!(
            "---\nwiki:\n  template:\n",
            "    name: PublicName\n",
            "    description: PublicDescription\n---\nbody",
        ).into()).unwrap();
    db.sync_template_candidate_for_page(&allowed_template)
        .unwrap();
    let hidden_template = db.create_page("/outside/template", "user",
        concat!(
            "---\nwiki:\n  template:\n",
            "    name: HiddenName\n",
            "    description: HiddenDescription\n---\nbody",
        ).into()).unwrap();
    db.sync_template_candidate_for_page(&hidden_template)
        .unwrap();
    let read = token(db, &[BearerScope::Read], &["/allow"]);
    let empty = token(db, &[BearerScope::Read], &["/missing"]);
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(Arc::new(RwLock::new(state))))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;

    /*
     * 上位prefixは403、cursorは存在・独立認可を要求しない
     */
    for uri in [
        "/api/pages?prefix=/",
        "/api/pages?prefix=/allow2",
        "/api/pages/deleted?path=/outside/deleted",
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", read),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 403, "{}", uri);
    }
    for uri in [
        "/api/pages?prefix=/allow&forward=/aaa-nonexistent&limit=2",
        "/api/pages?prefix=/outside/../allow&rewind=/zz-nonexistent&limit=2",
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", read),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["items"].as_array().unwrap().len(), 2);
        assert_eq!(body["has_more"], true);
        assert!(body["anchor"].as_str().unwrap().starts_with("/allow/"));
        for item in body["items"].as_array().unwrap() {
            assert!(item["path"].as_str().unwrap().starts_with("/allow/"));
        }
    }

    /*
     * 削除時pathの候補とtemplate名/説明は許可対象だけを返す
     */
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/pages/deleted?path=/allow/deleted")
            .insert_header((header::AUTHORIZATION, format!("Bearer {}", read)))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body, serde_json::json!([deleted.to_string()]));
    for (bearer, count) in [(&read, 1), (&empty, 0)] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/pages/template")
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", bearer),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body.as_array().unwrap().len(), count);
        assert!(!body.to_string().contains("HiddenName"));
        assert!(!body.to_string().contains("HiddenDescription"));
        assert!(!body.to_string().contains(&hidden_template.to_string()));
    }
    drop(app);
    fs::remove_dir_all(base).unwrap();
}

///
/// 許可後100件、全target merge100、空集合とDB削除状態を検証する
///
/// # 戻り値
/// テスト結果。
///
/// # 概要
/// 隔離fixtureの許可/非許可・状態・順序を比較し、公開契約を確認する。
///
/// # 注記
/// 許可前上位100件を境界外で埋め、同score許可候補を100件超作る。
///
#[actix_web::test]
async fn search_filters_before_top100_and_merges_stably() {
    /*
     * 境界外の高scoreと許可内の同score候補を一括index化する
     */
    let (base, state) = fixture();
    let db = state.db();
    let mut allowed = Vec::new();
    let mut indexed = Vec::new();
    for index in 0..110 {
        indexed.push(
            db.create_page(
                format!("/outside/{}", index),
                "user",
                format!(
                    "# needle\nHiddenMarker {}\n```\nneedle\n```",
                    "needle ".repeat(20)
                ),
            )
            .unwrap(),
        );
    }
    for index in 0..105 {
        allowed.push(
            db.create_page(
                format!("/allow/{}", index),
                "user",
                "# needle\nneedle padding padding padding\n```\nneedle\n```"
                    .into(),
            )
            .unwrap(),
        );
    }
    let deleted = db
        .create_page("/deleted/needle", "user", "needle".into())
        .unwrap();
    indexed.extend(allowed.iter().cloned());
    indexed.push(deleted.clone());
    fts::rebuild_index(state.fts_config(), &[]).unwrap();
    fts::update_pages_index(state.fts_config(), db, &indexed, false).unwrap();
    let unfiltered = fts::search_index(
        state.fts_config(),
        fts::FtsSearchTarget::Body,
        "needle",
        false,
        false,
    )
    .unwrap();
    assert_eq!(unfiltered.len(), 100);
    assert!(
        unfiltered
            .iter()
            .all(|hit| !allowed.contains(&hit.page_id()))
    );
    // FTSのdeleted=falseが残った候補でも返却状態はDB正本を使う。
    db.delete_page_by_id(&deleted).unwrap();
    let mut max_scores = std::collections::HashMap::new();
    for target in [
        fts::FtsSearchTarget::Body,
        fts::FtsSearchTarget::Headings,
        fts::FtsSearchTarget::Code,
    ] {
        for hit in fts::search_index_for_page_ids(
            state.fts_config(),
            target,
            "needle",
            false,
            false,
            &allowed,
        )
        .unwrap()
        {
            let score = max_scores.entry(hit.page_id()).or_insert(hit.score());
            *score = f32::max(*score, hit.score());
        }
    }
    allowed.sort();
    allowed.truncate(100);
    let read = token(db, &[BearerScope::Read], &["/allow"]);
    let deleted_read = token(db, &[BearerScope::Read], &["/deleted"]);
    let empty = token(db, &[BearerScope::Read], &["/missing"]);
    let app = test::init_service(
        actix_web::App::new()
            .app_data(web::Data::new(Arc::new(RwLock::new(state))))
            .service(super::create_api_scope(1024 * 1024)),
    )
    .await;

    /*
     * 同scoreを各targetの収集時とmerge時に安定化する
     */
    for target in ["body", "body,headings,code", "code,headings,body"] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!(
                    "/api/pages/search?expr=needle&target={}",
                    target
                ))
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", read),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200);
        let body: Value = test::read_body_json(resp).await;
        let items = body.as_array().unwrap();
        assert_eq!(items.len(), 100);
        let ids = items
            .iter()
            .map(|item| item["page_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        let expected =
            allowed.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(ids, expected);
        for item in items {
            assert!(item["path"].as_str().unwrap().starts_with("/allow/"));
            assert_eq!(item["deleted"], false);
            assert_eq!(item["revision"], 1);
            if target.contains(',') {
                let id = crate::database::types::PageId::from_string(
                    item["page_id"].as_str().unwrap(),
                )
                .unwrap();
                assert_eq!(
                    (item["score"].as_f64().unwrap() as f32).to_bits(),
                    max_scores[&id].to_bits()
                );
            }
        }
        assert!(!body.to_string().contains("outside"));
        assert!(!body.to_string().contains("HiddenMarker"));
    }

    /*
     * DB deletedを正本とし、空許可集合ならFTSを実行しない
     */
    for (bearer, uri, count) in [
        (
            &deleted_read,
            "/api/pages/search?expr=needle&with_deleted=true",
            1,
        ),
        (&deleted_read, "/api/pages/search?expr=needle", 0),
        (&empty, "/api/pages/search?expr=needle", 0),
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(uri)
                .insert_header((
                    header::AUTHORIZATION,
                    format!("Bearer {}", bearer),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body.as_array().unwrap().len(), count);
        if count == 1 {
            assert_eq!(body[0]["deleted"], true);
            assert_eq!(body[0]["path"], "/deleted/needle");
        }
    }
    drop(app);
    fs::remove_dir_all(base).unwrap();
}
