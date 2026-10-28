//! Reproducible kernel measurements; excludes IPC, WebView rendering and disk capture.
use diffkin_analysis::analyze;
use std::{sync::atomic::AtomicBool, time::Instant};
fn main() {
    let functions: Vec<_> = (0..1000)
        .map(|n| format!("function f{n}(value: number) {{ return value > {n}; }}\n"))
        .collect();
    let ordinary = functions.join("");
    let edited = ordinary.replacen("value > 500", "value >= 500", 1);
    let mixed_before="function pay(balance: number) {\n  const amount = balance - 1;\n  return amount > 2;\n}\nfunction log() { return 0; }\n".to_string();
    let mixed_after="function log() { return 0; }\nfunction pay(balance: number){\n const total = balance - 1;\n return total >= 2;\n}\n".to_string();
    let mut reports = vec![];
    for (name, before, after) in [
        ("ordinary-1000-functions", ordinary, edited),
        ("move-rename-condition", mixed_before, mixed_after),
        (
            "large-file-rejection",
            "a".repeat(2 * 1024 * 1024),
            "b".repeat(2 * 1024 * 1024),
        ),
    ] {
        let mut micros = Vec::new();
        let mut outcomes = Vec::new();
        for _ in 0..30 {
            let start = Instant::now();
            let result = analyze("bench.ts", &before, &after, &AtomicBool::new(false));
            micros.push(start.elapsed().as_micros() as u64);
            outcomes.push(match result {
                Ok(r) => serde_json::json!({"events":r.events.len(),"fallbacks":r.fallbacks}),
                Err(e) => serde_json::json!({"error":e}),
            });
        }
        let mut sorted = micros.clone();
        sorted.sort_unstable();
        reports.push(serde_json::json!({"scenario":name,"beforeBytes":before.len(),"afterBytes":after.len(),"samplesMicros":micros,"p50Micros":sorted[15],"p95Micros":sorted[28],"maxMicros":sorted[29],"outcome":outcomes[0]}));
    }
    let mut cancellation_micros = Vec::new();
    let mut cancelled = 0;
    for _ in 0..30 {
        use std::{
            sync::{atomic::Ordering, Arc},
            thread,
            time::Duration,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_started = started.clone();
        let before = format!("const x = {}1{};", "(".repeat(40_000), ")".repeat(40_000));
        let after = before.replacen('1', "2", 1);
        let worker = thread::spawn(move || {
            worker_started.store(true, Ordering::Release);
            analyze("deep.ts", &before, &after, &worker_cancel)
        });
        while !started.load(Ordering::Acquire) {
            thread::yield_now();
        }
        thread::sleep(Duration::from_millis(2));
        let requested = Instant::now();
        cancel.store(true, Ordering::Relaxed);
        let outcome = worker.join().expect("analysis thread");
        cancellation_micros.push(requested.elapsed().as_micros() as u64);
        cancelled += usize::from(matches!(
            outcome,
            Err(diffkin_domain::CompareError::Cancelled { .. })
        ));
    }
    let mut sorted = cancellation_micros.clone();
    sorted.sort_unstable();
    reports.push(serde_json::json!({"scenario":"active-parser-cancellation","beforeBytes":80012,"samplesMicros":cancellation_micros,"p50Micros":sorted[15],"p95Micros":sorted[28],"maxMicros":sorted[29],"cancelledRuns":cancelled,"measurement":"cancel flag set after worker entry plus 2ms; elapsed until thread joined, not just UI result discarded"}));
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"scope":"kernel only, warm process; not cold app start or WebView performance","ruleVersion":diffkin_analysis::RULE_VERSION,"reports":reports})).unwrap());
}
