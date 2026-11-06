import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import type {
  ComparisonManifest,
  ComparisonSpec,
  FileAnalysis,
  FrozenFile,
} from "@diffkin/protocol";

// The opt-in native benchmark observes this boundary without replacing Tauri internals.
async function invoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
  const samples = (
    window as unknown as {
      __diffkinBenchmarkIpc?: { command: string; ms: number }[];
    }
  ).__diffkinBenchmarkIpc;
  if (!samples) return nativeInvoke<T>(command, args);
  const start = performance.now();
  try {
    return await nativeInvoke<T>(command, args);
  } finally {
    samples.push({ command, ms: performance.now() - start });
  }
}

export interface DiffkinClient {
  open(spec: ComparisonSpec, jobId: string): Promise<ComparisonManifest>;
  page(id: string, offset: number): Promise<ComparisonManifest>;
  catalog(id: string): Promise<ComparisonManifest>;
  preview(id: string, fileId: string): Promise<FrozenFile>;
  file(id: string, fileId: string, jobId: string): Promise<FileAnalysis>;
  cancel(jobId: string): Promise<void>;
}

export const desktopAvailable = isTauri();
export const client: DiffkinClient = {
  open: (spec, jobId) => invoke("open_comparison", { spec, jobId }),
  page: (id, offset) => invoke("list_changes", { id, offset, limit: 200 }),
  catalog: (id) => invoke("list_change_catalog", { id }),
  preview: (id, fileId) => invoke("preview_file", { id, fileId }),
  file: (id, fileId, jobId) => invoke("request_file", { id, fileId, jobId }),
  cancel: (jobId) => invoke("cancel_job", { jobId }),
};

export function message(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    if ("code" in error && error.code === "stale") {
      if (error.message === "Workspace active snapshot has changed")
        return "工作区快照已切换，请重新打开工作区并确认要审查的版本。";
      if (error.message === "Workspace snapshot is not retained")
        return "此快照已移出历史记录，请重新打开工作区并选择可用版本。";
      if (
        error.message ===
        "Evidence basis does not match the frozen repository, file, content or analysis rule"
      )
        return "审查依据已变化，请刷新后重新核对文件。";
    }
    if (
      "code" in error &&
      error.code === "budget-exceeded" &&
      error.message === "Snapshot exceeded its capture time budget"
    )
      return "捕获仓库快照超时";
    return String(error.message);
  }
  return String(error);
}

// Only the latest generation may publish a result or clear the loading indicator.
export class Generation {
  private value = 0;
  ticket() {
    return this.value;
  }
  next() {
    return ++this.value;
  }
  isCurrent(value: number) {
    return value === this.value;
  }
}
