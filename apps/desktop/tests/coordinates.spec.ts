import { test, expect } from "@playwright/test";
import { editorCoordinates } from "../src/lib/editor-coordinates";
import { createFormatProofChecker, isProvenFormat } from "../src/lib/presentation";
import type { AnalysisResult, ChangeEvent } from "@diffkin/protocol";

test("indexed coordinates preserve CRLF, lone CR and Unicode offsets", () => {
  for (const source of ["", "plain\ntext", "\r\n中文😀e\u0301\ra\nb\r\n", "x\r\n".repeat(5000)]) {
    const coordinate = editorCoordinates(source);
    for (let offset = 0; offset <= source.length; offset++)
      expect(coordinate(offset)).toBe(source.slice(0, offset).replace(/\r\n?/g, "\n").length);
  }
});

test("indexed proof checks retain all evidence requirements and duplicate coverage semantics", () => {
  const event: ChangeEvent = {
    id: "format",
    title: "format",
    facets: ["format"],
    before: null,
    after: null,
    rawEditIds: [0, 1],
    children: [],
    evidence: [],
  };
  const result: AnalysisResult = {
    schemaVersion: 1,
    ruleVersion: "test",
    language: "typescript",
    raw: { schemaVersion: 1, lines: [], added: 0, removed: 0 },
    events: [event],
    fallbacks: [],
    coverage: [
      { rawEditId: 0, presentation: "folded-format", eventIds: ["format"] },
      { rawEditId: 0, presentation: "visible", eventIds: [] },
      { rawEditId: 1, presentation: "folded-format", eventIds: ["format"] },
    ],
  };
  for (const coverage of [
    result.coverage,
    result.coverage.slice(0, 2),
    [],
    [{ rawEditId: 0, presentation: "text-fallback" as const, eventIds: ["format"] }],
  ]) {
    const analysis = { ...result, coverage };
    const check = createFormatProofChecker(analysis);
    for (const candidate of [
      event,
      { ...event, facets: ["format", "edit"] },
      { ...event, rawEditIds: [] },
      { ...event, id: "wrong-proof" },
    ])
      expect(check(candidate)).toBe(isProvenFormat(candidate, analysis));
  }
});
