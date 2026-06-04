mod support;

use support::SkillTestFixture;

#[test]
fn skill_local_delete_removes_unmanaged_local_skill_dir() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");
    let fix = SkillTestFixture::new(&app, &handle, "codex", "Codex Local Delete");

    let local_dir = fix.cli_skills_root.join("local-delete-skill");
    std::fs::create_dir_all(&local_dir).expect("create local skill dir");
    std::fs::write(local_dir.join("SKILL.md"), "name: Local Delete Skill\n")
        .expect("write local skill md");

    let ok = aio_coding_hub_lib::test_support::skill_local_delete(
        &handle,
        fix.workspace_id,
        "local-delete-skill",
    )
    .expect("delete local skill");
    assert!(ok, "local delete should succeed");
    assert!(!local_dir.exists(), "local skill dir should be removed");
}

#[test]
fn skill_local_delete_blocks_managed_dir() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");
    let fix = SkillTestFixture::new(&app, &handle, "codex", "Codex Local Delete Managed");

    let local_dir = fix.cli_skills_root.join(&fix.skill_key);
    std::fs::create_dir_all(&local_dir).expect("create local skill dir");
    std::fs::write(local_dir.join("SKILL.md"), "name: Managed Local Skill\n")
        .expect("write local skill md");
    std::fs::write(
        local_dir.join(".aio-coding-hub.managed"),
        "aio-coding-hub\n",
    )
    .expect("write managed marker");

    let err = aio_coding_hub_lib::test_support::skill_local_delete(
        &handle,
        fix.workspace_id,
        &fix.skill_key,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.starts_with("SKILL_LOCAL_DELETE_BLOCKED_MANAGED:"),
        "unexpected error: {err}"
    );
    assert!(local_dir.exists(), "managed local skill dir should remain");
}

#[test]
fn skill_local_delete_removes_copied_foreign_managed_marker_dir() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");
    let fix = SkillTestFixture::new(&app, &handle, "claude", "Claude Local Delete Copied");

    let dir_name = "copied-managed-skill";
    let local_dir = fix.cli_skills_root.join(dir_name);
    std::fs::create_dir_all(&local_dir).expect("create copied skill dir");
    std::fs::write(local_dir.join("SKILL.md"), "name: Copied Managed Skill\n")
        .expect("write copied skill md");
    std::fs::write(
        local_dir.join(".aio-coding-hub.managed"),
        "aio-coding-hub\n",
    )
    .expect("write managed marker");

    let ok =
        aio_coding_hub_lib::test_support::skill_local_delete(&handle, fix.workspace_id, dir_name)
            .expect("delete copied foreign managed marker skill");

    assert!(ok, "local delete should succeed");
    assert!(
        !local_dir.exists(),
        "copied foreign managed marker skill dir should be removed"
    );
}
