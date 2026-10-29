import type { AnalysisResult, ChangeEvent } from "@diffkin/protocol";

export function isProvenFormat(event: ChangeEvent, result: AnalysisResult): boolean {
  return (
    event.facets.length === 1 &&
    event.facets[0] === "format" &&
    event.rawEditIds.length > 0 &&
    event.rawEditIds.every((id) =>
      result.coverage.some(
        (entry) =>
          entry.rawEditId === id &&
          entry.presentation === "folded-format" &&
          entry.eventIds.includes(event.id),
      ),
    )
  );
}

/** Build once per immutable analysis when checking many events in the same render. */
export function createFormatProofChecker(result: AnalysisResult) {
  const coverage = new Map<number, typeof result.coverage>();
  for (const entry of result.coverage) {
    const entries = coverage.get(entry.rawEditId) ?? [];
    entries.push(entry);
    coverage.set(entry.rawEditId, entries);
  }
  return (event: ChangeEvent): boolean =>
    event.facets.length === 1 &&
    event.facets[0] === "format" &&
    event.rawEditIds.length > 0 &&
    event.rawEditIds.every((id) =>
      (coverage.get(id) ?? []).some(
        (entry) => entry.presentation === "folded-format" && entry.eventIds.includes(event.id),
      ),
    );
}
