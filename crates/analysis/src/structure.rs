use crate::{compare, line_breaks, source_line_count, MAX_BYTES, MAX_LINES};
use diffkin_domain::{
    AnalysisResult, ChangeEvent, CompareError, CompareRequest, Comparison, CoverageEntry, LineKind,
    Presentation, SourceRange, SCHEMA_VERSION,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tree_sitter::{Language, Node, ParseOptions, Parser, Tree};

#[path = "rename.rs"]
mod language_renames;
#[path = "language_syntax.rs"]
mod language_syntax;

pub const RULE_VERSION: &str = "syntax-v2";
const MAX_NODES: usize = 100_000;
const STRUCTURE_BUDGET: Duration = Duration::from_secs(1);

struct Budget<'a> {
    cancel: &'a AtomicBool,
    deadline: Instant,
}
impl Budget<'_> {
    fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("Analysis cancelled".into())
        } else if Instant::now() >= self.deadline {
            Err("Structure budget exceeded; original edits remain visible".into())
        } else {
            Ok(())
        }
    }
}

pub fn source_range(source: &str, start: usize, end: usize) -> SourceRange {
    debug_assert!(start <= end && source.is_char_boundary(start) && source.is_char_boundary(end));
    SourceRange {
        byte_start: start as u32,
        byte_end: end as u32,
        utf16_start: source[..start].encode_utf16().count() as u32,
        utf16_end: source[..end].encode_utf16().count() as u32,
        start_line: line_breaks(&source[..start]) as u32 + 1,
        end_line: line_breaks(&source[..end]) as u32 + 1,
    }
}

fn language(path: &str) -> Option<(&'static str, Language)> {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "js" | "mjs" | "cjs" | "jsx" => {
            Some(("javascript", tree_sitter_javascript::LANGUAGE.into()))
        }
        "ts" | "mts" | "cts" => Some((
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        )),
        "tsx" => Some(("tsx", tree_sitter_typescript::LANGUAGE_TSX.into())),
        "py" | "pyi" => Some(("python", tree_sitter_python::LANGUAGE.into())),
        "java" => Some(("java", tree_sitter_java::LANGUAGE.into())),
        "go" => Some(("go", tree_sitter_go::LANGUAGE.into())),
        "rs" => Some(("rust", tree_sitter_rust::LANGUAGE.into())),
        _ => None,
    }
}

fn parse(source: &str, language: &Language, budget: &Budget<'_>) -> Result<Tree, String> {
    budget.check()?;
    let mut parser = Parser::new();
    parser.set_language(language).map_err(|e| e.to_string())?;
    let mut progress = |_: &tree_sitter::ParseState| {
        if budget.check().is_err() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let mut input = |offset: usize, _: tree_sitter::Point| &source.as_bytes()[offset..];
    let tree = parser
        .parse_with_options(
            &mut input,
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or_else(|| {
            "Parser cancelled or exhausted its budget; original edits remain visible".to_string()
        })?;
    if tree.root_node().has_error() {
        return Err("Syntax error or unsupported syntax; original edits remain visible".into());
    }
    Ok(tree)
}

// Atomic text-bearing nodes include every byte, not just their named children.
fn atomic(kind: &str) -> bool {
    kind.ends_with("string_literal")
        || matches!(
            kind,
            "string"
                | "template_string"
                | "regex"
                | "jsx_text"
                | "html_character_reference"
                | "comment"
                | "hash_bang_line"
                | "character_literal"
                | "char_literal"
                | "rune_literal"
                | "text_block"
                | "concatenated_string"
                | "macro_invocation"
                | "macro_definition"
                | "token_tree"
                | "line_comment"
                | "block_comment"
        )
}
fn check_walk(count: usize, budget: &Budget<'_>) -> Result<(), String> {
    if count >= MAX_NODES {
        return Err("Syntax node budget exceeded".into());
    }
    if count.is_multiple_of(256) {
        budget.check()?;
    }
    Ok(())
}

fn sensitive(node: Node<'_>, budget: &Budget<'_>) -> Result<bool, String> {
    let mut stack = vec![node];
    let mut count = 0;
    while let Some(node) = stack.pop() {
        check_walk(count, budget)?;
        count += 1;
        if matches!(
            node.kind(),
            "comment"
                | "hash_bang_line"
                | "jsx_element"
                | "jsx_self_closing_element"
                | "template_string"
                | "line_comment"
                | "block_comment"
                | "macro_invocation"
                | "macro_definition"
                | "token_tree"
                | "attribute_item"
                | "inner_attribute_item"
        ) {
            return Ok(true);
        }
        for i in 0..node.child_count() {
            stack.push(node.child(i).expect("child"));
        }
    }
    Ok(false)
}

fn signature(
    node: Node<'_>,
    source: &str,
    names: &HashMap<String, String>,
    budget: &Budget<'_>,
    language: &str,
) -> Result<Vec<String>, String> {
    let mut signature = Vec::new();
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if signature.len() >= MAX_NODES {
            return Err("Syntax node budget exceeded".into());
        }
        if signature.len().is_multiple_of(256) {
            budget.check()?;
        }
        if let Some(inner) = language_syntax::ungroup(node, language, budget, source)? {
            stack.push(inner);
            continue;
        }
        let terminal = node.child_count() == 0 || atomic(node.kind());
        let value = if terminal {
            let raw = &source[node.byte_range()];
            if node.kind() == "identifier" {
                names.get(raw).map(String::as_str).unwrap_or(raw)
            } else {
                raw
            }
        } else {
            ""
        };
        signature.push(format!(
            "{}:{}:{}",
            node.kind(),
            if terminal { 0 } else { node.child_count() },
            value
        ));
        // Error-recovering grammars can assign similar CSTs across restricted
        // productions (notably `return\n(x)`). Preserve the lexical line break.
        if matches!(
            language,
            "javascript" | "typescript" | "tsx" | "go" | "python"
        ) && matches!(
            node.kind(),
            "return"
                | "throw"
                | "break"
                | "continue"
                | "yield"
                | "async"
                | "fallthrough"
                | "raise"
                | "assert"
                | "++"
                | "--"
        ) {
            let after_newline = source[node.end_byte()..]
                .chars()
                .take_while(|c| c.is_whitespace() || *c == '\u{feff}')
                .any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'));
            signature.push(format!("restricted-newline-after:{after_newline}"));
            if matches!(node.kind(), "++" | "--") {
                let before_newline = source[..node.start_byte()]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_whitespace() || *c == '\u{feff}')
                    .any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'));
                signature.push(format!("restricted-newline-before:{before_newline}"));
            }
        }
        if language == "go"
            && terminal
            && matches!(
                node.kind(),
                "identifier"
                    | "field_identifier"
                    | "type_identifier"
                    | "int_literal"
                    | "float_literal"
                    | "imaginary_literal"
                    | "rune_literal"
                    | "interpreted_string_literal"
                    | "raw_string_literal"
                    | "break"
                    | "continue"
                    | "fallthrough"
                    | "raise"
                    | "assert"
                    | "return"
                    | "++"
                    | "--"
                    | ")"
                    | "]"
                    | "}"
            )
        {
            let rest = &source[node.end_byte()..];
            let next = rest.trim_start().chars().next();
            // Go inserts a semicolon after these tokens at a newline. The
            // semicolon may be omitted before }, and EOF inserts it either way.
            let newline = next.is_some()
                && next != Some('}')
                && rest
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .any(|c| matches!(c, '\n' | '\r'));
            signature.push(format!("go-inserted-semicolon:{newline}"));
        }
        if !terminal {
            for i in (0..node.child_count()).rev() {
                stack.push(node.child(i).expect("child"));
            }
        }
    }
    Ok(signature)
}

// Supported binding domain: a function's simple parameters and simple body-level let/const.
// Any additional scope, dynamic lookup, destructuring, shorthand or label prevents confirmation.
fn bindings(node: Node<'_>, source: &str, budget: &Budget<'_>) -> Option<Vec<String>> {
    let body = node.child_by_field_name("body")?;
    let params = node.child_by_field_name("parameters")?;
    let mut declarations = Vec::new();
    for i in 0..params.named_child_count() {
        let param = params.named_child(i as u32)?;
        let id = if param.kind() == "identifier" {
            param
        } else if param.kind() == "required_parameter" {
            if param.child_by_field_name("value").is_some() {
                return None;
            }
            param.child_by_field_name("pattern")?
        } else {
            return None;
        };
        if id.kind() != "identifier" {
            return None;
        }
        declarations.push(source[id.byte_range()].to_owned());
    }
    let mut stack = vec![body];
    let mut count = 0;
    while let Some(current) = stack.pop() {
        check_walk(count, budget).ok()?;
        count += 1;
        let kind = current.kind();
        if matches!(
            kind,
            "function_declaration"
                | "function_expression"
                | "arrow_function"
                | "generator_function_declaration"
                | "generator_function"
                | "class_declaration"
                | "class"
                | "with_statement"
                | "catch_clause"
                | "for_statement"
                | "for_in_statement"
                | "switch_statement"
                | "shorthand_property_identifier"
                | "shorthand_property_identifier_pattern"
                | "object_pattern"
                | "array_pattern"
                | "labeled_statement"
                | "variable_declaration"
                | "yield_expression"
                | "await_expression"
                | "method_definition"
                | "function_type"
                | "constructor_type"
                | "call_signature"
                | "method_signature"
                | "abstract_method_signature"
                | "function_signature"
                | "interface_declaration"
                | "type_alias_declaration"
                | "internal_module"
                | "module"
                | "enum_declaration"
                | "ambient_declaration"
        ) {
            return None;
        }
        if kind == "statement_block" && current.id() != body.id() {
            return None;
        }
        if kind == "identifier" && matches!(&source[current.byte_range()], "eval" | "arguments") {
            return None;
        }
        if kind == "variable_declarator" {
            let name = current.child_by_field_name("name")?;
            if name.kind() != "identifier" || current.parent()?.parent()?.id() != body.id() {
                return None;
            }
            declarations.push(source[name.byte_range()].to_owned());
        }
        for i in (0..current.child_count()).rev() {
            stack.push(current.child(i)?);
        }
    }
    let distinct: HashSet<_> = declarations.iter().collect();
    if distinct.len() != declarations.len() {
        return None;
    }
    Some(declarations)
}

fn identifier_paths(
    node: Node<'_>,
    source: &str,
    names: &HashSet<String>,
    budget: &Budget<'_>,
) -> Result<BTreeMap<Vec<usize>, String>, String> {
    let mut result = BTreeMap::new();
    let mut stack = vec![(node, vec![])];
    let mut count = 0;
    let mut path_cells = 0;
    while let Some((node, path)) = stack.pop() {
        check_walk(count, budget)?;
        count += 1;
        if path.len() > 256 {
            return Err("Binding syntax depth budget exceeded".into());
        }
        if node.kind() == "identifier" && names.contains(&source[node.byte_range()]) {
            result.insert(path.clone(), source[node.byte_range()].to_owned());
        }
        for i in (0..node.child_count()).rev() {
            let mut child_path = path.clone();
            child_path.push(i as usize);
            path_cells += child_path.len();
            if path_cells > 1_000_000 {
                return Err("Binding path memory budget exceeded".into());
            }
            stack.push((node.child(i).expect("child"), child_path));
        }
    }
    Ok(result)
}

fn renames(
    old: Node<'_>,
    new: Node<'_>,
    before: &str,
    after: &str,
    budget: &Budget<'_>,
) -> Result<Vec<(String, String)>, String> {
    let (Some(a), Some(b)) = (bindings(old, before, budget), bindings(new, after, budget)) else {
        return Err("Local bindings not confirmed: nested/dynamic scope, shorthand, destructuring or unsupported declaration; original edits remain visible".into());
    };
    if a.len() != b.len() {
        return Err("Local declaration set changed; rename not confirmed".into());
    }
    let mapping: Vec<_> = a
        .iter()
        .zip(&b)
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    if mapping.is_empty() {
        return Ok(vec![]);
    }
    if mapping.iter().any(|(a, b)| {
        matches!(a.as_str(), "eval" | "arguments") || matches!(b.as_str(), "eval" | "arguments")
    }) {
        return Err("Dynamic binding name; rename not confirmed".into());
    }
    let a_names: HashSet<String> = mapping.iter().map(|(a, _)| a.clone()).collect();
    let b_names: HashSet<String> = mapping.iter().map(|(_, b)| b.clone()).collect();
    if !a_names.is_disjoint(&b_names) {
        return Err("Overlapping binding rename; original edits remain visible".into());
    }
    if !identifier_paths(old, before, &b_names, budget)?.is_empty()
        || !identifier_paths(new, after, &a_names, budget)?.is_empty()
    {
        return Err(
            "Binding name collides with an existing reference; rename not confirmed".into(),
        );
    }
    let old_uses = identifier_paths(old, before, &a_names, budget)?;
    let new_uses = identifier_paths(new, after, &b_names, budget)?;
    let mapping_map: HashMap<_, _> = mapping.iter().cloned().collect();
    if old_uses.len() != new_uses.len()
        || old_uses
            .iter()
            .any(|(path, name)| mapping_map.get(name) != new_uses.get(path))
    {
        return Err("Binding reference paths changed; additions/removals remain raw edits".into());
    }
    Ok(mapping)
}

fn event(
    title: String,
    facets: Vec<&str>,
    before: &str,
    after: &str,
    a: Option<(usize, usize)>,
    b: Option<(usize, usize)>,
    evidence: Vec<String>,
) -> ChangeEvent {
    ChangeEvent {
        id: String::new(),
        title,
        facets: facets.into_iter().map(str::to_owned).collect(),
        before: a.map(|(s, e)| source_range(before, s, e)),
        after: b.map(|(s, e)| source_range(after, s, e)),
        evidence,
        raw_edit_ids: vec![],
        children: vec![],
    }
}

fn whole_lines(node: Node<'_>, source: &str) -> Option<(usize, usize)> {
    let start = source[..node.start_byte()]
        .rfind(['\n', '\r'])
        .map_or(0, |i| i + 1);
    let mut end = source[node.end_byte()..]
        .find(['\n', '\r'])
        .map_or(source.len(), |i| node.end_byte() + i + 1);
    if end > 0 && source.as_bytes()[end - 1] == b'\r' && source.as_bytes().get(end) == Some(&b'\n')
    {
        end += 1;
    }
    if source[start..node.start_byte()]
        .chars()
        .all(char::is_whitespace)
        && source[node.end_byte()..end]
            .chars()
            .all(char::is_whitespace)
    {
        Some((start, end))
    } else {
        None
    }
}

fn format_regions(
    a: Node<'_>,
    b: Node<'_>,
    before: &str,
    after: &str,
    language: &str,
    budget: &Budget<'_>,
) -> Result<Vec<ChangeEvent>, String> {
    if !language_syntax::indentation_safe(before, language)
        || !language_syntax::indentation_safe(after, language)
    {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    let mut stack = vec![(a, b)];
    let mut count = 0;
    let names = HashMap::new();
    while let Some((old, new)) = stack.pop() {
        check_walk(count, budget)?;
        count += 1;
        if old.kind() != new.kind() || before[old.byte_range()] == after[new.byte_range()] {
            continue;
        }
        if !sensitive(old, budget)?
            && !sensitive(new, budget)?
            && signature(old, before, &names, budget, language)?
                == signature(new, after, &names, budget, language)?
        {
            if let (Some(a), Some(b)) = (whole_lines(old, before), whole_lines(new, after)) {
                result.push(event("局部排版变化".into(),vec!["format"],before,after,Some(a),Some(b),vec!["local-cst-trivia-v2: corresponding subtree has identical safe-normalized syntax/tokens; the entire covered original lines contain only this subtree and whitespace".into()]));
                continue;
            }
        }
        if atomic(old.kind()) || old.named_child_count() != new.named_child_count() {
            continue;
        }
        for i in (0..old.named_child_count()).rev() {
            let oa = old.named_child(i as u32).expect("child");
            let nb = new.named_child(i as u32).expect("child");
            // Nested scopes are not paired by ordinal position.
            if matches!(
                oa.kind(),
                "function_declaration"
                    | "function_definition"
                    | "function_item"
                    | "method_declaration"
                    | "method_definition"
                    | "class_declaration"
                    | "class_definition"
                    | "arrow_function"
                    | "lambda"
            ) {
                continue;
            }
            stack.push((oa, nb));
        }
    }
    Ok(result)
}

fn compare_trees(
    before: &str,
    after: &str,
    a: Node<'_>,
    b: Node<'_>,
    budget: &Budget<'_>,
    fallbacks: &mut Vec<String>,
    language: &str,
) -> Result<Vec<ChangeEvent>, String> {
    let empty = HashMap::new();
    if before == after {
        return Ok(vec![]);
    }
    if signature(a, before, &empty, budget, language)?
        == signature(b, after, &empty, budget, language)?
        && !sensitive(a, budget)?
        && !sensitive(b, budget)?
        && language_syntax::indentation_safe(before, language)
        && language_syntax::indentation_safe(after, language)
        && before.starts_with('\u{feff}') == after.starts_with('\u{feff}')
    {
        return Ok(vec![event("仅排版变化".into(),vec!["format"],before,after,Some((0,before.len())),Some((0,after.len())),vec!["cst-trivia-v2: complete syntax shape and tokens match after safe scalar grouping normalization; no sensitive region".into()])]);
    }
    let mut events = Vec::new();
    for (name, old, new, moved) in language_syntax::pair(a, b, before, after, language, budget)? {
        budget.check()?;
        let old_raw = &before[old.byte_range()];
        let new_raw = &after[new.byte_range()];
        if old_raw == new_raw && !moved {
            continue;
        }
        let old_signature = signature(old, before, &empty, budget, language)?;
        let new_signature = signature(new, after, &empty, budget, language)?;
        let rename_result = if matches!(language, "python" | "java" | "go" | "rust") {
            language_renames::detect(language, old, new, before, after, &|| budget.check())
        } else {
            renames(old, new, before, after, budget)
        };
        let renamed = match rename_result {
            Ok(names) => names,
            Err(reason) => {
                fallbacks.push(format!("{name}: {reason}"));
                vec![]
            }
        };
        let normalization: HashMap<String, String> = renamed.iter().cloned().collect();
        let normalized_equal = !renamed.is_empty()
            && signature(old, before, &normalization, budget, language)? == new_signature;
        let format = old_signature == new_signature
            && !sensitive(old, budget)?
            && !sensitive(new, budget)?
            && old_raw != new_raw
            && language_syntax::indentation_safe(before, language)
            && language_syntax::indentation_safe(after, language);
        let edit = old_signature != new_signature && !normalized_equal;
        let mut facets = Vec::new();
        let mut evidence = Vec::new();
        if moved {
            facets.push("move");
            evidence.push("unique-function-order-v1: same unique scope/name/signature; outside the longest unchanged relative-order anchor sequence (order may affect behavior; equally minimal reorderings may exist)".into());
        }
        if !renamed.is_empty() {
            facets.push("rename");
            evidence.push(format!("simple-local-binding-v1: {}. Declaration and every identifier-reference syntax path correspond; no nested/dynamic/shorthand scope. Rename remains visible: callers/keyword arguments and runtime binding effects are not proven equivalent",renamed.iter().map(|(a,b)|format!("{a} → {b}")).collect::<Vec<_>>().join(", ")));
        }
        if format {
            facets.push("format");
            evidence.push("cst-trivia-v2: complete function structure/tokens match after explicitly safe scalar grouping normalization".into());
        }
        if edit {
            facets.push("edit");
            evidence.push("raw-edit-v1: internal edits remain visible".into());
        }
        if facets.is_empty() {
            continue;
        }
        events.push(event(
            name.clone(),
            facets,
            before,
            after,
            Some(if format && !moved {
                whole_lines(old, before).unwrap_or((old.start_byte(), old.end_byte()))
            } else {
                (old.start_byte(), old.end_byte())
            }),
            Some(if format && !moved {
                whole_lines(new, after).unwrap_or((new.start_byte(), new.end_byte()))
            } else {
                (new.start_byte(), new.end_byte())
            }),
            evidence,
        ));
        if edit {
            events.push(event(format!("{name} 内部修改"),vec!["edit"],before,after,Some((old.start_byte(),old.end_byte())),Some((new.start_byte(),new.end_byte())),vec!["Review the paired original function bodies; movement/rename does not prove behavioral equivalence".into()]));
            events.extend(format_regions(old, new, before, after, language, budget)?);
        }
    }
    if a.named_child_count() == b.named_child_count() {
        for i in 0..a.named_child_count() {
            let old = a.named_child(i as u32).expect("child");
            let new = b.named_child(i as u32).expect("child");
            if matches!(
                old.kind(),
                "lexical_declaration"
                    | "expression_statement"
                    | "import_statement"
                    | "const_declaration"
                    | "var_declaration"
                    | "let_declaration"
            ) && old.kind() == new.kind()
            {
                events.extend(format_regions(old, new, before, after, language, budget)?);
            }
        }
    }
    Ok(events)
}

struct ScriptBlock {
    start: usize,
    end: usize,
    header: String,
    language: String,
}
fn vue_scripts(source: &str, budget: &Budget<'_>) -> Result<Vec<ScriptBlock>, String> {
    let tree = parse(source, &tree_sitter_html::LANGUAGE.into(), budget)?;
    let root = tree.root_node();
    let mut scripts = Vec::new();
    let mut modes = HashSet::new();
    for i in 0..root.named_child_count() {
        let node = root.named_child(i as u32).expect("child");
        if node.kind() != "script_element" {
            continue;
        }
        let mut start_tag = None;
        let mut content = None;
        let mut end_tag = None;
        for j in 0..node.named_child_count() {
            let child = node.named_child(j as u32).expect("child");
            match child.kind() {
                "start_tag" => start_tag = Some(child),
                "raw_text" => content = Some(child),
                "end_tag" => end_tag = Some(child),
                _ => {}
            }
        }
        let (Some(tag), Some(end)) = (start_tag, end_tag) else {
            return Err("Incomplete Vue script block; text fallback".into());
        };
        let header = &source[tag.byte_range()];
        let mut lang = "js".to_string();
        let mut setup = false;
        for j in 0..tag.named_child_count() {
            let attr = tag.named_child(j as u32).expect("child");
            if attr.kind() != "attribute" {
                continue;
            }
            let name = attr
                .named_child(0)
                .map(|n| &source[n.byte_range()])
                .unwrap_or("");
            if name == "setup" {
                setup = true;
            }
            if name == "src" {
                return Err("Vue external script src is not loaded; text fallback".into());
            }
            if name == "lang" {
                let value = attr
                    .named_child(1)
                    .map(|n| source[n.byte_range()].trim_matches(['\'', '"']))
                    .unwrap_or("");
                lang = value.to_string();
            }
        }
        if !modes.insert(setup) {
            return Err("Duplicate Vue script mode; text fallback".into());
        }
        if !matches!(lang.as_str(), "js" | "ts" | "jsx" | "tsx") {
            return Err(format!(
                "Unsupported Vue script language {lang}; text fallback"
            ));
        }
        scripts.push(ScriptBlock {
            start: content.map_or(tag.end_byte(), |n| n.start_byte()),
            end: content.map_or(end.start_byte(), |n| n.end_byte()),
            header: header.to_owned(),
            language: lang,
        });
    }
    if scripts.len() > 2 {
        return Err("Ambiguous Vue script blocks; text fallback".into());
    }
    Ok(scripts)
}

fn vue_events(
    before: &str,
    after: &str,
    budget: &Budget<'_>,
    fallbacks: &mut Vec<String>,
) -> Result<Vec<ChangeEvent>, String> {
    let a = vue_scripts(before, budget)?;
    let b = vue_scripts(after, budget)?;
    if a.len() != b.len() {
        return Err("Vue block set changed; text fallback".into());
    }
    let mut events = Vec::new();
    for (a, b) in a.iter().zip(&b) {
        if a.header != b.header {
            continue;
        }
        let old = &before[a.start..a.end];
        let new = &after[b.start..b.end];
        let (name, lang) = language(&format!("block.{}", a.language)).expect("supported script");
        let old_tree = parse(old, &lang, budget)?;
        let new_tree = parse(new, &lang, budget)?;
        for mut item in compare_trees(
            old,
            new,
            old_tree.root_node(),
            new_tree.root_node(),
            budget,
            fallbacks,
            name,
        )? {
            item.before = item.before.map(|r| {
                source_range(
                    before,
                    a.start + r.byte_start as usize,
                    a.start + r.byte_end as usize,
                )
            });
            item.after = item.after.map(|r| {
                source_range(
                    after,
                    b.start + r.byte_start as usize,
                    b.start + r.byte_end as usize,
                )
            });
            item.evidence.push("vue-script-block-v1: original SFC offsets; template/style and block attributes stay in raw view".into());
            events.push(item);
        }
    }
    Ok(events)
}

fn intersects(range: &SourceRange, start: usize, end: usize) -> bool {
    (range.byte_start as usize) < end && range.byte_end as usize > start
}
fn contains(range: &SourceRange, start: usize, end: usize) -> bool {
    range.byte_start as usize <= start && range.byte_end as usize >= end
}

fn encloses_event(parent: &ChangeEvent, child: &ChangeEvent) -> bool {
    [&parent.before, &parent.after]
        .iter()
        .zip([&child.before, &child.after])
        .all(|(parent, child)| match (parent.as_ref(), child.as_ref()) {
            (Some(parent), Some(child)) => {
                parent.byte_start <= child.byte_start && parent.byte_end >= child.byte_end
            }
            _ => false,
        })
}

fn coverage(
    raw: &Comparison,
    events: &mut [ChangeEvent],
    fallback: bool,
    budget: Option<&Budget<'_>>,
) -> Result<Vec<CoverageEntry>, String> {
    let mut steps: usize = 0;
    let mut old_offset = 0;
    let mut new_offset = 0;
    let mut coverage = Vec::new();
    for (index, line) in raw.lines.iter().enumerate() {
        let (start, end, old) = match line.kind {
            LineKind::Equal => {
                old_offset += line.text.len();
                new_offset += line.text.len();
                continue;
            }
            LineKind::Delete => {
                let start = old_offset;
                old_offset += line.text.len();
                (start, old_offset, true)
            }
            LineKind::Insert => {
                let start = new_offset;
                new_offset += line.text.len();
                (start, new_offset, false)
            }
        };
        let mut intersecting = Vec::new();
        for (event_index, item) in events.iter_mut().enumerate() {
            steps += 1;
            if steps.is_multiple_of(256) {
                if let Some(budget) = budget {
                    budget.check()?;
                }
            }
            let span = if old { &item.before } else { &item.after };
            if span
                .as_ref()
                .is_some_and(|span| intersects(span, start, end))
            {
                item.raw_edit_ids.push(index as u32);
                intersecting.push(event_index);
            }
        }
        let proofs: Vec<_> = intersecting
            .iter()
            .copied()
            .filter(|&i| {
                let proof = &events[i];
                let span = if old { &proof.before } else { &proof.after };
                proof.facets == ["format"]
                    && span.as_ref().is_some_and(|span| contains(span, start, end))
                    && intersecting.iter().all(|&j| {
                        events[j].facets == ["format"] || encloses_event(&events[j], proof)
                    })
            })
            .collect();
        let format = !proofs.is_empty();
        let ids = if format {
            proofs.iter().map(|&i| events[i].id.clone()).collect()
        } else {
            intersecting.iter().map(|&i| events[i].id.clone()).collect()
        };
        coverage.push(CoverageEntry {
            raw_edit_id: index as u32,
            presentation: if fallback {
                Presentation::TextFallback
            } else if format {
                Presentation::FoldedFormat
            } else {
                Presentation::Visible
            },
            event_ids: ids,
        });
    }
    Ok(coverage)
}

/// Analyze immutable UTF-8 sources. Unsupported syntax always retains the raw diff.
pub fn analyze(
    path: &str,
    before: &str,
    after: &str,
    cancel: &AtomicBool,
) -> Result<AnalysisResult, CompareError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(CompareError::Cancelled {
            message: "Analysis cancelled".into(),
        });
    }
    let raw = compare(&CompareRequest {
        before: before.into(),
        after: after.into(),
    })?;
    let budget = Budget {
        cancel,
        deadline: Instant::now() + STRUCTURE_BUDGET,
    };
    let mut result = AnalysisResult {
        schema_version: SCHEMA_VERSION,
        language: "text".into(),
        rule_version: RULE_VERSION.into(),
        raw,
        events: vec![],
        coverage: vec![],
        fallbacks: vec![],
    };
    if before == after {
        return Ok(result);
    }
    let is_vue = path.to_ascii_lowercase().ends_with(".vue");
    let analyzed = if is_vue {
        result.language = "vue".into();
        result.fallbacks.push("Vue template/style and block attributes use raw text; cross-block bindings are not inferred".into());
        vue_events(before, after, &budget, &mut result.fallbacks)
    } else if let Some((name, language)) = language(path) {
        result.language = name.into();
        (|| {
            let a = parse(before, &language, &budget)?;
            let b = parse(after, &language, &budget)?;
            compare_trees(
                before,
                after,
                a.root_node(),
                b.root_node(),
                &budget,
                &mut result.fallbacks,
                name,
            )
        })()
    } else {
        Err("Unsupported structural language; original text edits remain visible".into())
    };
    let analyzed = analyzed.and_then(|events| budget.check().map(|()| events));
    let failed = analyzed.is_err();
    match analyzed {
        Ok(events) => result.events = events,
        Err(reason) => result.fallbacks.push(reason),
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(CompareError::Cancelled {
            message: "Analysis cancelled".into(),
        });
    }
    for (index, item) in result.events.iter_mut().enumerate() {
        item.id = format!("event-{index}");
    }
    // Pair explicit internal-edit children without allowing cycles or duplicate coverage accounting.
    for index in 1..result.events.len() {
        if result.events[index].title.ends_with(" 内部修改") {
            let id = result.events[index].id.clone();
            result.events[index - 1].children.push(id);
        }
    }
    for index in 0..result.events.len() {
        if result.events[index].facets == ["format"] {
            if let Some(parent) = (0..index).rev().find(|&p| {
                result.events[p].facets != ["format"]
                    && !result.events[p].title.ends_with(" 内部修改")
                    && encloses_event(&result.events[p], &result.events[index])
            }) {
                let id = result.events[index].id.clone();
                result.events[parent].children.push(id);
            }
        }
    }
    if result.events.is_empty() && result.fallbacks.is_empty() {
        result.fallbacks.push(
            "No supported structural relation confirmed; original edits remain visible".into(),
        );
    }
    result.coverage = match coverage(&result.raw, &mut result.events, failed, Some(&budget)) {
        Ok(coverage) => coverage,
        Err(reason) => {
            result.events.clear();
            result.fallbacks.push(reason);
            coverage(&result.raw, &mut result.events, true, None)
                .expect("empty event coverage is bounded")
        }
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(CompareError::Cancelled {
            message: "Analysis cancelled".into(),
        });
    }
    Ok(result)
}

/// Explicit support limits for callers that choose a large-file presentation before analysis.
pub fn supports_detail(before: &str, after: &str) -> bool {
    [before, after]
        .iter()
        .all(|source| source.len() <= MAX_BYTES && source_line_count(source) <= MAX_LINES)
}
