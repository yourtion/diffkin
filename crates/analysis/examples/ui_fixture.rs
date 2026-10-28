//! Deterministic real-kernel inputs for browser integration tests (transport is mocked).
use diffkin_analysis::analyze;
use serde_json::json;
use std::sync::atomic::AtomicBool;

fn main() {
    let mut cases = vec![
        ("case.js", "function approve(balance){return balance>0;}\nfunction audit(){return 0;}\n", "function audit(){return 0;}\nfunction approve(available){return available>=0;}\n"),
        ("case.ts", "function approve(balance:number){return balance>0;}\nfunction audit(){return 0;}\n", "function audit(){return 0;}\nfunction approve(available:number){return available>=0;}\n"),
        ("Case.vue", "<script>\nfunction approve(balance){return balance>0;}\nfunction audit(){return 0;}\n</script>", "<script>\nfunction audit(){return 0;}\nfunction approve(available){return available>=0;}\n</script>"),
        ("case.py", "def approve(balance):\n    return balance > 0\n\ndef audit():\n    return 0\n", "def audit():\n    return 0\n\ndef approve(available):\n    return available >= 0\n"),
        ("Case.java", "class Case { boolean approve(int balance){return balance>0;}\nint audit(){return 0;} }", "class Case { int audit(){return 0;}\nboolean approve(int available){return available>=0;} }"),
        ("case.go", "package sample\nfunc approve(balance int) bool {return balance>0}\nfunc audit() int {return 0}\n", "package sample\nfunc audit() int {return 0}\nfunc approve(available int) bool {return available>=0}\n"),
        ("case.rs", "fn approve(balance:i32)->bool{balance>0}\nfn audit()->i32{0}\n", "fn audit()->i32{0}\nfn approve(available:i32)->bool{available>=0}\n"),
    ];
    cases.extend([
        (
            "format/case.js",
            "function f(x){return (x+1);}\n",
            "function f(x) {\n  return x + 1;\n}\n",
        ),
        (
            "format/case.ts",
            "function f(x:number){return (x+1);}\n",
            "function f(x: number) {\n  return x + 1;\n}\n",
        ),
        (
            "format/Case.vue",
            "<script>\nfunction f(x){return (x+1);}\n</script>\n",
            "<script>\nfunction f(x) {\n  return x + 1;\n}\n</script>\n",
        ),
        (
            "format/case.py",
            "def f(x):\n    return (x+1)\n",
            "def f(x):\n    return x + 1\n",
        ),
        (
            "format/Case.java",
            "class Case {int f(int x){return (x+1);}}\n",
            "class Case {\n int f(int x) {\n  return x + 1;\n }\n}\n",
        ),
        (
            "format/case.go",
            "package sample\nfunc f(x int) int{return (x+1)}\n",
            "package sample\nfunc f(x int) int {\n return x + 1\n}\n",
        ),
        (
            "format/case.rs",
            "fn f(x:i32)->i32{return (x+1);}\n",
            "fn f(x: i32) -> i32 {\n return x + 1;\n}\n",
        ),
    ]);
    let files: Vec<_> = cases.iter().enumerate().map(|(index,(path,before,after))| {
        let summary = json!({"id":format!("case-{index}"),"path":path,"oldPath":null,"status":"modified","oldMode":"100644","newMode":"100644","beforeId":format!("before-{index}"),"afterId":format!("after-{index}"),"notices":[]});
        json!({"file":{"summary":summary,"before":before,"after":after,"beforeTruncated":false,"afterTruncated":false},"notice":null,"analysis":analyze(path,before,after,&AtomicBool::new(false)).unwrap()})
    }).collect();
    let manifest = json!({"id":"kernel-multilanguage","repository":"/kernel-fixture","mode":"commits","baseLabel":"fixture-before","headLabel":"fixture-after","files":files.iter().map(|f|f["file"]["summary"].clone()).collect::<Vec<_>>(),"totalFiles":files.len(),"warnings":[]});
    println!("{}", json!({"manifest":manifest,"files":files}));
}
