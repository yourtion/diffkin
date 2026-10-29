//! Native worker timings; excludes IPC/WebView and allocator RSS.
#[allow(dead_code)]
#[path = "../src/runtime.rs"]
mod runtime;
use diffkin_domain::{CompareMode, ComparisonSpec};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let spec = ComparisonSpec {
        repository: args[0].clone(),
        mode: CompareMode::Commits,
        base: args.get(1).cloned(),
        head: args.get(2).cloned(),
        include_untracked: false,
        verify_all: false,
    };
    let token = AtomicBool::new(false);
    let start = Instant::now();
    let snapshot = Arc::new(diffkin_application::capture(&spec, &token).unwrap());
    let capture_ms = start.elapsed().as_secs_f64() * 1000.0;
    let manifest = snapshot.manifest(0, usize::MAX);
    let file = manifest
        .files
        .iter()
        .find(|file| file.path.ends_with(".ts") || file.path.ends_with(".js"))
        .unwrap_or(&manifest.files[0]);
    let runtime = runtime::Runtime::default();
    runtime.install_comparison(snapshot.clone());
    let mut records = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let frozen = snapshot.read_file(&file.id).unwrap();
        let preview = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let analysis = diffkin_application::analyze_file(frozen, &token).unwrap();
        let analyze = start.elapsed().as_secs_f64() * 1000.0;
        runtime.remember(manifest.id.clone(), file.id.clone(), analysis);
        let start = Instant::now();
        assert!(runtime.cached(&manifest.id, &file.id).is_some());
        let cache = start.elapsed().as_secs_f64() * 1000.0;
        records.push(serde_json::json!({"previewMs":preview,"analysisMs":analyze,"cacheMs":cache}));
    }
    println!(
        "{}",
        serde_json::json!({"scope":"Native workers only; warm filesystem; excludes IPC, WebView and RSS", "manifest":manifest.id,"file":file.path,"captureMs":capture_ms,"samples":records})
    );
}
