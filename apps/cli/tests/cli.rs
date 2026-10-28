use std::{path::Path, process::Command};

#[test]
fn cli_exit_codes_and_json_match_comparison() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/correctness");
    let before = fixtures.join("condition.before.ts");
    let after = fixtures.join("condition.after.ts");
    let output = Command::new(env!("CARGO_BIN_EXE_diffkin"))
        .args([&before, &after])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["added"], 1);
    let identical = Command::new(env!("CARGO_BIN_EXE_diffkin"))
        .args([&before, &before])
        .output()
        .unwrap();
    assert_eq!(identical.status.code(), Some(0));
    let missing = Command::new(env!("CARGO_BIN_EXE_diffkin"))
        .arg("--unknown")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
}

#[test]
fn structured_mode_and_repository_cli_share_the_application_results() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/correctness");
    let structured = Command::new(env!("CARGO_BIN_EXE_diffkin"))
        .arg("--structure")
        .args([
            fixtures.join("condition.before.ts"),
            fixtures.join("condition.after.ts"),
        ])
        .output()
        .unwrap();
    assert_eq!(structured.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&structured.stdout).unwrap();
    assert_eq!(json["raw"]["added"], 1);
    assert!(json["events"].is_array());
    let root = std::env::temp_dir().join(format!("diffkin-cli-repository-{}", std::process::id()));
    fs_create(&root);
    let initialized = Command::new("git")
        .args(["init", "-q"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(initialized.status.success());
    std::fs::write(root.join("new.ts"), "const x = 1;\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_diffkin"))
        .arg("--repo")
        .arg(&root)
        .args(["--mode", "worktree", "--untracked"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["manifest"]["totalFiles"], 1);
    assert_eq!(json["files"][0]["file"]["after"], "const x = 1;\n");
    assert_eq!(json["files"][0]["analysis"]["raw"]["added"], 1);
    let invalid = Command::new(env!("CARGO_BIN_EXE_diffkin"))
        .arg("--repo")
        .arg(&root)
        .args(["--mode", "staged", "--untracked"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(root);
}
fn fs_create(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
    std::fs::create_dir_all(path).unwrap();
}
