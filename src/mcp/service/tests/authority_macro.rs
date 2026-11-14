/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! authority展開とMCP取得・保存境界を確認する
//!

use super::*;

///
/// resource/promptの元本文だけを展開し、保存値とmetadataを保持する
///
/// # 概要
/// 同じDBを別authorityで取得して追従とprompt引数の非再処理を確認する。
///
/// # 注記
/// code内展開・scope拒否とrawページ取得も照合する。
///
/// # 戻り値
/// なし。
///
#[test]
fn authority_macro_resource_prompt_and_raw_source() {
    let (base, manager) = open_test_manager();
    manager.add_user("user", "pass", None).unwrap();
    let resource_source = concat!(
        "---\nmcp:\n  primitive: resource\n",
        "  resource_path: /docs/authority\n  name: resource\n",
        "  description: '{{mcp:authority}}'\n---\n",
        "luwiki://{{mcp:authority}}/pages/manual\n",
        "`{{mcp:authority}}` {{today}} {{@value}}\n",
    );
    let prompt_source = concat!(
        "---\nmcp:\n  primitive: prompt\n  name: authority-test\n",
        "  description: '{{mcp:authority}}'\n",
        "  system: 'System {{mcp:authority}} {{@value}}'\n",
        "  arguments:\n    - name: value\n",
            "      description: value\n      required: true\n",
        "---\n{{mcp:authority}} / {{@value}}\n",
        "```text\n{{mcp:authority}}\n```\n",
    );
    manager.create_page("/resources/a", "user", resource_source.into())
        .unwrap();
    manager.create_page("/prompts/a", "user", prompt_source.into()).unwrap();
    let auth = AuthContext::new(AuthUser::new("user".into()),
        BearerScopeSet::from_iter([BearerScope::Read]),
        PathPrefixSet::from_iter(["/"]), None);
    let arguments = JsonObject::from_iter([("value".into(),
        JsonValue::String("{{mcp:authority}}/{{@value}}".into()))]);

    /*
     * 再取得時に現在のauthorityへ追従し、挿入値は展開しない
     */
    for authority in ["local.luwiki", "docs.example.test"] {
        let service = McpService::with_resource_authority(authority.into());
        let result = service.read_resource(&auth, &manager,
            &format!("luwiki://{authority}/docs/authority")).unwrap();
        assert_eq!(result.text(), format!(
            "luwiki://{authority}/pages/manual\n`{authority}` \
             {{{{today}}}} {{{{@value}}}}\n",
        ));
        assert_eq!(result.revision(), Some(1));
        let prompt = service.get_prompt(&auth, &manager,
            "authority-test", Some(&arguments)).unwrap();
        assert_eq!(prompt.message(), format!(
            "System {authority} {{{{mcp:authority}}}}/{{{{@value}}}}\n\n\
             {authority} / {{{{mcp:authority}}}}/{{{{@value}}}}\n\
             ```text\n{authority}\n```\n",
        ));
        assert_eq!(prompt.revision(), 1);
        let listed = service.list_resources(&auth, &manager, None).unwrap();
        assert!(listed.items().iter().any(|item| {
            item.description() == "{{mcp:authority}}"
        }));
        assert_eq!(service.get_page(&auth, &manager, "/resources/a", None)
            .unwrap().content(), resource_source);
        assert_eq!(service.get_page(&auth, &manager, "/prompts/a", None)
            .unwrap().content(), prompt_source);
    }

    /*
     * 認可不足は展開より先に拒否する
     */
    let denied = AuthContext::new(AuthUser::new("user".into()),
        BearerScopeSet::from_iter([BearerScope::Update]),
        PathPrefixSet::from_iter(["/"]), None);
    assert_eq!(McpService::new().read_resource(&denied, &manager,
        "luwiki://local.luwiki/docs/authority").unwrap_err().code(),
        McpErrorCode::Forbidden);
    fs::remove_dir_all(base).unwrap();
}

///
/// 初期Sandboxへ案内を入れ、既存ページは上書きしない
///
/// # 概要
/// 初期生成と利用者編集済みの両方でraw保存・再初期化を確認する。
///
/// # 注記
/// 案内更新はOP5、既存環境への反映はOP7として区別する。
///
/// # 戻り値
/// なし。
///
#[test]
fn authority_macro_sandbox_seed_does_not_overwrite() {
    for existing in [false, true] {
        let (base, manager) = open_test_manager();
        manager.add_user("user", "pass", None).unwrap();
        if existing {
            manager.create_page("/Sandbox", "user", "User edited".into())
                .unwrap();
        }
        /*
         * 再初期化でもsourceとrevisionは維持する
         */
        manager.ensure_default_root("user").unwrap();
        manager.ensure_default_root("user").unwrap();
        let auth = AuthContext::new(AuthUser::new("user".into()),
            BearerScopeSet::from_iter([BearerScope::Read]),
            PathPrefixSet::from_iter(["/"]), None);
        let page = McpService::new().get_page(&auth, &manager,
            "/Sandbox", None).unwrap();
        assert_eq!(page.revision(), 1);
        if existing {
            assert_eq!(page.content(), "User edited");
        } else {
            assert_eq!(page.content(), include_str!(
                "../../../../assets/default_sandbox.md",
            ));
            assert!(page.content().contains("{{mcp:authority}}"));
        }
        fs::remove_dir_all(base).unwrap();
    }
}
