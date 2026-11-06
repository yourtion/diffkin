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
        Arc, Mutex,
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
            revisions: (repository.spec.mode == CompareMode::Commits)
                .then(|| repository.spec.commit_revisions().map(str::to_owned)),
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
    fn summary(&self) -> WorkspaceSummary {
        WorkspaceSummary {
            id: self.config.id.clone(),
            name: self.config.name.clone(),
            repository_count: self.config.repositories.len() as u32,
            snapshot_count: self.snapshots.len() as u32,
            error: None,
        }
    }
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
    fn active_for_write(&self, expected: &str) -> Result<&FrozenWorkspace, RepositoryError> {
        if self.active_snapshot_id.as_deref() != Some(expected) {
            return Err(stale("Workspace active snapshot has changed"));
        }
        self.snapshot(expected)
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
        self.validate_reviews()?;
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
    fn validate_reviews(&self) -> Result<(), RepositoryError> {
        if self.reviews.len() > MAX_REVIEWS {
            return Err(budget("Review record limit reached"));
        }
        for record in &self.reviews {
            self.snapshot(&record.basis.workspace_snapshot_id)?
                .validate_basis(&record.basis, false)?;
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
struct Envelope<T> {
    format_version: u32,
    checksum: String,
    payload: T,
}

/// A v2 primary file replaces only review state. Its immutable v1 base contains all frozen evidence.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewOverlay {
    workspace_id: String,
    base_checksum: String,
    reviews: Vec<ReviewRecord>,
}

pub struct WorkspaceStore {
    root: PathBuf,
    summaries: Mutex<BTreeMap<String, (String, WorkspaceSummary)>>,
}
enum ConfigWrite {
    Save,
    IfChanged,
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
            summaries: Mutex::default(),
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
    fn read_bytes(&self, id: &str) -> Result<Vec<u8>, RepositoryError> {
        Self::read_regular(&self.path(id)?)
    }
    fn read_regular(path: &Path) -> Result<Vec<u8>, RepositoryError> {
        let metadata = fs::symlink_metadata(path).map_err(invalid)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_ARCHIVE as u64
        {
            return Err(budget("Workspace file is not regular or exceeds 64 MiB"));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .map_err(invalid)?
            .take(MAX_ARCHIVE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(invalid)?;
        if bytes.len() > MAX_ARCHIVE {
            return Err(budget("Workspace file grew past 64 MiB"));
        }
        Ok(bytes)
    }
    fn decode(id: &str, bytes: &[u8]) -> Result<Envelope<Archive>, RepositoryError> {
        let envelope: Envelope<Archive> = serde_json::from_slice(bytes).map_err(invalid)?;
        if envelope.format_version != 1
            || envelope.payload.config.id != id
            || hash(&json(&envelope.payload)?) != envelope.checksum
        {
            return Err(invalid("Workspace checksum, identity or format is invalid"));
        }
        envelope.payload.validate()?;
        Ok(envelope)
    }
    fn evidence(&self, id: &str, bytes: &[u8]) -> Result<Option<Vec<u8>>, RepositoryError> {
        #[derive(Deserialize)]
        struct Version {
            format_version: u32,
        }
        let version: Version = serde_json::from_slice(bytes).map_err(invalid)?;
        match version.format_version {
            1 => Ok(None),
            2 => {
                let evidence = Self::read_regular(&self.path(id)?.with_extension("evidence"))?;
                if bytes.len() + evidence.len() > MAX_ARCHIVE {
                    return Err(budget("Workspace evidence exceeds 64 MiB"));
                }
                Ok(Some(evidence))
            }
            _ => Err(invalid("Unknown workspace archive format")),
        }
    }
    fn unpack(
        id: &str,
        bytes: &[u8],
        evidence: Option<&[u8]>,
    ) -> Result<(Archive, String), RepositoryError> {
        let Some(evidence) = evidence else {
            let base = Self::decode(id, bytes)?;
            return Ok((base.payload, base.checksum));
        };
        let envelope: Envelope<&serde_json::value::RawValue> =
            serde_json::from_slice(bytes).map_err(invalid)?;
        if envelope.format_version != 2
            || hash(envelope.payload.get().as_bytes()) != envelope.checksum
        {
            return Err(invalid("Invalid workspace review envelope"));
        }
        let overlay: ReviewOverlay =
            serde_json::from_str(envelope.payload.get()).map_err(invalid)?;
        if overlay.workspace_id != id {
            return Err(invalid("Review state belongs to another workspace"));
        }
        let base = Self::decode(id, evidence)?;
        if base.checksum != overlay.base_checksum {
            return Err(invalid("Review state does not match its frozen evidence"));
        }
        let mut archive = base.payload;
        archive.reviews = overlay.reviews;
        archive.validate_reviews()?;
        Ok((archive, base.checksum))
    }
    fn read(&self, id: &str) -> Result<Archive, RepositoryError> {
        let bytes = self.read_bytes(id)?;
        let evidence = self.evidence(id, &bytes)?;
        Ok(Self::unpack(id, &bytes, evidence.as_deref())?.0)
    }
    fn write(&self, archive: &Archive) -> Result<(), RepositoryError> {
        archive.validate()?;
        let envelope = Envelope {
            format_version: 1,
            checksum: hash(&json(archive)?),
            payload: archive,
        };
        let bytes = json(&envelope)?;
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
        self.publish(&destination, &bytes)?;
        let evidence = destination.with_extension("evidence");
        match fs::remove_file(evidence) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(invalid(error)),
        }
        if let Ok(mut cache) = self.summaries.lock() {
            cache.retain(|id, _| files.iter().any(|(present, _)| present == id));
            cache.insert(archive.config.id.clone(), (hash(&bytes), archive.summary()));
        }
        Ok(())
    }
    fn publish(&self, destination: &Path, bytes: &[u8]) -> Result<(), RepositoryError> {
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
            file.write_all(bytes).map_err(invalid)?;
            file.sync_all().map_err(invalid)?;
            fs::rename(&temporary, destination).map_err(invalid)?;
            #[cfg(unix)]
            File::open(&self.root)
                .and_then(|file| file.sync_all())
                .map_err(invalid)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        Ok(())
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
            let evidence_size = match fs::symlink_metadata(entry.path().with_extension("evidence"))
            {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => meta.len(),
                Ok(_) => return Err(invalid("Workspace evidence must be a regular file")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(error) => return Err(invalid(error)),
            };
            files.push((id.into(), metadata.len().saturating_add(evidence_size)));
            if files.len() > MAX_WORKSPACES {
                return Err(budget("Workspace store contains too many files"));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }
    pub fn list(&self) -> Result<Vec<WorkspaceSummary>, RepositoryError> {
        let _lock = self.lock()?;
        let files = self.managed_files()?;
        let mut cache = self.summaries.lock().map_err(invalid)?;
        cache.retain(|id, _| files.iter().any(|(present, _)| present == id));
        let mut summaries = Vec::with_capacity(files.len());
        for (id, _) in files {
            let summary = self.read_bytes(&id).and_then(|bytes| {
                let evidence = self.evidence(&id, &bytes)?;
                let mut digest = hash(&bytes);
                if let Some(base) = &evidence {
                    digest.push_str(&hash(base));
                }
                if let Some((_, summary)) = cache.get(&id).filter(|(key, _)| key == &digest) {
                    return Ok(summary.clone());
                }
                let (archive, _) = Self::unpack(&id, &bytes, evidence.as_deref())?;
                let summary = archive.summary();
                cache.insert(id.clone(), (digest, summary.clone()));
                Ok(summary)
            });
            summaries.push(summary.unwrap_or_else(|error| WorkspaceSummary {
                name: id.clone(),
                id,
                repository_count: 0,
                snapshot_count: 0,
                error: Some(error.to_string()),
            }));
        }
        Ok(summaries)
    }
    pub fn load(&self, id: &str) -> Result<WorkspaceSession, RepositoryError> {
        let _lock = self.lock()?;
        Ok(self.read(id)?.session())
    }
    pub fn save_config(
        &self,
        config: &WorkspaceConfig,
    ) -> Result<WorkspaceSession, RepositoryError> {
        Ok(self.persist_config(config, ConfigWrite::Save)?.session())
    }
    fn persist_config(
        &self,
        config: &WorkspaceConfig,
        mode: ConfigWrite,
    ) -> Result<Archive, RepositoryError> {
        validate_config(config)?;
        let _lock = self.lock()?;
        let mut archive = if self.path(&config.id)?.try_exists().map_err(invalid)? {
            let archive = self.read(&config.id)?;
            if matches!(mode, ConfigWrite::IfChanged) && &archive.config == config {
                return Ok(archive);
            }
            archive
        } else {
            Archive::new(config.clone())
        };
        archive.config = config.clone();
        self.write(&archive)?;
        Ok(archive)
    }
    pub fn capture(
        &self,
        config: &WorkspaceConfig,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceSession, RepositoryError> {
        self.capture_and_open(config, cancel)
            .map(|(session, _)| session)
    }
    pub fn capture_and_open(
        &self,
        config: &WorkspaceConfig,
        cancel: &AtomicBool,
    ) -> Result<(WorkspaceSession, FrozenWorkspace), RepositoryError> {
        self.capture_from(config, None, cancel)
    }
    /// Refreshes an existing view when a snapshot ID is supplied. Setup may save a new config.
    pub fn capture_from(
        &self,
        config: &WorkspaceConfig,
        displayed: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<(WorkspaceSession, FrozenWorkspace), RepositoryError> {
        cancelled(cancel)?;
        let starting_snapshot = self.prepare_capture(config, displayed)?;
        let snapshot = capture_workspace(config, cancel)?;
        self.commit_capture(config, starting_snapshot.as_deref(), snapshot, cancel)
    }
    fn prepare_capture(
        &self,
        config: &WorkspaceConfig,
        displayed: Option<&str>,
    ) -> Result<Option<String>, RepositoryError> {
        if let Some(displayed) = displayed {
            let _lock = self.lock()?;
            let archive = self.read(&config.id)?;
            archive.active_for_write(displayed)?;
            if &archive.config != config {
                return Err(stale("Workspace configuration changed during capture"));
            }
            return Ok(archive.active_snapshot_id);
        }
        Ok(self
            .persist_config(config, ConfigWrite::IfChanged)?
            .active_snapshot_id)
    }
    fn commit_capture(
        &self,
        config: &WorkspaceConfig,
        starting_snapshot: Option<&str>,
        snapshot: FrozenWorkspace,
        cancel: &AtomicBool,
    ) -> Result<(WorkspaceSession, FrozenWorkspace), RepositoryError> {
        let _lock = self.lock()?;
        cancelled(cancel)?;
        let mut archive = self.read(&config.id)?;
        if archive.active_snapshot_id.as_deref() != starting_snapshot {
            return Err(stale("Workspace active snapshot has changed"));
        }
        if &archive.config != config {
            return Err(stale("Workspace configuration changed during capture"));
        }
        archive.active_snapshot_id = Some(snapshot.manifest.id.clone());
        archive.snapshots.push(snapshot);
        archive.trim()?;
        self.write(&archive)?;
        let session = archive.session();
        // trim always retains the active snapshot at the end.
        let frozen = archive
            .snapshots
            .pop()
            .ok_or_else(|| stale("Workspace has no snapshot"))?;
        Ok((session, frozen))
    }
    pub fn restore(
        &self,
        id: &str,
        snapshot_id: Option<&str>,
    ) -> Result<FrozenWorkspace, RepositoryError> {
        Ok(self.archive_at(id, snapshot_id)?.active()?.clone())
    }
    pub fn open_session(
        &self,
        id: &str,
        snapshot_id: Option<&str>,
    ) -> Result<(WorkspaceSession, Option<FrozenWorkspace>), RepositoryError> {
        let archive = self.archive_at(id, snapshot_id)?;
        let session = archive.session();
        let frozen = archive
            .snapshots
            .into_iter()
            .find(|snapshot| Some(&snapshot.manifest.id) == session.active_snapshot_id.as_ref());
        Ok((session, frozen))
    }
    fn archive_at(&self, id: &str, snapshot_id: Option<&str>) -> Result<Archive, RepositoryError> {
        let _lock = self.lock()?;
        let mut archive = self.read(id)?;
        if let Some(requested) = snapshot_id {
            if archive.active_snapshot_id.as_deref() != Some(requested) {
                archive.snapshot(requested)?;
                archive.active_snapshot_id = Some(requested.to_owned());
                self.write(&archive)?;
            }
        }
        Ok(archive)
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
        let bytes = self.read_bytes(id)?;
        let base = self.evidence(id, &bytes)?;
        let (mut archive, base_checksum) = Self::unpack(id, &bytes, base.as_deref())?;
        archive
            .active_for_write(&basis.workspace_snapshot_id)?
            .validate_basis(&basis, true)?;
        if let Some(record) = archive.reviews.iter_mut().find(|r| r.basis == basis) {
            if record.state == state {
                return Ok(archive.session());
            }
            record.state = state;
        } else {
            if archive.reviews.len() >= MAX_REVIEWS {
                return Err(budget("Review record limit reached"));
            }
            archive.reviews.push(ReviewRecord { basis, state });
        }
        let session = archive.session();
        let overlay = ReviewOverlay {
            workspace_id: id.into(),
            base_checksum,
            reviews: archive.reviews,
        };
        let header = json(&Envelope {
            format_version: 2,
            checksum: hash(&json(&overlay)?),
            payload: &overlay,
        })?;
        let primary = self.path(id)?;
        let evidence = primary.with_extension("evidence");
        let evidence_len = base.as_ref().unwrap_or(&bytes).len() as u64;
        let size = evidence_len + header.len() as u64;
        let others: u64 = self
            .managed_files()?
            .iter()
            .filter(|(name, _)| name != id)
            .map(|(_, size)| size)
            .sum();
        if size > MAX_ARCHIVE as u64 || size + others > MAX_STORE {
            // Embedded historical reviews may occupy space in both parts. A full rewrite
            // compacts them and enforces the original archive and store budgets.
            archive.reviews = overlay.reviews;
            self.write(&archive)?;
            return Ok(session);
        }
        if base.is_none() {
            // Pin the existing, fully verified archive before atomically publishing the small primary.
            // A crash between the two renames leaves a valid v1 primary.
            self.publish(&evidence, &bytes)?;
        }
        self.publish(&primary, &header)?;
        Ok(session)
    }
    pub fn add_relation(
        &self,
        id: &str,
        relation: ManualRelation,
    ) -> Result<WorkspaceSession, RepositoryError> {
        let _lock = self.lock()?;
        let archive = self.read(id)?;
        archive.active()?.validate_basis(&relation.from, true)?;
        archive.active()?.validate_basis(&relation.to, true)?;
        self.persist_relation(archive, relation)
    }
    pub fn link_files(
        &self,
        id: &str,
        draft: WorkspaceRelationDraft,
    ) -> Result<WorkspaceSession, RepositoryError> {
        let _lock = self.lock()?;
        let archive = self.read(id)?;
        let frozen = archive.active_for_write(&draft.workspace_snapshot_id)?;
        let relation = ManualRelation {
            id: draft.id,
            from: frozen.basis(&draft.from.repository_id, &draft.from.file_id)?,
            to: frozen.basis(&draft.to.repository_id, &draft.to.file_id)?,
            note: draft.note,
        };
        self.persist_relation(archive, relation)
    }
    fn persist_relation(
        &self,
        mut archive: Archive,
        relation: ManualRelation,
    ) -> Result<WorkspaceSession, RepositoryError> {
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
        self.export_at(id, None)
    }
    pub fn export_at(
        &self,
        id: &str,
        snapshot_id: Option<&str>,
    ) -> Result<WorkspaceExport, RepositoryError> {
        let _lock = self.lock()?;
        let archive = self.read(id)?;
        let mut repositories = Vec::new();
        let snapshot_id = snapshot_id.or(archive.active_snapshot_id.as_deref());
        if let Some(requested) = snapshot_id {
            let snapshot = archive.snapshot(requested)?;
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
            snapshot_id: snapshot_id.map(str::to_owned),
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
    fn refresh_writes_only_at_commit_and_rejects_an_obsolete_capture() {
        let root = std::env::temp_dir().join(format!(
            "diffkin-refresh-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = WorkspaceStore::new(root.clone()).unwrap();
        let other = WorkspaceStore::new(root.clone()).unwrap();
        let config = WorkspaceConfig {
            id: "project".into(),
            name: "Draft".into(),
            repositories: vec![WorkspaceRepository {
                id: "repo".into(),
                label: "Repo".into(),
                spec: ComparisonSpec {
                    repository: root
                        .join("missing-repository")
                        .to_string_lossy()
                        .into_owned(),
                    mode: CompareMode::Worktree,
                    base: None,
                    head: None,
                    verify_all: false,
                    include_untracked: false,
                    path_scope: None,
                },
            }],
        };
        let cancel = AtomicBool::new(false);
        let first = store
            .capture(&config, &cancel)
            .unwrap()
            .active_snapshot_id
            .unwrap();
        let path = store.path("project").unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
            .unwrap();
        let stamp = file.metadata().unwrap().modified().unwrap();
        drop(file);
        assert_eq!(
            store.prepare_capture(&config, None).unwrap().as_deref(),
            Some(first.as_str())
        );
        let starting = store.prepare_capture(&config, Some(&first)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), stamp);
        let pending = capture_workspace(&config, &cancel).unwrap();
        // A second instance completes between the first instance's Git read and its commit.
        let (winner, _) = other.capture_from(&config, Some(&first), &cancel).unwrap();
        assert!(matches!(
            store.commit_capture(&config, starting.as_deref(), pending, &cancel),
            Err(RepositoryError::Stale { .. })
        ));
        assert!(matches!(
            store.capture_from(&config, Some(&first), &cancel),
            Err(RepositoryError::Stale { .. })
        ));
        let starting = store
            .prepare_capture(&config, winner.active_snapshot_id.as_deref())
            .unwrap();
        let pending = capture_workspace(&config, &cancel).unwrap();
        let mut renamed = config.clone();
        renamed.name = "Other instance".into();
        other.save_config(&renamed).unwrap();
        assert!(matches!(
            store.commit_capture(&config, starting.as_deref(), pending, &cancel),
            Err(RepositoryError::Stale { .. })
        ));
        assert!(matches!(
            store.capture_from(&config, winner.active_snapshot_id.as_deref(), &cancel),
            Err(RepositoryError::Stale { .. })
        ));
        let saved = store.load("project").unwrap();
        assert_eq!(saved.config, renamed);
        assert_eq!(saved.active_snapshot_id, winner.active_snapshot_id);
        assert_eq!(saved.snapshots.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
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
