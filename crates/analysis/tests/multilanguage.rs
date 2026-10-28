use diffkin_analysis::analyze;
use diffkin_domain::{LineKind, Presentation};
use serde::Deserialize;
use std::{collections::HashMap, sync::atomic::AtomicBool};
#[derive(Deserialize)]
struct Case {
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
    fold: bool,
    visible: Option<String>,
}
#[test]
fn multilingual_format_movement_and_semantic_counterexamples() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../../fixtures/multilanguage/cases.json")).unwrap();
    let mut failures = vec![];
    let mut counts = HashMap::new();
    for case in &cases {
        let result = analyze(
            &case.path,
            &case.before,
            &case.after,
            &AtomicBool::new(false),
        )
        .unwrap();
        *counts.entry(result.language.clone()).or_insert(0) += 1;
        let fmt = result
            .events
            .iter()
            .any(|e| e.facets.iter().any(|f| f == "format"));
        let moved = result
            .events
            .iter()
            .any(|e| e.facets.iter().any(|f| f == "move"));
        let folded = result
            .coverage
            .iter()
            .any(|e| e.presentation == Presentation::FoldedFormat);
        if (fmt, moved, folded)
            != (
                case.expected.format,
                case.expected.moved,
                case.expected.fold,
            )
        {
            failures.push(format!(
                "{} expected {:?} got {:?}; events={:?}; fallback={:?}",
                case.id,
                (
                    case.expected.format,
                    case.expected.moved,
                    case.expected.fold
                ),
                (fmt, moved, folded),
                result.events,
                result.fallbacks
            ));
        }
        if let Some(needle) = &case.expected.visible {
            if !result.coverage.iter().any(|e| {
                e.presentation != Presentation::FoldedFormat
                    && result.raw.lines[e.raw_edit_id as usize]
                        .text
                        .contains(needle)
            }) {
                failures.push(format!("{} hidden semantic edit {needle:?}", case.id));
            }
        }
        assert_eq!(
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Insert)
                .map(|l| l.text.as_str())
                .collect::<String>(),
            case.before
        );
        assert_eq!(
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Delete)
                .map(|l| l.text.as_str())
                .collect::<String>(),
            case.after
        );
        assert_eq!(
            result.coverage.len(),
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Equal)
                .count()
        );
        if case.expected.moved && case.expected.visible.is_some() {
            assert!(
                result
                    .events
                    .iter()
                    .any(|e| e.facets.iter().any(|f| f == "edit")),
                "{} moved internal edit missing",
                case.id
            );
        }
    }
    eprintln!("Multilingual corpus: {} cases, {:?}", cases.len(), counts);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
