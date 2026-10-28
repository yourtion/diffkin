use diffkin_analysis::analyze;
use diffkin_domain::AnalysisResult;
use std::sync::atomic::AtomicBool;

fn compare(path: &str, before: &str, after: &str) -> AnalysisResult {
    analyze(path, before, after, &AtomicBool::new(false)).unwrap()
}
fn renamed(result: &AnalysisResult) -> bool {
    result
        .events
        .iter()
        .any(|event| event.facets.iter().any(|facet| facet == "rename"))
}
fn assert_rename(path: &str, before: &str, after: &str) {
    let result = compare(path, before, after);
    assert!(
        renamed(&result),
        "{path}: {:?}\n{:?}",
        result.fallbacks,
        result.events
    );
    // Rename grouping is always visible; it must never authorize formatting folds.
    assert!(result
        .coverage
        .iter()
        .all(|entry| entry.presentation != diffkin_domain::Presentation::FoldedFormat));
}
fn assert_no_rename(path: &str, before: &str, after: &str) {
    let result = compare(path, before, after);
    assert!(
        !renamed(&result),
        "Unsafe rename for {path}: {:?}",
        result.events
    );
    assert!(result.raw.added + result.raw.removed > 0);
    assert!(result
        .coverage
        .iter()
        .all(|entry| entry.presentation != diffkin_domain::Presentation::FoldedFormat));
}

#[test]
fn python_simple_parameters_locals_reassignment_and_parentheses() {
    assert_rename(
        "f.py",
        "def f(count):\n    return count > 0\n",
        "def f(amount):\n    return amount > 0\n",
    );
    assert_rename(
        "f.py",
        "def f(value: int):\n    total = value + 1\n    total = total + 1\n    return total\n",
        "def f(value: int):\n    result = value + 1\n    result = result + 1\n    return result\n",
    );
    assert_rename(
        "f.py",
        "def f(count):\n    return count > 0\n",
        "def f(amount):\n    return (amount) >= 0\n",
    );
}

#[test]
fn python_binding_ambiguity_and_reference_changes_fall_back() {
    for (before, after) in [
        (
            "def f(count):\n    def inner():\n        return count\n    return inner()\n",
            "def f(amount):\n    def inner():\n        return amount\n    return inner()\n",
        ),
        (
            "def f(count):\n    global outside\n    return count\n",
            "def f(amount):\n    global outside\n    return amount\n",
        ),
        (
            "def f(count):\n    return [count for count in xs]\n",
            "def f(amount):\n    return [amount for amount in xs]\n",
        ),
        (
            "def f(count):\n    return obj.count + count\n",
            "def f(amount):\n    return obj.amount + amount\n",
        ),
        (
            "def f(count):\n    return count + outside\n",
            "def f(outside):\n    return outside + outside\n",
        ),
        (
            "def f(count):\n    return count\n",
            "def f(amount):\n    return amount + amount\n",
        ),
        (
            "def f():\n    total = total + 1\n    return total\n",
            "def f():\n    result = result + 1\n    return result\n",
        ),
        (
            "def f(count):\n    return call(count=count)\n",
            "def f(amount):\n    return call(amount=amount)\n",
        ),
        (
            "def f(count):\n    return locals()['count']\n",
            "def f(amount):\n    return locals()['amount']\n",
        ),
        (
            "def f(count):\n    value: count\n    return count\n",
            "def f(amount):\n    value: amount\n    return amount\n",
        ),
    ] {
        assert_no_rename("f.py", before, after);
    }
}

#[test]
fn java_simple_parameters_and_locals() {
    assert_rename(
        "A.java",
        "class A { int f(int count) { return count + 1; } }",
        "class A { int f(int amount) { return amount + 1; } }",
    );
    assert_rename(
        "A.java",
        "class A { int f(int input) { int total = input + 1; return total; } }",
        "class A { int f(int input) { int result = input + 1; return result; } }",
    );
    assert_rename(
        "A.java",
        "class A { boolean f(int count) { return count > 0; } }",
        "class A { boolean f(int amount) { return (amount) >= 0; } }",
    );
}

#[test]
fn java_members_overloads_nested_scopes_and_capture_stay_visible() {
    for (before, after) in [
        ("class A { int f(int count) { return this.count + count; } }", "class A { int f(int amount) { return this.amount + amount; } }"),
        ("class A { int f(int count) { return count; } int f(long count) { return 1; } }", "class A { int f(int amount) { return amount; } int f(long count) { return 1; } }"),
        ("class A { int f(int count) { { int other = count; } return count; } }", "class A { int f(int amount) { { int other = amount; } return amount; } }"),
        ("class A { int f(int count) { Runnable r = () -> use(count); return count; } }", "class A { int f(int amount) { Runnable r = () -> use(amount); return amount; } }"),
        ("class A { int f(int count) { return count + outside; } }", "class A { int f(int outside) { return outside + outside; } }"),
        ("class A { int f(int count) { return count; } }", "class A { int f(int amount) { return amount + amount; } }"),
        ("class A { int f(int count) { return count(); } }", "class A { int f(int amount) { return amount(); } }"),
        ("class A { int f(int count) { Object o = new Object() { int x = count; }; return count; } }", "class A { int f(int amount) { Object o = new Object() { int x = amount; }; return amount; } }"),
    ] { assert_no_rename("A.java", before, after); }
}

#[test]
fn go_simple_parameters_and_var_bindings() {
    assert_rename(
        "f.go",
        "package p\nfunc f(count int) int { return count + 1 }\n",
        "package p\nfunc f(amount int) int { return amount + 1 }\n",
    );
    assert_rename(
        "f.go",
        "package p\nfunc f(input int) int { var total = input + 1; return total }\n",
        "package p\nfunc f(input int) int { var result = input + 1; return result }\n",
    );
    assert_rename(
        "f.go",
        "package p\nfunc f(count int) bool { return count > 0 }\n",
        "package p\nfunc f(amount int) bool { return (amount) >= 0 }\n",
    );
}

#[test]
fn go_short_declarations_receivers_closures_and_fields_fall_back() {
    for (before, after) in [
        ("package p\nfunc f(input int) int { total := input; return total }\n", "package p\nfunc f(input int) int { result := input; return result }\n"),
        ("package p\nfunc (r R) f(count int) int { return r.count + count }\n", "package p\nfunc (r R) f(amount int) int { return r.amount + amount }\n"),
        ("package p\nfunc f(count int) int { var g = func() int { return count }; return g() }\n", "package p\nfunc f(amount int) int { var g = func() int { return amount }; return g() }\n"),
        ("package p\nfunc f(count int) int { return object.count + count }\n", "package p\nfunc f(amount int) int { return object.amount + amount }\n"),
        ("package p\nfunc f(count int) int { return count + outside }\n", "package p\nfunc f(outside int) int { return outside + outside }\n"),
        ("package p\nfunc f(count int) int { return count }\n", "package p\nfunc f(amount int) int { return amount + amount }\n"),
        ("package p\nfunc f() int { var total = total + 1; return total }\n", "package p\nfunc f() int { var result = result + 1; return result }\n"),
    ] { assert_no_rename("f.go", before, after); }
}

#[test]
fn rust_simple_parameters_and_let_bindings() {
    assert_rename(
        "f.rs",
        "fn f(count: i32) -> i32 { count + 1 }",
        "fn f(amount: i32) -> i32 { amount + 1 }",
    );
    assert_rename(
        "f.rs",
        "fn f(input: i32) -> i32 { let total = input + 1; total }",
        "fn f(input: i32) -> i32 { let result = input + 1; result }",
    );
    assert_rename(
        "f.rs",
        "fn f(count: i32) -> bool { count > 0 }",
        "fn f(amount: i32) -> bool { (amount) >= 0 }",
    );
}

#[test]
fn rust_macro_closure_pattern_shadow_and_member_collisions_fall_back() {
    for (before, after) in [
        (
            "fn f(count: i32) { println!(\"{}\", count); }",
            "fn f(amount: i32) { println!(\"{}\", amount); }",
        ),
        (
            "fn f(count: i32) -> i32 { let g = || count; g() }",
            "fn f(amount: i32) -> i32 { let g = || amount; g() }",
        ),
        (
            "fn f(input: (i32, i32)) -> i32 { let (count, other) = input; count + other }",
            "fn f(input: (i32, i32)) -> i32 { let (amount, other) = input; amount + other }",
        ),
        (
            "fn f(count: i32) -> i32 { let count = count + 1; count }",
            "fn f(amount: i32) -> i32 { let amount = amount + 1; amount }",
        ),
        (
            "fn f(count: i32) -> i32 { object.count + count }",
            "fn f(amount: i32) -> i32 { object.amount + amount }",
        ),
        (
            "fn f(count: i32) -> i32 { count + outside }",
            "fn f(outside: i32) -> i32 { outside + outside }",
        ),
        (
            "fn f(count: i32) -> i32 { count }",
            "fn f(amount: i32) -> i32 { amount + amount }",
        ),
        (
            "fn f() -> i32 { let total = total + 1; total }",
            "fn f() -> i32 { let result = result + 1; result }",
        ),
    ] {
        assert_no_rename("f.rs", before, after);
    }
}

#[test]
fn every_language_keeps_the_internal_condition_edit_separate() {
    for (path, before, after) in [
        (
            "f.py",
            "def f(count):\n    return count > 0\n",
            "def f(amount):\n    return amount >= 0\n",
        ),
        (
            "A.java",
            "class A { boolean f(int count) { return count > 0; } }",
            "class A { boolean f(int amount) { return amount >= 0; } }",
        ),
        (
            "f.go",
            "package p\nfunc f(count int) bool { return count > 0 }\n",
            "package p\nfunc f(amount int) bool { return amount >= 0 }\n",
        ),
        (
            "f.rs",
            "fn f(count: i32) -> bool { count > 0 }",
            "fn f(amount: i32) -> bool { amount >= 0 }",
        ),
    ] {
        let result = compare(path, before, after);
        assert!(renamed(&result), "{path}: {:?}", result.fallbacks);
        assert!(
            result.events.iter().any(|event| event.facets == ["edit"]),
            "{path}: an independent internal edit is required: {:?}",
            result.events
        );
    }
}

#[test]
fn removed_references_are_not_relabelled_as_a_pure_rename() {
    for (path, before, after) in [
        (
            "f.py",
            "def f(count):\n    return count + count\n",
            "def f(amount):\n    return amount\n",
        ),
        (
            "A.java",
            "class A { int f(int count) { return count + count; } }",
            "class A { int f(int amount) { return amount; } }",
        ),
        (
            "f.go",
            "package p\nfunc f(count int) int { return count + count }\n",
            "package p\nfunc f(amount int) int { return amount }\n",
        ),
        (
            "f.rs",
            "fn f(count: i32) -> i32 { count + count }",
            "fn f(amount: i32) -> i32 { amount }",
        ),
    ] {
        assert_no_rename(path, before, after);
    }
}

#[test]
fn declaration_additions_and_capture_binding_changes_are_not_guessed() {
    assert_no_rename(
        "f.py",
        "def f(count):\n    return count\n",
        "def f(amount):\n    other = 1\n    return amount\n",
    );
    assert_no_rename(
        "f.py",
        "def f(count):\n    nonlocal outside\n    return count\n",
        "def f(amount):\n    nonlocal outside\n    return amount\n",
    );
    assert_no_rename(
        "f.py",
        "def f(count):\n    class Local:\n        field = count\n    return count\n",
        "def f(amount):\n    class Local:\n        field = amount\n    return amount\n",
    );
    assert_no_rename(
        "A.java",
        "class A { int f(int count) { return count; } }",
        "class A { int f(int amount) { int other = 1; return amount; } }",
    );
    assert_no_rename(
        "f.go",
        "package p\nfunc f(count int) int { count, other := pair(); return count + other }\n",
        "package p\nfunc f(amount int) int { amount, other := pair(); return amount + other }\n",
    );
    assert_no_rename(
        "f.rs",
        "fn f(count: i32) -> i32 { count }",
        "fn f(amount: i32) -> i32 { let other = 1; amount }",
    );
}

#[test]
fn unchanged_property_spelling_still_does_not_become_a_binding_reference() {
    assert_no_rename(
        "f.py",
        "def f(count):\n    return obj.count + count\n",
        "def f(amount):\n    return obj.count + amount\n",
    );
    assert_no_rename(
        "A.java",
        "class A { int f(int count) { return this.count + count; } }",
        "class A { int f(int amount) { return this.count + amount; } }",
    );
    assert_no_rename(
        "f.go",
        "package p\nfunc f(count int) int { return obj.count + count }\n",
        "package p\nfunc f(amount int) int { return obj.count + amount }\n",
    );
    assert_no_rename(
        "f.rs",
        "fn f(count: i32) -> i32 { obj.count + count }",
        "fn f(amount: i32) -> i32 { obj.count + amount }",
    );
}
