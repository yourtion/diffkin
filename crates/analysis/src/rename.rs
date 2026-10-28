//! Conservative local binding correspondence for the first four non-JS languages.
//!
//! A correspondence is navigation evidence, never a proof of behavioral equivalence.
//! Unsupported scopes and ambiguous identifier roles deliberately reject the whole pair.
use std::collections::{BTreeMap, HashMap, HashSet};
use tree_sitter::Node;

type Check<'a> = &'a dyn Fn() -> Result<(), String>;
type SyntaxPath = Vec<String>;

#[derive(Debug)]
struct Binding {
    name: String,
    declaration: usize,
    scope_start: usize,
    role: &'static str,
}

struct Function {
    bindings: Vec<Binding>,
    paths: HashMap<usize, SyntaxPath>,
    references: BTreeMap<SyntaxPath, String>,
    non_references: HashSet<String>,
    free_references: HashSet<String>,
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .collect()
}
fn field_is(node: Node<'_>, field: &str) -> bool {
    node.parent()
        .and_then(|parent| parent.child_by_field_name(field))
        .is_some_and(|candidate| candidate.id() == node.id())
}
fn identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "field_identifier" | "type_identifier" | "package_identifier"
    )
}
fn comment(kind: &str) -> bool {
    matches!(kind, "comment" | "line_comment" | "block_comment")
}
fn reference_role(language: &str, node: Node<'_>, body: Node<'_>) -> bool {
    if node.kind() != "identifier" || node.start_byte() < body.start_byte() {
        return false;
    }
    let mut ancestor = Some(node);
    while let Some(current) = ancestor {
        if current.id() == body.id() {
            break;
        }
        if field_is(current, "type")
            || field_is(current, "return_type")
            || matches!(
                current.kind(),
                "type" | "type_arguments" | "type_parameters"
            )
        {
            return false;
        }
        ancestor = current.parent();
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    match language {
        "python" => {
            !(parent.kind() == "attribute" && field_is(node, "attribute"))
                && !(parent.kind() == "keyword_argument" && field_is(node, "name"))
        }
        "java" => {
            !((parent.kind() == "method_invocation" || parent.kind() == "method_reference")
                && field_is(node, "name"))
                && !(parent.kind() == "field_access" && field_is(node, "field"))
        }
        "go" => !(parent.kind() == "selector_expression" && field_is(node, "field")),
        "rust" => {
            !matches!(parent.kind(), "lifetime" | "label")
                && !(parent.kind() == "field_expression" && field_is(node, "field"))
        }
        _ => false,
    }
}

fn push_binding(
    bindings: &mut Vec<Binding>,
    node: Node<'_>,
    source: &str,
    scope_start: usize,
    role: &'static str,
) -> Result<(), String> {
    if node.kind() != "identifier" {
        return Err(
            "Only a simple identifier binding is supported; patterns remain visible".into(),
        );
    }
    let name = text(node, source);
    if name == "_" || bindings.iter().any(|binding| binding.name == name) {
        return Err("Discarded, redeclared or shadowed bindings are not confirmed".into());
    }
    bindings.push(Binding {
        name: name.into(),
        declaration: node.id(),
        scope_start,
        role,
    });
    Ok(())
}

fn parameter_bindings(
    language: &str,
    node: Node<'_>,
    source: &str,
    body: Node<'_>,
) -> Result<Vec<Binding>, String> {
    let params = node
        .child_by_field_name("parameters")
        .ok_or("Function parameter list is unavailable")?;
    let mut bindings = Vec::new();
    for parameter in named_children(params) {
        if comment(parameter.kind()) {
            continue;
        }
        match language {
            "python" => {
                let name = if parameter.kind() == "identifier" {
                    parameter
                } else if parameter.kind() == "typed_parameter" {
                    named_children(parameter)
                        .into_iter()
                        .find(|child| child.kind() == "identifier" && !field_is(*child, "type"))
                        .ok_or("Unsupported Python typed parameter")?
                } else {
                    return Err(
                        "Python defaults, splats and parameter separators remain visible".into(),
                    );
                };
                push_binding(&mut bindings, name, source, body.start_byte(), "parameter")?;
            }
            "java" => {
                if parameter.kind() != "formal_parameter" {
                    return Err("Java receiver and variadic parameters remain visible".into());
                }
                let name = parameter
                    .child_by_field_name("name")
                    .ok_or("Unsupported Java parameter")?;
                push_binding(&mut bindings, name, source, body.start_byte(), "parameter")?;
            }
            "go" => {
                if parameter.kind() != "parameter_declaration" {
                    return Err("Go variadic parameters remain visible".into());
                }
                let mut cursor = parameter.walk();
                let names: Vec<_> = parameter
                    .children_by_field_name("name", &mut cursor)
                    .collect();
                if names.is_empty() {
                    return Err("Unnamed Go parameters remain visible".into());
                }
                for name in names {
                    push_binding(&mut bindings, name, source, body.start_byte(), "parameter")?;
                }
            }
            "rust" => {
                if parameter.kind() != "parameter" {
                    return Err("Rust self and variadic parameters remain visible".into());
                }
                let name = parameter
                    .child_by_field_name("pattern")
                    .ok_or("Unsupported Rust parameter")?;
                push_binding(&mut bindings, name, source, body.start_byte(), "parameter")?;
            }
            _ => return Err("Local rename language is unsupported".into()),
        }
    }
    Ok(bindings)
}

fn unsafe_kind(language: &str, kind: &str) -> bool {
    if kind.contains("type_parameter") || kind == "generic_parameters" {
        return true;
    }
    match language {
        "python" => matches!(
            kind,
            "lambda"
                | "class_definition"
                | "global_statement"
                | "nonlocal_statement"
                | "list_comprehension"
                | "set_comprehension"
                | "dictionary_comprehension"
                | "generator_expression"
                | "for_statement"
                | "with_statement"
                | "except_clause"
                | "match_statement"
                | "import_statement"
                | "import_from_statement"
                | "future_import_statement"
                | "named_expression"
                | "delete_statement"
        ),
        "java" => {
            matches!(
                kind,
                "lambda_expression"
                    | "class_declaration"
                    | "interface_declaration"
                    | "record_declaration"
                    | "enum_declaration"
                    | "class_body"
                    | "this"
                    | "super"
                    | "field_access"
                    | "method_reference"
                    | "catch_clause"
                    | "for_statement"
                    | "enhanced_for_statement"
                    | "switch_expression"
                    | "switch_statement"
                    | "resource"
                    | "instanceof_expression"
            ) || kind.contains("pattern")
        }
        "go" => matches!(
            kind,
            "func_literal"
                | "short_var_declaration"
                | "type_declaration"
                | "const_declaration"
                | "range_clause"
                | "for_statement"
                | "type_switch_statement"
                | "expression_switch_statement"
                | "receive_statement"
                | "communication_case"
                | "labeled_statement"
                | "goto_statement"
                | "composite_literal"
        ),
        "rust" => matches!(
            kind,
            "macro_invocation"
                | "closure_expression"
                | "async_block"
                | "const_block"
                | "for_expression"
                | "while_expression"
                | "loop_expression"
                | "match_expression"
                | "if_let_expression"
                | "let_condition"
                | "let_chain"
                | "struct_expression"
                | "shorthand_field_initializer"
                | "use_declaration"
                | "type_item"
                | "struct_item"
                | "enum_item"
                | "impl_item"
                | "trait_item"
                | "const_item"
                | "static_item"
        ),
        _ => true,
    }
}
fn callable(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "method_declaration"
            | "constructor_declaration"
            | "function_item"
    )
}

fn local_bindings(
    language: &str,
    node: Node<'_>,
    source: &str,
    body: Node<'_>,
    bindings: &mut Vec<Binding>,
) -> Result<(), String> {
    let kind = node.kind();
    let declaration = match language {
        "python" if kind == "assignment" => {
            if node
                .parent()
                .and_then(|parent| parent.parent())
                .is_none_or(|parent| parent.id() != body.id())
            {
                return Err("Only Python body-level simple assignments are supported".into());
            }
            let name = node
                .child_by_field_name("left")
                .ok_or("Assignment target is unavailable")?;
            if name.kind() != "identifier" {
                return Err("Python unpacking and attribute assignment remain visible".into());
            }
            // Reassignment uses the same function-local binding; only the first assignment declares it.
            if bindings
                .iter()
                .any(|binding| binding.name == text(name, source))
            {
                return Ok(());
            }
            Some((name, node.end_byte()))
        }
        "java" if kind == "variable_declarator" => {
            if node
                .parent()
                .is_none_or(|parent| parent.kind() != "local_variable_declaration")
                || node
                    .parent()
                    .and_then(|parent| parent.parent())
                    .is_none_or(|parent| parent.id() != body.id())
            {
                return Err("Only Java body-level local declarations are supported".into());
            }
            Some((
                node.child_by_field_name("name")
                    .ok_or("Local name is unavailable")?,
                node.end_byte(),
            ))
        }
        "go" if kind == "var_spec" => {
            let parent = node
                .parent()
                .ok_or("Go variable declaration is unavailable")?;
            let declaration = if parent.kind() == "var_spec_list" {
                parent.parent().ok_or("Go var group is unavailable")?
            } else {
                parent
            };
            if declaration.kind() != "var_declaration"
                || declaration.parent().is_none_or(|parent| {
                    parent.kind() != "statement_list"
                        || parent
                            .parent()
                            .is_none_or(|parent| parent.id() != body.id())
                })
            {
                return Err("Only Go body-level var declarations are supported".into());
            }
            let mut cursor = node.walk();
            let names: Vec<_> = node.children_by_field_name("name", &mut cursor).collect();
            if names.is_empty() {
                return Err("Go variable name is unavailable".into());
            }
            for name in names {
                push_binding(bindings, name, source, node.end_byte(), "local")?;
            }
            return Ok(());
        }
        "rust" if kind == "let_declaration" => {
            if node.parent().is_none_or(|parent| parent.id() != body.id()) {
                return Err("Only Rust body-level let declarations are supported".into());
            }
            Some((
                node.child_by_field_name("pattern")
                    .ok_or("Let pattern is unavailable")?,
                node.end_byte(),
            ))
        }
        _ => None,
    };
    if let Some((name, scope_start)) = declaration {
        push_binding(bindings, name, source, scope_start, "local")?;
    }
    Ok(())
}

fn inspect(
    language: &str,
    root: Node<'_>,
    source: &str,
    check: Check<'_>,
) -> Result<Function, String> {
    check()?;
    if root.has_error() || root.is_missing() {
        return Err("Syntax errors prevent binding confirmation".into());
    }
    if language == "go" && root.kind() == "method_declaration" {
        return Err("Go receiver bindings and fields remain visible".into());
    }
    if language == "go"
        && root
            .child_by_field_name("result")
            .is_some_and(|result| result.kind() == "parameter_list")
    {
        return Err("Go named/multiple result bindings remain visible".into());
    }
    if language == "python" {
        let mut parent = root.parent();
        while let Some(current) = parent {
            if current.kind() == "class_definition" {
                return Err("Python class scope requires symbol analysis".into());
            }
            parent = current.parent();
        }
    }
    if language == "java" {
        let name = root
            .child_by_field_name("name")
            .ok_or("Method name is unavailable")?;
        if root.parent().is_some_and(|parent| {
            named_children(parent)
                .iter()
                .filter(|node| {
                    matches!(
                        node.kind(),
                        "method_declaration" | "constructor_declaration"
                    ) && node
                        .child_by_field_name("name")
                        .is_some_and(|other| text(other, source) == text(name, source))
                })
                .count()
                > 1
        }) {
            return Err("Java overloaded declarations require symbol analysis".into());
        }
    }
    let body = root
        .child_by_field_name("body")
        .ok_or("Function body is unavailable")?;
    let mut bindings = parameter_bindings(language, root, source, body)?;
    let mut nodes = Vec::new();
    let mut paths = HashMap::new();
    let mut stack = vec![(root, Vec::<String>::new())];
    let mut cells = 0;
    while let Some((mut node, path)) = stack.pop() {
        if nodes.len().is_multiple_of(128) {
            check()?;
        }
        if nodes.len() >= 80_000 || path.len() > 128 {
            return Err("Local binding analysis budget exceeded".into());
        }
        // Parentheses preserve an expression's binding role. Tuples, casts, patterns and
        // argument-list nodes are not skipped, so this cannot turn them into equivalence.
        while node.kind() == "parenthesized_expression" && node.named_child_count() == 1 {
            node = node.named_child(0).expect("one named child");
        }
        if node.id() != root.id() && callable(node.kind()) {
            return Err("Nested callable scopes or captured bindings remain visible".into());
        }
        if node.id() != body.id() && matches!(node.kind(), "block" | "constructor_body") {
            return Err("Nested block scopes remain visible".into());
        }
        if unsafe_kind(language, node.kind()) {
            return Err(format!(
                "Unsupported {language} binding construct: {}",
                node.kind()
            ));
        }
        if language == "python"
            && node.kind() == "identifier"
            && matches!(
                text(node, source),
                "eval" | "exec" | "locals" | "globals" | "vars"
            )
        {
            return Err("Python dynamic namespace access remains visible".into());
        }
        if language == "java"
            && node.kind() == "method_invocation"
            && node.child_by_field_name("object").is_some()
        {
            return Err("Java member invocation requires symbol analysis".into());
        }
        local_bindings(language, node, source, body, &mut bindings)?;
        paths.insert(node.id(), path.clone());
        nodes.push(node);
        for (index, child) in named_children(node)
            .into_iter()
            .filter(|child| !comment(child.kind()))
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .rev()
        {
            let mut next = path.clone();
            next.push(format!("{}:{index}", node.kind()));
            cells += next.len();
            if cells > 1_000_000 {
                return Err("Local binding path budget exceeded".into());
            }
            stack.push((child, next));
        }
    }
    let declarations: HashSet<_> = bindings.iter().map(|binding| binding.declaration).collect();
    let by_name: HashMap<_, _> = bindings
        .iter()
        .map(|binding| (binding.name.as_str(), binding))
        .collect();
    let mut references = BTreeMap::new();
    let mut non_references = HashSet::new();
    let mut free_references = HashSet::new();
    for (index, node) in nodes.into_iter().enumerate() {
        if index.is_multiple_of(128) {
            check()?;
        }
        if !identifier_kind(node.kind()) || declarations.contains(&node.id()) {
            continue;
        }
        let name = text(node, source);
        if reference_role(language, node, body) {
            if let Some(binding) = by_name.get(name) {
                if node.start_byte() < binding.scope_start {
                    return Err(
                        "Use before a local declaration may resolve an outer binding".into(),
                    );
                }
                references.insert(paths[&node.id()].clone(), name.to_owned());
            } else {
                free_references.insert(name.to_owned());
            }
        } else {
            non_references.insert(name.to_owned());
        }
    }
    Ok(Function {
        bindings,
        paths,
        references,
        non_references,
        free_references,
    })
}

/// Confirm correspondence only inside the explicitly supported local binding domain.
/// Every declaration and every reference of a renamed binding must have a counterpart.
pub fn detect(
    language: &str,
    old: Node<'_>,
    new: Node<'_>,
    before: &str,
    after: &str,
    check: Check<'_>,
) -> Result<Vec<(String, String)>, String> {
    let old = inspect(language, old, before, check)?;
    let new = inspect(language, new, after, check)?;
    if old.bindings.len() != new.bindings.len() {
        return Err("Local declaration set changed; rename not confirmed".into());
    }
    let mut mapping = Vec::new();
    for (a, b) in old.bindings.iter().zip(&new.bindings) {
        if a.role != b.role || old.paths.get(&a.declaration) != new.paths.get(&b.declaration) {
            return Err(
                "Declaration roles or syntax positions changed; rename not confirmed".into(),
            );
        }
        if a.name != b.name {
            mapping.push((a.name.clone(), b.name.clone()));
        }
    }
    if mapping.is_empty() {
        return Ok(mapping);
    }
    let old_names: HashSet<_> = mapping.iter().map(|(old, _)| old.as_str()).collect();
    let new_names: HashSet<_> = mapping.iter().map(|(_, new)| new.as_str()).collect();
    if !old_names.is_disjoint(&new_names) {
        return Err("Overlapping binding renames require symbol analysis".into());
    }
    for name in old_names.iter().chain(&new_names) {
        if old.non_references.contains(*name)
            || new.non_references.contains(*name)
            || old.free_references.contains(*name)
            || new.free_references.contains(*name)
        {
            return Err(
                "A renamed name collides with a property, type, callable or outer reference".into(),
            );
        }
    }
    let map: HashMap<_, _> = mapping
        .iter()
        .map(|(old, new)| (old.as_str(), new.as_str()))
        .collect();
    let expected: BTreeMap<_, _> = old
        .references
        .iter()
        .filter_map(|(path, name)| map.get(name.as_str()).map(|name| (path, *name)))
        .collect();
    let actual: BTreeMap<_, _> = new
        .references
        .iter()
        .filter(|(_, name)| new_names.contains(name.as_str()))
        .map(|(path, name)| (path, name.as_str()))
        .collect();
    if expected != actual {
        return Err(
            "Binding reference roles, counts or positions changed; rename not confirmed".into(),
        );
    }
    check()?;
    Ok(mapping)
}
