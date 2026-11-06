use crate::{internal, request_file, Shared};
use diffkin_domain::{
    ExplanationRequest, ExplanationResult, PreparedExplanation, PreparedQuiz, QuizRequest,
    QuizResult, RepositoryError,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::State;

#[derive(Default)]
pub struct Explanations {
    next: AtomicU64,
    previews: Mutex<VecDeque<PreparedExplanation>>,
    quizzes: Mutex<VecDeque<PreparedQuiz>>,
    gate: Arc<Mutex<()>>,
}

#[tauri::command]
pub async fn explanation_prepare(
    comparison_id: String,
    file_id: String,
    event_id: Option<String>,
    raw_start: Option<usize>,
    job_id: String,
    state: State<'_, Shared>,
    explanations: State<'_, Explanations>,
) -> Result<PreparedExplanation, RepositoryError> {
    let file = request_file(comparison_id.clone(), file_id, job_id, state).await?;
    let context =
        diffkin_explanation::context(&comparison_id, &file, event_id.as_deref(), raw_start)?;
    let prepared = PreparedExplanation {
        id: explanations
            .next
            .fetch_add(1, Ordering::Relaxed)
            .to_string(),
        context,
        tools: diffkin_explanation::tools(),
    };
    let mut previews = explanations.previews.lock().unwrap();
    if previews.len() == 8 {
        previews.pop_front();
    }
    previews.push_back(prepared.clone());
    Ok(prepared)
}

#[tauri::command]
pub async fn explanation_run(
    request: ExplanationRequest,
    job_id: String,
    state: State<'_, Shared>,
    explanations: State<'_, Explanations>,
) -> Result<ExplanationResult, RepositoryError> {
    let prepared = explanations
        .previews
        .lock()
        .unwrap()
        .iter()
        .find(|p| p.id == request.prepared_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Stale {
            message: "解释预览已过期，请关闭面板后重新打开".into(),
        })?;
    run_model_job(
        state.inner(),
        explanations.gate.clone(),
        job_id,
        move |token| diffkin_explanation::explain(prepared, &request, token),
    )
    .await
}

#[tauri::command]
pub async fn quiz_prepare(
    comparison_id: String,
    file_id: String,
    raw_start: Option<usize>,
    job_id: String,
    state: State<'_, Shared>,
    explanations: State<'_, Explanations>,
) -> Result<PreparedQuiz, RepositoryError> {
    let file = request_file(comparison_id, file_id, job_id, state).await?;
    let mut builder = diffkin_explanation::quiz::QuizBuilder::default();
    builder.add_file(&file, raw_start)?;
    let id = explanations
        .next
        .fetch_add(1, Ordering::Relaxed)
        .to_string();
    let prepared = builder.finish(id, diffkin_explanation::tools())?;
    let mut quizzes = explanations.quizzes.lock().unwrap();
    if quizzes.len() == 8 {
        quizzes.pop_front();
    }
    quizzes.push_back(prepared.clone());
    Ok(prepared)
}

#[tauri::command]
pub async fn quiz_generate(
    request: QuizRequest,
    job_id: String,
    state: State<'_, Shared>,
    explanations: State<'_, Explanations>,
) -> Result<QuizResult, RepositoryError> {
    let prepared = explanations
        .quizzes
        .lock()
        .unwrap()
        .iter()
        .find(|p| p.id == request.prepared_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Stale {
            message: "测验预览已过期，请重新选择范围".into(),
        })?;
    run_model_job(
        state.inner(),
        explanations.gate.clone(),
        job_id,
        move |token| diffkin_explanation::quiz::generate_quiz(prepared, &request, token),
    )
    .await
}

async fn run_model_job<T: Send + 'static>(
    runtime: &Shared,
    gate: Arc<Mutex<()>>,
    job_id: String,
    work: impl FnOnce(&AtomicBool) -> Result<T, RepositoryError> + Send + 'static,
) -> Result<T, RepositoryError> {
    let token = runtime.begin(&job_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _permit = crate::permit(&gate, &token)?;
        work(&token)
    })
    .await
    .map_err(internal);
    runtime.finish(&job_id);
    result?
}
