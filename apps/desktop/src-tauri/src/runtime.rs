use diffkin_application::{FrozenComparison, FrozenWorkspace};
use diffkin_domain::{FileAnalysis, RepositoryError};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, TryLockError,
    },
};

#[derive(Default)]
pub struct Runtime {
    pub capture_gate: Mutex<()>,
    pub analysis_gate: Mutex<()>,
    pub current: Mutex<Option<Arc<FrozenComparison>>>,
    pub cache: Mutex<VecDeque<(String, String, FileAnalysis)>>,
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
    pub fn snapshot(&self, id: &str) -> Result<Arc<FrozenComparison>, RepositoryError> {
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
        for workspace in self.workspaces.lock().unwrap().iter().rev() {
            for repository in workspace.manifest().repositories {
                if repository
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.id == id)
                {
                    return workspace.repository(&repository.repository_id);
                }
            }
        }
        Err(RepositoryError::Stale { message: "This comparison is no longer open. Restore its workspace or refresh the repository.".into() })
    }
    pub fn cached(&self, id: &str, file_id: &str) -> Option<FileAnalysis> {
        self.cache
            .lock()
            .unwrap()
            .iter()
            .find(|(snapshot, file, _)| snapshot == id && file == file_id)
            .map(|(_, _, result)| result.clone())
    }
    pub fn remember(&self, id: String, file_id: String, result: FileAnalysis) {
        let mut cache = self.cache.lock().unwrap();
        // Each analyzed pair is bounded by 200 KB per side. Retain at most 8 results.
        if cache.len() >= 8 {
            cache.pop_front();
        }
        if result.analysis.is_some() {
            cache.push_back((id, file_id, result));
        }
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

    fn workspace_fixture(id: &str, repository_id: &str, content: &str) -> Arc<FrozenWorkspace> {
        let summary = serde_json::json!({"id":repository_id,"path":"src/index.ts","oldPath":null,"status":"added","oldMode":null,"newMode":"100644","beforeId":null,"afterId":"a".repeat(40),"notices":[]});
        let manifest = serde_json::json!({"id":repository_id,"repository":"/unused","mode":"worktree","baseLabel":"unborn HEAD","headLabel":"worktree:test","files":[summary.clone()],"totalFiles":1,"warnings":[]});
        let file = serde_json::json!({"summary":summary,"before":null,"after":content,"beforeTruncated":false,"afterTruncated":false});
        let comparison =
            serde_json::json!({"manifest":manifest.clone(),"files":{repository_id:file}});
        Arc::new(serde_json::from_value(serde_json::json!({"manifest":{"id":id,"workspaceId":"project","capturedAtMs":"1","repositories":[{"repositoryId":"repo","label":"Repository","manifest":manifest,"error":null}]},"repositories":{"repo":comparison}})).unwrap())
    }
    #[test]
    fn workspace_snapshots_and_single_repository_cache_coexist_without_path_aliasing() {
        let runtime = Runtime::default();
        let first = workspace_fixture("first", "repo-first", "first content");
        let second = workspace_fixture("second", "repo-second", "second content");
        let single = workspace_fixture("single", "repo-single", "single content");
        runtime.install_workspace(first.clone());
        runtime.install_workspace(second);
        *runtime.current.lock().unwrap() = Some(single.repository("repo").unwrap());
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
