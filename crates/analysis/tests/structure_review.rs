use diffkin_analysis::analyze;
use diffkin_domain::Presentation;
use std::sync::atomic::AtomicBool;

fn assert_no_fold(path: &str, before: &str, after: &str) {
    let result = analyze(path, before, after, &AtomicBool::new(false)).unwrap();
    assert!(
        result.coverage.iter().all(|entry| entry.presentation != Presentation::FoldedFormat),
        "Semantic or validity change must remain visible: {path}\n{before:?} -> {after:?}\n{result:#?}"
    );
}

#[test]
fn java_minimum_integer_parentheses_change_literal_validity() {
    assert_no_fold(
        "Counter.java",
        "class Counter { int count() { return -2147483648; } }\n",
        "class Counter { int count() { return -(2147483648); } }\n",
    );
}

#[test]
fn java_minimum_long_parentheses_change_literal_validity() {
    assert_no_fold(
        "Counter.java",
        "class Counter { long count() { return -9223372036854775808L; } }\n",
        "class Counter { long count() { return -(9223372036854775808L); } }\n",
    );
}

#[test]
fn go_deferred_call_parentheses_are_not_general_expression_grouping() {
    assert_no_fold(
        "main.go",
        "package main\nfunc main() { defer cleanup() }\n",
        "package main\nfunc main() { defer (cleanup()) }\n",
    );
}

#[test]
fn javascript_bom_whitespace_does_not_mask_return_line_terminator() {
    assert_no_fold(
        "return.js",
        "function value() { return\u{feff}(1); }\n",
        "function value() { return\u{feff}\n(1); }\n",
    );
}

#[test]
fn restricted_production_line_breaks_remain_visible() {
    let cases = [
        ("function f(){throw (1);}", "function f(){throw\n(1);}"),
        ("function* f(){yield (1);}", "function* f(){yield\n(1);}"),
        (
            "function f(){a:while(1){break a;}}",
            "function f(){a:while(1){break\na;}}",
        ),
        (
            "function f(){a:while(1){continue a;}}",
            "function f(){a:while(1){continue\na;}}",
        ),
        ("let f = async x => x;", "let f = async\nx => x;"),
        (
            "function f(){let x=1,y=2;x\n++y;}",
            "function f(){let x=1,y=2;x++\ny;}",
        ),
        (
            "function f(){return (1);}",
            "function f(){return\u{2028}(1);}",
        ),
        (
            "function f(){return (1);}",
            "function f(){return\u{2029}(1);}",
        ),
    ];
    for (before, after) in cases {
        assert_no_fold("restricted.js", before, after);
    }
}

#[test]
fn identical_methods_in_different_classes_are_not_move_candidates() {
    let before = "class First { value() { return 1; } }\nclass Second { value() { return 2; } }\n";
    let after = "class First { value() { return 2; } }\nclass Second { value() { return 1; } }\n";
    let result = analyze("classes.js", before, after, &AtomicBool::new(false)).unwrap();
    assert!(result
        .events
        .iter()
        .all(|event| !event.facets.iter().any(|facet| facet == "move")));
    assert!(result
        .coverage
        .iter()
        .all(|entry| entry.presentation != Presentation::FoldedFormat));
}
