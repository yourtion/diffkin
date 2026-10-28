//! Cross-language semantic traps maintained independently of the normalizer's examples.
use diffkin_analysis::analyze;
use diffkin_domain::{LineKind, Presentation};
use std::sync::atomic::AtomicBool;

#[test]
fn grouping_and_line_break_semantics_remain_visible() {
    let cases = [
        (
            "optional chain receiver",
            "a.js",
            "function f(x){return x?.a.b;}",
            "function f(x){return (x?.a).b;}",
        ),
        (
            "optional method receiver",
            "a.ts",
            "function f(x:any){return x?.m();}",
            "function f(x:any){return (x?.m)();}",
        ),
        (
            "JS directive",
            "a.js",
            "function f(){(\"use strict\");return this;}",
            "function f(){\"use strict\";return this;}",
        ),
        (
            "ASI return",
            "a.js",
            "function f(){return (1);}",
            "function f(){return\n(1);}",
        ),
        (
            "Python tuple",
            "a.py",
            "def f(x):\n    return (x,)\n",
            "def f(x):\n    return (x)\n",
        ),
        (
            "Python indentation",
            "a.py",
            "def f(x):\n    if x:\n        print(x)\n    return x\n",
            "def f(x):\n    if x:\n        print(x)\n        return x\n",
        ),
        (
            "Python docstring",
            "a.py",
            "def f():\n    (\"hello\")\n    return 1\n",
            "def f():\n    (\"hello\",)\n    return 1\n",
        ),
        (
            "Python fstring debug",
            "a.py",
            "def f(x):\n    return f\"{x = }\"\n",
            "def f(x):\n    return f\"{x=}\"\n",
        ),
        (
            "Rust tuple",
            "a.rs",
            "fn f(x:i32){let value=(x,);}",
            "fn f(x:i32){let value=(x);}",
        ),
        (
            "Rust macro tokens",
            "a.rs",
            "fn f(){println!(\"a b\");}",
            "fn f(){println!(\"ab\");}",
        ),
        (
            "Rust macro groups",
            "a.rs",
            "fn f(){probe!((x));}",
            "fn f(){probe!(x);}",
        ),
        (
            "Java cast",
            "A.java",
            "class A{int f(double x){return (int)x;}}",
            "class A{int f(double x){return x;}}",
        ),
        (
            "Java text block",
            "A.java",
            "class A{String f(){return \"\"\"\n a b\n \"\"\";}}",
            "class A{String f(){return \"\"\"\n ab\n \"\"\";}}",
        ),
        (
            "Go increment token",
            "a.go",
            "package a\nfunc f(){x++;}\n",
            "package a\nfunc f(){x--; }\n",
        ),
        (
            "Go line semicolon",
            "a.go",
            "package a\nfunc f(x int) int{return (x)}\n",
            "package a\nfunc f(x int) int{return\n(x)}\n",
        ),
        (
            "Vue template spaces",
            "A.vue",
            "<template><span>a b</span></template><script>const a=1;</script>",
            "<template><span>ab</span></template><script>const a=1;</script>",
        ),
    ];
    for (name, path, before, after) in cases {
        let result = analyze(path, before, after, &AtomicBool::new(false)).unwrap();
        // These deliberately isolate a semantic edit without a separate pure-format sibling.
        assert!(
            result
                .coverage
                .iter()
                .all(|c| c.presentation != Presentation::FoldedFormat),
            "{name}: unsafe formatting proof: {:?}",
            result.events
        );
        assert_eq!(
            result.coverage.len(),
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Equal)
                .count(),
            "{name}: missing coverage"
        );
        assert_eq!(
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Insert)
                .map(|l| l.text.as_str())
                .collect::<String>(),
            before,
            "{name}: old source"
        );
        assert_eq!(
            result
                .raw
                .lines
                .iter()
                .filter(|l| l.kind != LineKind::Delete)
                .map(|l| l.text.as_str())
                .collect::<String>(),
            after,
            "{name}: new source"
        );
    }
}

#[test]
fn go_parenthesis_removal_must_preserve_the_closing_tokens_newline() {
    for (before, after) in [
        (
            "package a\nfunc f(ready bool){if ready {return}}\n",
            "package a\nfunc f(ready bool){if (ready)\n{return}}\n",
        ),
        (
            "package a\nfunc f(x int) int{return x+1}\n",
            "package a\nfunc f(x int) int{return (x)\n+1}\n",
        ),
    ] {
        let result = analyze("a.go", before, after, &AtomicBool::new(false)).unwrap();
        assert!(
            result
                .coverage
                .iter()
                .all(|c| c.presentation != Presentation::FoldedFormat),
            "{:?}",
            result.events
        );
    }
}
