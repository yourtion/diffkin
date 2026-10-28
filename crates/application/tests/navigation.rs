use diffkin_application::navigation::list_refs;
use diffkin_domain::{RepositoryError, RepositoryRefKind};
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
        let p = std::env::temp_dir().join(format!(
            "diffkin-ref-test-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        let f = Self(p);
        f.git(&["init", "-q", "-b", "main"]);
        f.git(&["config", "user.name", "Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        fs::write(f.0.join("source.txt"), "fixture").unwrap();
        f.git(&["add", "source.txt"]);
        f.git(&["commit", "-qm", "fixture"]);
        f
    }
    fn command(&self) -> Command {
        let mut c = Command::new("git");
        c.current_dir(&self.0)
            .args(["-c", "core.hooksPath=", "-c", "commit.gpgsign=false"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            );
        c
    }
    fn git(&self, args: &[&str]) -> String {
        let out = self.command().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().into()
    }
    fn list(&self, query: &str, offset: u32, limit: u32) -> diffkin_domain::RepositoryRefs {
        list_refs(
            self.0.to_str().unwrap(),
            query,
            offset,
            limit,
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
fn branches_tags_remote_and_detached_head() {
    let f = Fixture::new();
    f.git(&["branch", "feature/nested"]);
    f.git(&["tag", "v1"]);
    f.git(&["tag", "-a", "v2", "-m", "annotated"]);
    f.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    f.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    let refs = f.list("", 0, 200);
    assert_eq!(refs.current_branch.as_deref(), Some("refs/heads/main"));
    assert!(refs
        .refs
        .iter()
        .any(|r| matches!(r.kind, RepositoryRefKind::Tag) && r.label == "v2"));
    assert!(refs
        .refs
        .iter()
        .any(|r| r.symbolic_target.as_deref() == Some("refs/remotes/origin/main")));
    f.git(&["checkout", "--detach", "-q"]);
    assert!(f.list("", 0, 200).current_branch.is_none());
}
#[test]
fn stable_pages_literal_filter_and_no_checkout_mutation() {
    let f = Fixture::new();
    for name in ["feature/Apple", "feature/banana", "feature/Apricot"] {
        f.git(&["branch", name]);
    }
    let state = f.git(&["status", "--porcelain=v1"]);
    let a = f.list("apple", 0, 1);
    assert_eq!(a.refs[0].label, "feature/Apple");
    let a = f.list("feature/", 0, 2);
    assert_eq!(a.refs.len(), 2);
    assert_eq!(a.next_offset, Some(2));
    let b = f.list("feature/", 2, 2);
    assert_eq!(b.refs.len(), 1);
    assert!(b.next_offset.is_none());
    assert_ne!(a.refs[0].name, b.refs[0].name);
    assert_eq!(f.git(&["status", "--porcelain=v1"]), state);
    assert!(list_refs(f.0.to_str().unwrap(), "*", 0, 200, &AtomicBool::new(false)).is_err());
}
#[test]
fn invalid_directory_cancel_and_unborn_repository() {
    let f = Fixture::new();
    assert!(matches!(
        list_refs(f.0.to_str().unwrap(), "", 0, 200, &AtomicBool::new(true)),
        Err(RepositoryError::Cancelled { .. })
    ));
    let plain = f.0.join("plain");
    fs::create_dir(&plain).unwrap();
    assert!(list_refs(
        "/nonexistent-diffkin-fixture",
        "",
        0,
        200,
        &AtomicBool::new(false)
    )
    .is_err());
    let empty = f.0.join("empty");
    fs::create_dir(&empty).unwrap();
    let out = Command::new("git")
        .current_dir(&empty)
        .args(["init", "-q", "-b", "main"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let r = list_refs(empty.to_str().unwrap(), "", 0, 200, &AtomicBool::new(false)).unwrap();
    assert!(r.refs.is_empty());
    assert_eq!(r.current_branch.as_deref(), Some("refs/heads/main"));
}
#[test]
fn huge_ref_collection_is_bounded_and_filter_can_find_late_refs() {
    let f = Fixture::new();
    let oid = f.git(&["rev-parse", "HEAD"]);
    let mut child = f
        .command()
        .args(["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    {
        let input = child.stdin.as_mut().unwrap();
        for i in 0..4200 {
            writeln!(input, "create refs/heads/generated/{i:04} {oid}").unwrap();
        }
    }
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    let r = f.list("", 0, 200);
    assert_eq!(r.refs.len(), 200);
    assert!(r.truncated);
    assert!(!r.warnings.is_empty());
    let late = f.list("4199", 0, 200);
    assert_eq!(late.refs.len(), 1);
    assert!(!late.truncated);
}

#[test]
fn literal_substrings_cross_nested_ref_components_and_unicode() {
    let f = Fixture::new();
    f.git(&["branch", "team/feature/deep/名字"]);
    for query in ["feature/", "re/de", "名字", "team"] {
        let page = f.list(query, 0, 200);
        assert_eq!(page.refs.len(), 1, "literal query {query}");
        assert_eq!(page.refs[0].name, "refs/heads/team/feature/deep/名字");
    }
    assert!(f.list("no-match", 0, 200).refs.is_empty());
}

#[test]
fn literal_filter_can_include_the_full_ref_namespace() {
    let f = Fixture::new();
    f.git(&["branch", "team/feature/deep"]);
    for query in [
        "refs/heads/team",
        "heads/team",
        "s/heads/team",
        "REFS/HEADS/TEAM",
    ] {
        let page = f.list(query, 0, 200);
        assert_eq!(page.refs.len(), 1, "literal query {query}");
        assert_eq!(page.refs[0].name, "refs/heads/team/feature/deep");
    }
    // Matching a namespace is also a literal substring query.
    assert_eq!(f.list("refs/heads/", 0, 200).refs.len(), 2);
}

#[cfg(unix)]
#[test]
fn ref_discovery_does_not_invoke_configured_fsmonitor_or_remote_helpers() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let script = f.0.join("unexpected.sh");
    fs::write(&script, "#!/bin/sh\necho invoked > unexpected-marker\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    f.git(&["config", "core.fsmonitor", script.to_str().unwrap()]);
    f.git(&["remote", "add", "origin", "ext::sh unexpected.sh"]);
    assert!(!f.list("", 0, 200).refs.is_empty());
    assert!(!f.0.join("unexpected-marker").exists());
}
