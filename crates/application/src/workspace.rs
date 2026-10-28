//! Bounded, immutable workspace evidence. Restoring never opens a repository path.
use crate::{capture, file_rule_version, FrozenComparison};
use diffkin_domain::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_REPOSITORIES: usize = 8;
const MAX_WORKSPACES: usize = 16;
const MAX_SNAPSHOTS: usize = 8;
const MAX_RELATIONS: usize = 256;
const MAX_REVIEWS: usize = 4096;
const MAX_ARCHIVE: usize = 64 * 1024 * 1024;
const MAX_STORE: u64 = 256 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
fn invalid(message: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::IoFailure {
        message: message.to_string(),
    }
}
fn budget(message: &str) -> RepositoryError {
    RepositoryError::BudgetExceeded {
        message: message.into(),
    }
}
fn stale(message: &str) -> RepositoryError {
    RepositoryError::Stale {
        message: message.into(),
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_id(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    let device = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        });
    !device
        && !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn hash_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn json<T: Serialize>(value: &T) -> Result<Vec<u8>, RepositoryError> {
    serde_json::to_vec(value).map_err(invalid)
}
fn cancelled(cancel: &AtomicBool) -> Result<(), RepositoryError> {
    if cancel.load(Ordering::Relaxed) {
        Err(RepositoryError::Cancelled {
            message: "Workspace capture cancelled".into(),
        })
    } else {
        Ok(())
    }
}
fn validate_config(config: &WorkspaceConfig) -> Result<(), RepositoryError> {
    if !safe_id(&config.id)
        || config.name.is_empty()
        || config.name.len() > 240
        || config.repositories.is_empty()
        || config.repositories.len() > MAX_REPOSITORIES
    {
        return Err(invalid(
            "Invalid workspace ID, name or repository count (1–8)",
        ));
    }
    let mut ids = BTreeSet::new();
    for repository in &config.repositories {
        if !safe_id(&repository.id)
            || !ids.insert(&repository.id)
            || repository.label.is_empty()
            || repository.label.len() > 240
            || repository.spec.repository.len() > 4096
            || repository.spec.repository.contains('\0')
            || !Path::new(&repository.spec.repository).is_absolute()
            || repository
                .spec
                .base
                .as_ref()
                .is_some_and(|s| s.len() > 1024 || s.contains('\0'))
            || repository
                .spec
                .head
                .as_ref()
                .is_some_and(|s| s.len() > 1024 || s.contains('\0'))
        {
            return Err(invalid(
                "Invalid or duplicate repository ID, label, absolute local path or revision",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenWorkspace {
    manifest: WorkspaceSnapshot,
    repositories: BTreeMap<String, Arc<FrozenComparison>>,
}
impl FrozenWorkspace {
    pub fn manifest(&self) -> WorkspaceSnapshot {
        self.manifest.clone()
    }
    pub fn repository(&self, id: &str) -> Result<Arc<FrozenComparison>, RepositoryError> {
        self.repositories
            .get(id)
            .cloned()
            .ok_or_else(|| stale("Repository is unavailable in this workspace snapshot"))
    }
    pub fn basis(
        &self,
        repository_id: &str,
        file_id: &str,
    ) -> Result<EvidenceBasis, RepositoryError> {
        let repository = self.repository(repository_id)?;
        let file = repository.read_file(file_id)?;
        Ok(EvidenceBasis {
            workspace_snapshot_id: self.manifest.id.clone(),
            repository_id: repository_id.into(),
            repository_snapshot_id: repository.manifest(0, 0).id,
            file_id: file_id.into(),
            before_id: file.summary.before_id.clone(),
            after_id: file.summary.after_id.clone(),
            schema_version: SCHEMA_VERSION,
            rule_version: file_rule_version(&file).into(),
        })
    }
    fn validate_basis(&self, basis: &EvidenceBasis, current: bool) -> Result<(), RepositoryError> {
        let mut expected = self.basis(&basis.repository_id, &basis.file_id)?;
        if !current {
            // Historical conclusions keep their original rule version, but never become current.
            if basis.schema_version == 0
                || basis.schema_version > SCHEMA_VERSION
                || !matches!(
                    basis.rule_version.as_str(),
                    "syntax-v1" | "syntax-v2" | "metadata-v1"
                )
            {
                return Err(stale("Unknown historical review schema or rule"));
            }
            expected.schema_version = basis.schema_version;
            expected.rule_version = basis.rule_version.clone();
        }
        if &expected != basis {
            return Err(stale("Evidence basis does not match the frozen repository, file, content or analysis rule"));
        }
        Ok(())
    }
    fn validate(&self, workspace_id: &str) -> Result<(), RepositoryError> {
        if self.manifest.workspace_id != workspace_id
            || !hash_id(&self.manifest.id)
            || self.manifest.captured_at_ms.parse::<u128>().is_err()
            || self.manifest.repositories.is_empty()
            || self.manifest.repositories.len() > MAX_REPOSITORIES
        {
            return Err(invalid("Invalid frozen workspace manifest"));
        }
        let mut ids = BTreeSet::new();
        let mut available = 0;
        for entry in &self.manifest.repositories {
            if !safe_id(&entry.repository_id) || !ids.insert(&entry.repository_id) {
                return Err(invalid("Invalid frozen repository identity"));
            }
            match (&entry.manifest, &entry.error) {
                (Some(manifest), None) => {
                    let repository = self.repository(&entry.repository_id)?;
                    repository.validate_stored()?;
                    if json(manifest)? != json(&repository.manifest(0, 200))? {
                        return Err(invalid("Frozen repository manifest mismatch"));
                    }
                    available += 1;
                }
                (None, Some(_)) if !self.repositories.contains_key(&entry.repository_id) => (),
                _ => return Err(invalid("Invalid frozen repository availability")),
            }
        }
        if available != self.repositories.len() {
            return Err(invalid("Orphaned frozen repository"));
        }
        Ok(())
    }
}

pub fn capture_workspace(
    config: &WorkspaceConfig,
    cancel: &AtomicBool,
) -> Result<FrozenWorkspace, RepositoryError> {
    validate_config(config)?;
    let mut repositories = BTreeMap::new();
    let mut entries = Vec::new();
    let mut bytes = 0;
    for repository in &config.repositories {
        cancelled(cancel)?;
        let (manifest, error) = match capture(&repository.spec, cancel) {
            Ok(snapshot) => {
                bytes += json(&snapshot)?.len();
                if bytes > MAX_ARCHIVE {
                    return Err(budget("Workspace capture exceeds 64 MiB"));
                }
                let manifest = snapshot.manifest(0, 200);
                repositories.insert(repository.id.clone(), Arc::new(snapshot));
                (Some(manifest), None)
            }
            Err(error @ RepositoryError::Cancelled { .. }) => return Err(error),
            Err(error) => (None, Some(error)),
        };
        entries.push(RepositorySnapshot {
            repository_id: repository.id.clone(),
            label: repository.label.clone(),
            manifest,
            error,
        });
    }
    cancelled(cancel)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(invalid)?;
    let identity = format!(
        "{}:{}:{}:{}",
        config.id,
        now.as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    Ok(FrozenWorkspace {
        manifest: WorkspaceSnapshot {
            id: hash(identity.as_bytes()),
            workspace_id: config.id.clone(),
            captured_at_ms: now.as_millis().to_string(),
            repositories: entries,
        },
        repositories,
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    schema_version: u32,
    config: WorkspaceConfig,
    snapshots: Vec<FrozenWorkspace>,
    active_snapshot_id: Option<String>,
    relations: Vec<ManualRelation>,
    reviews: Vec<ReviewRecord>,
}
impl Archive {
    fn new(config: WorkspaceConfig) -> Self {
        Self {
            schema_version: 1,
            config,
            snapshots: vec![],
            active_snapshot_id: None,
            relations: vec![],
            reviews: vec![],
        }
    }
    fn session(&self) -> WorkspaceSession {
        WorkspaceSession {
            config: self.config.clone(),
            snapshots: self
                .snapshots
                .iter()
                .map(FrozenWorkspace::manifest)
                .collect(),
            active_snapshot_id: self.active_snapshot_id.clone(),
            relations: self.relations.clone(),
            reviews: self.reviews.clone(),
        }
    }
    fn snapshot(&self, id: &str) -> Result<&FrozenWorkspace, RepositoryError> {
        self.snapshots
            .iter()
            .find(|s| s.manifest.id == id)
            .ok_or_else(|| stale("Workspace snapshot is not retained"))
    }
    fn active(&self) -> Result<&FrozenWorkspace, RepositoryError> {
        self.snapshot(
            self.active_snapshot_id
                .as_deref()
                .ok_or_else(|| stale("Workspace has no snapshot"))?,
        )
    }
    fn validate(&self) -> Result<(), RepositoryError> {
        validate_config(&self.config)?;
        if self.schema_version != 1
            || self.snapshots.len() > MAX_SNAPSHOTS
            || self.relations.len() > MAX_RELATIONS
            || self.reviews.len() > MAX_REVIEWS
        {
            return Err(budget(
                "Unsupported workspace schema or archive count limit",
            ));
        }
        let mut ids = BTreeSet::new();
        for snapshot in &self.snapshots {
            if !ids.insert(&snapshot.manifest.id) {
                return Err(invalid("Duplicate workspace snapshot"));
            }
            snapshot.validate(&self.config.id)?;
        }
        if self.active_snapshot_id.is_some() {
            self.active()?;
        }
        for record in &self.reviews {
            self.snapshot(&record.basis.workspace_snapshot_id)?
                .validate_basis(&record.basis, false)?;
        }
        let mut relation_ids = BTreeSet::new();
        for relation in &self.relations {
            if !safe_id(&relation.id)
                || !relation_ids.insert(&relation.id)
                || relation.note.len() > 4096
                || relation.from == relation.to
                || relation.from.workspace_snapshot_id != relation.to.workspace_snapshot_id
            {
                return Err(invalid("Invalid manual relation"));
            }
            self.snapshot(&relation.from.workspace_snapshot_id)?
                .validate_basis(&relation.from, false)?;
            self.snapshot(&relation.to.workspace_snapshot_id)?
                .validate_basis(&relation.to, false)?;
        }
        Ok(())
    }
    fn trim(&mut self) -> Result<(), RepositoryError> {
        loop {
            if self.snapshots.len() <= MAX_SNAPSHOTS && json(self)?.len() <= MAX_ARCHIVE - 1024 {
                return Ok(());
            }
            let removable = self.snapshots.iter().position(|snapshot| {
                let id = &snapshot.manifest.id;
                self.active_snapshot_id.as_ref() != Some(id)
                    && !self
                        .reviews
                        .iter()
                        .any(|r| &r.basis.workspace_snapshot_id == id)
                    && !self.relations.iter().any(|r| {
                        &r.from.workspace_snapshot_id == id || &r.to.workspace_snapshot_id == id
                    })
            });
            let index = removable.ok_or_else(|| budget("Workspace evidence limit reached; referenced snapshots are retained. Create another workspace."))?;
            self.snapshots.remove(index);
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format_version: u32,
    checksum: String,
    payload: Archive,
}

pub struct WorkspaceStore {
    root: PathBuf,
}
impl WorkspaceStore {
    pub fn new(root: PathBuf) -> Result<Self, RepositoryError> {
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid(
                    "Workspace directory must not be a symlink or non-directory",
                ))
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(invalid(error)),
        }
        fs::create_dir_all(&root).map_err(invalid)?;
        let metadata = fs::symlink_metadata(&root).map_err(invalid)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("Workspace directory changed during creation"));
        }
        Ok(Self {
            root: fs::canonicalize(root).map_err(invalid)?,
        })
    }
    fn ensure_root(&self) -> Result<(), RepositoryError> {
        let metadata = fs::symlink_metadata(&self.root).map_err(invalid)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || fs::canonicalize(&self.root).map_err(invalid)? != self.root
        {
            return Err(invalid("Workspace directory was replaced or redirected"));
        }
        Ok(())
    }
    fn path(&self, id: &str) -> Result<PathBuf, RepositoryError> {
        self.ensure_root()?;
        if !safe_id(id) {
            return Err(invalid("Invalid workspace ID"));
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    fn lock(&self) -> Result<File, RepositoryError> {
        self.ensure_root()?;
        let path = self.root.join(".lock");
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err(invalid("Workspace lock must be a regular file"));
        }
        let file = private_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(invalid)?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(std::fs::TryLockError::WouldBlock)
                    if start.elapsed() < Duration::from_secs(2) =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => return Err(invalid(format!("Workspace store is busy: {error}"))),
            }
        }
    }
    fn read(&self, id: &str) -> Result<Archive, RepositoryError> {
        let path = self.path(id)?;
        let metadata = fs::symlink_metadata(&path).map_err(invalid)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_ARCHIVE as u64
        {
            return Err(budget("Workspace file is not regular or exceeds 64 MiB"));
        }
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(invalid)?
            .take(MAX_ARCHIVE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(invalid)?;
        if bytes.len() > MAX_ARCHIVE {
            return Err(budget("Workspace file grew past 64 MiB"));
        }
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(invalid)?;
        if envelope.format_version != 1
            || envelope.payload.config.id != id
            || hash(&json(&envelope.payload)?) != envelope.checksum
        {
            return Err(invalid("Workspace checksum, identity or format is invalid"));
        }
        envelope.payload.validate()?;
        Ok(envelope.payload)
    }
    fn write(&self, archive: &Archive) -> Result<(), RepositoryError> {
        archive.validate()?;
        let bytes = json(&Envelope {
            format_version: 1,
            checksum: hash(&json(archive)?),
            payload: Archive {
                schema_version: archive.schema_version,
                config: archive.config.clone(),
                snapshots: archive.snapshots.clone(),
                active_snapshot_id: archive.active_snapshot_id.clone(),
                relations: archive.relations.clone(),
                reviews: archive.reviews.clone(),
            },
        })?;
        if bytes.len() > MAX_ARCHIVE {
            return Err(budget("Workspace file exceeds 64 MiB"));
        }
        let destination = self.path(&archive.config.id)?;
        let files = self.managed_files()?;
        if !files.iter().any(|(id, _)| id == &archive.config.id) && files.len() >= MAX_WORKSPACES {
            return Err(budget("At most 16 workspaces are retained"));
        }
        let other_bytes: u64 = files
            .iter()
            .filter(|(id, _)| id != &archive.config.id)
            .map(|(_, size)| size)
            .sum();
        if other_bytes + bytes.len() as u64 > MAX_STORE {
            return Err(budget("Workspace store exceeds 256 MiB"));
        }
        // One fixed staging file bounds crash leftovers. The store lock excludes other writers.
        let temporary = self.root.join(".pending");
        match fs::symlink_metadata(&temporary) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                fs::remove_file(&temporary).map_err(invalid)?
            }
            Ok(_) => return Err(invalid("Workspace staging file must be regular")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(invalid(error)),
        }
        let result = (|| {
            let mut file = private_options()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(invalid)?;
            file.write_all(&bytes).map_err(invalid)?;
            file.sync_all().map_err(invalid)?;
            fs::rename(&temporary, &destination).map_err(invalid)?;
            #[cfg(unix)]
            File::open(&self.root)
                .and_then(|file| file.sync_all())
                .map_err(invalid)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
    fn managed_files(&self) -> Result<Vec<(String, u64)>, RepositoryError> {
        self.ensure_root()?;
        let mut files: Vec<(String, u64)> = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(invalid)? {
            let entry = entry.map_err(invalid)?;
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|s| s.strip_suffix(".json")) else {
                continue;
            };
            if !safe_id(id) {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(invalid)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(invalid("Managed workspace files must be regular files"));
            }
            files.push((id.into(), metadata.len()));
            if files.len() > MAX_WORKSPACES {
                return Err(budget("Workspace store contains too many files"));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }
    pub fn list(&self) -> Result<Vec<WorkspaceSummary>, RepositoryError> {
        self.managed_files()?
            .into_iter()
            .map(|(id, _)| match self.read(&id) {
                Ok(archive) => Ok(WorkspaceSummary {
                    id,
                    name: archive.config.name,
                    repository_count: archive.config.repositories.len() as u32,
                    snapshot_count: archive.snapshots.len() as u32,
                    error: None,
                }),
                Err(error) => Ok(WorkspaceSummary {
                    name: id.clone(),
                    id,
                    repository_count: 0,
                    snapshot_count: 0,
                    error: Some(error.to_string()),
                }),
            })
            .collect()
    }
    pub fn load(&self, id: &str) -> Result<WorkspaceSession, RepositoryError> {
        Ok(self.read(id)?.session())
    }
    pub fn save_config(
        &self,
        config: &WorkspaceConfig,
    ) -> Result<WorkspaceSession, RepositoryError> {
        validate_config(config)?;
        let _lock = self.lock()?;
        let mut archive = if self.path(&config.id)?.try_exists().map_err(invalid)? {
            self.read(&config.id)?
        } else {
            Archive::new(config.clone())
        };
        archive.config = config.clone();
        self.write(&archive)?;
        Ok(archive.session())
    }
    pub fn capture(
        &self,
        config: &WorkspaceConfig,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceSession, RepositoryError> {
        cancelled(cancel)?;
        self.save_config(config)?;
        let snapshot = capture_workspace(config, cancel)?;
        let _lock = self.lock()?;
        cancelled(cancel)?;
        let mut archive = self.read(&config.id)?;
        if json(&archive.config)? != json(config)? {
            return Err(stale("Workspace configuration changed during capture"));
        }
        archive.active_snapshot_id = Some(snapshot.manifest.id.clone());
        archive.snapshots.push(snapshot);
        archive.trim()?;
        self.write(&archive)?;
        Ok(archive.session())
    }
    pub fn restore(
        &self,
        id: &str,
        snapshot_id: Option<&str>,
    ) -> Result<FrozenWorkspace, RepositoryError> {
        let _lock = self.lock()?;
        let mut archive = self.read(id)?;
        let snapshot = if let Some(snapshot_id) = snapshot_id {
            archive.snapshot(snapshot_id)?
        } else {
            archive.active()?
        }
        .clone();
        if archive.active_snapshot_id.as_ref() != Some(&snapshot.manifest.id) {
            archive.active_snapshot_id = Some(snapshot.manifest.id.clone());
            self.write(&archive)?;
        }
        Ok(snapshot)
    }
    pub fn relocate(
        &self,
        id: &str,
        repository_id: &str,
        path: &str,
    ) -> Result<WorkspaceSession, RepositoryError> {
        let _lock = self.lock()?;
        let mut archive = self.read(id)?;
        let repository = archive
            .config
            .repositories
            .iter_mut()
            .find(|r| r.id == repository_id)
            .ok_or_else(|| stale("Unknown workspace repository"))?;
        repository.spec.repository = path.into();
        validate_config(&archive.config)?;
        self.write(&archive)?;
        Ok(archive.session())
    }
    pub fn set_review(
        &self,
        id: &str,
        basis: EvidenceBasis,
        state: ReviewState,
    ) -> Result<WorkspaceSession, RepositoryError> {
        let _lock = self.lock()?;
        let mut archive = self.read(id)?;
        archive.active()?.validate_basis(&basis, true)?;
        if let Some(record) = archive.reviews.iter_mut().find(|r| r.basis == basis) {
            record.state = state;
        } else {
            if archive.reviews.len() >= MAX_REVIEWS {
                return Err(budget("Review record limit reached"));
            }
            archive.reviews.push(ReviewRecord { basis, state });
        }
        self.write(&archive)?;
        Ok(archive.session())
    }
    pub fn add_relation(
        &self,
        id: &str,
        relation: ManualRelation,
    ) -> Result<WorkspaceSession, RepositoryError> {
        let _lock = self.lock()?;
        let mut archive = self.read(id)?;
        archive.active()?.validate_basis(&relation.from, true)?;
        archive.active()?.validate_basis(&relation.to, true)?;
        if let Some(existing) = archive.relations.iter_mut().find(|r| r.id == relation.id) {
            *existing = relation;
        } else {
            if archive.relations.len() >= MAX_RELATIONS {
                return Err(budget("Manual relation limit reached"));
            }
            archive.relations.push(relation);
        }
        self.write(&archive)?;
        Ok(archive.session())
    }
    pub fn export(&self, id: &str) -> Result<WorkspaceExport, RepositoryError> {
        let archive = self.read(id)?;
        let mut repositories = Vec::new();
        if let Ok(snapshot) = archive.active() {
            for entry in &snapshot.manifest.repositories {
                let manifest = snapshot
                    .repositories
                    .get(&entry.repository_id)
                    .map(|r| r.manifest(0, usize::MAX));
                repositories.push(ExportRepository {
                    repository_id: entry.repository_id.clone(),
                    snapshot_id: manifest.as_ref().map(|m| m.id.clone()),
                    mode: manifest.as_ref().map(|m| m.mode),
                    base_identity: manifest
                        .as_ref()
                        .and_then(|m| export_identity(&m.base_label)),
                    head_identity: manifest
                        .as_ref()
                        .and_then(|m| export_identity(&m.head_label)),
                    available: manifest.is_some(),
                    files: manifest
                        .map(|m| {
                            m.files
                                .into_iter()
                                .map(|f| ExportFile {
                                    file_id: f.id,
                                    before_id: f.before_id,
                                    after_id: f.after_id,
                                    status: f.status,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                });
            }
        }
        Ok(WorkspaceExport {
            schema_version: 1,
            workspace_id: id.into(),
            snapshot_id: archive.active_snapshot_id,
            repositories,
            relations: archive
                .relations
                .into_iter()
                .map(|r| ExportRelation {
                    id: r.id,
                    from: r.from,
                    to: r.to,
                })
                .collect(),
            reviews: archive.reviews,
        })
    }
}
fn private_options() -> OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = OpenOptions::new();
        options.mode(0o600);
        options
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
    }
}

fn export_identity(value: &str) -> Option<String> {
    let identity = value
        .strip_prefix("index:")
        .or_else(|| value.strip_prefix("worktree:"))
        .unwrap_or(value);
    if ([40, 64].contains(&identity.len()) && identity.bytes().all(|b| b.is_ascii_hexdigit()))
        || value == "unborn HEAD"
    {
        Some(value.into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_identity_is_checked_even_after_envelope_checksum_is_recomputed() {
        let content = "const value = 1;\n";
        let mut object = format!("blob {}\0", content.len()).into_bytes();
        object.extend_from_slice(content.as_bytes());
        let oid = format!("{:x}", sha1::Sha1::digest(object));
        let summary = FileSummary {
            id: "a".repeat(64),
            path: "a.ts".into(),
            old_path: None,
            status: FileStatus::Added,
            old_mode: None,
            new_mode: Some("100644".into()),
            before_id: None,
            after_id: Some(oid),
            notices: vec![],
        };
        let manifest = ComparisonManifest {
            id: "b".repeat(64),
            repository: "/unused".into(),
            mode: CompareMode::Worktree,
            base_label: "unborn HEAD".into(),
            head_label: "worktree:identity".into(),
            files: vec![summary.clone()],
            total_files: 1,
            warnings: vec![],
        };
        let mut files = BTreeMap::new();
        files.insert(
            summary.id.clone(),
            FrozenFile {
                summary,
                before: None,
                after: Some(content.into()),
                before_truncated: false,
                after_truncated: false,
            },
        );
        let valid = serde_json::json!({"manifest":manifest,"files":files});
        let snapshot: FrozenComparison = serde_json::from_value(valid.clone()).unwrap();
        assert!(snapshot.validate_stored().is_ok());
        let mut forged = valid;
        forged["files"]["a".repeat(64)]["after"] = "const value = 2;\n".into();
        let snapshot: FrozenComparison = serde_json::from_value(forged).unwrap();
        assert!(snapshot.validate_stored().is_err());
    }
    #[test]
    fn export_identity_is_an_allowlist_not_a_path_scrubber() {
        assert_eq!(
            export_identity(&format!("index:{}", "a".repeat(64))),
            Some(format!("index:{}", "a".repeat(64)))
        );
        for secret in [
            "/Users/private/repository",
            "https://token@host/repo",
            "worktree:/path",
            "branch-token",
        ] {
            assert!(export_identity(secret).is_none());
        }
    }
}
