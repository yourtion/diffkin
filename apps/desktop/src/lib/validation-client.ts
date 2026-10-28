import { invoke } from "@tauri-apps/api/core";
import type {
  ValidationLog,
  ValidationLogPage,
  ValidationPlan,
  ValidationResult,
  ValidationSpec,
} from "@diffkin/protocol";

export const validationClient = {
  prepare: (spec: ValidationSpec, jobId: string): Promise<ValidationPlan> =>
    invoke("validation_prepare", { spec, jobId }),
  run: (
    planId: string,
    target: "candidate" | "baseline",
    jobId: string,
  ): Promise<ValidationResult> => invoke("validation_run", { planId, target, jobId }),
  cancel: (jobId: string): Promise<void> => invoke("cancel_job", { jobId }),
  logs: (jobId: string, afterSequence: number): Promise<ValidationLogPage> =>
    invoke("validation_logs", { jobId, afterSequence }),
  history: (): Promise<ValidationResult[]> => invoke("validation_history", { limit: 30 }),
  plan: (planId: string): Promise<ValidationPlan> => invoke("validation_plan", { planId }),
  result: (id: string): Promise<ValidationResult> => invoke("validation_result", { id }),
};

export function parseArguments(input: string): string[] {
  const value: unknown = JSON.parse(input);
  if (!Array.isArray(value) || !value.every((item): item is string => typeof item === "string")) {
    throw new Error('参数必须是 JSON 字符串数组，例如 ["--version"]。');
  }
  return value;
}

export function appendLogs(current: ValidationLog[], incoming: ValidationLog[]) {
  const bySequence = new Map(current.map((entry) => [entry.sequence, entry]));
  for (const entry of incoming) bySequence.set(entry.sequence, entry);
  const ordered = [...bySequence.values()].sort((a, b) => a.sequence - b.sequence);
  const tail: ValidationLog[] = [];
  let truncated = false;
  let characters = 0;
  for (let index = ordered.length - 1; index >= 0 && tail.length < 400; index--) {
    const entry = ordered[index];
    const remaining = 65_536 - characters;
    if (remaining <= 0) break;
    const text = entry.text.slice(-remaining);
    truncated ||= text.length !== entry.text.length;
    tail.unshift({ ...entry, text });
    characters += text.length;
  }
  return {
    entries: tail,
    truncated: truncated || tail.length !== ordered.length,
  };
}
