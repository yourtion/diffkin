import { expect, test, type Page } from "@playwright/test";
import type { ValidationPlan, ValidationResult } from "@diffkin/protocol";
import { appendLogs, parseArguments } from "../src/lib/validation-client";

test("log tails discard old output and keep an explicit truncation signal", () => {
  const large = appendLogs(
    [{ sequence: 1, stream: "stdout", text: "old output" }],
    [{ sequence: 2, stream: "stderr", text: "x".repeat(70_000) }],
  );
  expect(large.truncated).toBe(true);
  expect(large.entries).toHaveLength(1);
  expect(large.entries[0].sequence).toBe(2);
  expect(large.entries[0].text).toHaveLength(65_536);
  const repeated = appendLogs(large.entries, [{ sequence: 2, stream: "stderr", text: "latest" }]);
  expect(repeated.entries).toEqual([{ sequence: 2, stream: "stderr", text: "latest" }]);
  expect(parseArguments('["a b", "$(echo literal)"]')).toEqual(["a b", "$(echo literal)"]);
});

const plan: ValidationPlan = {
  id: "plan-fixed-1",
  createdAtMs: 1_700_000_000_000,
  repository: "/repo",
  program: "/usr/bin/python3",
  programId: "sha256:python-fixed",
  args: ["--version"],
  workingDirectory: ".",
  timeoutSeconds: 120,
  commandId: "sha256:command-fixed",
  candidate: {
    commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    tree: "candidate-tree",
    contentId: "candidate-source-fixed",
    root: "/validation/plan-fixed-1/candidate",
    fileCount: 3,
    totalBytes: 123,
    complete: true,
    locks: [{ path: "uv.lock", digest: "sha256:lock-fixed" }],
  },
  baseline: null,
  executable: true,
  blockers: [],
  dependencyStatus: "lockfiles-recorded-not-resolved",
  limitations: ["Cross-repository dependency resolution has not been confirmed."],
};
const completed: ValidationResult = {
  id: "result-fixed-1",
  planId: plan.id,
  target: "candidate",
  status: "passed",
  exitCode: 0,
  startedAtMs: 1_700_000_000_000,
  finishedAtMs: 1_700_000_001_000,
  sourceId: plan.candidate.contentId,
  programId: plan.programId,
  commandId: plan.commandId,
  dependencyStatus: plan.dependencyStatus,
  sourceChanged: false,
  changedPaths: [],
  logTail: [{ sequence: 1, stream: "stdout", text: "Python 3.13\n" }],
  logTruncated: false,
  message: "Completed on the fixed candidate source.",
};

async function openValidation(
  page: Page,
  options: { slow?: boolean; dirty?: boolean; blocked?: boolean; historical?: boolean } = {},
) {
  await page.addInitScript(
    ({ plan, completed, options }) => {
      const calls: { command: string; args: Record<string, unknown> }[] = [];
      let prepared = plan;
      let saved: ValidationResult[] = options.historical
        ? [{ ...completed, id: "historical-result", sourceId: "historical-source-identity" }]
        : [];
      let finishRun: ((result: ValidationResult) => void) | undefined;
      let selectedTarget: "candidate" | "baseline" = "candidate";
      Object.defineProperty(window, "isTauri", { value: true });
      Object.defineProperty(window, "__VALIDATION_CALLS__", { value: calls });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (command: string, args: Record<string, unknown>) => {
            calls.push({ command, args });
            if (command === "validation_history")
              return saved.map((item) => ({ ...item, logTail: [] }));
            if (command === "validation_result") return saved.find((item) => item.id === args.id);
            if (command === "validation_prepare") {
              const spec = args.spec as {
                args: string[];
                workingDirectory: string;
                timeoutSeconds: number;
                baselineRef: string | null;
              };
              prepared = {
                ...plan,
                args: spec.args,
                workingDirectory: spec.workingDirectory,
                timeoutSeconds: spec.timeoutSeconds,
                executable: !options.blocked,
                blockers: options.blocked ? ["Executable identity cannot be confirmed."] : [],
                baseline: spec.baselineRef
                  ? {
                      ...plan.candidate,
                      commit: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                      contentId: "baseline-source-fixed",
                    }
                  : null,
              };
              return prepared;
            }
            if (command === "validation_plan") return prepared;
            if (command === "validation_logs") {
              return {
                entries: [{ sequence: 1, stream: "stdout", text: "streaming output\n" }],
                nextSequence: 1,
                truncated: true,
              };
            }
            if (command === "validation_run") {
              selectedTarget = args.target as "candidate" | "baseline";
              const result: ValidationResult = {
                ...completed,
                target: selectedTarget,
                sourceId:
                  selectedTarget === "candidate"
                    ? prepared.candidate.contentId
                    : prepared.baseline!.contentId,
                sourceChanged: !!options.dirty,
                changedPaths: options.dirty ? ["src/main.py"] : [],
              };
              if (!options.slow) {
                saved = [result, ...saved];
                return result;
              }
              return new Promise<ValidationResult>((resolve) => {
                finishRun = (final) => {
                  saved = [final, ...saved];
                  resolve(final);
                };
              });
            }
            if (command === "cancel_job") {
              finishRun?.({
                ...completed,
                target: selectedTarget,
                status: "cancelled",
                exitCode: null,
                message: "Cancelled by user.",
                logTail: [{ sequence: 2, stream: "system", text: "Process group cancelled.\n" }],
              });
              return;
            }
            throw new Error(`Unexpected IPC: ${command}`);
          },
        },
      });
    },
    { plan, completed, options },
  );
  await page.goto("/");
  await page.getByLabel("Git 仓库路径").fill("/repo");
  await page.getByRole("button", { name: "验证", exact: true }).click();
  await expect(page.getByRole("region", { name: "固定版本验证", exact: true })).toBeVisible();
}

async function prepare(page: Page) {
  await page.getByLabel("程序路径或名称", { exact: true }).fill("python3");
  await page.getByLabel("参数（JSON 字符串数组）", { exact: true }).fill('["--version"]');
  await page.getByRole("button", { name: "创建验证计划", exact: true }).click();
  await expect(page.getByRole("region", { name: "已创建的验证计划" })).toBeVisible();
}

test("a fixed, reviewable plan never executes until the user clicks run", async ({ page }) => {
  await openValidation(page);
  await page.getByLabel("程序路径或名称", { exact: true }).fill("python3");
  await page
    .getByLabel("参数（JSON 字符串数组）", { exact: true })
    .fill('["--test", "a b", "$(echo unchanged)"]');
  await page.getByRole("button", { name: "创建验证计划", exact: true }).click();
  const review = page.getByRole("region", { name: "已创建的验证计划" });
  await expect(review).toContainText(plan.candidate.commit);
  await expect(review).toContainText("/usr/bin/python3");
  await expect(review).toContainText('["--test","a b","$(echo unchanged)"]');
  await expect(review).toContainText("不构成安全沙箱");
  expect(
    await page.evaluate(() =>
      (
        window as unknown as { __VALIDATION_CALLS__: { command: string }[] }
      ).__VALIDATION_CALLS__.filter((call) => call.command === "validation_run"),
    ),
  ).toHaveLength(0);
  await page.getByLabel("候选提交或引用", { exact: true }).fill("different-branch");
  await expect(review).toContainText(plan.candidate.commit);
  await page.getByRole("button", { name: "运行候选计划", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "通过 · 单仓库固定版本" })).toBeVisible();
  await expect(page.getByRole("region", { name: "验证执行结果" })).toContainText(
    "candidate-source-fixed",
  );
  const run = await page.evaluate(() =>
    (
      window as unknown as {
        __VALIDATION_CALLS__: { command: string; args: Record<string, unknown> }[];
      }
    ).__VALIDATION_CALLS__.find((call) => call.command === "validation_run"),
  );
  expect(run?.args).toMatchObject({ planId: "plan-fixed-1", target: "candidate" });
  await page.screenshot({ path: "test-results/validation-run.png", fullPage: true });
});

test("baseline execution remains separately bound to its baseline source", async ({ page }) => {
  await openValidation(page);
  await page.getByLabel("同时准备基线", { exact: true }).check();
  await prepare(page);
  await page.getByRole("button", { name: "运行基线计划", exact: true }).click();
  const result = page.getByRole("region", { name: "验证执行结果" });
  await expect(result).toContainText("基线执行");
  await expect(result).toContainText("baseline-source-fixed");
  await expect(result).toContainText("跨仓库依赖未经联合验证确认");
});

test("running logs are bounded and cancellation waits for the backend result", async ({ page }) => {
  await openValidation(page, { slow: true });
  await prepare(page);
  await page.getByRole("button", { name: "运行候选计划", exact: true }).click();
  await expect(page.getByLabel("验证日志", { exact: true })).toContainText("streaming output");
  await expect(page.getByText("日志已截断，仅显示有界尾部。", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "取消运行", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "已取消" })).toBeVisible();
  await expect(page.getByLabel("验证日志", { exact: true })).toContainText(
    "Process group cancelled.",
  );
});

test("a source-modifying command cannot display a passed status", async ({ page }) => {
  await openValidation(page, { dirty: true });
  await prepare(page);
  await page.getByRole("button", { name: "运行候选计划", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "结果不确定 · 验证改写了源码" }),
  ).toBeVisible();
  await expect(page.getByRole("region", { name: "验证执行结果" })).toContainText("src/main.py");
  await expect(page.getByRole("status").filter({ hasText: "通过" })).toHaveCount(0);
});

test("blocked plans and malformed argument arrays do not execute", async ({ page }) => {
  await openValidation(page, { blocked: true });
  await page.getByLabel("程序路径或名称", { exact: true }).fill("python3");
  await page.getByLabel("参数（JSON 字符串数组）", { exact: true }).fill("[1]");
  await page.getByRole("button", { name: "创建验证计划", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("参数必须是 JSON 字符串数组");
  await prepare(page);
  await expect(page.getByRole("button", { name: "运行候选计划", exact: true })).toBeDisabled();
  await expect(page.getByRole("alert")).toContainText("Executable identity cannot be confirmed");
});

test("historical results display their original source identity", async ({ page }) => {
  await openValidation(page, { historical: true });
  const history = page.getByRole("region", { name: "验证历史", exact: true });
  await expect(history).toContainText("historical-source-identity");
  await expect(page.getByRole("region", { name: "验证执行结果" })).toHaveCount(0);
  await history.getByRole("button").filter({ hasText: "historical-source-identity" }).click();
  await expect(page.getByRole("region", { name: "验证执行结果" })).toContainText(
    "historical-source-identity",
  );
  await expect(page.getByLabel("验证日志", { exact: true })).toContainText("Python 3.13");
  await expect(page.getByRole("region", { name: "已创建的验证计划" })).toContainText(
    plan.candidate.commit,
  );
});
