import { test, expect, type Page } from "@playwright/test";
import type { ComparisonManifest, FileAnalysis, FileSummary } from "@diffkin/protocol";

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
    delayCapture?: number;
    refreshIdentity?: boolean;
    refreshRules?: boolean;
    delayPage?: number;
  } = {},
  payload: FileAnalysis = result,
) {
  await page.addInitScript(
    ({ manifest, result, options }) => {
      const calls: string[] = [];
      let captures = 0;
      Object.defineProperty(window, "isTauri", { value: true });
      Object.defineProperty(window, "__DIFFKIN_TEST_CALLS__", { value: calls });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (command: string, args: Record<string, unknown>) => {
            calls.push(command);
            if (command === "cancel_job") return;
            if (command === "open_comparison") {
              captures++;
              if (options.delayCapture)
                await new Promise((resolve) => setTimeout(resolve, options.delayCapture));
              return {
                ...manifest,
                id: options.refreshIdentity ? `frozen-${captures}` : manifest.id,
                totalFiles: options.delayPage ? 3 : manifest.totalFiles,
              };
            }
            if (command === "list_changes") {
              if (options.delayPage) {
                const generation = captures;
                await new Promise((resolve) =>
                  setTimeout(resolve, options.delayPage! * generation),
                );
                throw new Error(`stale page ${generation}`);
              }
              return { ...manifest, files: [] };
            }
            if (command === "request_file") {
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
}
async function open(page: Page) {
  await page.getByLabel("Git 仓库路径").fill("/repo");
  await page.getByRole("button", { name: "打开比较", exact: true }).click();
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
  await open(page);
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("已取消");
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
  await expect(
    page.getByText("浏览器仅预览界面。请运行桌面应用以读取本地 Git 仓库。"),
  ).toBeVisible();
  await page.getByLabel("Git 仓库路径").fill("/repo");
  await expect(page.getByRole("button", { name: "打开比较", exact: true })).toBeDisabled();
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
