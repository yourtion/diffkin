//! Conservative language seams: grouping and declaration identities are syntax-specific.
use super::{atomic, check_walk, Budget};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(super) fn ungroup<'a>(
    node: Node<'a>,
    language: &str,
    budget: &Budget<'_>,
    source: &str,
) -> Result<Option<Node<'a>>, String> {
    if node.kind() != "parenthesized_expression" || node.named_child_count() != 1 {
        return Ok(None);
    }
    // Java's minimum signed literal is legal only as the direct operand of
    // unary minus. Parentheses may therefore change compile-time validity.
    // Keep the same conservative boundary in Rust's typed numeric expressions.
    if matches!(language, "java" | "rust")
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "unary_expression")
    {
        return Ok(None);
    }
    if language == "go" {
        let rest = &source[node.end_byte()..];
        let next = rest.trim_start().chars().next();
        if next.is_some()
            && next != Some('}')
            && rest
                .chars()
                .take_while(|c| c.is_whitespace())
                .any(|c| matches!(c, '\n' | '\r'))
        {
            // Removing this wrapper would also remove the ')' token which
            // caused an inserted semicolon. Its lexical boundary must survive.
            return Ok(None);
        }
    }
    let child = node.named_child(0).expect("one child");
    // A small scalar-expression domain excludes directives/docstrings, tuples,
    // generators, walrus, casts, lambdas, optional-chain boundaries and macros.
    let mut stack = vec![child];
    let mut count = 0;
    while let Some(current) = stack.pop() {
        check_walk(count, budget)?;
        count += 1;
        if current.is_named()
            && !matches!(
                current.kind(),
                "identifier"
                    | "number"
                    | "integer"
                    | "float"
                    | "integer_literal"
                    | "float_literal"
                    | "decimal_integer_literal"
                    | "decimal_floating_point_literal"
                    | "int_literal"
                    | "true"
                    | "false"
                    | "boolean"
                    | "binary_expression"
                    | "binary_operator"
                    | "boolean_operator"
                    | "unary_expression"
                    | "unary_operator"
                    | "comparison_operator"
                    | "parenthesized_expression"
            )
        {
            return Ok(None);
        }
        if current.kind() == "parenthesized_expression" && current.named_child_count() != 1 {
            return Ok(None);
        }
        for index in 0..current.child_count() {
            stack.push(current.child(index).expect("child"));
        }
    }
    if matches!(language, "javascript" | "typescript" | "tsx") {
        let mut parent = node.parent();
        while let Some(current) = parent {
            if current.kind() == "parenthesized_expression" {
                parent = current.parent();
                continue;
            }
            if matches!(
                current.kind(),
                "member_expression" | "subscript_expression" | "call_expression"
            ) {
                for index in 0..current.child_count() {
                    if current
                        .child(index)
                        .is_some_and(|n| matches!(n.kind(), "optional_chain" | "?."))
                    {
                        return Ok(None);
                    }
                }
            }
            break;
        }
    }
    Ok(Some(child))
}

pub(super) fn indentation_safe(source: &str, language: &str) -> bool {
    language != "python"
        || !source.split(['\n', '\r']).any(|line| {
            line.chars()
                .take_while(|c| c.is_whitespace())
                .any(|c| c != ' ')
        })
}

#[derive(Clone)]
pub(super) struct Entity<'a> {
    pub name: String,
    pub key: String,
    pub scope: String,
    pub node: Node<'a>,
}
fn tokens(node: Node<'_>, source: &str, budget: &Budget<'_>) -> Result<String, String> {
    let mut result = String::new();
    let mut stack = vec![node];
    let mut count = 0;
    while let Some(node) = stack.pop() {
        check_walk(count, budget)?;
        count += 1;
        if node.child_count() == 0 || atomic(node.kind()) {
            result.push_str(node.kind());
            result.push(':');
            result.push_str(&source[node.byte_range()]);
            result.push('|');
        } else {
            for i in (0..node.child_count()).rev() {
                stack.push(node.child(i).expect("child"));
            }
        }
    }
    Ok(result)
}
fn method_signature(
    node: Node<'_>,
    source: &str,
    language: &str,
    budget: &Budget<'_>,
) -> Result<String, String> {
    let mut suffix = String::new();
    if language == "java" {
        if let Some(params) = node.child_by_field_name("parameters") {
            for i in 0..params.named_child_count() {
                let param = params.named_child(i as u32).expect("parameter");
                if let Some(ty) = param.child_by_field_name("type") {
                    suffix.push_str(&tokens(ty, source, budget)?);
                }
                for j in 0..param.named_child_count() {
                    let child = param.named_child(j as u32).expect("child");
                    if child.kind() == "dimensions" {
                        suffix.push_str(&tokens(child, source, budget)?);
                    }
                }
                if param.kind() == "spread_parameter" {
                    suffix.push_str("...");
                }
                suffix.push(';');
            }
        }
    } else if language == "go" {
        if let Some(receiver) = node.child_by_field_name("receiver") {
            for i in 0..receiver.named_child_count() {
                if let Some(ty) = receiver
                    .named_child(i as u32)
                    .and_then(|n| n.child_by_field_name("type"))
                {
                    suffix.push_str(&tokens(ty, source, budget)?);
                }
            }
        }
    } else if matches!(language, "javascript" | "typescript" | "tsx")
        && node.kind() == "method_definition"
    {
        for i in 0..node.child_count() {
            let child = node.child(i).expect("child");
            if matches!(child.kind(), "get" | "set" | "static") {
                suffix.push_str(child.kind());
            }
        }
    }
    Ok(suffix)
}

pub(super) fn entities<'a>(
    root: Node<'a>,
    source: &str,
    language: &str,
    budget: &Budget<'_>,
) -> Result<Vec<Entity<'a>>, String> {
    let mut result = Vec::new();
    let mut scopes = HashMap::<String, usize>::new();
    let mut stack = vec![(root, String::new(), Vec::<String>::new())];
    let mut count = 0;
    let mut ancestry = Vec::new();
    while let Some((node, scope, ancestors)) = stack.pop() {
        check_walk(count, budget)?;
        count += 1;
        let kind = node.kind();
        if matches!(
            kind,
            "function_declaration"
                | "generator_function_declaration"
                | "function_definition"
                | "method_declaration"
                | "constructor_declaration"
                | "function_item"
                | "method_definition"
        ) {
            if let Some(name) = node.child_by_field_name("name") {
                let text = &source[name.byte_range()];
                let suffix = method_signature(node, source, language, budget)?;
                let key = format!("{scope}/{kind}:{text}({suffix})");
                result.push(Entity {
                    name: if scope.is_empty() {
                        text.into()
                    } else {
                        format!("{scope}::{text}")
                    },
                    key,
                    scope: scope.clone(),
                    node,
                });
                ancestry.push(ancestors);
            }
            continue;
        }
        if matches!(language, "javascript" | "typescript" | "tsx") && kind == "variable_declarator"
        {
            if let (Some(name), Some(value)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) {
                if name.kind() == "identifier"
                    && matches!(value.kind(), "arrow_function" | "function_expression")
                {
                    let text = &source[name.byte_range()];
                    result.push(Entity {
                        name: if scope.is_empty() {
                            text.into()
                        } else {
                            format!("{scope}::{text}")
                        },
                        key: format!("{scope}/binding:{text}"),
                        scope: scope.clone(),
                        node: value,
                    });
                    ancestry.push(ancestors);
                }
            }
            continue;
        }
        if matches!(
            kind,
            "class_definition"
                | "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "annotation_type_declaration"
                | "impl_item"
                | "trait_item"
                | "mod_item"
        ) {
            if ancestors.len() >= 64 || scope.len() > 4096 {
                return Err(
                    "Declaration scope depth budget exceeded; original text remains visible".into(),
                );
            }
            if let Some(body) = node.child_by_field_name("body") {
                let label = if kind == "impl_item" {
                    let mut head = String::new();
                    for i in 0..node.child_count() {
                        let c = node.child(i).expect("child");
                        if c.id() != body.id() {
                            head.push_str(&tokens(c, source, budget)?);
                        }
                    }
                    format!("impl[{head}]")
                } else {
                    node.child_by_field_name("name")
                        .map(|n| source[n.byte_range()].to_owned())
                        .unwrap_or_else(|| kind.into())
                };
                let nested = if scope.is_empty() {
                    label
                } else {
                    format!("{scope}::{label}")
                };
                *scopes.entry(nested.clone()).or_default() += 1;
                let mut parents = ancestors;
                parents.push(nested.clone());
                stack.push((body, nested, parents));
            }
            continue;
        }
        // Do not descend into expression or function bodies: only declared containers.
        if matches!(
            kind,
            "program"
                | "module"
                | "source_file"
                | "translation_unit"
                | "class_body"
                | "interface_body"
                | "enum_body"
                | "declaration_list"
                | "block"
                | "export_statement"
                | "decorated_definition"
                | "lexical_declaration"
                | "variable_declaration"
        ) {
            for i in (0..node.named_child_count()).rev() {
                stack.push((
                    node.named_child(i as u32).expect("child"),
                    scope.clone(),
                    ancestors.clone(),
                ));
            }
        }
    }
    Ok(result
        .into_iter()
        .zip(ancestry)
        .filter(|(_, parents)| parents.iter().all(|p| scopes.get(p) == Some(&1)))
        .map(|(entity, _)| entity)
        .collect())
}

pub(super) fn pair<'a>(
    a: Node<'a>,
    b: Node<'a>,
    before: &str,
    after: &str,
    language: &str,
    budget: &Budget<'_>,
) -> Result<Vec<(String, Node<'a>, Node<'a>, bool)>, String> {
    let old = entities(a, before, language, budget)?;
    let new = entities(b, after, language, budget)?;
    let mut old_counts = HashMap::new();
    let mut new_counts = HashMap::new();
    for e in &old {
        *old_counts.entry(e.key.as_str()).or_insert(0) += 1;
    }
    for e in &new {
        *new_counts.entry(e.key.as_str()).or_insert(0) += 1;
    }
    let unique = |key: &str| old_counts.get(key) == Some(&1) && new_counts.get(key) == Some(&1);
    let mut stationary = HashSet::<String>::new();
    let scopes: HashSet<_> = old.iter().map(|e| e.scope.as_str()).collect();
    for scope in scopes {
        budget.check()?;
        let old_order: Vec<_> = old
            .iter()
            .filter(|e| e.scope == scope && unique(&e.key))
            .collect();
        let new_order: Vec<_> = new
            .iter()
            .filter(|e| e.scope == scope && unique(&e.key))
            .collect();
        let positions: HashMap<_, _> = new_order
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.as_str(), i))
            .collect();
        let seq: Vec<_> = old_order
            .iter()
            .map(|e| positions[e.key.as_str()])
            .collect();
        let mut tails: Vec<usize> = vec![];
        let mut predecessors = vec![None; seq.len()];
        for (index, &pos) in seq.iter().enumerate() {
            let slot = tails.partition_point(|&prev| seq[prev] < pos);
            if slot > 0 {
                predecessors[index] = Some(tails[slot - 1]);
            }
            if slot == tails.len() {
                tails.push(index);
            } else {
                tails[slot] = index;
            }
        }
        let mut cursor = tails.last().copied();
        while let Some(index) = cursor {
            stationary.insert(old_order[index].key.clone());
            cursor = predecessors[index];
        }
    }
    let new_nodes: HashMap<_, _> = new.iter().map(|e| (e.key.as_str(), e.node)).collect();
    Ok(old
        .iter()
        .filter(|e| unique(&e.key))
        .map(|e| {
            (
                e.name.clone(),
                e.node,
                new_nodes[e.key.as_str()],
                !stationary.contains(&e.key),
            )
        })
        .collect())
}
