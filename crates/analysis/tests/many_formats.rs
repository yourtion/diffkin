use diffkin_analysis::analyze;
use diffkin_domain::{LineKind, Presentation};
use std::sync::atomic::AtomicBool;

#[test]
fn many_formatted_functions_keep_the_real_condition_edit_visible() {
    let before: String = (0..200)
        .map(|i| format!("// 中文 😀\r\nfunction f{i}(x: number) {{ return x > {i}; }}\r\n"))
        .collect();
    let after = before
        .replace("return x", "return   x")
        .replacen("x > 75", "x >= 75", 1);
    let result = analyze("many.ts", &before, &after, &AtomicBool::new(false)).unwrap();
    assert!(!result.events.is_empty(), "{:?}", result.fallbacks);
    assert!(
        result
            .events
            .iter()
            .filter(|event| event.facets == ["format"])
            .count()
            >= 199
    );
    for (id, line) in result.raw.lines.iter().enumerate() {
        if line.kind != LineKind::Equal && line.text.contains("function f75(") {
            assert!(result
                .coverage
                .iter()
                .any(|entry| entry.raw_edit_id == id as u32
                    && entry.presentation == Presentation::Visible));
        }
    }
    for event in &result.events {
        for (source, span) in [(&before, &event.before), (&after, &event.after)] {
            if let Some(span) = span {
                assert_eq!(
                    *span,
                    diffkin_analysis::source_range(
                        source,
                        span.byte_start as usize,
                        span.byte_end as usize
                    )
                );
            }
        }
    }
}
