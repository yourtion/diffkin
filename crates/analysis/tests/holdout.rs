//! Supplemental combinations written after the first 105-case implementation.
//! Author-generated engineering checks, not independent user validation.
use diffkin_analysis::analyze;
use diffkin_domain::Presentation;
use std::sync::atomic::AtomicBool;

#[test]
fn nested_method_scope_is_not_a_confirmed_local_rename() {
    let before = "function outer(value){return {method(value){return value;},result:value};}";
    let after = "function outer(amount){return {method(amount){return amount;},result:amount};}";
    let result = analyze("holdout.js", before, after, &AtomicBool::new(false)).unwrap();
    assert!(
        !result
            .events
            .iter()
            .any(|e| e.facets.iter().any(|f| f == "rename")),
        "nested method has a distinct binding scope"
    );
    assert!(
        !result.fallbacks.is_empty(),
        "unsupported binding scope must be explained"
    );
}

#[test]
fn mixed_movement_rename_condition_keeps_all_edits_visible() {
    let before="function approve(balance:number){const available=balance-1;return available>0;}\nfunction audit(){return 0;}\n";
    let after="function audit() {\n return 0;\n}\nfunction approve(balance: number) {\n const remaining = balance - 1;\n return remaining >= 0;\n}\n";
    let result = analyze("holdout.ts", before, after, &AtomicBool::new(false)).unwrap();
    let item = result.events.iter().find(|e| e.title == "approve").unwrap();
    for facet in ["move", "rename", "edit"] {
        assert!(item.facets.iter().any(|f| f == facet), "missing {facet}");
    }
    assert!(!item.children.is_empty());
    let delta = result
        .coverage
        .iter()
        .find(|e| {
            result.raw.lines[e.raw_edit_id as usize]
                .text
                .contains("remaining >= 0")
        })
        .unwrap();
    assert_ne!(delta.presentation, Presentation::FoldedFormat);
}

#[test]
fn unsafe_combinations_never_gain_format_proofs() {
    let cases = [
        (
            "ASI with movement",
            "case.js",
            "function a(){return value;}\nfunction b(){return 2;}",
            "function b(){return 2;}\nfunction a(){return\n value;}",
        ),
        (
            "optional-chain grouping with formatting",
            "case.ts",
            "function f(x:any){return x?.a.b;}",
            "function f(x: any) {\n return (x?.a).b;\n}",
        ),
        (
            "directive moved off target",
            "case.ts",
            "// @ts-ignore\nconst n:number=\"x\";",
            "// @ts-ignore\nrun();\nconst n: number = \"x\";",
        ),
        (
            "template binding renamed",
            "case.vue",
            "<template><p>{{total}}</p></template>\n<script setup>const total=1;</script>",
            "<template><p>{{amount}}</p></template>\n<script setup>const amount = 1;</script>",
        ),
        (
            "template slot scope changed",
            "case.vue",
            "<template><Slot v-slot=\"{value}\">{{value}}</Slot></template>",
            "<template><Slot v-slot=\"{amount}\">{{amount}}</Slot></template>",
        ),
        (
            "TS overload parameter rename",
            "case.ts",
            "function f(value:string):string;\nfunction f(value:any){return value;}",
            "function f(amount:string):string;\nfunction f(amount:any){return amount;}",
        ),
        (
            "JS this arrow capture",
            "case.js",
            "function f(value){return ()=>({value});}",
            "function f(amount){return ()=>({amount});}",
        ),
        (
            "escaped directive string",
            "case.js",
            "\"use strict\";\nfunction f(){return this;}",
            "\"use\\x20strict\";\nfunction f(){return this;}",
        ),
        (
            "computed property text",
            "case.ts",
            "function f(value:number){return {[\"value\"]:value};}",
            "function f(amount:number){return {[\"amount\"]:amount};}",
        ),
        (
            "regex division boundary",
            "case.js",
            "function f(x){return /a b/.test(x);}",
            "function f(x) { return /ab/.test(x); }",
        ),
    ];
    for (name, path, before, after) in cases {
        let result = analyze(path, before, after, &AtomicBool::new(false)).unwrap();
        assert!(
            result
                .coverage
                .iter()
                .all(|e| e.presentation != Presentation::FoldedFormat),
            "{name}"
        );
    }
}

#[test]
fn cr_only_and_mixed_line_endings_keep_original_coordinates() {
    use diffkin_domain::LineKind;
    let before = "const a=1;\rconst b=2;\r\nconst c=3;\n";
    let after = "const a=1;\rconst b=4;\r\nconst c=3;\n";
    let result = analyze("cr.ts", before, after, &AtomicBool::new(false)).unwrap();
    let deleted = result
        .raw
        .lines
        .iter()
        .find(|line| line.kind == LineKind::Delete)
        .unwrap();
    assert_eq!(deleted.old_line, Some(2));
    assert_eq!(deleted.text, "const b=2;\r\n");
    let b = before.find("const b").unwrap();
    let c = before.find("const c").unwrap();
    assert_eq!(diffkin_analysis::source_range(before, b, c).start_line, 2);
    assert_eq!(diffkin_analysis::source_range(before, b, c).end_line, 3);
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
    assert!(!diffkin_analysis::supports_detail(&"x\r".repeat(5001), ""));
}
