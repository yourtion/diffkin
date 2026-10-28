use diffkin_domain::{
    CompareError, CompareRequest, Comparison, DiffLine, LineKind, SCHEMA_VERSION,
};
use similar::{ChangeTag, TextDiff};
use std::time::Duration;

mod structure;
pub use structure::{analyze, source_range, supports_detail, RULE_VERSION};

pub const MAX_BYTES: usize = 200_000;
pub const MAX_LINES: usize = 5_000;

/// Count editor-visible line breaks without changing source bytes. CRLF is one break.
pub(crate) fn line_breaks(text: &str) -> usize {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter(|(i, b)| **b == b'\r' || (**b == b'\n' && (*i == 0 || bytes[*i - 1] != b'\r')))
        .count()
}
pub(crate) fn source_line_count(text: &str) -> usize {
    line_breaks(text) + usize::from(!text.is_empty() && !text.ends_with(['\r', '\n']))
}
fn source_lines(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\r' || bytes[cursor] == b'\n' {
            cursor += 1;
            if bytes[cursor - 1] == b'\r' && bytes.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
            result.push(&text[start..cursor]);
            start = cursor;
        } else {
            cursor += 1;
        }
    }
    if start < text.len() {
        result.push(&text[start..]);
    }
    result
}

/// Text-only prototype. Never folds or claims semantic equivalence.
pub fn compare(request: &CompareRequest) -> Result<Comparison, CompareError> {
    for text in [&request.before, &request.after] {
        if text.len() > MAX_BYTES || source_line_count(text) > MAX_LINES {
            return Err(CompareError::InputTooLarge {
                message: format!("Each input is limited to {MAX_BYTES} bytes and {MAX_LINES} lines in this prototype."),
            });
        }
    }
    // On timeout similar uses coarser edits, preserving the complete text.
    let before_lines = source_lines(&request.before);
    let after_lines = source_lines(&request.after);
    let diff = TextDiff::configure()
        .timeout(Duration::from_millis(200))
        .diff_slices(&before_lines, &after_lines);
    let mut added = 0;
    let mut removed = 0;
    let lines = diff
        .iter_all_changes()
        .map(|change| {
            let kind = match change.tag() {
                ChangeTag::Equal => LineKind::Equal,
                ChangeTag::Delete => {
                    removed += 1;
                    LineKind::Delete
                }
                ChangeTag::Insert => {
                    added += 1;
                    LineKind::Insert
                }
            };
            DiffLine {
                kind,
                old_line: change.old_index().map(|line| line as u32 + 1),
                new_line: change.new_index().map(|line| line as u32 + 1),
                text: change.value().to_owned(),
            }
        })
        .collect();
    Ok(Comparison {
        schema_version: SCHEMA_VERSION,
        lines,
        added,
        removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_source_text_can_be_reconstructed() {
        for (before, after) in [
            ("", ""),
            ("", "new\n"),
            ("old", ""),
            ("中文😀\r\nlast", "中文😀\nlast\n"),
            ("if (a > b) {\n  run();\n}\n", "if (a >= b) { run(); }\n"),
            ("same\nsame\nsame\n", "same\nchanged\nsame\n"),
        ] {
            let result = compare(&CompareRequest {
                before: before.into(),
                after: after.into(),
            })
            .unwrap();
            let reconstructed_before: String = result
                .lines
                .iter()
                .filter(|line| line.kind != LineKind::Insert)
                .map(|line| line.text.as_str())
                .collect();
            let reconstructed_after: String = result
                .lines
                .iter()
                .filter(|line| line.kind != LineKind::Delete)
                .map(|line| line.text.as_str())
                .collect();
            assert_eq!(reconstructed_before, before);
            assert_eq!(reconstructed_after, after);
        }
    }

    #[test]
    fn rejects_large_inputs_before_comparing() {
        let result = compare(&CompareRequest {
            before: "a".repeat(MAX_BYTES + 1),
            after: String::new(),
        });
        assert!(matches!(result, Err(CompareError::InputTooLarge { .. })));
    }

    #[test]
    fn changes_have_correct_line_numbers() {
        let result = compare(&CompareRequest {
            before: "a\nb\n".into(),
            after: "a\nc\n".into(),
        })
        .unwrap();
        assert_eq!((result.added, result.removed), (1, 1));
        assert_eq!(
            (result.lines[1].old_line, result.lines[1].new_line),
            (Some(2), None)
        );
        assert_eq!(
            (result.lines[2].old_line, result.lines[2].new_line),
            (None, Some(2))
        );
    }
}
