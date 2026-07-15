mod support;

use serde_json::Value;

fn write_codex_direct_files(app: &support::TestApp, config: &str, auth: &str) {
    let codex_dir = app.home_dir().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(codex_dir.join("config.toml"), config).expect("write config");
    std::fs::write(codex_dir.join("auth.json"), auth).expect("write auth");
}

#[test]
fn codex_route_service_roundtrip_plan_apply_verify_and_restore() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";

    write_codex_direct_files(
        &app,
        r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = aio_coding_hub_lib::test_support::cli_proxy_codex_plan_external_enable_json(
        &handle,
        aio_origin,
        guarded_origin,
    )
    .expect("plan external enable");
    assert_eq!(support::json_str(&plan, "current_route_mode"), "unproxied");
    assert!(support::json_bool(&plan, "cli_proxy_enable_required"));
    assert!(plan
        .get("provider_sync")
        .and_then(|value| value.get("change_required"))
        .and_then(Value::as_bool)
        .unwrap_or(false));

    let guarded = aio_coding_hub_lib::test_support::cli_proxy_codex_apply_guarded_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": support::json_u64(&plan, "generation"),
            "expectedCanonicalSha256": support::json_str(&plan, "canonical_config_sha256"),
            "aioOrigin": aio_origin,
            "guardedOrigin": guarded_origin,
            "desiredEnabled": true,
            "sourceCommit": "0123456789abcdef0123456789abcdef01234567",
            "processShouldRun": true
        }),
    )
    .expect("apply guarded route");
    assert_eq!(
        guarded
            .get("route")
            .and_then(|value| value.get("route_mode"))
            .and_then(Value::as_str),
        Some("guarded")
    );

    let verify_guarded =
        aio_coding_hub_lib::test_support::cli_proxy_codex_verify_route_json(&handle)
            .expect("verify guarded route");
    assert_eq!(
        verify_guarded
            .get("effective_origin")
            .and_then(Value::as_str),
        Some(guarded_origin)
    );

    let direct = aio_coding_hub_lib::test_support::cli_proxy_codex_apply_direct_aio_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": guarded
                .get("route")
                .and_then(|value| value.get("generation"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            "expectedCanonicalSha256": guarded
                .get("route")
                .and_then(|value| value.get("canonical_config_sha256"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "aioOrigin": aio_origin,
            "desiredEnabled": true,
            "sourceCommit": null,
            "processShouldRun": false
        }),
    )
    .expect("apply direct aio route");
    assert_eq!(
        direct
            .get("route")
            .and_then(|value| value.get("effective_origin"))
            .and_then(Value::as_str),
        Some(aio_origin)
    );

    let restore = aio_coding_hub_lib::test_support::cli_proxy_codex_restore_unproxied_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": direct
                .get("route")
                .and_then(|value| value.get("generation"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            "expectedCanonicalSha256": direct
                .get("route")
                .and_then(|value| value.get("canonical_config_sha256"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "aioOrigin": aio_origin,
            "desiredEnabled": false,
            "keepCliProxyEnabled": false,
            "sourceCommit": null,
            "processShouldRun": false
        }),
    )
    .expect("restore unproxied route");
    assert_eq!(
        restore
            .get("route")
            .and_then(|value| value.get("route_mode"))
            .and_then(Value::as_str),
        Some("unproxied")
    );
    assert_eq!(
        restore
            .get("route")
            .and_then(|value| value.get("cli_proxy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn codex_route_reconcile_reports_no_pending_transition_after_commit() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let reconcile =
        aio_coding_hub_lib::test_support::cli_proxy_codex_reconcile_pending_route_json(&handle)
            .expect("reconcile route");
    assert!(reconcile
        .get("pending_transition_reconciled")
        .is_some_and(Value::is_null));
}
