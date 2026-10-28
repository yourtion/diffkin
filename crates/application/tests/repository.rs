use diffkin_application::{analyze_file, capture};
use diffkin_domain::{CompareMode, ComparisonSpec, FileStatus, RepositoryError};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        Self::with_format(None)
    }
    fn with_format(format: Option<&str>) -> Self {
        let path = std::env::temp_dir().join(format!(
            "diffkin-git-test-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        let repo = Self(path);
        let mut args = vec!["init", "-q", "--initial-branch=main"];
        let option = format.map(|format| format!("--object-format={format}"));
        if let Some(option) = &option {
            args.push(option);
        }
        repo.git(&args);
        repo.git(&["config", "user.name", "Diffkin Test"]);
        repo.git(&["config", "user.email", "diffkin@example.invalid"]);
        repo
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(&self.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .args(["-c", "core.hooksPath="])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().into()
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }
    fn commit(&self) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-qm", "fixture"]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn spec(&self, mode: CompareMode) -> ComparisonSpec {
        ComparisonSpec {
            repository: self.0.to_string_lossy().into_owned(),
            mode,
            base: None,
            head: None,
            include_untracked: false,
        }
    }
    fn capture(&self, mode: CompareMode) -> diffkin_application::FrozenComparison {
        capture(&self.spec(mode), &AtomicBool::new(false)).unwrap()
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn one(snapshot: &diffkin_application::FrozenComparison) -> diffkin_domain::FrozenFile {
    let manifest = snapshot.manifest(0, 200);
    assert_eq!(manifest.total_files, 1, "{manifest:?}");
    snapshot.read_file(&manifest.files[0].id).unwrap()
}

#[test]
fn staged_unstaged_worktree_and_freezing() {
    let repo = Repo::new();
    repo.write("a.ts", "const a = 1;\n");
    repo.commit();
    repo.write("a.ts", "const a = 2;\n");
    repo.git(&["add", "a.ts"]);
    repo.write("a.ts", "const a = 3;\n");
    let staged = one(&repo.capture(CompareMode::Staged));
    let snapshot = repo.capture(CompareMode::Unstaged);
    let unstaged = one(&snapshot);
    let all = one(&repo.capture(CompareMode::Worktree));
    assert_eq!(staged.before.as_deref(), Some("const a = 1;\n"));
    assert_eq!(staged.after.as_deref(), Some("const a = 2;\n"));
    assert_eq!(unstaged.before, staged.after);
    assert_eq!(unstaged.after.as_deref(), Some("const a = 3;\n"));
    assert_eq!(all.before, staged.before);
    assert_eq!(all.after, unstaged.after);
    repo.write("a.ts", "changed again");
    assert_eq!(one(&snapshot).after, unstaged.after);
    assert_ne!(
        repo.capture(CompareMode::Unstaged).manifest(0, 1).id,
        snapshot.manifest(0, 1).id
    );
}
#[test]
fn commit_refs_resolve_once() {
    let repo = Repo::new();
    repo.write("a.ts", "one");
    let first = repo.commit();
    repo.write("a.ts", "two");
    let second = repo.commit();
    let snapshot = repo.capture(CompareMode::Commits);
    let manifest = snapshot.manifest(0, 10);
    assert_eq!(manifest.base_label, first);
    assert_eq!(manifest.head_label, second);
    repo.write("a.ts", "three");
    repo.commit();
    assert_eq!(one(&snapshot).after.as_deref(), Some("two"));
}
#[test]
fn reads_do_not_change_index_or_refs() {
    let repo = Repo::new();
    repo.write("a", "one");
    repo.commit();
    repo.write("a", "two");
    let index = repo.0.join(".git/index");
    let bytes = fs::read(&index).unwrap();
    let head = repo.git(&["rev-parse", "HEAD"]);
    for mode in [
        CompareMode::Staged,
        CompareMode::Unstaged,
        CompareMode::Worktree,
    ] {
        repo.capture(mode);
    }
    assert_eq!(fs::read(index).unwrap(), bytes);
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
}
#[test]
fn unborn_and_untracked_selection() {
    let repo = Repo::new();
    repo.write("new.ts", "const a = 1;");
    assert_eq!(
        repo.capture(CompareMode::Staged)
            .manifest(0, 10)
            .total_files,
        0
    );
    assert_eq!(
        repo.capture(CompareMode::Worktree)
            .manifest(0, 10)
            .total_files,
        0
    );
    let mut spec = repo.spec(CompareMode::Worktree);
    spec.include_untracked = true;
    assert_eq!(
        one(&capture(&spec, &AtomicBool::new(false)).unwrap())
            .summary
            .status,
        FileStatus::Added
    );
    repo.git(&["add", "new.ts"]);
    assert_eq!(
        one(&repo.capture(CompareMode::Staged)).summary.status,
        FileStatus::Added
    );
}
#[test]
fn ignored_untracked_paths_are_excluded() {
    let repo = Repo::new();
    repo.write(".gitignore", "ignored\n");
    repo.commit();
    repo.write("ignored", "secret");
    repo.write("visible", "yes");
    let mut spec = repo.spec(CompareMode::Worktree);
    spec.include_untracked = true;
    assert_eq!(
        one(&capture(&spec, &AtomicBool::new(false)).unwrap())
            .summary
            .path,
        "visible"
    );
}
#[test]
fn exact_rename_and_copy_are_distinct() {
    let repo = Repo::new();
    repo.write("old.ts", "same");
    repo.commit();
    fs::rename(repo.0.join("old.ts"), repo.0.join("new.ts")).unwrap();
    repo.git(&["add", "--all"]);
    let file = one(&repo.capture(CompareMode::Staged));
    assert_eq!(file.summary.status, FileStatus::Renamed);
    assert_eq!(file.summary.old_path.as_deref(), Some("old.ts"));
    assert_eq!(file.before, file.after);
    repo.commit();
    repo.write("copy.ts", "same");
    repo.git(&["add", "copy.ts"]);
    assert_eq!(
        one(&repo.capture(CompareMode::Staged)).summary.status,
        FileStatus::Added
    );
}
#[test]
fn repeated_content_renames_are_ambiguous() {
    let repo = Repo::new();
    for path in ["a", "b"] {
        repo.write(path, "same");
    }
    repo.commit();
    for path in ["a", "b"] {
        fs::remove_file(repo.0.join(path)).unwrap();
    }
    for path in ["c", "d"] {
        repo.write(path, "same");
    }
    repo.git(&["add", "--all"]);
    let manifest = repo.capture(CompareMode::Staged).manifest(0, 20);
    assert_eq!(manifest.total_files, 4);
    assert!(manifest
        .files
        .iter()
        .all(|file| file.status != FileStatus::Renamed));
}
#[test]
fn binary_invalid_utf8_lfs_large_are_explicit() {
    let repo = Repo::new();
    repo.write("binary", b"a\0b");
    repo.write("invalid", [0xff, 0xfe]);
    repo.write(
        "lfs",
        "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 123\n",
    );
    repo.write("large", vec![b'x'; 2 * 1024 * 1024 + 1]);
    repo.git(&["add", "--all"]);
    let snapshot = repo.capture(CompareMode::Staged);
    for summary in snapshot.manifest(0, 20).files {
        let file = snapshot.read_file(&summary.id).unwrap();
        match summary.path.as_str() {
            "binary" | "invalid" => {
                assert!(file.after.is_none());
                assert!(analyze_file(file, &AtomicBool::new(false))
                    .unwrap()
                    .analysis
                    .is_none());
            }
            "lfs" => assert!(file
                .summary
                .notices
                .iter()
                .any(|notice| notice.contains("LFS"))),
            "large" => {
                assert!(file.after_truncated);
                assert!(analyze_file(file, &AtomicBool::new(false))
                    .unwrap()
                    .analysis
                    .is_none());
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn bounded_large_worktree_preview_is_frozen() {
    let repo = Repo::new();
    repo.write("big.ts", "old");
    repo.commit();
    repo.write("big.ts", "😀".repeat(600_000));
    let snapshot = repo.capture(CompareMode::Worktree);
    let file = one(&snapshot);
    assert!(file.after_truncated);
    assert!(file.after.as_ref().unwrap().len() <= 2 * 1024 * 1024);
    repo.write("big.ts", "new again");
    assert_eq!(one(&snapshot).after, file.after);
}
#[test]
fn filters_external_diff_and_fsmonitor_never_execute() {
    let repo = Repo::new();
    repo.write("a.ts", "first\n");
    repo.commit();
    repo.write(".gitattributes", "*.ts filter=evil diff=evil\n");
    let marker = repo.0.join("executed");
    let shell = format!(
        "echo unsafe > '{}'",
        marker.to_string_lossy().replace('\\', "/")
    );
    for key in [
        "filter.evil.clean",
        "filter.evil.smudge",
        "diff.evil.command",
        "core.fsmonitor",
    ] {
        repo.git(&["config", key, &shell]);
    }
    repo.write("a.ts", "second\r\n");
    assert_eq!(
        one(&repo.capture(CompareMode::Unstaged)).after.as_deref(),
        Some("second\r\n")
    );
    assert!(!marker.exists());
}
#[test]
fn cancelled_and_invalid_revision_errors_are_distinct() {
    let repo = Repo::new();
    assert!(matches!(
        capture(&repo.spec(CompareMode::Worktree), &AtomicBool::new(true)),
        Err(RepositoryError::Cancelled { .. })
    ));
    let mut spec = repo.spec(CompareMode::Commits);
    spec.base = Some("--all".into());
    assert!(matches!(
        capture(&spec, &AtomicBool::new(false)),
        Err(RepositoryError::InvalidRevision { .. })
    ));
}
#[test]
fn pagination_and_unknown_file_ids() {
    let repo = Repo::new();
    for n in 0..5 {
        repo.write(&format!("{n}.ts"), "new");
    }
    repo.git(&["add", "--all"]);
    let snapshot = repo.capture(CompareMode::Staged);
    let page = snapshot.manifest(2, 2);
    assert_eq!(page.total_files, 5);
    assert_eq!(page.files.len(), 2);
    assert_eq!(page.files[0].path, "2.ts");
    assert!(snapshot.manifest(99, 2).files.is_empty());
    assert!(matches!(
        snapshot.read_file("other"),
        Err(RepositoryError::Stale { .. })
    ));
}
#[test]
fn missing_worktree_file_is_deletion() {
    let repo = Repo::new();
    repo.write("a", "before");
    repo.commit();
    fs::remove_file(repo.0.join("a")).unwrap();
    let file = one(&repo.capture(CompareMode::Worktree));
    assert_eq!(file.summary.status, FileStatus::Deleted);
    assert_eq!(file.before.as_deref(), Some("before"));
    assert!(file.after.is_none());
    assert!(analyze_file(file, &AtomicBool::new(false))
        .unwrap()
        .analysis
        .is_some());
}
#[test]
fn conflict_is_visible_without_structural_analysis() {
    let repo = Repo::new();
    repo.write("a.ts", "base");
    repo.commit();
    repo.git(&["checkout", "-qb", "other"]);
    repo.write("a.ts", "other");
    repo.commit();
    repo.git(&["checkout", "-q", "main"]);
    repo.write("a.ts", "main");
    repo.commit();
    let status = Command::new("git")
        .current_dir(&repo.0)
        .args(["-c", "core.hooksPath=", "merge", "other"])
        .output()
        .unwrap()
        .status;
    assert!(!status.success());
    let file = one(&repo.capture(CompareMode::Unstaged));
    assert_eq!(file.summary.status, FileStatus::Conflict);
    assert!(file.after.as_deref().unwrap().contains("<<<<<<<"));
    assert!(analyze_file(file, &AtomicBool::new(false))
        .unwrap()
        .analysis
        .is_none());
}
#[test]
fn submodule_gitlinks_do_not_fetch_or_descend() {
    let repo = Repo::new();
    repo.write("a", "one");
    let first = repo.commit();
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{first},dependency"),
    ]);
    let file = one(&repo.capture(CompareMode::Staged));
    assert_eq!(file.summary.new_mode.as_deref(), Some("160000"));
    assert!(file.after.as_deref().unwrap().contains(&first));
    assert!(analyze_file(file, &AtomicBool::new(false))
        .unwrap()
        .analysis
        .is_none());
}
#[test]
fn replacement_refs_are_disabled() {
    let repo = Repo::new();
    repo.write("a", "original");
    let original = repo.commit();
    repo.write("a", "replacement");
    let replacement = repo.commit();
    repo.git(&["replace", &original, &replacement]);
    let mut spec = repo.spec(CompareMode::Commits);
    spec.base = Some(original);
    spec.head = Some(replacement);
    let file = one(&capture(&spec, &AtomicBool::new(false)).unwrap());
    assert_eq!(file.before.as_deref(), Some("original"));
    assert_eq!(file.after.as_deref(), Some("replacement"));
}
#[test]
fn sha256_object_ids_are_preserved() {
    let repo = Repo::with_format(Some("sha256"));
    repo.write("a", "one");
    repo.commit();
    repo.write("a", "two");
    let file = one(&repo.capture(CompareMode::Unstaged));
    assert_eq!(file.summary.before_id.unwrap().len(), 64);
    assert_eq!(file.summary.after_id.unwrap().len(), 64);
}
#[cfg(unix)]
#[test]
fn unusual_paths_retain_distinct_ids() {
    use std::io::Write;
    use std::process::Stdio;
    let repo = Repo::new();
    repo.write("-option", "one");
    repo.write("line\nname", "two");
    repo.git(&["add", "--all"]);
    // APFS rejects non-UTF-8 filenames, but the Git index can still contain them.
    let oid = repo.git(&["rev-parse", ":-option"]);
    let mut child = Command::new("git")
        .current_dir(&repo.0)
        .args(["update-index", "-z", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut record = format!("100644 {oid}\tbad").into_bytes();
    record.extend_from_slice(&[0xff, 0]);
    child.stdin.take().unwrap().write_all(&record).unwrap();
    assert!(child.wait().unwrap().success());
    let manifest = repo.capture(CompareMode::Staged).manifest(0, 10);
    assert_eq!(manifest.total_files, 3);
    assert!(manifest.files.iter().any(|file| file.path == "line\\nname"));
    assert!(manifest.files.iter().any(|file| file
        .notices
        .iter()
        .any(|notice| notice.contains("non-UTF-8"))));
    assert_eq!(
        manifest
            .files
            .iter()
            .map(|file| &file.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
}
#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() {
    use std::os::unix::fs::symlink;
    let repo = Repo::new();
    symlink("/private/etc/passwd", repo.0.join("link")).unwrap();
    repo.git(&["add", "link"]);
    let file = one(&repo.capture(CompareMode::Staged));
    assert_eq!(file.after.as_deref(), Some("/private/etc/passwd"));
    assert_eq!(file.summary.new_mode.as_deref(), Some("120000"));
    repo.commit();
    fs::remove_file(repo.0.join("link")).unwrap();
    symlink("/etc/shadow", repo.0.join("link")).unwrap();
    assert_eq!(
        one(&repo.capture(CompareMode::Worktree)).after.as_deref(),
        Some("/etc/shadow")
    );
}
#[cfg(unix)]
#[test]
fn executable_mode_changes_are_visible() {
    use std::os::unix::fs::PermissionsExt;
    let repo = Repo::new();
    repo.write("script", "same");
    repo.commit();
    fs::set_permissions(repo.0.join("script"), fs::Permissions::from_mode(0o755)).unwrap();
    let file = one(&repo.capture(CompareMode::Unstaged));
    assert_eq!(file.before, file.after);
    assert_eq!(file.summary.old_mode.as_deref(), Some("100644"));
    assert_eq!(file.summary.new_mode.as_deref(), Some("100755"));
}

#[test]
fn active_capture_cancellation_releases_resources_promptly() {
    let repo = Repo::new();
    for index in 0..200 {
        repo.write(&format!("{index}.txt"), "x".repeat(32 * 1024));
    }
    repo.commit();
    let flag = std::sync::Arc::new(AtomicBool::new(false));
    let worker_flag = flag.clone();
    let spec = repo.spec(CompareMode::Worktree);
    let worker = std::thread::spawn(move || capture(&spec, &worker_flag));
    std::thread::sleep(std::time::Duration::from_millis(10));
    let started = std::time::Instant::now();
    flag.store(true, Ordering::Relaxed);
    assert!(matches!(
        worker.join().unwrap(),
        Err(RepositoryError::Cancelled { .. })
    ));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "cancellation did not release capture promptly"
    );
}

#[test]
fn changing_worktree_is_never_published_as_a_stable_snapshot() {
    let repo = Repo::new();
    repo.write("a.txt", "a".repeat(4 * 1024 * 1024));
    repo.commit();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let writer_done = done.clone();
    let path = repo.0.join("a.txt");
    let writer = std::thread::spawn(move || {
        use std::io::{Seek, SeekFrom, Write};
        let mut value = b'b';
        while !writer_done.load(Ordering::Relaxed) {
            let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();
            file.write_all(&[value]).unwrap();
            file.flush().unwrap();
            drop(file);
            value = if value == b'b' { b'c' } else { b'b' };
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    let result = capture(&repo.spec(CompareMode::Worktree), &AtomicBool::new(false));
    done.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(
        matches!(result, Err(RepositoryError::Stale { .. })),
        "a concurrently changing file was published"
    );
}

#[test]
fn intent_to_add_is_not_mistaken_for_staged_empty_content() {
    let repo = Repo::new();
    repo.write("new.ts", "const actual = 1;\n");
    repo.git(&["add", "--intent-to-add", "new.ts"]);
    assert_eq!(
        repo.capture(CompareMode::Staged)
            .manifest(0, 10)
            .total_files,
        0
    );
    let file = one(&repo.capture(CompareMode::Unstaged));
    assert_eq!(file.summary.status, FileStatus::Added);
    assert!(file.before.is_none());
    assert_eq!(file.after.as_deref(), Some("const actual = 1;\n"));
    assert!(file
        .summary
        .notices
        .iter()
        .any(|notice| notice.contains("Intent-to-add")));
}

#[test]
fn missing_promisor_blob_fails_without_fetching() {
    let repo = Repo::new();
    repo.write("a.ts", "before");
    repo.commit();
    let missing = repo.git(&["rev-parse", "HEAD:a.ts"]);
    repo.write("a.ts", "after");
    repo.commit();
    let object = repo
        .0
        .join(".git/objects")
        .join(&missing[..2])
        .join(&missing[2..]);
    fs::remove_file(object).unwrap();
    repo.git(&["config", "remote.origin.promisor", "true"]);
    repo.git(&[
        "config",
        "remote.origin.url",
        "ssh://example.invalid/missing.git",
    ]);
    let marker = repo.0.join("unexpected-fetch");
    repo.git(&[
        "config",
        "core.sshCommand",
        &format!(
            "echo fetch > '{}'",
            marker.to_string_lossy().replace('\\', "/")
        ),
    ]);
    let result = capture(&repo.spec(CompareMode::Commits), &AtomicBool::new(false));
    assert!(matches!(result, Err(RepositoryError::IoFailure { .. })));
    assert!(!marker.exists());
}
