use diffkin_application::{capture_workspace, WorkspaceStore};
use diffkin_domain::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "diffkin-workspace-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> WorkspaceStore {
        WorkspaceStore::new(self.0.join("store")).unwrap()
    }
    fn repository(&self, id: &str, changed: bool) -> WorkspaceRepository {
        let path = self.0.join(id);
        fs::create_dir_all(path.join("src")).unwrap();
        let git = |args: &[&str]| {
            let result = Command::new("git")
                .current_dir(&path)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                )
                .args([
                    "-c",
                    "core.hooksPath=",
                    "-c",
                    "user.name=Workspace Test",
                    "-c",
                    "user.email=test@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        git(&["init", "-q", "--initial-branch=main"]);
        fs::write(
            path.join("src/index.ts"),
            format!("export const name = '{id}';\n"),
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        if changed {
            fs::write(
                path.join("src/index.ts"),
                format!("export const name = '{id}-secret-content';\n"),
            )
            .unwrap();
        }
        WorkspaceRepository {
            id: id.into(),
            label: format!("private-label-{id}"),
            spec: ComparisonSpec {
                repository: path.to_string_lossy().into_owned(),
                mode: CompareMode::Worktree,
                base: None,
                head: None,
                include_untracked: false,
            },
        }
    }
    fn config(&self) -> WorkspaceConfig {
        WorkspaceConfig {
            id: "project".into(),
            name: "private-workspace-name".into(),
            repositories: vec![
                self.repository("frontend", true),
                self.repository("api", true),
                self.repository("library", false),
            ],
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn token() -> AtomicBool {
    AtomicBool::new(false)
}
fn basis(snapshot: &diffkin_application::FrozenWorkspace, repo: &str) -> EvidenceBasis {
    let manifest = snapshot.repository(repo).unwrap().manifest(0, 200);
    snapshot.basis(repo, &manifest.files[0].id).unwrap()
}
#[test]
fn three_repositories_same_filename_and_unchanged_context_are_isolated() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let snapshot = capture_workspace(&config, &token()).unwrap();
    let manifest = snapshot.manifest();
    assert_eq!(manifest.repositories.len(), 3);
    assert_eq!(
        snapshot
            .repository("library")
            .unwrap()
            .manifest(0, 200)
            .total_files,
        0
    );
    let frontend = basis(&snapshot, "frontend");
    let api = basis(&snapshot, "api");
    assert_ne!(frontend.repository_snapshot_id, api.repository_snapshot_id);
    assert_ne!(frontend.after_id, api.after_id);
    assert_eq!(
        snapshot
            .repository("frontend")
            .unwrap()
            .read_file(&frontend.file_id)
            .unwrap()
            .summary
            .path,
        "src/index.ts"
    );
    assert_eq!(
        snapshot
            .repository("api")
            .unwrap()
            .read_file(&api.file_id)
            .unwrap()
            .summary
            .path,
        "src/index.ts"
    );
    assert!(snapshot
        .repository("api")
        .unwrap()
        .read_file(&frontend.file_id)
        .is_err());
}
#[test]
fn restore_after_removing_live_repositories_retains_review_and_relation_evidence() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = fixture.store();
    store.capture(&config, &token()).unwrap();
    let snapshot = store.restore("project", None).unwrap();
    let front = basis(&snapshot, "frontend");
    let api = basis(&snapshot, "api");
    store
        .set_review("project", front.clone(), ReviewState::Reviewed)
        .unwrap();
    store
        .add_relation(
            "project",
            ManualRelation {
                id: "contract".into(),
                from: front.clone(),
                to: api,
                note: "private-note-token".into(),
            },
        )
        .unwrap();
    for repo in &config.repositories {
        fs::remove_dir_all(&repo.spec.repository).unwrap();
    }
    drop(store);
    let reopened = fixture.store();
    let old = reopened.restore("project", None).unwrap();
    assert!(old
        .repository("frontend")
        .unwrap()
        .read_file(&front.file_id)
        .unwrap()
        .after
        .unwrap()
        .contains("frontend-secret-content"));
    let session = reopened.load("project").unwrap();
    assert_eq!(session.reviews.len(), 1);
    assert_eq!(session.relations.len(), 1);
    assert_eq!(session.reviews[0].basis, front);
    let export = serde_json::to_string(&reopened.export("project").unwrap()).unwrap();
    for secret in [
        fixture.0.to_str().unwrap(),
        "secret-content",
        "private-note-token",
        "private-label",
        "private-workspace-name",
        "src/index.ts",
    ] {
        assert!(!export.contains(secret), "Export leaked {secret}");
    }
}
#[test]
fn refresh_has_new_basis_and_historical_activation_is_persisted() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = fixture.store();
    store.capture(&config, &token()).unwrap();
    let old = store.restore("project", None).unwrap();
    let old_basis = basis(&old, "frontend");
    store
        .set_review("project", old_basis.clone(), ReviewState::Reviewed)
        .unwrap();
    store.capture(&config, &token()).unwrap();
    let new = store.restore("project", None).unwrap();
    let new_basis = basis(&new, "frontend");
    assert_ne!(
        old_basis.workspace_snapshot_id,
        new_basis.workspace_snapshot_id
    );
    assert_eq!(old_basis.after_id, new_basis.after_id);
    assert!(store
        .set_review("project", old_basis.clone(), ReviewState::NeedsAttention)
        .is_err());
    let session = store.load("project").unwrap();
    assert_eq!(session.reviews.len(), 1);
    assert_ne!(session.reviews[0].basis, new_basis);
    store.restore("project", Some(&old.manifest().id)).unwrap();
    assert_eq!(
        store.load("project").unwrap().active_snapshot_id,
        Some(old.manifest().id)
    );
    store
        .set_review("project", old_basis, ReviewState::NeedsAttention)
        .unwrap();
    assert_eq!(
        store.restore("project", None).unwrap().manifest().id,
        old.manifest().id
    );
}
#[test]
fn missing_repository_is_partial_and_relocation_preserves_repository_identity() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    let original = config.repositories[1].spec.repository.clone();
    config.repositories[1].spec.repository =
        fixture.0.join("missing").to_string_lossy().into_owned();
    let store = fixture.store();
    store.capture(&config, &token()).unwrap();
    let old = store.restore("project", None).unwrap();
    assert!(old.repository("frontend").is_ok());
    assert!(old.repository("api").is_err());
    assert!(old.manifest().repositories[1].error.is_some());
    let relocated = store.relocate("project", "api", &original).unwrap();
    assert_eq!(relocated.config.repositories[1].id, "api");
    store.capture(&relocated.config, &token()).unwrap();
    let fresh = store.restore("project", None).unwrap();
    assert!(fresh.repository("api").is_ok());
    assert!(store
        .restore("project", Some(&old.manifest().id))
        .unwrap()
        .repository("api")
        .is_err());
}
#[test]
fn tampered_review_and_relation_bases_are_rejected() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = fixture.store();
    store.capture(&config, &token()).unwrap();
    let snapshot = store.restore("project", None).unwrap();
    let valid = basis(&snapshot, "frontend");
    let mut cases = Vec::new();
    let mut b = valid.clone();
    b.repository_id = "api".into();
    cases.push(b);
    let mut b = valid.clone();
    b.after_id = None;
    cases.push(b);
    let mut b = valid.clone();
    b.schema_version = 2;
    cases.push(b);
    let mut b = valid.clone();
    b.rule_version = "syntax-v1".into();
    cases.push(b);
    let mut b = valid.clone();
    b.rule_version = "metadata-v1".into();
    cases.push(b);
    let mut b = valid.clone();
    b.repository_snapshot_id = "bad".into();
    cases.push(b);
    for bad in cases {
        assert!(store
            .set_review("project", bad.clone(), ReviewState::Reviewed)
            .is_err());
        assert!(store
            .add_relation(
                "project",
                ManualRelation {
                    id: "bad".into(),
                    from: bad,
                    to: basis(&snapshot, "api"),
                    note: String::new()
                }
            )
            .is_err());
    }
    assert!(store
        .add_relation(
            "project",
            ManualRelation {
                id: "self".into(),
                from: valid.clone(),
                to: valid,
                note: String::new()
            }
        )
        .is_err());
    assert!(store.load("project").unwrap().reviews.is_empty());
}
#[test]
fn metadata_rule_is_issued_by_server_and_cannot_be_changed_to_syntax() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.repositories.truncate(1);
    fs::write(
        PathBuf::from(&config.repositories[0].spec.repository).join("src/index.ts"),
        [0, 1, 2],
    )
    .unwrap();
    let store = fixture.store();
    store.capture(&config, &token()).unwrap();
    let snapshot = store.restore("project", None).unwrap();
    let mut b = basis(&snapshot, "frontend");
    assert_eq!(b.rule_version, "metadata-v1");
    store
        .set_review("project", b.clone(), ReviewState::Reviewed)
        .unwrap();
    b.rule_version = diffkin_analysis::RULE_VERSION.into();
    assert!(store
        .set_review("project", b, ReviewState::Reviewed)
        .is_err());
}
#[test]
fn checksum_corruption_and_source_forgery_fail_closed_without_reading_live_source() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = fixture.store();
    store.capture(&config, &token()).unwrap();
    let path = fixture.0.join("store/project.json");
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"{truncated").unwrap();
    assert!(store.restore("project", None).is_err());
    assert!(store.list().unwrap()[0].error.is_some());
    fs::write(&path, &original).unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
    document["checksum"] = "0".repeat(64).into();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(store.load("project").is_err());
    let text = String::from_utf8(original.clone())
        .unwrap()
        .replace("frontend-secret-content", "forged-secret-content");
    let mut document: serde_json::Value = serde_json::from_str(&text).unwrap();
    // Re-encoding a Value does not preserve struct field order: checksum rejection is still expected.
    document["checksum"] = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&document["payload"]).unwrap())
    )
    .into();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(store.load("project").is_err());
    fs::write(&path, original).unwrap();
    assert!(store.restore("project", None).is_ok());
}
#[test]
fn invalid_config_counts_and_cancellation_leave_no_snapshot() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    let store = fixture.store();
    for invalid in ["../escape", "CON", "Lpt1", "nul"] {
        config.id = invalid.into();
        assert!(store.save_config(&config).is_err());
    }
    config.id = "project".into();
    config.repositories[1].id = config.repositories[0].id.clone();
    assert!(store.save_config(&config).is_err());
    config.repositories[1].id = "api".into();
    config.repositories[0].spec.repository = "relative/path".into();
    assert!(store.save_config(&config).is_err());
    config.repositories[0].spec.repository =
        fixture.0.join("frontend").to_string_lossy().into_owned();
    assert!(store.capture(&config, &AtomicBool::new(true)).is_err());
    assert!(store.list().unwrap().is_empty());
    store.save_config(&config).unwrap();
    assert!(store.load("project").unwrap().snapshots.is_empty());
    for index in 1..16 {
        config.id = format!("workspace{index}");
        store.save_config(&config).unwrap();
    }
    config.id = "seventeenth".into();
    assert!(store.save_config(&config).is_err());
    assert_eq!(store.list().unwrap().len(), 16);
}
#[test]
fn unreferenced_history_is_bounded_and_referenced_history_is_not_silently_lost() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.repositories.truncate(1);
    let store = fixture.store();
    for _ in 0..10 {
        store.capture(&config, &token()).unwrap();
    }
    assert_eq!(store.load("project").unwrap().snapshots.len(), 8);
    for _ in 0..8 {
        let snapshot = store.restore("project", None).unwrap();
        store
            .set_review(
                "project",
                basis(&snapshot, "frontend"),
                ReviewState::Reviewed,
            )
            .unwrap();
        if store.load("project").unwrap().reviews.len() < 8 {
            store.capture(&config, &token()).unwrap();
        }
    }
    let old = store.load("project").unwrap();
    assert!(store.capture(&config, &token()).is_err());
    let new = store.load("project").unwrap();
    assert_eq!(old.active_snapshot_id, new.active_snapshot_id);
    assert_eq!(new.reviews.len(), 8);
    assert_eq!(new.snapshots.len(), 8);
}
#[cfg(unix)]
#[test]
fn managed_symlink_is_rejected_and_private_files_are_not_world_readable() {
    use std::os::unix::{fs::symlink, fs::PermissionsExt};
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = fixture.store();
    store.save_config(&config).unwrap();
    let path = fixture.0.join("store/project.json");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let outside = fixture.0.join("outside.json");
    fs::rename(&path, &outside).unwrap();
    symlink(&outside, &path).unwrap();
    assert!(store.load("project").is_err());
    assert!(store.save_config(&config).is_err());
    assert!(outside.exists());
}

#[test]
fn failed_atomic_write_preserves_prior_archive_and_oversized_files_are_rejected() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    let store = fixture.store();
    store.save_config(&config).unwrap();
    let path = fixture.0.join("store/project.json");
    let original = fs::read(&path).unwrap();
    fs::create_dir(fixture.0.join("store/.pending")).unwrap();
    config.name = "updated".into();
    assert!(store.save_config(&config).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        store.load("project").unwrap().config.name,
        "private-workspace-name"
    );
    fs::remove_dir(fixture.0.join("store/.pending")).unwrap();
    fs::write(fixture.0.join("store/.pending"), b"crashed write").unwrap();
    store.save_config(&config).unwrap();
    assert_eq!(store.load("project").unwrap().config.name, "updated");
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    assert!(matches!(
        store.load("project"),
        Err(RepositoryError::BudgetExceeded { .. })
    ));
}

#[cfg(unix)]
#[test]
fn redirected_store_root_lock_and_staging_never_write_the_target() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let config = fixture.config();
    let source = fixture.0.join("source");
    fs::create_dir(&source).unwrap();
    let sentinel = source.join("sentinel");
    fs::write(&sentinel, b"do not modify").unwrap();
    let alias = fixture.0.join("alias");
    symlink(&source, &alias).unwrap();
    assert!(WorkspaceStore::new(alias).is_err());
    let store = fixture.store();
    store.save_config(&config).unwrap();
    let root = fixture.0.join("store");
    fs::rename(&root, fixture.0.join("store-backup")).unwrap();
    symlink(&source, &root).unwrap();
    assert!(store.load("project").is_err());
    assert!(store.save_config(&config).is_err());
    assert!(!source.join("project.json").exists());
    fs::remove_file(&root).unwrap();
    fs::rename(fixture.0.join("store-backup"), &root).unwrap();
    fs::remove_file(root.join(".lock")).unwrap();
    symlink(&sentinel, root.join(".lock")).unwrap();
    assert!(store.save_config(&config).is_err());
    fs::remove_file(root.join(".lock")).unwrap();
    symlink(&sentinel, root.join(".pending")).unwrap();
    assert!(store.save_config(&config).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"do not modify");
}
