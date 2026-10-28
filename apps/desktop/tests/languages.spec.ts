import { expect, test } from "@playwright/test";
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree, language } from "@codemirror/language";
import { languageExtensions } from "../src/lib/languages";

const samples = [
  ["counter.mts", "const count: number = 1;", "TypeAnnotation"],
  ["counter.cts", "const count: number = 1;", "TypeAnnotation"],
  ["C:\\repo\\COMPONENT.TSX", "export const Component = () => <button />;", "JSXElement"],
  ["counter.py", "def count(value: int):\n    return value + 1\n", "FunctionDefinition"],
  ["Counter.java", "class Counter { int count() { return 1; } }", "ClassDeclaration"],
  ["counter.go", "package main\nfunc count(value int) int { return value + 1 }\n", "FunctionDecl"],
  ["counter.rs", "fn count(value: i32) -> i32 { value + 1 }", "FunctionItem"],
] as const;

for (const [path, doc, expectedNode] of samples) {
  test(`syntax highlighting parses ${path} without grammar errors`, () => {
    const state = EditorState.create({ doc, extensions: languageExtensions(path, false) });
    const tree = ensureSyntaxTree(state, doc.length, 1000);
    expect(state.facet(language)).not.toBeNull();
    expect(tree?.toString()).toContain(expectedNode);
    const errors: string[] = [];
    tree?.iterate({
      enter: (node) => {
        if (node.type.isError) errors.push(node.name);
      },
    });
    expect(errors).toEqual([]);
  });
}

test("Vue SFC uses mixed template/script parsing and scoped script uses TypeScript", () => {
  const doc =
    '<script setup lang="ts">const count: number = 1;</script>\n<template><p>{{ count }}</p></template>';
  const whole = EditorState.create({ doc, extensions: languageExtensions("Counter.vue", false) });
  const tree = ensureSyntaxTree(whole, doc.length, 1000);
  expect(tree?.toString()).toContain("TypeAnnotation");
  expect(tree?.toString()).toContain("Interpolation");
  const script = "const count: number = 1;";
  const scoped = EditorState.create({
    doc: script,
    extensions: languageExtensions("Counter.vue", true),
  });
  expect(ensureSyntaxTree(scoped, script.length, 1000)?.topNode.name).toBe("Script");
});

test("unknown extensions remain plain text", () => {
  const state = EditorState.create({
    doc: "plain content",
    extensions: languageExtensions("README.md", false),
  });
  expect(state.facet(language)).toBeNull();
});
