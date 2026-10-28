#![cfg(unix)]
use diffkin_domain::*;
use diffkin_validation::*;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    store: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("diffkin-validation-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let store = root.join("store");
        fs::create_dir_all(&repo).unwrap();
        let f = Self { root, repo, store };
        f.git(&["init", "-q"]);
        f.git(&["config", "user.name", "Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        fs::write(f.repo.join("source.txt"), "original\n").unwrap();
        fs::write(f.repo.join("Cargo.lock"), "# controlled fixture\n").unwrap();
        f.commit();
        f
    }
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(&self.repo)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().into()
    }
    fn commit(&self) {
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "fixture"]);
    }
    fn spec(&self, script: &str) -> ValidationSpec {
        ValidationSpec {
            repository: self.repo.to_string_lossy().into(),
            candidate_ref: "HEAD".into(),
            baseline_ref: None,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            working_directory: ".".into(),
            timeout_seconds: 5,
        }
    }
    fn plan(&self, script: &str) -> ValidationPlan {
        prepare(&self.spec(script), &self.store, &AtomicBool::new(false)).unwrap()
    }
    fn run(&self, plan: &ValidationPlan) -> ValidationResult {
        run(
            &self.store,
            &plan.id,
            ValidationTarget::Candidate,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn complete_capture_bound_result_and_unmodified_workspace() {
    let f = Fixture::new();
    let before = f.git(&["status", "--porcelain"]);
    let plan=f.plan("test \"$(cat source.txt)\" = original && test -f Cargo.lock && printf stdout && printf stderr >&2");
    assert_eq!(plan.candidate.file_count, 2);
    assert!(plan.candidate.complete);
    assert_eq!(plan.candidate.locks.len(), 1);
    fs::write(f.repo.join("source.txt"), "working copy differs").unwrap();
    let index = f.git(&["ls-files", "--stage"]);
    let r = f.run(&plan);
    assert_eq!(r.status, ValidationStatus::Passed);
    assert_eq!(r.source_id, plan.candidate.content_id);
    assert_eq!(r.command_id, plan.command_id);
    assert_eq!(r.dependency_status, "unconfirmed");
    assert_eq!(r.log_tail.len(), 2);
    assert_eq!(r.log_tail[0].sequence, 1);
    assert_eq!(f.git(&["ls-files", "--stage"]), index);
    assert_eq!(
        fs::read_to_string(f.repo.join("source.txt")).unwrap(),
        "working copy differs"
    );
    assert!(before.is_empty());
    assert_eq!(load_result(&f.store, &r.id).unwrap().status, r.status);
}
#[test]
fn baseline_and_candidate_share_command_but_not_source() {
    let f = Fixture::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    fs::write(f.repo.join("source.txt"), "candidate\n").unwrap();
    f.commit();
    let mut spec = f.spec("test \"$(cat source.txt)\" = candidate");
    spec.baseline_ref = Some(base);
    let p = prepare(&spec, &f.store, &AtomicBool::new(false)).unwrap();
    let candidate = f.run(&p);
    let base = run(
        &f.store,
        &p.id,
        ValidationTarget::Baseline,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(candidate.status, ValidationStatus::Passed);
    assert_eq!(base.status, ValidationStatus::Failed);
    assert_eq!(candidate.command_id, base.command_id);
    assert_ne!(candidate.source_id, base.source_id);
}
#[test]
fn changed_source_is_indeterminate_and_repeated_run_is_fresh() {
    let f = Fixture::new();
    let p = f.plan("test \"$(cat source.txt)\" = original || exit 2; echo changed >source.txt");
    for _ in 0..2 {
        let r = f.run(&p);
        assert_eq!(r.status, ValidationStatus::Indeterminate);
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.changed_paths, vec!["source.txt"]);
    }
    assert_eq!(
        fs::read_to_string(f.repo.join("source.txt")).unwrap(),
        "original\n"
    );
}
#[test]
fn symlink_lfs_filters_are_explicitly_not_run() {
    for kind in ["symlink", "lfs", "filter"] {
        let f = Fixture::new();
        match kind {
            "symlink" => std::os::unix::fs::symlink("source.txt", f.repo.join("link")).unwrap(),
            "lfs" => fs::write(
                f.repo.join("large"),
                "version https://git-lfs.github.com/spec/v1\noid sha256:abcd\nsize 4\n",
            )
            .unwrap(),
            _ => fs::write(f.repo.join(".gitattributes"), "*.txt filter=custom\n").unwrap(),
        };
        f.commit();
        let p = f.plan("exit 0");
        assert!(!p.executable);
        assert!(!p.candidate.complete);
        assert_eq!(f.run(&p).status, ValidationStatus::NotRun);
    }
}
#[test]
fn timeout_is_indeterminate_and_process_group_stops() {
    let f = Fixture::new();
    let mut spec = f.spec("sleep 20 & echo $!; wait");
    spec.timeout_seconds = 1;
    let p = prepare(&spec, &f.store, &AtomicBool::new(false)).unwrap();
    let start = Instant::now();
    let r = f.run(&p);
    assert_eq!(r.status, ValidationStatus::Indeterminate);
    assert!(r.message.contains("Timeout"));
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_child_stopped(&r);
}
fn assert_child_stopped(r: &ValidationResult) {
    let pid: i32 = r
        .log_tail
        .iter()
        .find(|l| l.stream == "stdout")
        .unwrap()
        .text
        .trim()
        .parse()
        .unwrap();
    for _ in 0..30 {
        let out = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&out.stdout);
        if state.trim().is_empty() || state.trim().starts_with('Z') {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("child {pid} remains live");
}
#[test]
fn active_cancel_releases_process_tree() {
    let f = Fixture::new();
    let p = f.plan("sleep 20 & echo $!; wait");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let start = Instant::now();
    let r = run(
        &f.store,
        &p.id,
        ValidationTarget::Candidate,
        &cancel,
        move |entry| {
            if entry.stream == "stdout" {
                flag.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert_eq!(r.status, ValidationStatus::Cancelled);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_child_stopped(&r);
}
#[test]
fn logs_are_bounded_and_callback_sequences_ordered() {
    let f = Fixture::new();
    let p = f.plan("yes output | head -c 2000000");
    let mut last = 0;
    let mut count = 0;
    let r = run(
        &f.store,
        &p.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |l| {
            assert_eq!(l.sequence, last + 1);
            last = l.sequence;
            count += l.text.len();
        },
    )
    .unwrap();
    assert_eq!(r.status, ValidationStatus::Passed);
    assert!(r.log_truncated);
    assert!(count <= 1024 * 1024);
    assert!(r.log_tail.iter().map(|l| l.text.len()).sum::<usize>() <= 64 * 1024);
    assert!(
        fs::metadata(f.store.join("runs").join(&r.id).join("logs.jsonl"))
            .unwrap()
            .len()
            < 2 * 1024 * 1024
    );
}
#[test]
fn frozen_source_tampering_prevents_execution() {
    let f = Fixture::new();
    let p = f.plan("exit 0");
    fs::write(
        PathBuf::from(&p.candidate.root).join("source.txt"),
        "tampered",
    )
    .unwrap();
    assert_eq!(f.run(&p).status, ValidationStatus::NotRun);
}
#[test]
fn failed_spawn_is_environment_error() {
    let f = Fixture::new();
    let tool = f.root.join("tool");
    fs::write(&tool, "not executable").unwrap();
    let mut spec = f.spec("");
    spec.program = tool.to_string_lossy().into();
    let p = prepare(&spec, &f.store, &AtomicBool::new(false)).unwrap();
    assert_eq!(f.run(&p).status, ValidationStatus::EnvError);
}
#[test]
fn restart_does_not_infer_success() {
    let f = Fixture::new();
    let p = f.plan("exit 0");
    let mut r = f.run(&p);
    r.status = ValidationStatus::Running;
    fs::write(
        f.store.join("runs").join(&r.id).join("result.json"),
        serde_json::to_vec(&r).unwrap(),
    )
    .unwrap();
    assert_eq!(recover_interrupted(&f.store).unwrap(), 1);
    assert_eq!(
        load_result(&f.store, &r.id).unwrap().status,
        ValidationStatus::Interrupted
    );
    assert_eq!(history(&f.store, 10).unwrap().len(), 1);
}
#[test]
fn invalid_paths_and_cancelled_preparation_fail() {
    let f = Fixture::new();
    let mut s = f.spec("exit 0");
    s.working_directory = "../repo".into();
    assert!(prepare(&s, &f.store, &AtomicBool::new(false)).is_err());
    assert!(matches!(
        prepare(&f.spec("exit 0"), &f.store, &AtomicBool::new(true)),
        Err(ValidationError::Cancelled { .. })
    ));
    assert!(load_plan(&f.store, "../../escape").is_err());
}

#[test]
fn changed_program_and_command_configuration_do_not_execute() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let tool = f.root.join("tool");
    fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let mut spec = f.spec("");
    spec.program = tool.to_string_lossy().into();
    spec.args.clear();
    let p = prepare(&spec, &f.store, &AtomicBool::new(false)).unwrap();
    fs::write(&tool, "#!/bin/sh\nexit 12\n").unwrap();
    assert_eq!(f.run(&p).status, ValidationStatus::NotRun);
    let mut p = f.plan("exit 0");
    p.args = vec!["-c".into(), "exit 2".into()];
    fs::write(
        f.store.join("plans").join(&p.id).join("plan.json"),
        serde_json::to_vec(&p).unwrap(),
    )
    .unwrap();
    assert!(run(
        &f.store,
        &p.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |_| {}
    )
    .is_err());
}
#[test]
fn submodule_and_unsafe_manifest_are_not_complete_candidates() {
    let f = Fixture::new();
    let commit = f.git(&["rev-parse", "HEAD"]);
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{commit},module"),
    ]);
    f.git(&["commit", "-qm", "gitlink"]);
    let p = f.plan("exit 0");
    assert!(!p.executable);
    assert!(p.blockers.iter().any(|b| b.contains("submodules")));
    assert_eq!(f.run(&p).status, ValidationStatus::NotRun);
}
#[test]
fn captures_unicode_crlf_and_executable_modes_without_checkout_filters() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    fs::write(f.repo.join("世界.txt"), b"a\r\nb\r\n").unwrap();
    fs::write(f.repo.join("script"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(f.repo.join("script"), fs::Permissions::from_mode(0o755)).unwrap();
    f.commit();
    let p = f.plan("test -x script && ./script");
    assert_eq!(
        fs::read(PathBuf::from(&p.candidate.root).join("世界.txt")).unwrap(),
        b"a\r\nb\r\n"
    );
    assert_eq!(f.run(&p).status, ValidationStatus::Passed);
}

#[test]
fn source_manifest_destroyed_by_command_is_indeterminate() {
    let f = Fixture::new();
    let p = f.plan("rm ../source.manifest.json");
    let r = f.run(&p);
    assert_eq!(r.status, ValidationStatus::Indeterminate);
    assert!(r.source_changed);
}
#[test]
fn store_inside_repository_is_rejected_without_creating_it() {
    let f = Fixture::new();
    let store = f.repo.join("nested").join("validation");
    assert!(prepare(&f.spec("exit 0"), &store, &AtomicBool::new(false)).is_err());
    assert!(!f.repo.join("nested").exists());
}
#[test]
fn symlinked_run_storage_does_not_mutate_repository() {
    let f = Fixture::new();
    let p = f.plan("exit 0");
    fs::remove_dir(f.store.join("runs")).unwrap();
    std::os::unix::fs::symlink(&f.repo, f.store.join("runs")).unwrap();
    assert!(run(
        &f.store,
        &p.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |_| {}
    )
    .is_err());
    assert!(f.git(&["status", "--porcelain"]).is_empty());
}

#[test]
fn active_plan_lock_blocks_duplicate_runs_and_recovery() {
    let f = Fixture::new();
    let p = f.plan("echo ready; sleep 20");
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let (send, recv) = std::sync::mpsc::channel();
    let store = f.store.clone();
    let id = p.id.clone();
    let thread = std::thread::spawn(move || {
        run(&store, &id, ValidationTarget::Candidate, &flag, |entry| {
            if entry.text.contains("ready") {
                send.send(()).unwrap();
            }
        })
        .unwrap()
    });
    recv.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(run(
        &f.store,
        &p.id,
        ValidationTarget::Candidate,
        &AtomicBool::new(false),
        |_| {}
    )
    .is_err());
    assert_eq!(recover_interrupted(&f.store).unwrap(), 0);
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(thread.join().unwrap().status, ValidationStatus::Cancelled);
}
