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
