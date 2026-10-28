use diffkin_application::FrozenComparison;
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
}
#[derive(Default)]
struct Jobs {
    active: HashMap<String, Arc<AtomicBool>>,
    early_cancellations: VecDeque<String>,
}
impl Runtime {
    pub fn begin(&self, id: &str) -> Result<Arc<AtomicBool>, RepositoryError> {
        let mut jobs = self.jobs.lock().unwrap();
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
    pub fn finish(&self, id: &str) {
        self.jobs.lock().unwrap().active.remove(id);
    }
    pub fn snapshot(&self, id: &str) -> Result<Arc<FrozenComparison>, RepositoryError> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .filter(|snapshot| snapshot.manifest(0, 0).id == id)
            .cloned()
            .ok_or_else(|| RepositoryError::Stale {
                message: "This comparison is no longer open. Refresh the repository.".into(),
            })
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
}
