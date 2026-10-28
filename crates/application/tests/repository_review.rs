//! Independent regression cases for capture optimizations.
use diffkin_application::capture;
use diffkin_domain::{CompareMode, ComparisonSpec};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "diffkin-repo-review-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let f = Self(root);
        f.git(&["init", "-q", "-b", "main"], None);
        f.git(&["config", "user.name", "Fixture"], None);
        f.git(&["config", "user.email", "fixture@example.invalid"], None);
        f
    }
    fn git(&self, args: &[&str], input: Option<&[u8]>) -> String {
        let mut child = Command::new("git")
            .current_dir(&self.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .args(["-c", "core.hooksPath=", "-c", "commit.gpgsign=false"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = input {
            child.stdin.take().unwrap().write_all(input).unwrap();
        } else {
            drop(child.stdin.take());
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    fn snapshot(&self, mode: CompareMode) -> diffkin_application::FrozenComparison {
        self.snapshot_with_verification(mode, false)
    }
    fn snapshot_with_verification(
        &self,
        mode: CompareMode,
        verify_all: bool,
    ) -> diffkin_application::FrozenComparison {
        capture(
            &ComparisonSpec {
                repository: self.0.to_string_lossy().into(),
                mode,
                base: None,
                head: None,
                include_untracked: true,
                verify_all,
            },
            &AtomicBool::new(false),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn intermediate_staged_changes_do_not_limit_empty_unstaged_or_worktree_comparisons() {
    let f = Fixture::new();
    let old = f.git(&["hash-object", "-w", "--stdin"], Some(b"original\n"));
    let new = f.git(&["hash-object", "-w", "--stdin"], Some(b"staged\n"));
    let entries = |oid: &str| {
        (0..2001)
            .map(|n| format!("100644 {oid}\tfile-{n:04}.txt\0"))
            .collect::<String>()
    };
    f.git(
        &["update-index", "-z", "--index-info"],
        Some(entries(&old).as_bytes()),
    );
    let tree = f.git(&["write-tree"], None);
    let commit = f.git(&["commit-tree", &tree, "-m", "baseline"], None);
    f.git(&["update-ref", "HEAD", &commit], None);
    f.git(
        &["update-index", "-z", "--index-info"],
        Some(entries(&new).as_bytes()),
    );
    f.git(&["checkout-index", "--all"], None);
    assert_eq!(
        f.snapshot(CompareMode::Unstaged).manifest(0, 1).total_files,
        0
    );
    // The worktree may also undo a large staged change while retaining that index.
    for n in 0..2001 {
        fs::write(f.0.join(format!("file-{n:04}.txt")), b"original\n").unwrap();
    }
    assert_eq!(
        f.snapshot(CompareMode::Worktree).manifest(0, 1).total_files,
        0
    );
}

#[cfg(unix)]
#[test]
fn default_global_attributes_do_not_hide_literal_crlf_bytes() {
    const MARKER: &str = "DIFFKIN_REVIEW_DEFAULT_ATTRIBUTES";
    if std::env::var_os(MARKER).is_none() {
        let f = Fixture::new();
        let home = f.0.join("home");
        fs::create_dir_all(home.join("git")).unwrap();
        fs::write(home.join("git/attributes"), b"*.txt text eol=crlf\n").unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "default_global_attributes_do_not_hide_literal_crlf_bytes",
                "--nocapture",
            ])
            .env(MARKER, "1")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let f = Fixture::new();
    fs::write(f.0.join("value.txt"), b"original\n").unwrap();
    f.git(&["add", "value.txt"], None);
    f.git(&["commit", "-qm", "baseline"], None);
    fs::remove_file(f.0.join("value.txt")).unwrap();
    f.git(&["checkout-index", "--all"], None);
    assert_eq!(fs::read(f.0.join("value.txt")).unwrap(), b"original\r\n");
    // Refresh the stat entry without changing the normalized object identity.
    f.git(&["add", "value.txt"], None);
    std::thread::sleep(std::time::Duration::from_millis(30));
    f.git(&["update-index", "--refresh", "--force-write-index"], None);
    assert!(f.git(&["diff", "--name-only"], None).is_empty());
    let snapshot = f.snapshot(CompareMode::Worktree);
    let manifest = snapshot.manifest(0, 1);
    assert_eq!(
        manifest.total_files, 1,
        "stat-clean global conversion must retain its literal byte difference"
    );
    let file = snapshot.read_file(&manifest.files[0].id).unwrap();
    assert_eq!(file.before.as_deref(), Some("original\n"));
    assert_eq!(file.after.as_deref(), Some("original\r\n"));
}

#[cfg(unix)]
#[test]
fn missing_worktree_attributes_still_apply_from_the_index() {
    const MARKER: &str = "DIFFKIN_REVIEW_MISSING_ATTRIBUTES";
    if std::env::var_os(MARKER).is_none() {
        let f = Fixture::new();
        let home = f.0.join("home");
        fs::create_dir(&home).unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "missing_worktree_attributes_still_apply_from_the_index",
                "--nocapture",
            ])
            .env(MARKER, "1")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let f = Fixture::new();
    fs::write(f.0.join(".gitattributes"), b"*.txt text eol=crlf\n").unwrap();
    fs::write(f.0.join("value.txt"), b"original\n").unwrap();
    f.git(&["add", ".gitattributes", "value.txt"], None);
    f.git(&["commit", "-qm", "baseline"], None);
    fs::remove_file(f.0.join("value.txt")).unwrap();
    f.git(&["checkout-index", "--all"], None);
    f.git(&["add", "value.txt"], None);
    std::thread::sleep(std::time::Duration::from_millis(30));
    f.git(&["update-index", "--refresh", "--force-write-index"], None);
    fs::remove_file(f.0.join(".gitattributes")).unwrap();
    let snapshot = f.snapshot(CompareMode::Worktree);
    let manifest = snapshot.manifest(0, 200);
    assert_eq!(
        manifest.total_files, 2,
        "deleted attribute file and literal CRLF bytes must both remain visible"
    );
    let summary = manifest
        .files
        .iter()
        .find(|file| file.path == "value.txt")
        .unwrap();
    let file = snapshot.read_file(&summary.id).unwrap();
    assert_eq!(file.before.as_deref(), Some("original\n"));
    assert_eq!(file.after.as_deref(), Some("original\r\n"));
    // Explicitly disabling a previously committed conversion also leaves the
    // existing raw bytes unchanged; current attributes alone are insufficient.
    fs::write(f.0.join(".gitattributes"), b"*.txt -text\n").unwrap();
    let snapshot = f.snapshot(CompareMode::Worktree);
    let manifest = snapshot.manifest(0, 200);
    assert_eq!(
        manifest.total_files, 2,
        "disabling a conversion must retain its existing literal byte difference"
    );
    assert!(manifest.files.iter().any(|file| file.path == "value.txt"));
}

#[cfg(unix)]
#[test]
fn full_verification_detects_bytes_after_conversion_history_was_committed_away() {
    const MARKER: &str = "DIFFKIN_REVIEW_HISTORICAL_ATTRIBUTES";
    if std::env::var_os(MARKER).is_none() {
        let f = Fixture::new();
        let home = f.0.join("home");
        fs::create_dir(&home).unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "full_verification_detects_bytes_after_conversion_history_was_committed_away",
                "--nocapture",
            ])
            .env(MARKER, "1")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let f = Fixture::new();
    fs::write(f.0.join(".gitattributes"), b"*.txt text eol=crlf\n").unwrap();
    fs::write(f.0.join("value.txt"), b"original\n").unwrap();
    f.git(&["add", ".gitattributes", "value.txt"], None);
    f.git(&["commit", "-qm", "baseline"], None);
    fs::remove_file(f.0.join("value.txt")).unwrap();
    f.git(&["checkout-index", "--all"], None);
    // Give Git an unequivocally non-racy stat cache; a same-second mtime
    // may otherwise cause `status` to rehash after the rules are removed.
    fs::OpenOptions::new()
        .write(true)
        .open(f.0.join("value.txt"))
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2)),
        )
        .unwrap();
    f.git(&["add", "value.txt"], None);
    f.git(&["update-index", "--refresh", "--force-write-index"], None);
    fs::remove_file(f.0.join(".gitattributes")).unwrap();
    f.git(&["add", ".gitattributes"], None);
    f.git(&["commit", "-qm", "remove conversion rules"], None);
    assert!(f.git(&["status", "--porcelain=v1"], None).is_empty());
    assert_eq!(fs::read(f.0.join("value.txt")).unwrap(), b"original\r\n");
    assert_ne!(
        f.git(&["rev-parse", "HEAD:value.txt"], None),
        f.git(&["hash-object", "--no-filters", "value.txt"], None)
    );
    // The default is explicitly a stat-based cache check, which cannot infer
    // conversion rules that have already disappeared from all current inputs.
    let fast = f.snapshot(CompareMode::Worktree).manifest(0, 1);
    assert_eq!(fast.total_files, 0);
    assert!(fast.warnings.iter().any(|warning| warning.contains("stat")));
    let snapshot = f.snapshot_with_verification(CompareMode::Worktree, true);
    let manifest = snapshot.manifest(0, 1);
    assert_eq!(manifest.total_files, 1);
    assert_eq!(manifest.files[0].path, "value.txt");
    let file = snapshot.read_file(&manifest.files[0].id).unwrap();
    assert_eq!(file.before.as_deref(), Some("original\n"));
    assert_eq!(file.after.as_deref(), Some("original\r\n"));
}
