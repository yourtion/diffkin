//! Atomic, session-local disk snapshots for fixed commit comparisons. Source reads
//! after publication never access Git or the working tree.
use super::*;
use std::{fs::OpenOptions, path::Path, sync::atomic::AtomicU64};

const MAX_PATHS: usize = 20_000;
const MAX_DISK_BYTES: u64 = 512 * 1024 * 1024;
const BATCH_BYTES: usize = 8 * 1024 * 1024;
const BATCH_PATHS: usize = 256;
const LARGE_CAPTURE_BUDGET: Duration = Duration::from_secs(120);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct DiskDirectory {
    path: PathBuf,
    lock: Option<File>,
    gc_guard: Option<File>,
}
impl DiskDirectory {
    fn create(root: &Path, context: &Context<'_>) -> Result<Self, RepositoryError> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(root).map_err(io_error)?;
        if fs::symlink_metadata(root)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(io_error(
                "Disk snapshot directory must not be a symbolic link",
            ));
        }
        // Serialize creation and abandoned-directory cleanup across app instances.
        let guard = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(".gc.lock"))
            .map_err(io_error)?;
        loop {
            context.check()?;
            match guard.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(2)),
                Err(error) => return Err(io_error(error)),
            }
        }
        for entry in fs::read_dir(root).map_err(io_error)? {
            context.check()?;
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let parts: Vec<_> = name.split('-').collect();
            if parts.len() != 4
                || parts[0] != "capture"
                || !parts[1..]
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                || !entry.file_type().map_err(io_error)?.is_dir()
            {
                continue;
            }
            let owner = OpenOptions::new()
                .read(true)
                .write(true)
                .open(entry.path().join(".owner"));
            match owner {
                Ok(lock) if lock.try_lock().is_ok() => {
                    drop(lock);
                    fs::remove_dir_all(entry.path()).map_err(io_error)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::remove_dir_all(entry.path()).map_err(io_error)?;
                }
                _ => (), // Never remove an active or inaccessible snapshot.
            }
        }
        let name = format!(
            "capture-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(io_error)?
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        );
        let path = root.join(name);
        builder.recursive(false).create(&path).map_err(io_error)?;
        let mut directory = Self {
            path,
            lock: None,
            gc_guard: None,
        };
        let lock = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(directory.path.join(".owner"))
            .map_err(io_error)?;
        lock.try_lock().map_err(io_error)?;
        directory.lock = Some(lock);
        guard.unlock().map_err(io_error)?;
        directory.gc_guard = Some(guard);
        Ok(directory)
    }
}
impl Drop for DiskDirectory {
    fn drop(&mut self) {
        // Serialize retirement with other instances' GC. Otherwise unlocking the
        // owner before deleting (needed on Windows) lets both remove the same tree.
        // Failed construction still holds create()'s local guard instead.
        if let Some(guard) = &self.gc_guard {
            let _ = guard.lock();
        }
        self.lock.take();
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct DiskComparison {
    manifest: ComparisonManifest,
    directory: DiskDirectory,
    records: BTreeMap<String, (u64, [u8; 32])>,
}
impl DiskComparison {
    pub fn manifest(&self, offset: usize, limit: usize) -> ComparisonManifest {
        ComparisonManifest {
            id: self.manifest.id.clone(),
            repository: self.manifest.repository.clone(),
            mode: self.manifest.mode,
            base_label: self.manifest.base_label.clone(),
            head_label: self.manifest.head_label.clone(),
            total_files: self.manifest.total_files,
            warnings: self.manifest.warnings.clone(),
            files: self
                .manifest
                .files
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect(),
        }
    }
    pub fn read_file(&self, id: &str) -> Result<FrozenFile, RepositoryError> {
        let (length, digest) = self
            .records
            .get(id)
            .ok_or_else(|| stale("File does not belong to this disk snapshot"))?;
        let mut bytes = Vec::new();
        File::open(self.directory.path.join(format!("{id}.json")))
            .map_err(io_error)?
            .take(length + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        let actual: [u8; 32] = Sha256::digest(&bytes).into();
        if bytes.len() as u64 != *length || actual != *digest {
            return Err(stale(
                "Frozen disk content changed or is damaged; capture again",
            ));
        }
        serde_json::from_slice(&bytes).map_err(io_error)
    }
}

/// Publish only after all bounded batches are frozen. Current/previous snapshots
/// own their directories; failed/cancelled captures clean themselves up on drop.
pub fn capture_large_commits(
    spec: &ComparisonSpec,
    root: &Path,
    cancel: &AtomicBool,
) -> Result<DiskComparison, RepositoryError> {
    capture_with_limit(spec, root, cancel, MAX_DISK_BYTES)
}

fn capture_with_limit(
    spec: &ComparisonSpec,
    root: &Path,
    cancel: &AtomicBool,
    disk_limit: u64,
) -> Result<DiskComparison, RepositoryError> {
    if spec.mode != CompareMode::Commits {
        return Err(RepositoryError::Unsupported { message: "Disk snapshots currently require two fixed commits; use a literal path scope for working-tree comparisons".into() });
    }
    let context = open_context(spec, cancel, Instant::now() + LARGE_CAPTURE_BUDGET)?;
    let base = context.resolve(spec.base.as_deref().unwrap_or("HEAD~1"))?;
    let head = context.resolve(spec.head.as_deref().unwrap_or("HEAD"))?;
    let (before, after, _) = raw_diff(&context, Some(&base), Some(&head))?;
    let paths: Vec<_> = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(path) != after.get(path))
        .collect();
    if paths.len() > MAX_PATHS {
        return Err(budget(
            "Disk comparison exceeds 20,000 changed paths; narrow the path scope",
        ));
    }
    let renames = exact_renames(&paths, &before, &after);
    let origins: BTreeSet<_> = renames.values().cloned().collect();
    // Resolve refs once. Every batch uses these same immutable object IDs.
    let fixed = ComparisonSpec {
        repository: context.root.to_string_lossy().into_owned(),
        base: Some(base.clone()),
        head: Some(head.clone()),
        ..spec.clone()
    };
    let oids: BTreeSet<_> = before
        .values()
        .chain(after.values())
        .filter(|entry| entry.mode != "160000")
        .map(|entry| entry.oid.clone())
        .collect();
    let mut sizes = BTreeMap::new();
    if !oids.is_empty() {
        let request = oids
            .iter()
            .map(|oid| format!("{oid}\n"))
            .collect::<String>();
        let checked = context.required(
            &[
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
            Some(request.as_bytes()),
        )?;
        for line in std::str::from_utf8(&checked).map_err(io_error)?.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() != 3 || fields[1] != "blob" || !oids.contains(fields[0]) {
                return Err(io_error("Unavailable disk snapshot blob"));
            }
            let length: usize = fields[2].parse().map_err(io_error)?;
            sizes.insert(
                fields[0].to_owned(),
                if length > MAX_FILE { 0 } else { length },
            );
        }
        if sizes.len() != oids.len() {
            return Err(io_error("Incomplete blob size listing"));
        }
    }
    let mut groups = Vec::new();
    let mut ranks = BTreeMap::new();
    for path in &paths {
        if origins.contains(path) {
            continue;
        }
        let old_path = renames.get(path).unwrap_or(path);
        ranks.insert(
            file_identity(path, old_path, before.get(old_path), after.get(path)),
            ranks.len(),
        );
        let mut group = vec![path.clone()];
        if old_path != path {
            group.push(old_path.clone());
        }
        let bytes = group
            .iter()
            .flat_map(|path| before.get(path).into_iter().chain(after.get(path)))
            .map(|entry| sizes.get(&entry.oid).copied().unwrap_or(0))
            .sum::<usize>();
        groups.push((group, bytes));
    }
    let directory = DiskDirectory::create(root, &context)?;
    let mut snapshot = DiskComparison {
        manifest: ComparisonManifest {
            id: String::new(),
            repository: fixed.repository.clone(),
            mode: CompareMode::Commits,
            base_label: base.clone(),
            head_label: head.clone(),
            total_files: 0,
            files: Vec::new(),
            warnings: Vec::new(),
        },
        directory,
        records: BTreeMap::new(),
    };
    let mut total_bytes = 0u64;
    let mut selected = BTreeSet::new();
    let mut selected_bytes = 0usize;
    let flush = |selected: &BTreeSet<Vec<u8>>,
                 snapshot: &mut DiskComparison,
                 total_bytes: &mut u64|
     -> Result<(), RepositoryError> {
        context.check()?;
        if selected.is_empty() {
            return Ok(());
        }
        let subset = CaptureSubset {
            paths: selected,
            renames: &renames,
        };
        let batch = capture_subset(&fixed, cancel, Some(&subset), context.deadline)?;
        for warning in batch.manifest.warnings {
            if !snapshot.manifest.warnings.contains(&warning) {
                snapshot.manifest.warnings.push(warning);
            }
        }
        for (id, file) in batch.files {
            context.check()?;
            let bytes = serde_json::to_vec(&file).map_err(io_error)?;
            *total_bytes += bytes.len() as u64;
            if *total_bytes > disk_limit {
                return Err(budget(
                    "Disk snapshot exceeds its 512 MiB content quota; narrow the path scope",
                ));
            }
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(snapshot.directory.path.join(format!("{id}.json")))
                .map_err(io_error)?;
            output.write_all(&bytes).map_err(io_error)?;
            snapshot
                .records
                .insert(id, (bytes.len() as u64, Sha256::digest(&bytes).into()));
            snapshot.manifest.files.push(file.summary);
        }
        Ok(())
    };
    for (group, bytes) in groups {
        if !selected.is_empty()
            && (selected.len() + group.len() > BATCH_PATHS || selected_bytes + bytes > BATCH_BYTES)
        {
            flush(&selected, &mut snapshot, &mut total_bytes)?;
            selected.clear();
            selected_bytes = 0;
        }
        selected.extend(group);
        selected_bytes += bytes;
    }
    flush(&selected, &mut snapshot, &mut total_bytes)?;
    context.check()?;
    snapshot
        .manifest
        .files
        .sort_by_key(|file| ranks.get(&file.id).copied().unwrap_or(usize::MAX));
    if snapshot.manifest.files.len() != ranks.len()
        || snapshot.records.len() != ranks.len()
        || snapshot
            .manifest
            .files
            .iter()
            .any(|file| !ranks.contains_key(&file.id))
    {
        return Err(io_error(
            "Disk snapshot catalog does not match the fixed comparison",
        ));
    }
    snapshot.manifest.total_files = snapshot.manifest.files.len() as u32;
    snapshot.manifest.id = comparison_identity(
        &context.root,
        spec.path_scope.as_ref(),
        &base,
        &head,
        &snapshot.manifest.files,
    );
    snapshot.manifest.warnings.push("大型提交比较使用临时磁盘冻结快照；源码按需读取，最多 20,000 个变化路径、512 MiB 内容、120 秒捕获。仅保留当前与上一次比较；正常退出清理，异常遗留在下次大型捕获时回收，不能作为持久归档。超过单文件预览上限的内容仍明确截断。".into());
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Fixture, ComparisonSpec, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "diffkin-disk-unit-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let repository = root.join("repo");
        fs::create_dir(&repository).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(&repository)
                .args([
                    "-c",
                    "core.hooksPath=",
                    "-c",
                    "user.name=Diffkin Test",
                    "-c",
                    "user.email=test@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{:?}", output.stderr);
        };
        git(&["init", "-q"]);
        fs::write(repository.join("source.ts"), "const value = 1;\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "before"]);
        fs::write(repository.join("source.ts"), "const value = 2;\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "after"]);
        let spec = ComparisonSpec {
            repository: repository.to_string_lossy().into_owned(),
            mode: CompareMode::Commits,
            base: None,
            head: None,
            path_scope: None,
            include_untracked: false,
            verify_all: false,
        };
        let cache = root.join("cache");
        (Fixture(root), spec, cache)
    }
    #[test]
    fn quota_failure_removes_partial_storage_and_other_modes_are_explicit() {
        let (_fixture, mut spec, root) = fixture();
        let result = capture_with_limit(&spec, &root, &AtomicBool::new(false), 1);
        assert!(matches!(
            result,
            Err(RepositoryError::BudgetExceeded { .. })
        ));
        assert!(!fs::read_dir(&root)
            .unwrap()
            .map(Result::unwrap)
            .any(|entry| entry.file_type().unwrap().is_dir()));
        spec.mode = CompareMode::Worktree;
        assert!(matches!(
            capture_large_commits(&spec, &root, &AtomicBool::new(false)),
            Err(RepositoryError::Unsupported { .. })
        ));
    }
    #[test]
    fn cleanup_reclaims_abandoned_storage_but_retains_live_and_unrelated_directories() {
        let (_fixture, spec, root) = fixture();
        let first = capture_large_commits(&spec, &root, &AtomicBool::new(false)).unwrap();
        let orphan = root.join("capture-1-2-3");
        fs::create_dir(&orphan).unwrap();
        fs::write(orphan.join(".owner"), "").unwrap();
        fs::write(orphan.join("partial.json"), "partial data").unwrap();
        let unrelated = root.join("capture-user-notes");
        fs::create_dir(&unrelated).unwrap();
        let second = capture_large_commits(&spec, &root, &AtomicBool::new(false)).unwrap();
        assert!(!orphan.exists());
        assert!(unrelated.exists());
        let id = &first.manifest(0, 1).files[0].id;
        assert!(first.read_file(id).is_ok());
        drop(second);
        assert!(first.read_file(id).is_ok());
    }

    #[test]
    fn retirement_waits_for_another_instances_cleanup_guard() {
        let (_fixture, spec, root) = fixture();
        let snapshot = capture_large_commits(&spec, &root, &AtomicBool::new(false)).unwrap();
        let directory = snapshot.directory.path.clone();
        let guard = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(".gc.lock"))
            .unwrap();
        guard.lock().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            started_tx.send(()).unwrap();
            drop(snapshot);
            done_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        let blocked = done_rx.recv_timeout(Duration::from_millis(50)).is_err();
        let retained = directory.exists();
        guard.unlock().unwrap();
        worker.join().unwrap();
        assert!(blocked && retained);
        assert!(!directory.exists());
    }
}
