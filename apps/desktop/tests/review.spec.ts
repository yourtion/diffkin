import { test, expect, type Page } from "@playwright/test";
import type {
  ComparisonManifest,
  ComparisonSpec,
  FileAnalysis,
  FileSummary,
} from "@diffkin/protocol";

const before =
  "export function canWithdraw(balance: number, amount: number) {\n  return balance > amount;\n}\n";
const after =
  "export function canWithdraw(balance: number, amount: number) {\n  return balance >= amount;\n}\n";
const summary = (id: string, path: string): FileSummary => ({
  id,
  path,
  oldPath: null,
  status: "modified",
  oldMode: "100644",
  newMode: "100644",
  beforeId: `old-${id}`,
  afterId: `new-${id}`,
  notices: [],
});
const first = summary("first", "src/withdraw.ts");
const second = summary("second", "src/other.ts");
const manifest: ComparisonManifest = {
  id: "frozen-1",
  repository: "/repo",
  mode: "commits",
  baseLabel: "commit:aaa",
  headLabel: "commit:bbb",
  files: [first, second],
  totalFiles: 2,
  warnings: [],
};
const result: FileAnalysis = {
  file: { summary: first, before, after, beforeTruncated: false, afterTruncated: false },
  notice: null,
  analysis: {
    schemaVersion: 1,
    language: "typescript",
    ruleVersion: "test-v1",
    raw: {
      schemaVersion: 1,
      added: 1,
      removed: 1,
      lines: [
        { kind: "equal", oldLine: 1, newLine: 1, text: before.split("\n")[0] + "\n" },
        { kind: "delete", oldLine: 2, newLine: null, text: "  return balance > amount;\n" },
        { kind: "insert", oldLine: null, newLine: 2, text: "  return balance >= amount;\n" },
        { kind: "equal", oldLine: 3, newLine: 3, text: "}\n" },
      ],
    },
    events: [
      {
        id: "edit-1",
        title: "canWithdraw: 条件修改",
        facets: ["edit"],
        before: {
          byteStart: 0,
          byteEnd: before.length,
          utf16Start: 0,
          utf16End: before.length,
          startLine: 1,
          endLine: 3,
        },
        after: {
          byteStart: 0,
          byteEnd: after.length,
          utf16Start: 0,
          utf16End: after.length,
          startLine: 1,
          endLine: 3,
        },
        evidence: ["Original condition token changed from > to >=."],
        rawEditIds: [1, 2],
        children: [],
      },
    ],
    coverage: [
      { rawEditId: 1, presentation: "visible", eventIds: ["edit-1"] },
      { rawEditId: 2, presentation: "visible", eventIds: ["edit-1"] },
    ],
    fallbacks: [],
  },
};

async function bridge(
  page: Page,
  options: {
    delayFirst?: number;
    delayPreview?: number;
    delayCapture?: number;
    warnings?: string[];
    refreshIdentity?: boolean;
    refreshRules?: boolean;
    delayPage?: number;
    count?: number;
    failRefresh?: boolean;
    budgetFailure?: "paths" | "time";
    explanationDelay?: number;
    missingCli?: boolean;
    failQuizRetry?: boolean;
  } = {},
  payload: FileAnalysis = result,
) {
  await page.addInitScript(
    ({ manifest, result, options }) => {
      if (options.count)
        manifest = {
          ...manifest,
          totalFiles: options.count,
          files: Array.from({ length: options.count }, (_, i) => ({
            ...manifest.files[0],
            id: `file-${i}`,
            path: `packages/p${Math.floor(i / 100)}/file-${String(i).padStart(4, "0")}.ts`,
          })),
        };
      const calls: string[] = [];
      let captures = 0;
      Object.defineProperty(window, "isTauri", { value: true });
      Object.defineProperty(window, "__DIFFKIN_TEST_CALLS__", { value: calls });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (command: string, args: Record<string, unknown>) => {
            calls.push(command);
            if (command === "preferences_load" || command === "preferences_save") return null;
            if (command === "quiz_prepare") {
              return {
                id: `quiz:${args.rawStart ?? "file"}`,
                questionCount: 3,
                added: 1,
                removed: 1,
                evidence: [
                  {
                    id: 1,
                    path: "src/withdraw.ts",
                    diff: "- L2 | return balance > amount;\n+ L2 | return balance >= amount;",
                  },
                ],
                tools: [
                  { provider: "codex", executable: options.missingCli ? null : "/opt/bin/codex" },
                ],
              };
            }
            if (command === "quiz_generate") {
              if (options.failQuizRetry && calls.filter((c) => c === command).length > 1)
                throw new Error("模型服务暂时不可用");
              if (options.explanationDelay)
                await new Promise((r) => setTimeout(r, options.explanationDelay));
              const req = args.request as { preparedId: string };
              return {
                preparedId: req.preparedId,
                quiz: {
                  questions: [
                    {
                      prompt: "余额等于金额时，新的条件返回什么？",
                      choices: ["true", "false", "undefined", "抛错"],
                      correctIndex: 0,
                      explanation: "等号使相等的输入通过。",
                      evidenceIds: [1],
                    },
                    {
                      prompt: "哪一组输入最能区分新旧实现？",
                      choices: ["5 和 5", "8 和 4", "2 和 7", "5 和 0"],
                      correctIndex: 0,
                      explanation: "只有两者相等时判断不同。",
                      evidenceIds: [1],
                    },
                    {
                      prompt: "余额小于金额时会怎样？",
                      choices: ["现在通过", "仍然拒绝", "抛出错误", "修改金额"],
                      correctIndex: 1,
                      explanation: "小于时两个条件都为假。",
                      evidenceIds: [1],
                    },
                  ],
                },
              };
            }
            if (command === "explanation_prepare") {
              const excerpt = (text: string) => ({
                startLine: 1,
                endLine: 3,
                focusStart: 2,
                focusEnd: 2,
                text,
              });
              return {
                id: `prepared:${args.eventId ?? args.rawStart ?? "file"}`,
                context: {
                  comparisonId: args.comparisonId,
                  fileId: args.fileId,
                  eventId: args.eventId,
                  beforeId: "old-first",
                  afterId: "new-first",
                  path: "src/withdraw.ts",
                  title: "取款边界",
                  before: excerpt(result.file.before!),
                  after: excerpt(result.file.after!),
                },
                tools: [
                  { provider: "codex", executable: options.missingCli ? null : "/opt/bin/codex" },
                  { provider: "claude", executable: null },
                ],
              };
            }
            if (command === "explanation_run") {
              if (options.explanationDelay)
                await new Promise((r) => setTimeout(r, options.explanationDelay));
              const req = args.request as { preparedId: string; provider: string };
              return {
                preparedId: req.preparedId,
                provider: req.provider,
                explanation: {
                  meaning: "判断是否可取款",
                  change: "余额等于金额时现在允许取款。",
                  rationale: "可能是为了允许取出全部余额。",
                  caveats: ["需要核实账户规则。"],
                  references: [{ side: "after", startLine: 2, endLine: 2 }],
                },
              };
            }
            if (command === "cancel_job") return;
            if (command === "open_comparison") {
              captures++;
              if (options.budgetFailure && !(args.spec as { pathScope?: string }).pathScope)
                throw {
                  code: "budget-exceeded",
                  message:
                    options.budgetFailure === "time"
                      ? "Snapshot exceeded its capture time budget"
                      : "Comparison exceeds 2,000 changed paths",
                };
              if (options.delayCapture)
                await new Promise((resolve) => setTimeout(resolve, options.delayCapture));
              if (options.failRefresh && captures > 1) throw new Error("capture failed");
              return {
                ...manifest,
                repository: (args.spec as ComparisonSpec).repository,
                mode: (args.spec as ComparisonSpec).mode,
                warnings: options.warnings ?? [],
                id: options.refreshIdentity ? `frozen-${captures}` : manifest.id,
                totalFiles: options.delayPage ? 3 : manifest.totalFiles,
                files: options.count ? manifest.files.slice(0, 200) : manifest.files,
              };
            }
            if (command === "list_change_catalog") return manifest;
            if (command === "list_changes") {
              if (options.delayPage) {
                const generation = captures;
                await new Promise((resolve) =>
                  setTimeout(resolve, options.delayPage! * generation),
                );
                throw new Error(`stale page ${generation}`);
              }
              const offset = Number(args.offset);
              return { ...manifest, files: manifest.files.slice(offset, offset + 200) };
            }
            if (command === "preview_file") {
              if (args.fileId === "first" && options.delayPreview)
                await new Promise((resolve) => setTimeout(resolve, options.delayPreview));
              return args.fileId === "first"
                ? result.file
                : {
                    ...result.file,
                    summary: manifest.files[1],
                    before: "const other = 1;\n",
                    after: "const other = 2;\n",
                  };
            }
            if (command === "request_file") {
              if (options.count)
                return {
                  ...result,
                  file: {
                    ...result.file,
                    summary: manifest.files.find((file) => file.id === args.fileId),
                  },
                };
              if (args.fileId === "first" && options.delayFirst)
                await new Promise((resolve) => setTimeout(resolve, options.delayFirst));
              if (args.fileId === "second")
                return {
                  file: {
                    ...result.file,
                    summary: manifest.files[1],
                    before: "const other = 1;\n",
                    after: "const other = 2;\n",
                  },
                  analysis: null,
                  notice: "Test text fallback",
                };
              return options.refreshRules && captures >= 2 && result.analysis
                ? { ...result, analysis: { ...result.analysis, ruleVersion: "test-v2" } }
                : result;
            }
            throw new Error(`Unexpected bridge call: ${command}`);
          },
        },
      });
    },
    { manifest, result: payload, options },
  );
  await page.goto("/");
  await page.getByRole("button", { name: "单仓库比较", exact: true }).click();
}
async function open(page: Page, panels = true) {
  await page.getByLabel("Git 仓库路径").fill("/repo");
  await page.getByRole("button", { name: "开始比较", exact: true }).click();
  if (panels) {
    await page.getByRole("button", { name: "变更详情", exact: true }).click();
  }
}

test("repository review supports raw diff, evidence, search, layout and bound review state", async ({
  page,
}) => {
  await bridge(page);
  await open(page);
  await expect(page.locator(".file-heading h2")).toHaveText("src/withdraw.ts");
  await expect(page.locator(".cm-mergeView")).toContainText("balance >= amount");
  await page.getByRole("button", { name: /条件修改/ }).click();
  await expect(page.getByRole("heading", { name: "判断依据" })).toBeVisible();
  await expect(page.locator(".evidence")).toContainText("Original condition token changed");
  await page.getByRole("button", { name: "统一", exact: true }).click();
  await expect(page.locator(".cm-deletedChunk")).toBeVisible();
  await page.getByRole("button", { name: /搜索 ⌘/ }).click();
  await expect(page.locator(".cm-search")).toBeVisible();
  await page.locator(".cm-search input[name=search]").fill("balance");
  await page.getByRole("button", { name: "标记已审查" }).click();
  await expect(page.getByRole("button", { name: "✓ 已审查此版本" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.screenshot({ path: "test-results/review.png", fullPage: true });
  await page.reload();
  await page.getByRole("button", { name: "单仓库比较", exact: true }).click();
  await open(page);
  await expect(page.getByRole("button", { name: "✓ 已审查此版本" })).toBeVisible();
});

test("late responses cannot replace the newly selected file", async ({ page }) => {
  await bridge(page, { delayFirst: 700 });
  await open(page);
  await page.getByRole("button", { name: /src\/other.ts/ }).click();
  await expect(page.locator(".file-heading h2")).toHaveText("src/other.ts");
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await page.waitForTimeout(850);
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await expect(page.locator(".merge-host")).not.toContainText("canWithdraw");
  const calls = await page.evaluate(
    () => (window as unknown as { __DIFFKIN_TEST_CALLS__: string[] }).__DIFFKIN_TEST_CALLS__,
  );
  expect(calls).toContain("cancel_job");
});

test("cancelled capture never publishes its late manifest", async ({ page }) => {
  await bridge(page, { delayCapture: 500 });
  await open(page, false);
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("已取消");
  await page.waitForTimeout(650);
  await expect(page.locator(".file-item")).toHaveCount(0);
});

test("file filtering and keyboard navigation remain usable", async ({ page }) => {
  await bridge(page);
  await open(page);
  await expect(page.locator(".file-item")).toHaveCount(2);
  await page.getByRole("textbox", { name: "筛选文件" }).fill("other");
  await expect(page.locator(".file-item")).toHaveCount(1);
  await page.getByRole("button", { name: /src\/other.ts/ }).click();
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await page.getByRole("textbox", { name: "筛选文件" }).fill("");
  await page.locator(".app-header").click();
  await page.keyboard.press("k");
  await expect(page.locator(".file-heading h2")).toHaveText("src/withdraw.ts");
});

test("browser preview never claims to read a native repository", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "单仓库比较", exact: true }).click();
  await expect(
    page.getByText("浏览器仅预览界面。请运行桌面应用以读取本地 Git 仓库。"),
  ).toBeVisible();
  await page.getByLabel("Git 仓库路径").fill("/repo");
  await expect(page.getByRole("button", { name: "开始比较", exact: true })).toBeDisabled();
});

test("only proven formatting collapses and the exact source can be restored", async ({ page }) => {
  const old = "function same(){return 1;}\n";
  const next = "function same() {\n  return 1;\n}\n";
  const payload: FileAnalysis = structuredClone(result);
  payload.file.before = old;
  payload.file.after = next;
  const analysis = payload.analysis!;
  analysis.events = [
    {
      id: "format",
      title: "仅排版变化",
      facets: ["format"],
      before: {
        byteStart: 0,
        byteEnd: old.length,
        utf16Start: 0,
        utf16End: old.length,
        startLine: 1,
        endLine: 2,
      },
      after: {
        byteStart: 0,
        byteEnd: next.length,
        utf16Start: 0,
        utf16End: next.length,
        startLine: 1,
        endLine: 4,
      },
      evidence: ["CST and tokens identical"],
      rawEditIds: [0, 1, 2, 3],
      children: [],
    },
  ];
  analysis.raw = {
    schemaVersion: 1,
    added: 3,
    removed: 1,
    lines: [
      { kind: "delete", oldLine: 1, newLine: null, text: old },
      ...next
        .trimEnd()
        .split("\n")
        .map((text, index) => ({
          kind: "insert" as const,
          oldLine: null,
          newLine: index + 1,
          text: text + "\n",
        })),
    ],
  };
  analysis.coverage = [0, 1, 2, 3].map((rawEditId) => ({
    rawEditId,
    presentation: "folded-format",
    eventIds: ["format"],
  }));
  await bridge(page, {}, payload);
  await open(page);
  await expect(page.getByRole("button", { name: "展开已证明的格式变化", exact: true })).toHaveCount(
    2,
  );
  await expect(page.locator(".merge-host")).not.toContainText("return 1", { useInnerText: true });
  await page.getByRole("button", { name: "统一", exact: true }).click();
  await expect(page.locator(".merge-host")).not.toContainText("return 1", { useInnerText: true });
  await page.getByRole("button", { name: "展开已证明的格式变化", exact: true }).click();
  await expect(page.locator(".cm-deletedChunk")).toBeVisible();
  await expect(page.locator(".merge-host")).toContainText("return 1", { useInnerText: true });
  await page.getByRole("button", { name: "双栏", exact: true }).click();
  await page.getByRole("button", { name: "完整原始视图", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("return 1");
  await expect(page.getByRole("button", { name: "展开已证明的格式变化", exact: true })).toHaveCount(
    0,
  );
  await page.getByRole("button", { name: "收起格式", exact: true }).click();
  await page.getByRole("button", { name: "展开已证明的格式变化", exact: true }).first().click();
  await expect(page.locator(".merge-host")).toContainText("return 1");
});

test("metadata-only files expose identities without a misleading empty comparison", async ({
  page,
}) => {
  const payload: FileAnalysis = {
    file: {
      ...result.file,
      before: null,
      after: null,
      summary: {
        ...first,
        oldMode: "160000",
        newMode: "160000",
        notices: ["Submodule commit changed"],
      },
    },
    analysis: null,
    notice: "Submodule: inspect object IDs",
  };
  await bridge(page, {}, payload);
  await open(page);
  await expect(page.locator(".merge-host")).toHaveCount(0);
  await page.locator(".source-identities summary").click();
  await expect(page.locator(".source-identities")).toContainText("old-first");
  await expect(page.locator(".source-identities")).toContainText("new-first");
});

test("a refreshed basis does not inherit reviewed status", async ({ page }) => {
  await bridge(page, { refreshIdentity: true });
  await open(page);
  await page.getByRole("button", { name: "标记已审查" }).click();
  await expect(page.getByRole("button", { name: "✓ 已审查此版本" })).toBeVisible();
  await page.getByRole("button", { name: "刷新比较" }).click();
  await expect(page.getByRole("button", { name: "标记已审查" })).toBeVisible();
});

test("changed analysis rules invalidate a review even for identical source snapshots", async ({
  page,
}) => {
  await bridge(page, { refreshRules: true });
  await open(page);
  await page.getByRole("button", { name: "标记已审查" }).click();
  await page.getByRole("button", { name: "刷新比较" }).click();
  await expect(page.locator(".analysis-info")).toContainText("test-v2");
  await expect(page.getByRole("button", { name: "标记已审查" })).toBeVisible();
});

test("late pagination errors cannot overwrite a new comparison or unlock its pending page", async ({
  page,
}) => {
  await bridge(page, { refreshIdentity: true, delayPage: 800 });
  await open(page);
  await page.getByRole("button", { name: /加载更多/ }).click();
  await page.getByRole("button", { name: "刷新比较" }).click();
  await page.getByRole("button", { name: /加载更多/ }).click();
  await page.waitForTimeout(950);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "加载中…", exact: true })).toBeDisabled();
});

test("format folding never hides a neighboring condition edit in a shared unified chunk", async ({
  page,
}) => {
  const oldFormat = "function same(){return 1;}\n";
  const newFormat = "function same() {\n  return 1;\n}\n";
  const payload = structuredClone(result);
  payload.file.before = oldFormat + "const limit = 1;\n";
  payload.file.after = newFormat + "const limit = 2;\n";
  const analysis = payload.analysis!;
  analysis.events = [
    {
      id: "format",
      title: "仅排版变化",
      facets: ["format"],
      before: {
        byteStart: 0,
        byteEnd: oldFormat.length,
        utf16Start: 0,
        utf16End: oldFormat.length,
        startLine: 1,
        endLine: 2,
      },
      after: {
        byteStart: 0,
        byteEnd: newFormat.length,
        utf16Start: 0,
        utf16End: newFormat.length,
        startLine: 1,
        endLine: 4,
      },
      evidence: ["CST identical"],
      rawEditIds: [0, 1, 2, 3],
      children: [],
    },
  ];
  analysis.coverage = [0, 1, 2, 3].map((rawEditId) => ({
    rawEditId,
    presentation: "folded-format",
    eventIds: ["format"],
  }));
  await bridge(page, {}, payload);
  await open(page);
  await page.getByRole("button", { name: "统一", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("limit = 1", { useInnerText: true });
  await expect(page.locator(".merge-host")).toContainText("limit = 2", { useInnerText: true });
  await page.getByRole("button", { name: "完整原始视图", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("return 1", { useInnerText: true });
});

test("moves connect original locations and retain grouped internal edits", async ({ page }) => {
  const movedBefore = "function pay(balance, amount) {\n  return balance > amount;\n}\n";
  const movedAfter = "function pay(balance, requested) {\n  return balance >= requested;\n}\n";
  const siblings = Array.from(
    { length: 50 },
    (_, i) => `function helper${i}() { return ${i}; }\n`,
  ).join("");
  const old = movedBefore + siblings;
  const next = siblings + movedAfter;
  const range = (source: string, start: number, end: number) => ({
    byteStart: start,
    byteEnd: end,
    utf16Start: start,
    utf16End: end,
    startLine: source.slice(0, start).split("\n").length,
    endLine: source.slice(0, end - 1).split("\n").length,
  });
  const parent = {
    ...result.analysis!.events[0],
    id: "move-pay",
    title: "pay: 移动与局部改名",
    facets: ["move", "rename", "edit"] as ("move" | "rename" | "edit")[],
    before: range(old, 0, movedBefore.length),
    after: range(next, siblings.length, next.length),
    children: ["edit-pay"],
  };
  const child = {
    ...parent,
    id: "edit-pay",
    title: "pay: 内部条件修改",
    facets: ["edit"] as "edit"[],
    children: [],
  };
  const payload: FileAnalysis = {
    ...result,
    file: { ...result.file, before: old, after: next },
    analysis: { ...result.analysis!, events: [parent, child] },
  };
  await bridge(page, {}, payload);
  await open(page);
  const overlay = page.getByRole("img", { name: /代码移动：pay/ });
  const curve = overlay.locator("path");
  await expect(overlay).toBeVisible();
  await expect(curve).toHaveAttribute("d", /^M .+ C .+/);
  await expect(page.locator(".move-overlay path")).toHaveCount(1);
  await expect(page.locator(".event-group")).toHaveCount(1);
  await expect(page.locator(".event-group .event")).toHaveCount(2);
  await page.getByLabel("折叠未改行").uncheck();
  await page.getByRole("button", { name: "起点 L1", exact: true }).click();
  await page.getByRole("button", { name: "终点 L51", exact: true }).click();
  await expect(page.locator(".cm-editor").last().locator(".cm-content")).toBeFocused();
  await expect(overlay).toHaveAttribute("data-before-clipped", "上方");
  await expect(curve).toHaveAttribute("stroke-dasharray", "4 3");
  const scrolledPath = await curve.getAttribute("d");
  await page.locator(".merge-host").evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect.poll(() => curve.getAttribute("d")).not.toBe(scrolledPath);
  await expect(overlay).toHaveAttribute("data-after-clipped", "下方");
  await page.getByRole("button", { name: "完整原始视图", exact: true }).click();
  await expect(overlay).toHaveAttribute("data-after-clipped", "下方");
  await page.getByRole("button", { name: "终点 L51", exact: true }).click();
  await expect(overlay).toHaveAttribute("data-after-clipped", "");
  const firstPath = await curve.getAttribute("d");
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect.poll(() => curve.getAttribute("d")).not.toBe(firstPath);
  await page.getByRole("button", { name: /pay: 内部条件修改/ }).click();
  await expect(page.getByLabel("仅对照这组变化")).toBeChecked();
  await expect(overlay).toHaveCount(0);
  await expect(page.locator(".cm-mergeView")).toContainText("balance > amount");
  await expect(page.locator(".cm-mergeView")).toContainText("balance >= requested");
  await expect(page.locator(".cm-mergeView")).not.toContainText("helper0");
  await page.getByRole("button", { name: /pay: 移动与局部改名/ }).click();
  await expect(overlay).toBeVisible();
  await page.screenshot({ path: "test-results/move-connection.png", fullPage: true });
  await page.getByRole("button", { name: "统一", exact: true }).click();
  await expect(overlay).toHaveCount(0);
  await page.getByRole("button", { name: "双栏", exact: true }).click();
  await expect(overlay).toBeVisible();
  await page.getByRole("button", { name: "返回完整文件", exact: true }).click();
  await expect(overlay).toHaveCount(0);
});

test("wide review shows files and lets the reader reclaim code space", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 800 });
  await bridge(page);
  await open(page, false);
  await expect(page.locator(".cm-content").first()).toBeVisible();
  await expect(page.getByRole("navigation", { name: "变更文件" })).toBeVisible();
  await page.screenshot({ path: "test-results/comparison-review.png" });
  await page.getByRole("button", { name: /变更文件 ·/ }).click();
  await expect(page.locator(".inspector")).toBeHidden();
  await expect(page.getByLabel("Git 仓库路径")).toBeHidden();
  const code = await page.locator(".merge-host").boundingBox();
  expect(code!.width).toBeGreaterThanOrEqual(900);
  expect(code!.height).toBeGreaterThanOrEqual(560);
  await page.getByRole("button", { name: /变更文件 ·/ }).click();
  const separator = page.getByRole("separator", { name: "文件导航宽度" });
  await separator.focus();
  await page.keyboard.press("ArrowRight");
  await expect(separator).toHaveAttribute("aria-valuenow", "280");
  await page.getByRole("button", { name: "变更详情", exact: true }).click();
  await expect(page.locator(".inspector")).toBeVisible();
  await page.getByRole("button", { name: "收起详情", exact: true }).click();
  await page.getByRole("button", { name: /变更文件 ·/ }).click();
  await page.screenshot({ path: "test-results/reading-focus-1200.png", fullPage: true });
  await page.setViewportSize({ width: 760, height: 540 });
  await page.emulateMedia({ colorScheme: "dark" });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await expect(page.getByRole("button", { name: "更改比较", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "更改比较", exact: true }).click();
  await expect(page.getByLabel("Git 仓库路径")).toBeVisible();
  await page.getByRole("button", { name: "返回当前比较", exact: true }).click();
  await page.getByRole("button", { name: /变更文件 ·/ }).click();
  await page.getByRole("button", { name: /src\/other.ts/ }).click();
  await expect(page.getByRole("navigation", { name: "变更文件" })).toBeHidden();
  await expect(page.locator(".file-heading h2")).toHaveText("src/other.ts");
  await page.screenshot({ path: "test-results/reading-focus-760-dark.png", fullPage: true });
});

for (const count of [420, 2000, 10_000])
  test(`complete search and virtual navigation cover ${count} changes`, async ({ page }) => {
    await bridge(page, { count });
    await open(page);
    expect(await page.locator(".file-item").count()).toBeLessThan(40);
    const last = `file-${String(count - 1).padStart(4, "0")}.ts`;
    await page.getByLabel("筛选文件").fill(last);
    await expect(page.locator(".file-item")).toHaveCount(1);
    await page.locator(".file-item").click();
    await expect(page.locator(".file-heading h2")).toContainText(last);
    await page.getByLabel("筛选文件").fill("");
    await page.getByRole("button", { name: "目录树", exact: true }).click();
    await expect(page.locator(".file-item.selected")).toBeVisible();
    await page.getByRole("button", { name: "浏览历史后退" }).click();
    await expect(page.locator(".file-heading h2")).toContainText("file-0000.ts");
    await page.getByRole("button", { name: "下个未审查", exact: true }).click();
    await expect(page.locator(".file-heading h2")).toContainText("file-0001.ts");
    expect(await page.locator(".file-item").count()).toBeLessThan(40);
  });

test("browsing a large file list survives unrelated review updates", async ({ page }) => {
  await bridge(page, { count: 10_000 });
  await open(page);
  await page.getByLabel("筛选文件").fill("file-");
  await expect(page.locator(".navigator-layout")).toContainText("10000 个匹配");
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  const viewport = page.locator(".virtual-files");
  await viewport.evaluate((node) => {
    node.scrollTop = 44 * 5000;
  });
  await expect.poll(() => viewport.evaluate((node) => node.scrollTop)).toBe(44 * 5000);
  await page.getByRole("button", { name: "变更详情", exact: true }).click();
  await expect.poll(() => viewport.evaluate((node) => node.scrollTop)).toBe(44 * 5000);
  await page.getByRole("button", { name: "标记已审查", exact: true }).click();
  await expect.poll(() => viewport.evaluate((node) => node.scrollTop)).toBe(44 * 5000);
  expect(await page.locator(".file-item").count()).toBeLessThan(40);
});

test("loading another file page keeps the navigation viewport", async ({ page }) => {
  await bridge(page, { count: 420 });
  await open(page);
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  const viewport = page.locator(".virtual-files");
  await viewport.evaluate((node) => {
    node.scrollTop = 44 * 150;
  });
  await expect.poll(() => viewport.evaluate((node) => node.scrollTop)).toBe(44 * 150);
  await page.getByRole("button", { name: "加载更多（200/420）", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "加载更多（400/420）", exact: true }),
  ).toBeVisible();
  await expect.poll(() => viewport.evaluate((node) => node.scrollTop)).toBe(44 * 150);
});

test("reopening navigation reveals a file selected while the panel was hidden", async ({
  page,
}) => {
  await bridge(page, { count: 420 });
  await open(page);
  await page.getByLabel("筛选文件").fill("file-0419.ts");
  await page.locator(".file-item").click();
  await page.getByLabel("筛选文件").fill("");
  await expect(page.locator(".file-item.selected")).toBeVisible();
  await page.getByRole("button", { name: /变更文件 ·/ }).click();
  await page.getByRole("button", { name: "浏览历史后退" }).click();
  await expect(page.locator(".file-heading h2")).toContainText("file-0000.ts");
  await page.getByRole("button", { name: /变更文件 ·/ }).click();
  await expect(page.locator(".file-item.selected")).toBeVisible();
});

test("virtual keyboard navigation reaches both ends of ten thousand files", async ({ page }) => {
  await bridge(page, { count: 10_000 });
  await open(page);
  await page.getByLabel("筛选文件").fill("file-");
  await expect(page.locator(".navigator-layout")).toContainText("10000 个匹配");
  await page.locator(".file-item").first().focus();
  await page.keyboard.press("End");
  const last = page.getByRole("button", { name: "packages/p99/file-9999.ts", exact: true });
  await expect(last).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator(".file-heading h2")).toContainText("file-9999.ts");
  await page.keyboard.press("Home");
  await expect(
    page.getByRole("button", { name: "packages/p0/file-0000.ts", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator(".file-heading h2")).toContainText("file-0000.ts");
});

test("a new snapshot reveals the selected file even when its path identity is unchanged", async ({
  page,
}) => {
  await bridge(page, { count: 420, refreshIdentity: true });
  await open(page);
  const viewport = page.locator(".virtual-files");
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  await viewport.evaluate((node) => {
    node.scrollTop = 44 * 150;
  });
  await expect.poll(() => viewport.evaluate((node) => node.scrollTop)).toBe(44 * 150);
  await page.getByRole("button", { name: "刷新比较", exact: true }).click();
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  await expect(page.locator(".file-item.selected")).toBeVisible();
});

test("relationship selection retains editors and file search survives navigation", async ({
  page,
}) => {
  await bridge(page);
  await open(page);
  await expect(page.locator(".cm-content").first()).toBeVisible();
  const content = await page.locator(".cm-content").first().elementHandle();
  await page.getByRole("button", { name: /条件修改/ }).click();
  expect(await content!.evaluate((node) => node.isConnected)).toBe(true);
  await page.getByRole("button", { name: /搜索 ⌘/ }).click();
  await page.locator(".cm-search input[name=search]").fill("balance");
  await page.getByRole("button", { name: "src/other.ts", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await page.getByRole("button", { name: "src/withdraw.ts", exact: true }).click();
  await expect(page.locator(".cm-search input[name=search]")).toHaveValue("balance");
});
test("failed refresh preserves frozen content and current file", async ({ page }) => {
  await bridge(page, { failRefresh: true, delayCapture: 250 });
  await open(page);
  await page.getByRole("button", { name: "src/other.ts", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await page.getByRole("button", { name: "刷新比较", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await expect(page.getByRole("alert")).toContainText("capture failed");
  await expect(page.locator(".file-heading h2")).toHaveText("src/other.ts");
  await expect(page.locator(".merge-host")).toContainText("other = 2");
});

test("routine capture notes stay out of the way while scope warnings remain visible", async ({
  page,
}) => {
  const warnings = [
    "Working-tree bytes are captured literally; clean/smudge filters and line-ending conversion are not executed. Capture is not a filesystem-wide atomic transaction.",
    "Fast capture uses Git index/stat metadata for unchanged tracked files. Changed, flagged, racy or known conversion-affected files are hashed literally. Historical conversion rules may no longer be discoverable; enable full literal verification to check all existing tracked bytes.",
    "Path scope: src. 范围外内容未捕获、未审查。",
  ];
  await bridge(page, { warnings });
  await open(page, false);
  await expect(page.getByText(warnings[2])).toBeVisible();
  await expect(page.locator(".notice", { hasText: warnings[0] })).not.toBeVisible();
  await expect(page.getByText(/快速捕获使用 Git/)).not.toBeVisible();
  await page.getByText("快照捕获说明", { exact: true }).click();
  await expect(page.getByText(/快速捕获使用 Git/)).toBeVisible();
});

test("switching repositories clears the old comparison even when the new capture fails", async ({
  page,
}) => {
  await bridge(page, { warnings: ["previous repository warning"], failRefresh: true });
  await open(page, false);
  await expect(page.getByText("previous repository warning")).toBeVisible();
  await page.getByRole("button", { name: "更改比较" }).click();
  await page.getByLabel("Git 仓库路径").fill("/missing");
  await expect(page.getByText("previous repository warning")).toHaveCount(0);
  await page.getByRole("button", { name: "开始比较", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("capture failed");
  await expect(page.getByText("previous repository warning")).toHaveCount(0);
  await expect(page.locator(".file-heading h2")).toHaveCount(0);
  await page.getByLabel("Git 仓库路径").fill("/next");
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("frozen text precedes analysis and cannot be marked reviewed prematurely", async ({
  page,
}) => {
  await bridge(page, { delayFirst: 1800 });
  await open(page, false);
  await expect(page.locator(".merge-host")).toContainText("balance");
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeDisabled();
  const content = await page.locator(".cm-content").first().elementHandle();
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  expect(await content!.evaluate((node) => node.isConnected)).toBe(true);
});

for (const budgetFailure of ["paths", "time"] as const) {
  test(`budget error (${budgetFailure}) exposes a usable scope recovery field`, async ({
    page,
  }) => {
    await bridge(page, { budgetFailure });
    await open(page, false);
    await expect(page.getByRole("alert")).toContainText("路径范围");
    await expect(page.getByRole("alert")).toContainText(
      budgetFailure === "time" ? "捕获仓库快照超时" : "2,000 changed paths",
    );
    await expect(page.getByLabel("捕获路径范围")).toBeFocused();
    await page.getByLabel("捕获路径范围").fill("src");
    await page.getByRole("button", { name: "开始比较", exact: true }).click();
    await expect(page.locator(".merge-host")).toContainText("balance");
  });
}
test("large frozen sources paginate independently and retain explicit truncation", async ({
  page,
}) => {
  const source = "a".repeat(220_000) + "TAIL😀";
  await bridge(
    page,
    {},
    {
      ...result,
      analysis: null,
      file: { ...result.file, before: source, after: source + "new", afterTruncated: true },
    },
  );
  await open(page, false);
  await expect(page.locator(".large-file-viewer")).toContainText("两侧独立翻页");
  await expect(page.locator(".large-file-viewer")).toContainText("末段不是文件结尾");
  expect(await page.locator(".source-segments textarea").first().inputValue()).toHaveLength(32768);
  for (let i = 0; i < 6; i++)
    await page.getByRole("button", { name: "修改后下一段", exact: true }).click();
  await expect(page.locator(".source-segments textarea").last()).toHaveValue(/TAIL😀new$/);
  await expect(page.locator(".source-segments textarea").first()).not.toHaveValue(/TAIL/);
  await expect(page.locator(".cm-content")).toHaveCount(0);
});

test("late text preview cannot replace a newer selected file", async ({ page }) => {
  await bridge(page, { delayPreview: 900, delayFirst: 1300 });
  await open(page);
  await page.getByRole("button", { name: "src/other.ts", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await page.waitForTimeout(1500);
  await expect(page.locator(".file-heading h2")).toHaveText("src/other.ts");
  await expect(page.locator(".merge-host")).toContainText("other = 2");
});
test("refresh keeps the selected path and resets history to the new snapshot", async ({ page }) => {
  await bridge(page, { refreshIdentity: true });
  await open(page);
  await page.getByRole("button", { name: "src/other.ts", exact: true }).click();
  await expect(page.getByRole("button", { name: "浏览历史后退" })).toBeEnabled();
  await page.getByRole("button", { name: "刷新比较", exact: true }).click();
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  await expect(page.locator(".file-heading h2")).toHaveText("src/other.ts");
  await expect(page.getByRole("button", { name: "浏览历史后退" })).toBeDisabled();
});
test("file navigation restores the reading scroll position", async ({ page }) => {
  const before = Array.from({ length: 400 }, (_, i) => `const before${i} = ${i};`).join("\n");
  const after = before.replaceAll("before", "after");
  await bridge(page, {}, { ...result, analysis: null, file: { ...result.file, before, after } });
  await open(page);
  await expect(page.locator(".merge-host")).toBeVisible();
  await page.locator(".merge-host").evaluate((node) => {
    node.scrollTop = 600;
  });
  await expect.poll(() => page.locator(".merge-host").evaluate((node) => node.scrollTop)).toBe(600);
  await page.getByRole("button", { name: "src/other.ts", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("other = 2");
  await page.getByRole("button", { name: "src/withdraw.ts", exact: true }).click();
  await expect.poll(() => page.locator(".merge-host").evaluate((node) => node.scrollTop)).toBe(600);
});

test("changing repositories clears old filters while same-repository refresh preserves them", async ({
  page,
}) => {
  await bridge(page);
  await open(page);
  await page.getByLabel("筛选文件").fill("withdraw");
  await page.getByRole("button", { name: "刷新比较", exact: true }).click();
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
  await expect(page.getByLabel("筛选文件")).toHaveValue("withdraw");
  await page.getByRole("button", { name: "更改比较", exact: true }).click();
  await page.getByLabel("Git 仓库路径").fill("/another-repo");
  await page.getByRole("button", { name: "开始比较", exact: true }).click();
  await expect(page.getByLabel("筛选文件")).toHaveValue("");
  await expect(page.getByRole("button", { name: "src/other.ts", exact: true })).toBeVisible();
});

test("explains a selected change on demand and shows its frozen evidence", async ({ page }) => {
  await bridge(page);
  await open(page);
  await page.getByRole("button", { name: /canWithdraw: 条件修改/ }).click();
  await page.getByRole("button", { name: "AI 解释", exact: true }).click();
  const panel = page.getByRole("complementary", { name: "AI 变更解释" });
  await expect(panel.getByLabel("解释范围")).toHaveValue("event");
  await panel.getByText("查看将发送的源码").click();
  await expect(panel.locator(".explanation-preview pre").last()).toContainText("balance >= amount");
  const calls = () =>
    page.evaluate(
      () => (window as unknown as { __DIFFKIN_TEST_CALLS__: string[] }).__DIFFKIN_TEST_CALLS__,
    );
  expect(await calls()).not.toContain("explanation_run");
  await panel.getByRole("button", { name: "生成解释", exact: true }).click();
  const answer = panel.getByRole("region", { name: "生成的解释" });
  await expect(answer).toContainText("余额等于金额时现在允许取款");
  await expect(answer.getByRole("heading", { name: /为什么可能这样改/ })).toContainText("推测");
  await answer.getByText("改后 L2–2").click();
  await expect(answer.locator("pre")).toContainText("2    return balance >= amount;");
  await expect(page.getByRole("button", { name: "标记已审查" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

test("changing explanation scope cancels its request and discards late answers", async ({
  page,
}) => {
  await bridge(page, { explanationDelay: 350 });
  await open(page, false);
  await page.getByRole("button", { name: "AI 解释", exact: true }).click();
  const panel = page.getByRole("complementary", { name: "AI 变更解释" });
  await expect(panel.getByLabel("解释范围")).toHaveValue("raw:1");
  await panel.getByRole("button", { name: "生成解释", exact: true }).click();
  await panel.getByLabel("解释范围").selectOption("file");
  await expect(panel.getByRole("button", { name: "生成解释", exact: true })).toBeEnabled();
  await page.waitForTimeout(450);
  await expect(panel.getByRole("region", { name: "生成的解释" })).toHaveCount(0);
  expect(
    await page.evaluate(
      () => (window as unknown as { __DIFFKIN_TEST_CALLS__: string[] }).__DIFFKIN_TEST_CALLS__,
    ),
  ).toContain("cancel_job");
});

test("missing model tools leave ordinary review available", async ({ page }) => {
  await bridge(page, { missingCli: true });
  await open(page, false);
  await page.getByRole("button", { name: "AI 解释", exact: true }).click();
  await expect(page.getByRole("button", { name: "生成解释", exact: true })).toBeDisabled();
  await expect(page.getByText(/未找到可用的 Codex/)).toBeVisible();
  await page.getByRole("button", { name: "关闭解释" }).click();
  await expect(page.getByLabel("修改后源码")).toContainText("balance >= amount");
});

test("quiz waits for answers, grades the final choices, and retries locally", async ({ page }) => {
  await bridge(page, { failQuizRetry: true });
  await open(page, false);
  await page.getByRole("button", { name: "变更测验", exact: true }).click();
  const panel = page.getByRole("complementary", { name: "变更测验" });
  await expect(panel.getByLabel("测验范围")).toHaveValue("file");
  await expect(panel).toContainText("共 3 道单选题");
  const generationCount = () =>
    page.evaluate(
      () =>
        (window as unknown as { __DIFFKIN_TEST_CALLS__: string[] }).__DIFFKIN_TEST_CALLS__.filter(
          (c) => c === "quiz_generate",
        ).length,
    );
  expect(await generationCount()).toBe(0);
  await panel.getByRole("button", { name: "生成测验" }).click();
  await expect(panel.getByText("等号使相等的输入通过。")).toHaveCount(0);
  await expect(panel.getByRole("button", { name: "下一题" })).toBeDisabled();
  await panel.getByRole("radio", { name: "A · true", exact: true }).check();
  await panel.getByRole("button", { name: "下一题" }).click();
  await panel.getByRole("button", { name: "上一题" }).click();
  await expect(panel.getByRole("radio", { name: "A · true", exact: true })).toBeChecked();
  await panel.getByRole("radio", { name: "B · false", exact: true }).check();
  await panel.getByRole("button", { name: "下一题" }).click();
  await panel.getByRole("radio").first().check();
  await panel.getByRole("button", { name: "下一题" }).click();
  await expect(panel.getByRole("button", { name: "交卷评分" })).toBeDisabled();
  await panel.getByRole("radio").nth(1).check();
  await panel.getByRole("button", { name: "交卷评分" }).click();
  await expect(panel.getByRole("heading", { name: "答对 2 / 3 题" })).toBeVisible();
  await expect(panel).toContainText("你的选择 B · false");
  await panel.getByText("核对源码", { exact: false }).first().click();
  await expect(panel.locator("pre").first()).toContainText("+ L2 | return balance >= amount");
  await panel.getByRole("button", { name: "重新出题" }).click();
  await expect(panel.getByText("模型服务暂时不可用")).toBeVisible();
  await expect(panel.getByRole("heading", { name: "答对 2 / 3 题" })).toBeVisible();
  await panel.getByRole("button", { name: "重答这套题" }).click();
  await expect(panel.getByRole("button", { name: "下一题" })).toBeDisabled();
  expect(await generationCount()).toBe(2);
});

test("a scope switch discards late quiz questions", async ({ page }) => {
  await bridge(page, { explanationDelay: 300 });
  await open(page, false);
  await page.getByRole("button", { name: "变更测验", exact: true }).click();
  const panel = page.getByRole("complementary", { name: "变更测验" });
  await panel.getByRole("button", { name: "生成测验" }).click();
  await panel.getByLabel("测验范围").selectOption("raw:1");
  await expect(panel.getByRole("button", { name: "生成测验" })).toBeEnabled();
  await page.waitForTimeout(400);
  await expect(panel.getByRole("region", { name: "答题" })).toHaveCount(0);
});
