#![cfg(unix)]

use diffkin_domain::{ValidationSpec, ValidationStatus, ValidationTarget};
use diffkin_validation::{load_plan, prepare, recover_interrupted, run};
use std::{fs, path::PathBuf, process::Command, sync::atomic::AtomicBool};
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    store: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("diffkin-validation-review-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let store = root.join("store");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&store).unwrap();
        let f = Self { root, repo, store };
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Review fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
        ] {
            f.git(&args);
        }
        fs::write(f.repo.join("source.txt"), "original\n").unwrap();
        f.git(&["add", "."]);
        f.git(&["commit", "-qm", "Controlled review fixture"]);
        f
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(&self.repo)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn spec(&self) -> ValidationSpec {
        ValidationSpec {
            repository: self.repo.to_string_lossy().into(),
            candidate_ref: "HEAD".into(),
            baseline_ref: None,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "test \"$(cat source.txt)\" = original".into()],
            working_directory: ".".into(),
            timeout_seconds: 2,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn symlinked_plan_storage_must_not_write_into_the_repository() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(&f.repo, f.store.join("plans")).unwrap();
    let before = f.git(&["status", "--porcelain"]);
    let result = prepare(&f.spec(), &f.store, &AtomicBool::new(false));
    assert_eq!(
        f.git(&["status", "--porcelain"]),
        before,
        "Preparing a plan wrote into the source repository through store/plans"
    );
    assert!(result.is_err(), "A plan directory symlink must be rejected");
}

#[test]
fn modified_fixed_commit_metadata_must_be_rejected() {
    let f = Fixture::new();
    let plan = prepare(&f.spec(), &f.store, &AtomicBool::new(false)).unwrap();
    let path = f.store.join("plans").join(&plan.id).join("plan.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["candidate"]["commit"] = "0000000000000000000000000000000000000000".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        load_plan(&f.store, &plan.id).is_err(),
        "Stored commit identity is unbound from the accepted source plan"
    );
}

#[test]
fn modified_dependency_claim_cannot_be_carried_into_a_passed_result() {
    let f = Fixture::new();
    let plan = prepare(&f.spec(), &f.store, &AtomicBool::new(false)).unwrap();
    let path = f.store.join("plans").join(&plan.id).join("plan.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["dependencyStatus"] = "confirmed".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    if let Ok(result) = run(
        &f.store,
        &plan.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |_| {},
    ) {
        assert!(
            result.status != ValidationStatus::Passed || result.dependency_status == "unconfirmed",
            "An unverified persisted claim was returned as dependency status: {result:?}"
        );
    }
}

#[test]
fn symlinked_run_storage_must_not_write_into_the_repository() {
    let f = Fixture::new();
    let plan = prepare(&f.spec(), &f.store, &AtomicBool::new(false)).unwrap();
    fs::remove_dir(f.store.join("runs")).unwrap();
    std::os::unix::fs::symlink(&f.repo, f.store.join("runs")).unwrap();
    let before = f.git(&["status", "--porcelain"]);
    let _ = run(
        &f.store,
        &plan.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |_| {},
    );
    assert_eq!(
        f.git(&["status", "--porcelain"]),
        before,
        "Running a plan wrote into the source repository through store/runs"
    );
}

#[test]
fn startup_recovers_lock_left_before_a_result_was_written() {
    let f = Fixture::new();
    let plan = prepare(&f.spec(), &f.store, &AtomicBool::new(false)).unwrap();
    // Model the crash window after create_new(running.lock), before result.json exists.
    fs::write(
        f.store.join("plans").join(&plan.id).join("running.lock"),
        "",
    )
    .unwrap();
    recover_interrupted(&f.store).unwrap();
    let result = run(
        &f.store,
        &plan.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |_| {},
    );
    assert!(
        result.is_ok(),
        "Startup left an orphan lock that permanently blocks this plan: {result:?}"
    );
}

#[test]
fn case_colliding_git_paths_are_not_a_complete_snapshot_on_insensitive_filesystems() {
    let f = Fixture::new();
    if !f.repo.join("SOURCE.txt").exists() {
        return; // A case-sensitive filesystem represents these two paths correctly.
    }
    let blob = f.git(&["rev-parse", "HEAD:source.txt"]);
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        "100644",
        blob.trim(),
        "SOURCE.txt",
    ]);
    f.git(&["commit", "-qm", "Two distinct Git paths sharing content"]);
    let result = prepare(&f.spec(), &f.store, &AtomicBool::new(false));
    if let Ok(plan) = result {
        assert!(!plan.executable || !plan.candidate.complete, "Two Git paths collapse to one physical file but the snapshot claims complete: {plan:?}");
    }
}

#[test]
fn case_colliding_git_directories_must_not_be_silently_merged() {
    let f = Fixture::new();
    if !f.repo.join("SOURCE.txt").exists() {
        return;
    }
    let blob = f.git(&["rev-parse", "HEAD:source.txt"]);
    for path in ["modules/first.txt", "MODULES/second.txt"] {
        f.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            "100644",
            blob.trim(),
            path,
        ]);
    }
    f.git(&[
        "commit",
        "-qm",
        "Distinct Git directories must remain distinct",
    ]);
    if let Ok(plan) = prepare(&f.spec(), &f.store, &AtomicBool::new(false)) {
        assert!(
            !plan.executable || !plan.candidate.complete,
            "Two distinct Git directories were silently merged into an executable snapshot"
        );
    }
}
