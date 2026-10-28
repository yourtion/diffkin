use diffkin_analysis::{analyze, source_range};
use diffkin_domain::{CompareError, LineKind, Presentation, SourceRange};
use serde::Deserialize;
use std::{collections::HashSet, sync::atomic::AtomicBool, time::Instant};

#[derive(Deserialize)]
struct Fixture {
    id: String,
    path: String,
    before: String,
    after: String,
    expected: Expected,
}
#[derive(Deserialize)]
struct Expected {
    format: bool,
    #[serde(rename = "move")]
    moved: bool,
    rename: bool,
    visible: Vec<String>,
    fallback: bool,
}

fn check_range(range: &SourceRange, source: &str) {
    let a = range.byte_start as usize;
    let b = range.byte_end as usize;
    assert!(
        a <= b && b <= source.len() && source.is_char_boundary(a) && source.is_char_boundary(b)
    );
    assert_eq!(*range, source_range(source, a, b));
    assert_eq!(
        source[..a].encode_utf16().count(),
        range.utf16_start as usize
    );
    assert_eq!(source[..b].encode_utf16().count(), range.utf16_end as usize);
}

#[test]
fn frozen_structural_acceptance_corpus() {
    let fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("../../../fixtures/structure/cases.json")).unwrap();
    assert!(fixtures.len() >= 80);
    let cancel = AtomicBool::new(false);
    let mut names = HashSet::new();
    let mut failures = vec![];
    let mut durations = vec![];
    let mut positives = [0usize; 3];
    let mut true_positives = [0usize; 3];
    let mut false_positives = [0usize; 3];
    for fixture in &fixtures {
        assert!(names.insert(fixture.id.clone()), "Duplicate fixture id");
        let start = Instant::now();
        let result = analyze(&fixture.path, &fixture.before, &fixture.after, &cancel).unwrap();
        durations.push(start.elapsed().as_micros());
        let actual = ["format", "move", "rename"].map(|facet| {
            result
                .events
                .iter()
                .any(|e| e.facets.iter().any(|f| f == facet))
        });
        let expected = [
            fixture.expected.format,
            fixture.expected.moved,
            fixture.expected.rename,
        ];
        for i in 0..3 {
            positives[i] += usize::from(expected[i]);
            true_positives[i] += usize::from(expected[i] && actual[i]);
            false_positives[i] += usize::from(!expected[i] && actual[i]);
        }
        if actual != expected {
            failures.push(format!(
                "{}: expected {:?}, got {:?}; {:?}",
                fixture.id, expected, actual, result.fallbacks
            ));
        }
        if fixture.expected.fallback && result.fallbacks.is_empty() {
            failures.push(format!("{}: expected fallback", fixture.id));
        }
        let reconstructed_before: String = result
            .raw
            .lines
            .iter()
            .filter(|l| l.kind != LineKind::Insert)
            .map(|l| l.text.as_str())
            .collect();
        let reconstructed_after: String = result
            .raw
            .lines
            .iter()
            .filter(|l| l.kind != LineKind::Delete)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(
            reconstructed_before, fixture.before,
            "{} before",
            fixture.id
        );
        assert_eq!(reconstructed_after, fixture.after, "{} after", fixture.id);
        let raw_ids: HashSet<u32> = result
            .raw
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind != LineKind::Equal)
            .map(|(i, _)| i as u32)
            .collect();
        let covered: HashSet<u32> = result.coverage.iter().map(|e| e.raw_edit_id).collect();
        assert_eq!(raw_ids, covered, "{} coverage gap", fixture.id);
        assert_eq!(
            result.coverage.len(),
            covered.len(),
            "{} duplicate coverage",
            fixture.id
        );
        for needle in &fixture.expected.visible {
            let visible = result.coverage.iter().any(|entry| {
                entry.presentation != Presentation::FoldedFormat
                    && result.raw.lines[entry.raw_edit_id as usize]
                        .text
                        .contains(needle)
            });
            if !visible {
                failures.push(format!(
                    "{}: hidden or missing required edit {needle:?}",
                    fixture.id
                ));
            }
        }
        let event_ids: HashSet<_> = result.events.iter().map(|e| &e.id).collect();
        for item in &result.events {
            if let Some(range) = &item.before {
                check_range(range, &fixture.before);
            }
            if let Some(range) = &item.after {
                check_range(range, &fixture.after);
            }
            assert!(item.raw_edit_ids.iter().all(|id| raw_ids.contains(id)));
            assert!(item
                .children
                .iter()
                .all(|id| event_ids.contains(id) && *id != item.id));
            assert!(!item.evidence.is_empty());
        }
        for item in &result.coverage {
            assert!(item.event_ids.iter().all(|id| event_ids.contains(id)));
            if item.presentation == Presentation::FoldedFormat {
                assert!(!item.event_ids.is_empty());
                assert!(item.event_ids.iter().all(|id| result
                    .events
                    .iter()
                    .any(|e| e.id == *id && e.facets == ["format"])));
            }
        }
    }
    durations.sort_unstable();
    eprintln!("Corpus: {} cases; expected format/move/rename {:?}; true positives {:?}; false positives {:?}; p50 {}us, p95 {}us, max {}us",fixtures.len(),positives,true_positives,false_positives,durations[durations.len()/2],durations[durations.len()*95/100],durations.last().unwrap());
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn unicode_crlf_coordinates_are_original_and_reversible() {
    let source = "\u{feff}中😀e\u{301}\r\nnext\n";
    for a in 0..=source.len() {
        for b in a..=source.len() {
            if source.is_char_boundary(a) && source.is_char_boundary(b) {
                check_range(&source_range(source, a, b), source);
            }
        }
    }
}

#[test]
fn cancellation_and_large_inputs_have_explicit_outcomes() {
    assert!(matches!(
        analyze("a.ts", "a", "b", &AtomicBool::new(true)),
        Err(CompareError::Cancelled { .. })
    ));
    assert!(matches!(
        analyze("a.ts", &"x".repeat(200_001), "", &AtomicBool::new(false)),
        Err(CompareError::InputTooLarge { .. })
    ));
}

#[test]
fn bounded_unicode_mutations_preserve_all_original_bytes() {
    let atoms = [
        "a", "中", "😀", "\r\n", "\n", "\u{feff}", "e\u{301}", "\0", "\t", " ",
    ];
    for seed in 0..300 {
        let before = (0..(seed % 47))
            .map(|i| atoms[(i * 17 + seed) % atoms.len()])
            .collect::<String>();
        let after = (0..(seed % 43))
            .map(|i| atoms[(i * 13 + seed + 3) % atoms.len()])
            .collect::<String>();
        let result = analyze("raw.txt", &before, &after, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Insert)
                .map(|l| l.text.as_str())
                .collect::<String>(),
            before
        );
        assert_eq!(
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Delete)
                .map(|l| l.text.as_str())
                .collect::<String>(),
            after
        );
        assert!(result
            .coverage
            .iter()
            .all(|e| e.presentation != Presentation::FoldedFormat));
    }
}

#[test]
fn relocating_one_function_does_not_mark_shifted_neighbors() {
    let functions: Vec<_> = (0..40)
        .map(|n| format!("function f{n}(){{return {n};}}\n"))
        .collect();
    let before = functions.join("");
    let after = format!("{}{}", functions[39], functions[..39].join(""));
    let result = analyze("a.ts", &before, &after, &AtomicBool::new(false)).unwrap();
    let moved: Vec<_> = result
        .events
        .iter()
        .filter(|e| e.facets.iter().any(|f| f == "move"))
        .collect();
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].title, "f39");
}

#[test]
fn pathological_parser_work_releases_on_cancellation() {
    use std::{
        sync::{atomic::Ordering, Arc},
        thread,
        time::Duration,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let before = format!("const x = {}1{};", "(".repeat(40_000), ")".repeat(40_000));
    let after = before.replacen('1', "2", 1);
    let worker = thread::spawn(move || analyze("a.ts", &before, &after, &worker_cancel));
    thread::sleep(Duration::from_millis(2));
    let requested = Instant::now();
    cancel.store(true, Ordering::Relaxed);
    let result = worker.join().unwrap();
    assert!(result.is_ok() || matches!(result, Err(CompareError::Cancelled { .. })));
    // This is a resource-release bound, not a platform performance claim.
    assert!(requested.elapsed() < Duration::from_secs(2));
}

#[test]
fn duplicate_vue_script_modes_do_not_produce_proofs() {
    let before = "<script>const a=1;</script><script>const b=2;</script>";
    let after = "<script>const a = 1;</script><script>const b = 2;</script>";
    let result = analyze("a.vue", before, after, &AtomicBool::new(false)).unwrap();
    assert!(result.events.is_empty());
    assert!(result.fallbacks.iter().any(|s| s.contains("Duplicate")));
    assert!(result
        .coverage
        .iter()
        .all(|e| e.presentation == Presentation::TextFallback));
}
