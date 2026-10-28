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
