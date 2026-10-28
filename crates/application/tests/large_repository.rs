//! Real Git regression fixtures: the tracked repository may be huge while its diff is small.
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
struct Repository(PathBuf);
impl Repository {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "diffkin-large-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let repo = Self(root);
        repo.git(&["init", "-q", "--initial-branch=main"], None);
        repo.git(&["config", "user.name", "Diffkin fixture"], None);
        repo.git(&["config", "user.email", "fixture@example.invalid"], None);
        repo.git(&["config", "index.version", "4"], None);
        repo
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
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn one_staged_change(&self, count: usize, long_paths: bool) {
        let old = self.git(
            &["hash-object", "-w", "--stdin"],
            Some(b"const value = 1;\n"),
        );
        let new = self.git(
            &["hash-object", "-w", "--stdin"],
            Some(b"const value = 2;\n"),
        );
        let prefix = if long_paths {
            format!(
                "{}/",
                (0..14)
                    .map(|n| format!("{n:02}{}", "x".repeat(198)))
                    .collect::<Vec<_>>()
                    .join("/")
            )
        } else {
            String::new()
        };
        let mut input = format!("100644 {old}\tchanged.ts\0").into_bytes();
        for n in 1..count {
            input.extend_from_slice(format!("100644 {old}\t{prefix}file-{n:06}.ts\0").as_bytes());
        }
        self.git(&["update-index", "-z", "--index-info"], Some(&input));
        let tree = self.git(&["write-tree"], None);
        let commit = self.git(&["commit-tree", &tree, "-m", "baseline"], None);
        self.git(&["update-ref", "HEAD", &commit], None);
        self.git(
            &[
                "update-index",
                "--cacheinfo",
                &format!("100644,{new},changed.ts"),
            ],
            None,
        );
    }
    fn assert_one_change(&self) {
        let snapshot = capture(
            &ComparisonSpec {
                repository: self.0.to_string_lossy().into(),
                mode: CompareMode::Staged,
                base: None,
                head: None,
                include_untracked: false,
                verify_all: false,
            },
            &AtomicBool::new(false),
        )
        .unwrap_or_else(|error| {
            panic!(
                "Small staged diff in huge repository must be capturable; exact failure: {error}"
            )
        });
        let manifest = snapshot.manifest(0, 200);
        assert_eq!(manifest.total_files, 1);
        assert_eq!(manifest.files[0].path, "changed.ts");
        let file = snapshot.read_file(&manifest.files[0].id).unwrap();
        assert_eq!(file.before.as_deref(), Some("const value = 1;\n"));
        assert_eq!(file.after.as_deref(), Some("const value = 2;\n"));
    }
}
impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn more_than_fifty_thousand_index_entries_with_one_change() {
    let repo = Repository::new();
    repo.one_staged_change(50_001, false);
    repo.assert_one_change();
}
#[test]
fn huge_git_listing_with_one_change() {
    let repo = Repository::new();
    repo.one_staged_change(30_000, true);
    repo.assert_one_change();
}
#[test]
fn default_worktree_with_fifty_thousand_real_files_and_one_change() {
    let repo = Repository::new();
    repo.one_staged_change(50_001, false);
    repo.git(&["checkout-index", "--all"], None);
    repo.git(&["update-index", "--refresh"], None);
    let started = std::time::Instant::now();
    let snapshot = capture(
        &ComparisonSpec {
            repository: repo.0.to_string_lossy().into(),
            mode: CompareMode::Worktree,
            base: None,
            head: None,
            include_untracked: true,
            verify_all: false,
        },
        &AtomicBool::new(false),
    )
    .unwrap_or_else(|error| {
        panic!("Default worktree capture with 50,001 real files and one change failed: {error}")
    });
    eprintln!(
        "50,001 tracked real files, one changed, capture: {:?}",
        started.elapsed()
    );
    assert_eq!(snapshot.manifest(0, 200).total_files, 1);
}
#[test]
fn boundary_controls_small_listing_with_fifty_thousand_entries() {
    let repo = Repository::new();
    repo.one_staged_change(50_000, false);
    repo.assert_one_change();
}

fn snapshot(repo: &Repository, mode: CompareMode) -> diffkin_application::FrozenComparison {
    capture(
        &ComparisonSpec {
            repository: repo.0.to_string_lossy().into(),
            mode,
            base: None,
            head: None,
            include_untracked: true,
            verify_all: false,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn commit_files(repo: &Repository) {
    repo.git(&["add", "."], None);
    repo.git(&["commit", "-qm", "baseline"], None);
}
#[test]
fn flagged_worktree_bytes_remain_visible_and_sparse_absence_is_context() {
    let repo = Repository::new();
    for name in ["assumed.ts", "sparse.ts", "absent.ts"] {
        fs::write(repo.0.join(name), "const value = 1;\n").unwrap();
    }
    commit_files(&repo);
    repo.git(&["update-index", "--assume-unchanged", "assumed.ts"], None);
    repo.git(
        &["update-index", "--skip-worktree", "sparse.ts", "absent.ts"],
        None,
    );
    fs::write(repo.0.join("assumed.ts"), "const value = 2;\n").unwrap();
    fs::write(repo.0.join("sparse.ts"), "const value = 3;\n").unwrap();
    fs::remove_file(repo.0.join("absent.ts")).unwrap();
    for mode in [CompareMode::Worktree, CompareMode::Unstaged] {
        let capture = snapshot(&repo, mode);
        let manifest = capture.manifest(0, 200);
        assert_eq!(manifest.total_files, 2);
        assert!(manifest.files.iter().all(|file| file.path != "absent.ts"));
        for file in manifest.files {
            assert_eq!(
                capture.read_file(&file.id).unwrap().before.as_deref(),
                Some("const value = 1;\n")
            );
        }
    }
}
#[test]
fn stat_clean_crlf_conversion_is_compared_literally() {
    for autocrlf in [false, true] {
        let repo = Repository::new();
        if autocrlf {
            repo.git(&["config", "core.autocrlf", "true"], None);
        } else {
            fs::write(repo.0.join(".gitattributes"), "*.ts text\n").unwrap();
        }
        fs::write(repo.0.join("source.ts"), "const a = 1;\r\n").unwrap();
        commit_files(&repo);
        assert!(repo.git(&["diff", "--name-only"], None).is_empty());
        let capture = snapshot(&repo, CompareMode::Worktree);
        let manifest = capture.manifest(0, 200);
        assert_eq!(manifest.total_files, 1);
        let file = capture.read_file(&manifest.files[0].id).unwrap();
        assert_eq!(file.before.as_deref(), Some("const a = 1;\n"));
        assert_eq!(file.after.as_deref(), Some("const a = 1;\r\n"));
    }
}
#[test]
fn minimal_user_stat_configuration_cannot_hide_changes() {
    let repo = Repository::new();
    fs::write(repo.0.join("source.ts"), "const a = 1;\n").unwrap();
    commit_files(&repo);
    for (name, value) in [
        ("core.trustctime", "false"),
        ("core.checkstat", "minimal"),
        ("core.ignorestat", "true"),
    ] {
        repo.git(&["config", name, value], None);
    }
    fs::write(repo.0.join("source.ts"), "const a = 2;\n").unwrap();
    let capture = snapshot(&repo, CompareMode::Worktree);
    assert_eq!(capture.manifest(0, 200).total_files, 1);
}
#[test]
fn huge_commit_trees_are_compared_by_changed_paths() {
    let repo = Repository::new();
    repo.one_staged_change(50_001, false);
    let base = repo.git(&["rev-parse", "HEAD"], None);
    let tree = repo.git(&["write-tree"], None);
    let head = repo.git(
        &["commit-tree", &tree, "-p", &base, "-m", "candidate"],
        None,
    );
    let capture = capture(
        &ComparisonSpec {
            repository: repo.0.to_string_lossy().into(),
            mode: CompareMode::Commits,
            base: Some(base),
            head: Some(head),
            include_untracked: false,
            verify_all: false,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(capture.manifest(0, 200).total_files, 1);
}
