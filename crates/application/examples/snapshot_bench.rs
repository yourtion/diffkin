//! Reports capture time only; excludes IPC, rendering and OS cold-cache claims.
use diffkin_domain::{CompareMode, ComparisonSpec};
use std::{sync::atomic::AtomicBool, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = match args.get(1).map(String::as_str) {
        Some("commits") => CompareMode::Commits,
        Some("staged") => CompareMode::Staged,
        Some("unstaged") => CompareMode::Unstaged,
        _ => CompareMode::Worktree,
    };
    let spec = ComparisonSpec {
        repository: args.first().expect("repository path").clone(),
        mode,
        base: None,
        head: None,
        include_untracked: false,
        verify_all: false,
    };
    let mut records = Vec::new();
    let mut changed = 0;
    for _ in 0..30 {
        let start = Instant::now();
        let snapshot =
            diffkin_application::capture(&spec, &AtomicBool::new(false)).expect("capture");
        changed = snapshot.manifest(0, 0).total_files;
        records.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let mut ordered = records.clone();
    ordered.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"mode": mode, "changedFiles": changed, "samplesMs": records, "p50Ms": ordered[14], "p95Ms": ordered[28], "maxMs": ordered[29], "scope": "Rust snapshot capture only; warm filesystem cache, no result cache; excludes IPC/UI"})
    );
}
