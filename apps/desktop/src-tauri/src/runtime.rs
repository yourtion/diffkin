use diffkin_application::{DiskComparison, FrozenComparison, FrozenWorkspace};
use diffkin_domain::{ComparisonManifest, FileAnalysis, FrozenFile, RepositoryError};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, TryLockError,
    },
};

const CACHE_BYTES: usize = 32 * 1024 * 1024;
type CacheEntry = (String, String, Arc<FileAnalysis>, usize);

pub enum Snapshot {
    Memory(Arc<FrozenComparison>),
    Disk(DiskComparison),
}
impl From<Arc<FrozenComparison>> for Snapshot {
    fn from(value: Arc<FrozenComparison>) -> Self {
        Self::Memory(value)
    }
}
impl Snapshot {
    pub fn manifest(&self, offset: usize, limit: usize) -> ComparisonManifest {
        match self {
            Self::Memory(value) => value.manifest(offset, limit),
            Self::Disk(value) => value.manifest(offset, limit),
        }
    }
    pub fn read_file(&self, id: &str) -> Result<FrozenFile, RepositoryError> {
        match self {
            Self::Memory(value) => value.read_file(id),
            Self::Disk(value) => value.read_file(id),
        }
    }
}

#[derive(Default)]
pub struct Runtime {
    pub capture_gate: Mutex<()>,
    pub analysis_gate: Mutex<()>,
    previous: Mutex<Option<Arc<Snapshot>>>,
    pub current: Mutex<Option<Arc<Snapshot>>>,
    cache: Mutex<VecDeque<CacheEntry>>,
    jobs: Mutex<Jobs>,
    workspaces: Mutex<VecDeque<Arc<FrozenWorkspace>>>,
}
#[derive(Default)]
struct Jobs {
    shutting_down: bool,
    active: HashMap<String, Arc<AtomicBool>>,
    early_cancellations: VecDeque<String>,
}
impl Runtime {
    #[cfg(feature = "native-benchmark")]
    pub fn clear_benchmark_cache(&self) {
        self.cache.lock().unwrap().clear();
    }
    pub fn install_comparison(&self, snapshot: impl Into<Snapshot>) {
        let snapshot = Arc::new(snapshot.into());
        let mut current = self.current.lock().unwrap();
        *self.previous.lock().unwrap() = current.replace(snapshot);
    }
    pub fn begin(&self, id: &str) -> Result<Arc<AtomicBool>, RepositoryError> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.shutting_down {
            return Err(RepositoryError::Cancelled {
                message: "Application is shutting down".into(),
            });
        }
        if jobs
            .early_cancellations
            .iter()
            .any(|cancelled| cancelled == id)
        {
            return Err(RepositoryError::Cancelled {
                message: "Job was cancelled before it started".into(),
            });
        }
        if id.is_empty()
            || id.len() > 128
            || jobs.active.contains_key(id)
            || jobs.active.len() >= 16
        {
            return Err(RepositoryError::BudgetExceeded {
                message: "Too many pending jobs or duplicate job ID".into(),
            });
        }
        let token = Arc::new(AtomicBool::new(false));
        jobs.active.insert(id.into(), token.clone());
        Ok(token)
    }
    pub fn cancel(&self, id: &str) {
        if id.is_empty() || id.len() > 128 {
            return;
        }
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(token) = jobs.active.get(id) {
            token.store(true, Ordering::Relaxed);
        } else if !jobs
            .early_cancellations
            .iter()
            .any(|cancelled| cancelled == id)
        {
            if jobs.early_cancellations.len() >= 128 {
                jobs.early_cancellations.pop_front();
            }
            jobs.early_cancellations.push_back(id.into());
        }
    }
    pub fn shutdown(&self) {
        self.jobs.lock().unwrap().shutting_down = true;
        // Once the latch is visible no new job can enter; finishing existing jobs is safe.
        self.cancel_all();
    }
    /// Tauri exits the process directly, so managed state destructors are not enough.
    pub fn release_snapshots(&self) {
        self.current.lock().unwrap().take();
        self.previous.lock().unwrap().take();
        self.cache.lock().unwrap().clear();
        self.workspaces.lock().unwrap().clear();
    }
    pub fn cancel_all(&self) {
        for token in self.jobs.lock().unwrap().active.values() {
            token.store(true, Ordering::Relaxed);
        }
    }
    pub fn has_active_jobs(&self) -> bool {
        !self.jobs.lock().unwrap().active.is_empty()
    }
    pub fn finish(&self, id: &str) {
        self.jobs.lock().unwrap().active.remove(id);
    }
    /// Keep two independent frozen workspace captures alongside the single-repository view.
    pub fn install_workspace(&self, snapshot: Arc<FrozenWorkspace>) {
        let id = snapshot.manifest().id;
        let mut workspaces = self.workspaces.lock().unwrap();
        workspaces.retain(|item| item.manifest().id != id);
        if workspaces.len() >= 2 {
            workspaces.pop_front();
        }
        workspaces.push_back(snapshot);
    }
    pub fn workspace(&self, id: &str) -> Result<Arc<FrozenWorkspace>, RepositoryError> {
        self.workspaces
            .lock()
            .unwrap()
            .iter()
            .find(|workspace| workspace.manifest().id == id)
            .cloned()
            .ok_or_else(|| RepositoryError::Stale {
                message:
                    "Workspace snapshot is no longer in memory. Restore it from local history."
                        .into(),
            })
    }
    pub fn snapshot(&self, id: &str) -> Result<Arc<Snapshot>, RepositoryError> {
        if let Some(snapshot) = self
            .current
            .lock()
            .unwrap()
            .as_ref()
            .filter(|snapshot| snapshot.manifest(0, 0).id == id)
            .cloned()
        {
            return Ok(snapshot);
        }
        if let Some(snapshot) = self
            .previous
            .lock()
            .unwrap()
            .as_ref()
            .filter(|snapshot| snapshot.manifest(0, 0).id == id)
            .cloned()
        {
            return Ok(snapshot);
        }
        for workspace in self.workspaces.lock().unwrap().iter().rev() {
            for repository in workspace.manifest().repositories {
                if repository
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.id == id)
                {
                    return workspace
                        .repository(&repository.repository_id)
                        .map(|value| Arc::new(Snapshot::Memory(value)));
                }
            }
        }
        Err(RepositoryError::Stale { message: "This comparison is no longer open. Restore its workspace or refresh the repository.".into() })
    }
    pub fn cached(&self, id: &str, file_id: &str) -> Option<Arc<FileAnalysis>> {
        let mut cache = self.cache.lock().unwrap();
        let index = cache
            .iter()
            .position(|(snapshot, file, _, _)| snapshot == id && file == file_id)?;
        let entry = cache.remove(index)?;
        let result = entry.2.clone();
        cache.push_back(entry);
        Some(result)
    }
    pub fn remember(&self, id: String, file_id: String, result: FileAnalysis) -> Arc<FileAnalysis> {
        let result = Arc::new(result);
        // Charge twice serialized bytes to cover string/vector overhead. This is a cache
        // accounting bound, not a claim about allocator RSS or transient IPC clones.
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self.0.saturating_add(bytes.len());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut counter = Counter(0);
        if serde_json::to_writer(&mut counter, &result).is_err() {
            return result;
        }
        let charge = counter.0.saturating_mul(2);
        let mut cache = self.cache.lock().unwrap();
        cache.retain(|(snapshot, file, _, _)| snapshot != &id || file != &file_id);
        if charge > CACHE_BYTES {
            return result;
        }
        let mut bytes: usize = cache.iter().map(|entry| entry.3).sum();
        while cache.len() >= 64 || bytes + charge > CACHE_BYTES {
            if let Some(entry) = cache.pop_front() {
                bytes -= entry.3;
            } else {
                break;
            }
        }
        cache.push_back((id, file_id, result.clone(), charge));
        result
    }
}
pub fn permit<'a>(
    gate: &'a Mutex<()>,
    token: &AtomicBool,
) -> Result<MutexGuard<'a, ()>, RepositoryError> {
    loop {
        check_cancel(token)?;
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(2))
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(RepositoryError::IoFailure {
                    message: "A previous worker failed; reopen the application".into(),
                })
            }
        }
    }
}
pub fn check_cancel(token: &AtomicBool) -> Result<(), RepositoryError> {
    if token.load(Ordering::Relaxed) {
        Err(RepositoryError::Cancelled {
            message: "Comparison cancelled".into(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_scoped_and_jobs_are_released() {
        let runtime = Runtime::default();
        let first = runtime.begin("first").unwrap();
        let second = runtime.begin("second").unwrap();
        runtime.cancel("first");
        assert!(check_cancel(&first).is_err());
        assert!(check_cancel(&second).is_ok());
        assert!(runtime.begin("first").is_err());
        runtime.finish("first");
        assert!(runtime.begin("first").is_ok());
    }

    #[test]
    fn early_cancellation_and_waiting_jobs_are_cancellable() {
        let runtime = Runtime::default();
        runtime.cancel("not-started");
        assert!(matches!(
            runtime.begin("not-started"),
            Err(RepositoryError::Cancelled { .. })
        ));
        let token = runtime.begin("waiting").unwrap();
        let _held = runtime.analysis_gate.lock().unwrap();
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| permit(&runtime.analysis_gate, &token).is_err());
            std::thread::sleep(std::time::Duration::from_millis(10));
            let start = std::time::Instant::now();
            runtime.cancel("waiting");
            assert!(waiting.join().unwrap());
            assert!(start.elapsed() < std::time::Duration::from_millis(250));
        });
    }
    #[test]
    fn shutdown_cancels_all_jobs_but_waits_for_workers_to_finish() {
        let runtime = Runtime::default();
        assert!(!runtime.has_active_jobs());
        let first = runtime.begin("shutdown-first").unwrap();
        let second = runtime.begin("shutdown-second").unwrap();
        runtime.cancel_all();
        assert!(check_cancel(&first).is_err());
        assert!(check_cancel(&second).is_err());
        assert!(runtime.has_active_jobs());
        runtime.finish("shutdown-first");
        assert!(runtime.has_active_jobs());
        runtime.finish("shutdown-second");
        assert!(!runtime.has_active_jobs());
    }

    #[test]
    fn shutdown_latch_rejects_late_jobs_after_existing_workers_exit() {
        let runtime = Runtime::default();
        let active = runtime.begin("active").unwrap();
        runtime.shutdown();
        assert!(check_cancel(&active).is_err());
        assert!(matches!(
            runtime.begin("late"),
            Err(RepositoryError::Cancelled { .. })
        ));
        runtime.finish("active");
        assert!(!runtime.has_active_jobs());
        assert!(runtime.begin("after-finish").is_err());
    }

    #[test]
    fn exit_releases_current_and_previous_snapshot_owners() {
        let runtime = Runtime::default();
        let first = workspace_fixture("first", "first-file", "before")
            .repository("repo")
            .unwrap();
        let second = workspace_fixture("second", "second-file", "after")
            .repository("repo")
            .unwrap();
        let first_owner = Arc::downgrade(&first);
        let second_owner = Arc::downgrade(&second);
        runtime.install_comparison(first);
        runtime.install_comparison(second);
        assert!(first_owner.upgrade().is_some());
        assert!(second_owner.upgrade().is_some());
        runtime.release_snapshots();
        assert!(first_owner.upgrade().is_none());
        assert!(second_owner.upgrade().is_none());
    }

    fn workspace_fixture(id: &str, repository_id: &str, content: &str) -> Arc<FrozenWorkspace> {
        let summary = serde_json::json!({"id":repository_id,"path":"src/index.ts","oldPath":null,"status":"added","oldMode":null,"newMode":"100644","beforeId":null,"afterId":"a".repeat(40),"notices":[]});
        let manifest = serde_json::json!({"id":repository_id,"repository":"/unused","mode":"worktree","baseLabel":"unborn HEAD","headLabel":"worktree:test","files":[summary.clone()],"totalFiles":1,"warnings":[]});
        let file = serde_json::json!({"summary":summary,"before":null,"after":content,"beforeTruncated":false,"afterTruncated":false});
        let comparison =
            serde_json::json!({"manifest":manifest.clone(),"files":{repository_id:file}});
        Arc::new(serde_json::from_value(serde_json::json!({"manifest":{"id":id,"workspaceId":"project","capturedAtMs":"1","repositories":[{"repositoryId":"repo","label":"Repository","manifest":manifest,"error":null}]},"repositories":{"repo":comparison}})).unwrap())
    }
    #[test]
    fn cache_is_lru_byte_bounded_and_snapshot_scoped() {
        let runtime = Runtime::default();
        let file = workspace_fixture("a", "file", "text")
            .repository("repo")
            .unwrap()
            .read_file("file")
            .unwrap();
        let small = FileAnalysis {
            file,
            analysis: None,
            notice: None,
        };
        for i in 0..64 {
            runtime.remember("a".into(), i.to_string(), small.clone());
        }
        assert!(runtime.cached("b", "0").is_none());
        let hit = runtime.cached("a", "0").unwrap();
        assert!(Arc::ptr_eq(&hit, &runtime.cached("a", "0").unwrap()));
        runtime.remember("a".into(), "64".into(), small.clone());
        assert!(runtime.cached("a", "1").is_none());
        assert!(runtime.cached("a", "0").is_some());
        let mut large = small.clone();
        large.file.after = Some("x".repeat(1024 * 1024));
        for i in 0..30 {
            runtime.remember("b".into(), i.to_string(), large.clone());
        }
        let cache = runtime.cache.lock().unwrap();
        assert!(cache.iter().map(|entry| entry.3).sum::<usize>() <= CACHE_BYTES);
        assert!(cache.len() < 30);
        drop(cache);
        runtime.remember("b".into(), "29".into(), small);
        assert_eq!(
            runtime
                .cache
                .lock()
                .unwrap()
                .iter()
                .filter(|entry| entry.0 == "b" && entry.1 == "29")
                .count(),
            1
        );
        large.file.after = Some("x".repeat(CACHE_BYTES));
        runtime.remember("huge".into(), "0".into(), large);
        assert!(runtime.cached("huge", "0").is_none());
    }

    #[test]
    fn refresh_retains_previous_frozen_snapshot_and_evicts_older_capture() {
        let runtime = Runtime::default();
        for id in ["first", "second", "third"] {
            runtime.install_comparison(workspace_fixture(id, id, id).repository("repo").unwrap());
        }
        assert!(runtime.snapshot("first").is_err());
        assert_eq!(
            runtime
                .snapshot("second")
                .unwrap()
                .read_file("second")
                .unwrap()
                .after
                .as_deref(),
            Some("second")
        );
        assert!(runtime.snapshot("third").is_ok());
    }
    #[test]
    fn workspace_snapshots_and_single_repository_cache_coexist_without_path_aliasing() {
        let runtime = Runtime::default();
        let first = workspace_fixture("first", "repo-first", "first content");
        let second = workspace_fixture("second", "repo-second", "second content");
        let single = workspace_fixture("single", "repo-single", "single content");
        runtime.install_workspace(first.clone());
        runtime.install_workspace(second);
        runtime.install_comparison(single.repository("repo").unwrap());
        for (id, expected) in [
            ("repo-first", "first content"),
            ("repo-second", "second content"),
            ("repo-single", "single content"),
        ] {
            assert_eq!(
                runtime
                    .snapshot(id)
                    .unwrap()
                    .read_file(id)
                    .unwrap()
                    .after
                    .as_deref(),
                Some(expected)
            );
        }
        runtime.install_workspace(first);
        runtime.install_workspace(workspace_fixture("third", "repo-third", "third content"));
        assert!(runtime.workspace("first").is_ok());
        assert!(runtime.workspace("second").is_err());
        assert!(runtime.snapshot("repo-single").is_ok());
        assert!(runtime.snapshot("repo-second").is_err());
        assert!(runtime.snapshot("repo-third").is_ok());
    }
}
