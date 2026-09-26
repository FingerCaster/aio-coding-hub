use super::*;
use crate::domain::native_cli::{NativeTargetMode, NativeTargetSelection};

fn setup() -> (tempfile::TempDir, NativeTarget) {
    let temp = tempfile::tempdir().unwrap();
    let target = targets::resolve(
        temp.path(),
        &NativeTargetSelection {
            client: NativeClient::Omp,
            mode: NativeTargetMode::Custom,
            agent_dir: Some(temp.path().join("agent").to_string_lossy().into()),
            profile: None,
        },
        None,
        None,
    )
    .unwrap();
    (temp, target)
}
fn seed(target: &NativeTarget, name: &str, content: &str) {
    std::fs::create_dir_all(&target.agent_dir).unwrap();
    std::fs::write(Path::new(&target.agent_dir).join(name), content).unwrap();
}
fn snapshot(target: &NativeTarget) -> OmpSettingsSnapshot {
    super::super::with_target_lock(target, |session| read(target, session)).unwrap()
}
fn patch(path: &[&str], value: Option<Value>) -> OmpSettingPatch {
    OmpSettingPatch {
        path: path.iter().map(|s| (*s).into()).collect(),
        value,
    }
}
fn input(target: &NativeTarget, patches: Vec<OmpSettingPatch>) -> OmpSettingsSaveInput {
    OmpSettingsSaveInput {
        target_id: target.target_id.clone(),
        expected_revision: snapshot(target).revision,
        patches,
    }
}

#[test]
fn reading_missing_config_is_pure_and_noop_does_not_create_files() {
    let (_tmp, target) = setup();
    let snap = snapshot(&target);
    assert!(snap.values.is_empty());
    assert_eq!(snap.agents.len(), 5);
    assert!(!Path::new(&target.agent_dir).exists());
    assert!(
        !save(
            &target,
            &input(&target, vec![patch(&["modelRoles", "default"], None)])
        )
        .unwrap()
        .changed
    );
    assert!(!Path::new(&target.agent_dir).exists());
}

#[test]
fn resetting_an_absent_field_preserves_existing_empty_sections() {
    let (_tmp, target) = setup();
    seed(&target, "config.yml", "# keep formatting\ntask: {}\n");
    let result = save(
        &target,
        &input(&target, vec![patch(&["task", "maxConcurrency"], None)]),
    )
    .unwrap();
    assert!(!result.changed);
    assert_eq!(
        std::fs::read_to_string(Path::new(&target.agent_dir).join("config.yml")).unwrap(),
        "# keep formatting\ntask: {}\n"
    );
}

#[test]
fn patches_preserve_secrets_unknown_fields_and_private_original_backup() {
    let (_tmp, target) = setup();
    let original = "# original comment\nauth:\n  broker:\n    token: SECRET\nfuture:\n  nested: [1, 2]\nmodelRoles:\n  review: custom/review\ntask:\n  customFuture: keep\n";
    seed(&target, "config.yaml", original);
    let before = snapshot(&target);
    assert!(!serde_json::to_string(&before).unwrap().contains("SECRET"));
    let result = save(
        &target,
        &input(
            &target,
            vec![
                patch(
                    &["modelRoles", "default"],
                    Some(json!("aio-channel/model:high")),
                ),
                patch(&["task", "maxConcurrency"], Some(json!(4))),
            ],
        ),
    )
    .unwrap();
    assert!(result.changed);
    assert_eq!(
        std::fs::read_to_string(result.backup_path.unwrap()).unwrap(),
        original
    );
    let doc = load(&target).unwrap();
    assert_eq!(doc.root["auth"]["broker"]["token"], "SECRET");
    assert_eq!(doc.root["future"]["nested"], json!([1, 2]));
    assert_eq!(doc.root["modelRoles"]["review"], "custom/review");
    assert_eq!(doc.root["task"]["customFuture"], "keep");
    assert!(doc.path.ends_with("config.yaml"));
    assert!(!Path::new(&target.agent_dir).join("config.yml").exists());
    save(
        &target,
        &input(&target, vec![patch(&["modelRoles", "default"], None)]),
    )
    .unwrap();
    assert!(load(&target).unwrap().root["modelRoles"]
        .get("default")
        .is_none());
}

#[test]
fn external_edits_and_filename_priority_changes_reject_stale_writes() {
    let (_tmp, target) = setup();
    seed(&target, "config.yaml", "task:\n  maxConcurrency: 2\n");
    let stale = input(
        &target,
        vec![patch(&["task", "maxConcurrency"], Some(json!(4)))],
    );
    seed(&target, "config.yaml", "task:\n  maxConcurrency: 3\n");
    assert!(save(&target, &stale).is_err());
    let stale = input(&target, stale.patches);
    seed(&target, "config.yml", "task:\n  maxConcurrency: 7\n");
    assert!(save(&target, &stale).is_err());
    assert_eq!(load(&target).unwrap().root["task"]["maxConcurrency"], 7);
}

#[test]
fn legacy_is_read_only_and_pi_targets_are_rejected() {
    let (_tmp, mut target) = setup();
    seed(&target, "settings.json", "{\"defaultModel\":\"legacy\"}");
    assert!(!snapshot(&target).writable);
    assert!(save(&target, &input(&target, vec![])).is_err());
    target.client = NativeClient::Pi;
    assert!(load(&target).is_err());
}

#[test]
fn duplicate_unsafe_and_oversized_documents_are_never_replaced() {
    let (_tmp, target) = setup();
    for text in [
        "task: {}\ntask: {}",
        "x: &ref 1\ny: *ref",
        "x: !tag 2",
        "---\nx: 1\n---\ny: 2",
        "- scalar",
    ] {
        seed(&target, "config.yml", text);
        assert!(load(&target).is_err(), "{text}");
        assert_eq!(
            std::fs::read_to_string(Path::new(&target.agent_dir).join("config.yml")).unwrap(),
            text
        );
    }
    seed(&target, "config.yml", &"x".repeat(MAX_CONFIG_BYTES + 1));
    assert!(load(&target).is_err());
}

#[test]
fn whitelist_types_ranges_and_record_members_are_validated() {
    for p in [
        patch(&["auth", "broker", "token"], Some(json!("no"))),
        patch(&["task.maxConcurrency"], Some(json!(4))),
        patch(&["task", "maxConcurrency"], Some(json!(-1))),
        patch(&["task", "maxConcurrency"], Some(json!(1.5))),
        patch(&["task", "enableLsp"], Some(json!("true"))),
        patch(&["defaultThinkingLevel"], Some(json!("off"))),
        patch(&["task", "agentModelOverrides"], Some(json!({}))),
        patch(&["modelRoles", "__proto__"], Some(json!("model"))),
    ] {
        assert!(validate_patch(&p).is_err());
    }
    for p in [
        patch(
            &["modelRoles", "review.custom"],
            Some(json!("provider/model:high")),
        ),
        patch(
            &["task", "agentModelOverrides", "reviewer"],
            Some(json!(["@slow", "@default"])),
        ),
        patch(&["task", "agentAdvisor", "scout"], Some(json!("off"))),
        patch(&["task", "maxRecursionDepth"], Some(json!(-1))),
        patch(&["task", "disabledAgents"], Some(json!(["sonic"]))),
    ] {
        validate_patch(&p).unwrap();
    }
}

#[test]
fn nested_scalar_parent_is_not_replaced_even_when_resetting_a_field() {
    let (_tmp, target) = setup();
    seed(&target, "config.yml", "task: invalid\n");
    assert!(save(
        &target,
        &input(
            &target,
            vec![patch(&["task", "maxConcurrency"], Some(json!(4)))]
        )
    )
    .is_err());
    assert_eq!(load(&target).unwrap().root["task"], "invalid");
}

#[test]
fn model_projection_exposes_ids_but_never_provider_credentials() {
    let (_tmp, target) = setup();
    seed(&target, "models.yml", "providers:\n  aio-channel:\n    api: openai-responses\n    baseUrl: http://127.0.0.1:1234\n    apiKey: SECRET\n    models:\n      - id: test-model\n        name: Test\n");
    let snap = snapshot(&target);
    assert_eq!(snap.models[0].selector, "aio-channel/test-model");
    assert!(!serde_json::to_string(&snap).unwrap().contains("SECRET"));
}

#[test]
fn custom_agent_creation_edit_conflict_and_unknown_frontmatter_preservation() {
    let (_tmp, target) = setup();
    let content = "---\nname: custom-review\ndescription: Custom reviewer\nmodel: '@slow'\nfuture: {nested: true}\n---\nPrompt with {{variables}}\n";
    let mut request = OmpAgentSaveInput {
        target_id: target.target_id.clone(),
        file_name: "review.md".into(),
        expected_revision: "missing".into(),
        content: content.into(),
    };
    let created = save_agent(&target, &request).unwrap();
    assert!(created.changed);
    assert!(created.backup_path.is_none());
    assert!(save_agent(&target, &request).is_err());
    let doc = read_agent(&target, "review.md").unwrap();
    assert_eq!(doc.content, content);
    request.expected_revision = doc.revision;
    request.content.push_str("Extra instruction\n");
    let changed = save_agent(&target, &request).unwrap();
    assert_eq!(
        std::fs::read_to_string(changed.backup_path.unwrap()).unwrap(),
        content
    );
    assert!(read_agent(&target, "review.md")
        .unwrap()
        .content
        .contains("future: {nested: true}"));
    assert_eq!(
        snapshot(&target)
            .agents
            .iter()
            .filter(|a| a.source == "custom")
            .count(),
        1
    );
}

#[test]
fn agent_paths_and_duplicate_names_are_rejected() {
    let (_tmp, target) = setup();
    for name in [
        "../escape.md",
        "C:/escape.md",
        "nested/a.md",
        "CON.md",
        "bad.md:stream",
        ".md",
        "x.txt",
    ] {
        assert!(read_agent(&target, name).is_err(), "{name}");
    }
    let mut request = OmpAgentSaveInput {
        target_id: target.target_id.clone(),
        file_name: "a.md".into(),
        expected_revision: "missing".into(),
        content: "---\nname: worker\ndescription: Work\n---\nPrompt\n".into(),
    };
    save_agent(&target, &request).unwrap();
    request.file_name = "b.md".into();
    assert!(save_agent(&target, &request).is_err());
    request.content = "---\nname: main\ndescription: Work\n---\nPrompt\n".into();
    assert!(save_agent(&target, &request).is_err());
}

#[test]
fn missing_agent_overrides_remain_visible_and_custom_agent_shadows_bundled() {
    let (_tmp, target) = setup();
    seed(&target, "config.yml", "task:\n  agentModelOverrides:\n    plugin-agent: '@slow'\n  disabledAgents: [removed-agent]\n");
    save_agent(
        &target,
        &OmpAgentSaveInput {
            target_id: target.target_id.clone(),
            file_name: "scout.md".into(),
            expected_revision: "missing".into(),
            content: "---\nname: scout\ndescription: My scout\n---\nRead\n".into(),
        },
    )
    .unwrap();
    let snap = snapshot(&target);
    assert_eq!(snap.agents.iter().filter(|a| a.name == "scout").count(), 1);
    assert_eq!(
        snap.agents
            .iter()
            .find(|a| a.name == "scout")
            .unwrap()
            .source,
        "custom"
    );
    assert!(snap.agents.iter().any(|a| a.name == "plugin-agent"));
    assert!(snap.agents.iter().any(|a| a.name == "removed-agent"));
}

#[test]
fn model_effort_projection_uses_native_efforts_and_legacy_ranges() {
    assert_eq!(
        thinking_levels(&json!({"efforts":["low", "high"]})),
        vec!["low", "high"]
    );
    assert_eq!(
        thinking_levels(&json!({"minLevel":"medium", "maxLevel":"xhigh"})),
        vec!["medium", "high", "xhigh"]
    );
}

#[test]
#[ignore = "Requires explicit AIO_OMP_TEST_EXE; uses isolated temporary HOME/config only"]
fn installed_omp_reads_configuration_written_by_aio() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let executable = std::env::var_os("AIO_OMP_TEST_EXE").expect("set an explicit OMP executable");
    let (temp, target) = setup();
    let request = input(
        &target,
        vec![
            patch(
                &["modelRoles", "default"],
                Some(json!("openai/gpt-5.4:high")),
            ),
            patch(
                &["modelRoles", "review"],
                Some(json!("openai/gpt-5.4:medium")),
            ),
            patch(&["defaultThinkingLevel"], Some(json!("auto"))),
            patch(&["task", "maxConcurrency"], Some(json!(4))),
            patch(
                &["task", "agentModelOverrides", "reviewer"],
                Some(json!("@review")),
            ),
            patch(&["task", "agentAdvisor", "scout"], Some(json!("off"))),
            patch(&["task", "disabledAgents"], Some(json!(["sonic"]))),
        ],
    );
    save(&target, &request).unwrap();
    for (key, expected) in [
        (
            "modelRoles",
            json!({"default":"openai/gpt-5.4:high", "review":"openai/gpt-5.4:medium"}),
        ),
        ("defaultThinkingLevel", json!("auto")),
        ("task.maxConcurrency", json!(4)),
        ("task.agentModelOverrides", json!({"reviewer":"@review"})),
        ("task.agentAdvisor", json!({"scout":"off"})),
        ("task.disabledAgents", json!(["sonic"])),
    ] {
        let mut command = Command::new(&executable);
        command
            .args(["config", "get", key, "--json"])
            .env_clear()
            .current_dir(temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for variable in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(variable) {
                command.env(variable, value);
            }
        }
        for variable in [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "TEMP",
            "TMP",
        ] {
            command.env(variable, temp.path());
        }
        command.env("PI_CODING_AGENT_DIR", &target.agent_dir);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(15) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("OMP config probe timed out");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "isolated OMP probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual["value"], expected, "native key {key}");
    }
}

#[cfg(unix)]
#[test]
fn symlinked_config_and_agent_directories_are_rejected() {
    use std::os::unix::fs::symlink;
    let (temp, target) = setup();
    std::fs::create_dir_all(&target.agent_dir).unwrap();
    let external = temp.path().join("external");
    std::fs::write(&external, "task: {}\n").unwrap();
    symlink(&external, Path::new(&target.agent_dir).join("config.yml")).unwrap();
    assert!(load(&target).is_err());
    symlink(temp.path(), Path::new(&target.agent_dir).join("agents")).unwrap();
    assert!(read_agent(&target, "a.md").is_err());
}
