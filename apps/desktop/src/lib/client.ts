import { invoke, isTauri } from "@tauri-apps/api/core";
import type { ComparisonManifest, ComparisonSpec, FileAnalysis } from "@diffkin/protocol";

export interface DiffkinClient {
  open(spec: ComparisonSpec, jobId: string): Promise<ComparisonManifest>;
  page(id: string, offset: number): Promise<ComparisonManifest>;
  file(id: string, fileId: string, jobId: string): Promise<FileAnalysis>;
  cancel(jobId: string): Promise<void>;
}

export const desktopAvailable = isTauri();
export const client: DiffkinClient = {
  open: (spec, jobId) => invoke("open_comparison", { spec, jobId }),
  page: (id, offset) => invoke("list_changes", { id, offset, limit: 200 }),
  file: (id, fileId, jobId) => invoke("request_file", { id, fileId, jobId }),
  cancel: (jobId) => invoke("cancel_job", { jobId }),
};

export function message(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return String(error);
}

// Only the latest generation may publish a result or clear the loading indicator.
export class Generation {
  private value = 0;
  next() {
    return ++this.value;
  }
  isCurrent(value: number) {
    return value === this.value;
  }
}
