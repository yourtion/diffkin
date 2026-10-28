use crate::{runtime::permit, Shared};
use diffkin_domain::*;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};

const LOG_BYTES: usize = 256 * 1024;
#[derive(Default)]
pub struct ValidationRuntime {
    gate: Mutex<()>,
    logs: Mutex<VecDeque<LogBuffer>>,
    recovery_error: Mutex<Option<String>>,
}
struct LogBuffer {
    job: String,
    entries: VecDeque<ValidationLog>,
    bytes: usize,
    truncated: bool,
    active: bool,
}
type ValidationShared = Arc<ValidationRuntime>;
fn error(error: impl std::fmt::Display) -> ValidationError {
    ValidationError::Io {
        message: error.to_string(),
    }
}
fn job_error(error: RepositoryError) -> ValidationError {
    match error {
        RepositoryError::Cancelled { message } => ValidationError::Cancelled { message },
        other => ValidationError::Invalid {
            message: other.to_string(),
        },
    }
}
fn location(app: &tauri::AppHandle) -> Result<PathBuf, ValidationError> {
    Ok(app.path().app_data_dir().map_err(error)?.join("validation"))
}
impl ValidationRuntime {
    fn ready(&self) -> Result<(), ValidationError> {
        if let Some(message) = self.recovery_error.lock().map_err(error)?.as_ref() {
            return Err(ValidationError::Io {
                message: message.clone(),
            });
        }
        Ok(())
    }
    fn begin_logs(&self, job: &str) {
        let mut logs = self.logs.lock().unwrap();
        logs.retain(|item| item.job != job);
        while logs.len() >= 16 {
            if let Some(index) = logs.iter().position(|item| !item.active) {
                logs.remove(index);
            } else {
                break;
            }
        }
        logs.push_back(LogBuffer {
            job: job.into(),
            entries: VecDeque::new(),
            bytes: 0,
            truncated: false,
            active: true,
        });
    }
    fn push(&self, job: &str, entry: &ValidationLog) {
        let mut logs = self.logs.lock().unwrap();
        if let Some(buffer) = logs.iter_mut().find(|item| item.job == job) {
            let mut entry = entry.clone();
            if entry.text.len() > 64 * 1024 {
                let mut end = 64 * 1024;
                while !entry.text.is_char_boundary(end) {
                    end -= 1;
                }
                entry.text.truncate(end);
                buffer.truncated = true;
            }
            buffer.bytes += entry.text.len();
            buffer.entries.push_back(entry);
            while buffer.bytes > LOG_BYTES || buffer.entries.len() > 512 {
                if let Some(removed) = buffer.entries.pop_front() {
                    buffer.bytes -= removed.text.len();
                    buffer.truncated = true;
                }
            }
        }
    }
    fn finish_logs(&self, job: &str) {
        let mut logs = self.logs.lock().unwrap();
        if let Some(buffer) = logs.iter_mut().find(|item| item.job == job) {
            buffer.active = false;
        }
        while logs.len() > 8 {
            if let Some(index) = logs.iter().position(|item| !item.active) {
                logs.remove(index);
            } else {
                break;
            }
        }
    }
    fn page(&self, job: &str, after: u32) -> Result<ValidationLogPage, ValidationError> {
        let logs = self.logs.lock().map_err(error)?;
        let buffer =
            logs.iter()
                .find(|item| item.job == job)
                .ok_or_else(|| ValidationError::Invalid {
                    message:
                        "Log buffer is no longer retained; load the persisted validation result"
                            .into(),
                })?;
        let mut bytes = 0;
        let entries: Vec<_> = buffer
            .entries
            .iter()
            .filter(|entry| entry.sequence > after)
            .take(32)
            .take_while(|entry| {
                bytes += entry.text.len();
                bytes <= 64 * 1024
            })
            .cloned()
            .collect();
        Ok(ValidationLogPage {
            next_sequence: entries.last().map_or(after, |entry| entry.sequence),
            truncated: buffer.truncated,
            entries,
        })
    }
}
pub fn recover(app: &tauri::AppHandle) {
    let runtime = app.state::<ValidationShared>();
    if let Err(cause) =
        location(app).and_then(|path| diffkin_validation::recover_interrupted(&path).map(|_| ()))
    {
        *runtime.recovery_error.lock().unwrap() =
            Some(format!("Validation recovery failed: {cause:?}"));
    }
}
#[tauri::command]
pub async fn validation_prepare(
    spec: ValidationSpec,
    job_id: String,
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    scope: State<'_, ValidationShared>,
) -> Result<ValidationPlan, ValidationError> {
    scope.ready()?;
    let path = location(&app)?;
    let runtime = state.inner().clone();
    let cancel = runtime.begin(&job_id).map_err(job_error)?;
    let guard = scope.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit(&guard.gate, &cancel).map_err(job_error)?;
        diffkin_validation::prepare(&spec, &path, &cancel)
    })
    .await
    .map_err(error);
    runtime.finish(&job_id);
    result?
}
#[tauri::command]
pub async fn validation_run(
    plan_id: String,
    target: ValidationTarget,
    job_id: String,
    app: tauri::AppHandle,
    state: State<'_, Shared>,
    scope: State<'_, ValidationShared>,
) -> Result<ValidationResult, ValidationError> {
    scope.ready()?;
    let path = location(&app)?;
    let runtime = state.inner().clone();
    let cancel = runtime.begin(&job_id).map_err(job_error)?;
    let guard = scope.inner().clone();
    let worker_job = job_id.clone();
    scope.begin_logs(&job_id);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit(&guard.gate, &cancel).map_err(job_error)?;
        diffkin_validation::run(&path, &plan_id, target, &cancel, |entry| {
            guard.push(&worker_job, entry)
        })
    })
    .await
    .map_err(error);
    scope.finish_logs(&job_id);
    runtime.finish(&job_id);
    result?
}
#[tauri::command]
pub fn validation_logs(
    job_id: String,
    after_sequence: u32,
    scope: State<'_, ValidationShared>,
) -> Result<ValidationLogPage, ValidationError> {
    scope.ready()?;
    scope.page(&job_id, after_sequence)
}
#[tauri::command]
pub async fn validation_history(
    limit: u32,
    app: tauri::AppHandle,
    scope: State<'_, ValidationShared>,
) -> Result<Vec<ValidationResult>, ValidationError> {
    scope.ready()?;
    let path = location(&app)?;
    tauri::async_runtime::spawn_blocking(move || diffkin_validation::history(&path, limit.min(100)))
        .await
        .map_err(error)?
}
#[tauri::command]
pub async fn validation_plan(
    plan_id: String,
    app: tauri::AppHandle,
    scope: State<'_, ValidationShared>,
) -> Result<ValidationPlan, ValidationError> {
    scope.ready()?;
    let path = location(&app)?;
    tauri::async_runtime::spawn_blocking(move || diffkin_validation::load_plan(&path, &plan_id))
        .await
        .map_err(error)?
}
#[tauri::command]
pub async fn validation_result(
    id: String,
    app: tauri::AppHandle,
    scope: State<'_, ValidationShared>,
) -> Result<ValidationResult, ValidationError> {
    scope.ready()?;
    let path = location(&app)?;
    tauri::async_runtime::spawn_blocking(move || diffkin_validation::load_result(&path, &id))
        .await
        .map_err(error)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_paging_is_bounded_and_reports_truncation() {
        let state = ValidationRuntime::default();
        state.begin_logs("run");
        for sequence in 1..=2000 {
            state.push(
                "run",
                &ValidationLog {
                    sequence,
                    stream: "stdout".into(),
                    text: "中".repeat(1024),
                },
            );
        }
        let page = state.page("run", 0).unwrap();
        assert!(page.truncated);
        assert!(page.entries.len() <= 32);
        assert!(page.entries.iter().map(|e| e.text.len()).sum::<usize>() <= 64 * 1024);
        assert!(page.next_sequence > 1);
        let next = state.page("run", page.next_sequence).unwrap();
        assert!(next.entries.iter().all(|e| e.sequence > page.next_sequence));
        state.finish_logs("run");
        for i in 0..20 {
            let id = i.to_string();
            state.begin_logs(&id);
            state.finish_logs(&id);
        }
        assert!(state.page("run", 0).is_err());
        assert_eq!(state.logs.lock().unwrap().len(), 8);
    }
}
