use super::*;

pub(super) fn condition() -> FileAnalysis {
    let before = "// outside selected context\r\n\r\n\r\n\r\n\r\nfunction 取款(balance, amount) {\r\n  return balance > amount;\r\n}\r\n";
    let file = FrozenFile {
        summary: FileSummary {
            id: "file".into(),
            path: "src/withdraw.js".into(),
            old_path: None,
            status: FileStatus::Modified,
            old_mode: Some("100644".into()),
            new_mode: Some("100644".into()),
            before_id: Some("old".into()),
            after_id: Some("new".into()),
            notices: vec![],
        },
        before: Some(before.into()),
        after: Some(before.replace(" > ", " >= ")),
        before_truncated: false,
        after_truncated: false,
    };
    diffkin_application::analyze_file(file, &AtomicBool::new(false)).unwrap()
}

fn selected() -> ExplanationContext {
    let data = condition();
    let start = data
        .analysis
        .as_ref()
        .unwrap()
        .raw
        .lines
        .iter()
        .position(|l| l.kind != LineKind::Equal)
        .unwrap();
    let ctx = context("snapshot", &data, None, Some(start)).unwrap();
    assert!(context("snapshot", &data, None, Some(start + 1)).is_err());
    ctx
}

#[test]
fn selects_frozen_hunk_with_neighboring_lines_only() {
    let ctx = selected();
    let before = ctx.before.unwrap();
    let after = ctx.after.unwrap();
    assert_eq!(
        (
            before.start_line,
            before.focus_start,
            before.focus_end,
            before.end_line
        ),
        (3, 7, 7, 8)
    );
    assert!(
        before.text.contains("取款") && before.text.contains(" > ") && after.text.contains(" >= ")
    );
    assert!(!before.text.contains("outside") && after.text.ends_with("}\r\n"));
}

#[test]
fn clips_exclusive_event_end_at_final_newline_and_preserves_cr_only_lines() {
    let text = "a\rb\r";
    let range = SourceRange {
        byte_start: 2,
        byte_end: 4,
        utf16_start: 2,
        utf16_end: 4,
        start_line: 2,
        end_line: 3,
    };
    let excerpt = excerpt(Some(text), Some(Focus::Event(&range)), true)
        .unwrap()
        .unwrap();
    assert_eq!(
        (excerpt.focus_start, excerpt.focus_end, excerpt.end_line),
        (2, 2, 2)
    );
}

#[test]
fn missing_truncated_and_oversized_inputs_do_not_become_explanations() {
    let mut data = condition();
    assert!(context("s", &data, Some("stale-event"), None).is_err());
    data.file.before_truncated = true;
    assert!(context("s", &data, None, None).is_err());
    data.file.before_truncated = false;
    data.file.before = None;
    assert!(context("s", &data, None, None).is_err());
    data.file.summary.status = FileStatus::Added;
    assert!(context("s", &data, None, None).unwrap().before.is_none());
    data.file.after = Some("x".repeat(CONTEXT_BYTES));
    assert!(context("s", &data, None, None).is_err());
}

fn answer() -> Value {
    json!({"meaning":"决定是否允许取款", "change":"余额等于金额时现在允许", "rationale":"可能为修正等额取款边界", "caveats":["仍需确认需求"], "references":[{"side":"after", "startLine":7,"endLine":7}]})
}

#[test]
fn rejects_invalid_references_and_unstructured_answers_for_both_providers() {
    let ctx = selected();
    for provider in [ExplanationProvider::Codex, ExplanationProvider::Claude] {
        let wrap = |a: Value| {
            match provider {
            ExplanationProvider::Claude => json!({"structured_output":a}).to_string(),
            ExplanationProvider::Codex => json!({"type":"item.completed","item":{"type":"agent_message","text":a.to_string()}}).to_string(),
        }
        };
        assert!(parse_output(provider, &wrap(answer()))
            .and_then(|v| validate_explanation(v, &ctx))
            .is_ok());
        let mut invalid = answer();
        invalid["references"][0]["endLine"] = json!(42);
        assert!(parse_output(provider, &wrap(invalid))
            .and_then(|v| validate_explanation(v, &ctx))
            .is_err());
        assert!(parse_output(provider, "unfinished").is_err());
    }
}

#[cfg(unix)]
#[test]
fn complete_adapters_pass_only_scoped_source_on_stdin_in_temporary_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new().unwrap();
    for provider in [ExplanationProvider::Codex, ExplanationProvider::Claude] {
        let program = dir.0.join("test-cli");
        let response = match provider {
            ExplanationProvider::Codex => {
                json!({"type":"item.completed","item":{"type":"agent_message","text":answer().to_string()}})
            }
            ExplanationProvider::Claude => json!({"structured_output":answer()}),
        };
        let check = match provider {
            ExplanationProvider::Codex => {
                "*--ignore-user-config*--sandbox*read-only*features.shell_tool=false*"
            }
            ExplanationProvider::Claude => {
                "*--safe-mode*--tools*--strict-mcp-config*--no-session-persistence*"
            }
        };
        fs::write(&program, format!("#!/bin/sh\nset -eu\ntest -f schema.json\ncase \"$*\" in {check}) ;; *) exit 5;; esac\nprompt=$(cat)\ncase \"$prompt\" in *'outside selected context'*) exit 6;; *'balance >= amount;'*) ;; *) exit 7;; esac\nprintf '%s\\n' '{}'\n",response)).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let request = ExplanationRequest {
            prepared_id: "p".into(),
            provider,
            model: String::new(),
            question: "余额相等呢？".into(),
        };
        let result = explain(
            PreparedExplanation {
                id: "p".into(),
                context: selected(),
                tools: vec![ExplanationTool {
                    provider,
                    executable: Some(program.to_str().unwrap().into()),
                }],
            },
            &request,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.explanation.references[0].start_line, 7);
    }
}

#[cfg(unix)]
#[test]
fn cancels_and_bounds_even_children_that_never_read_stdin() {
    let wait = || {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 10 & wait"]);
        cmd
    };
    let cancel = AtomicBool::new(false);
    let input = "x".repeat(128 * 1024);
    assert!(matches!(
        process::run(&mut wait(), &input, &cancel, Duration::from_millis(80)),
        Err(RepositoryError::BudgetExceeded { .. })
    ));
    std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(Duration::from_millis(80));
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        assert!(matches!(
            process::run(&mut wait(), &input, &cancel, Duration::from_secs(2)),
            Err(RepositoryError::Cancelled { .. })
        ));
    });
}

/// Opt-in only: sends the synthetic fixture above using the selected CLI's real account.
#[cfg(unix)]
#[test]
#[ignore = "requires an explicitly selected installed and authenticated CLI"]
fn installed_cli_explains_synthetic_change() {
    let provider = match std::env::var("DIFFKIN_TEST_AI").as_deref() {
        Ok("codex") => ExplanationProvider::Codex,
        Ok("claude") => ExplanationProvider::Claude,
        _ => panic!("set DIFFKIN_TEST_AI to codex or claude"),
    };
    let result = explain(
        PreparedExplanation {
            id: "live".into(),
            context: selected(),
            tools: tools(),
        },
        &ExplanationRequest {
            prepared_id: "live".into(),
            provider,
            model: String::new(),
            question: "余额等于金额时什么不同？".into(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!result.explanation.references.is_empty());
    println!("{}", result.explanation.change);
}
