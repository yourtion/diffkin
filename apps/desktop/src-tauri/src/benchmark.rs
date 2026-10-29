//! Test-build-only control. No listener or control command exists in production builds.
use crate::{runtime::Runtime, Shared};
use serde_json::{json, Value};
use tauri::{Manager, State};

#[tauri::command]
pub async fn benchmark_control(
    action: String,
    report: Option<Value>,
    app: tauri::AppHandle,
    state: State<'_, Shared>,
) -> Result<Value, String> {
    if app.config().identifier != "dev.diffkin.benchmark" {
        return Err("Benchmark requires its isolated application identifier".into());
    }
    let config = std::env::var("DIFFKIN_BENCH_CONFIG").map_err(|e| e.to_string())?;
    match action.as_str() {
        "config" => serde_json::from_str(&config).map_err(|e| e.to_string()),
        "reset" => {
            state.clear_benchmark_cache();
            Ok(Value::Null)
        }
        "status" => Ok(json!({"active": Runtime::has_active_jobs(&state)})),
        "report" => {
            let path = std::env::var("DIFFKIN_BENCH_OUTPUT").map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err("Benchmark report exceeds 4 MiB".into());
            }
            std::fs::write(path, bytes).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "viewport" => {
            let value = report.ok_or("Missing viewport")?;
            let width = value["width"].as_u64().ok_or("Missing width")?;
            let height = value["height"].as_u64().ok_or("Missing height")?;
            if !matches!((width, height), (760, 540) | (1200, 800)) {
                return Err("Unsupported benchmark viewport".into());
            }
            app.get_webview_window("main")
                .ok_or("Missing window")?
                .set_size(tauri::LogicalSize::new(width as f64, height as f64))
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "exit" => {
            app.exit(0);
            Ok(Value::Null)
        }
        _ => Err("Unknown benchmark action".into()),
    }
}
