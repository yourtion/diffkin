import { test, expect, type Page } from "@playwright/test";
import type {
  WorkspaceConfig,
  WorkspaceSession,
  WorkspaceRelationDraft,
  WorkspaceSnapshot,
  EvidenceBasis,
  FileAnalysis,
  FileSummary,
} from "@diffkin/protocol";

async function bridge(page: Page, delayed = false, targetPaging = false) {
  await page.addInitScript(
    ({ delayed, targetPaging }) => {
      let session: WorkspaceSession | null = JSON.parse(
        localStorage.getItem("test.workspace") ?? "null",
      );
      let revision = Number(localStorage.getItem("test.revision") ?? "0");
      const calls: string[] = [];
      const save = () => {
        localStorage.setItem("test.workspace", JSON.stringify(session));
        localStorage.setItem("test.revision", String(revision));
      };
      const summary = (index: number, version: number): FileSummary => ({
        id: "same-file-id",
        path: "src/shared.ts",
        oldPath: null,
        status: "modified",
        oldMode: "100644",
        newMode: "100644",
        beforeId: `before-${index}`,
        afterId: `after-${index}-${version}`,
        notices: [],
      });
      const active = () =>
        session!.snapshots.find((item) => item.id === session!.activeSnapshotId)!;
      const basis = (repoId: string, fileId: string): EvidenceBasis => {
        const repo = active().repositories.find((item) => item.repositoryId === repoId)!;
        const file = repo.manifest!.files.find((item) => item.id === fileId)!;
        return {
          workspaceSnapshotId: active().id,
          repositoryId: repoId,
          repositorySnapshotId: repo.manifest!.id,
          fileId,
          beforeId: file.beforeId,
          afterId: file.afterId,
          schemaVersion: 1,
          ruleVersion: "syntax-v2",
        };
      };
      Object.defineProperty(window, "isTauri", { value: true });
      Object.defineProperty(window, "__WORKSPACE_CALLS__", { value: calls });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (command: string, args: Record<string, unknown>) => {
            calls.push(command);
            if (command === "preferences_load" || command === "preferences_save") return null;
            if (command === "cancel_job") return;
            if (command === "workspace_capture" && revision > 0 && "delayRefresh" in window) {
              await new Promise<void>((resolve) => {
                Object.assign(window, { finishRefresh: resolve });
              });
              throw new Error("old workspace capture failed");
            }
            if (command === "workspace_list")
              return session
                ? [
                    {
                      id: session.config.id,
                      name: session.config.name,
                      repositoryCount: session.config.repositories.length,
                      snapshotCount: session.snapshots.length,
                      error: null,
                    },
                  ]
                : [];
            if (command === "workspace_save") {
              session = {
                config: args.config as WorkspaceConfig,
                snapshots: session?.snapshots ?? [],
                activeSnapshotId: session?.activeSnapshotId ?? null,
                relations: session?.relations ?? [],
                reviews: session?.reviews ?? [],
              };
              save();
              return session;
            }
            if (command === "workspace_capture") {
              if (args.snapshotId && args.snapshotId !== session?.activeSnapshotId)
                throw { code: "stale", message: "Workspace active snapshot has changed" };
              revision++;
              const config = args.config as WorkspaceConfig;
              const id = `workspace-snapshot-${revision}`;
              const snapshot: WorkspaceSnapshot = {
                id,
                workspaceId: config.id,
                capturedAtMs: String(Date.now()),
                repositories: config.repositories.map((repo, index) => ({
                  repositoryId: repo.id,
                  label: repo.label,
                  revisions:
                    repo.spec.mode === "commits"
                      ? [repo.spec.base || "HEAD~1", repo.spec.head || "HEAD"]
                      : undefined,
                  error: null,
                  manifest: {
                    id: `repo-${index}-v${revision}`,
                    repository: repo.spec.repository,
                    mode: repo.spec.mode,
                    baseLabel: `commit:base-${index}`,
                    headLabel: `commit:head-${index}-${revision}`,
                    files:
                      index === 2
                        ? []
                        : targetPaging && index === 1
                          ? Array.from({ length: 200 }, (_, n) => ({
                              ...summary(index, revision),
                              id: `target-${n}`,
                              path: `src/extra${n}.ts`,
                            }))
                          : [summary(index, revision)],
                    totalFiles: index === 2 ? 0 : targetPaging && index === 1 ? 201 : 1,
                    warnings: [],
                  },
                })),
              };
              if ("timeoutSdk" in window && !config.repositories[1]?.spec.pathScope) {
                const sdk = snapshot.repositories[1];
                sdk.manifest = null;
                sdk.error = {
                  code: "budget-exceeded",
                  message: "Snapshot exceeded its capture time budget",
                };
              }
              session = {
                config,
                snapshots: [...(session?.snapshots ?? []), snapshot],
                activeSnapshotId: id,
                relations: session?.relations ?? [],
                reviews: session?.reviews ?? [],
              };
              save();
              return session;
            }
            if (command === "workspace_restore") {
              if (args.snapshotId) session!.activeSnapshotId = args.snapshotId as string;
              save();
              return session;
            }
            if (command === "workspace_review") {
              const value = args.basis as EvidenceBasis;
              if (value.workspaceSnapshotId !== active().id)
                throw { code: "stale", message: "Workspace active snapshot has changed" };
              session!.reviews = session!.reviews.filter(
                (record) => JSON.stringify(record.basis) !== JSON.stringify(value),
              );
              session!.reviews.push({ basis: value, state: args.state as "reviewed" | "pending" });
              save();
              return session;
            }
            if (command === "workspace_link_files") {
              const draft = args.draft as WorkspaceRelationDraft;
              if (draft.workspaceSnapshotId !== active().id)
                throw { code: "stale", message: "Workspace active snapshot has changed" };
              session!.relations.push({
                id: draft.id,
                note: draft.note,
                from: basis(draft.from.repositoryId, draft.from.fileId),
                to: basis(draft.to.repositoryId, draft.to.fileId),
              });
              save();
              return session;
            }
            if (command === "workspace_export")
              return {
                schemaVersion: 1,
                workspaceId: session!.config.id,
                snapshotId: args.snapshotId ?? session!.activeSnapshotId,
                repositories: [],
                relations: [],
                reviews: session!.reviews,
              };
            if (command === "list_changes")
              return {
                id: args.id,
                files: [{ ...summary(1, revision), id: "target-201", path: "src/late.ts" }],
                totalFiles: 201,
              };
            if (command === "request_file") {
              const parts = String(args.id).match(/^repo-(\d+)-v(\d+)$/)!;
              const index = Number(parts[1]),
                version = Number(parts[2]);
              if (delayed && index === 0) await new Promise((resolve) => setTimeout(resolve, 600));
              const before = `const value${index} = 0;\n`,
                after = `const value${index} = ${version};\n`;
              const result: FileAnalysis = {
                file: {
                  summary:
                    targetPaging && index === 1
                      ? {
                          ...summary(index, version),
                          id: String(args.fileId),
                          path:
                            args.fileId === "target-201"
                              ? "src/late.ts"
                              : `src/extra${String(args.fileId).slice(7)}.ts`,
                        }
                      : summary(index, version),
                  before,
                  after,
                  beforeTruncated: false,
                  afterTruncated: false,
                },
                notice: null,
                analysis: {
                  schemaVersion: 1,
                  ruleVersion: "syntax-v2",
                  language: "typescript",
                  raw: {
                    schemaVersion: 1,
                    added: 1,
                    removed: 1,
                    lines: [
                      { kind: "delete", oldLine: 1, newLine: null, text: before },
                      { kind: "insert", oldLine: null, newLine: 1, text: after },
                    ],
                  },
                  events: [],
                  coverage: [
                    { rawEditId: 0, presentation: "text-fallback", eventIds: [] },
                    { rawEditId: 1, presentation: "text-fallback", eventIds: [] },
                  ],
                  fallbacks: [],
                },
              };
              return result;
            }
            throw new Error(`Unexpected workspace call ${command}`);
          },
        },
      });
    },
    { delayed, targetPaging },
  );
  await page.goto("/");
}
async function capture(page: Page) {
  await page.getByRole("button", { name: "新建工作区", exact: true }).click();
  await page.getByLabel("工作区名称", { exact: true }).fill("API + SDK + Web");
  await page.getByLabel("仓库 1 名称", { exact: true }).fill("API");
  await page.getByLabel("仓库 1 路径", { exact: true }).fill("/fixtures/api");
  await page.getByRole("button", { name: "关联另一个仓库", exact: true }).click();
  await page.getByLabel("仓库 2 名称", { exact: true }).fill("SDK");
  await page.getByLabel("仓库 2 路径", { exact: true }).fill("/fixtures/sdk");
  await page.getByRole("button", { name: "关联另一个仓库", exact: true }).click();
  await page.getByLabel("仓库 3 名称", { exact: true }).fill("Web");
  await page.getByLabel("仓库 3 路径", { exact: true }).fill("/fixtures/web");
  await page.getByRole("button", { name: "比较工作区", exact: true }).click();
  await expect(page.getByRole("navigation", { name: "工作区仓库" })).toBeVisible();
}

test("workspace keeps same-path files and reviews separate, retains unchanged context", async ({
  page,
}) => {
  await bridge(page);
  await capture(page);
  await expect(page.locator(".merge-host")).toContainText("value0 = 1");
  await page.getByRole("button", { name: "标记已审查", exact: true }).click();
  await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
  await page
    .getByRole("navigation", { name: "工作区仓库" })
    .getByRole("button", { name: /SDK/ })
    .click();
  await expect(page.locator(".file-heading h2")).toHaveText("src/shared.ts");
  await expect(page.locator(".merge-host")).toContainText("value1 = 1");
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeVisible();
  await page
    .getByRole("navigation", { name: "工作区仓库" })
    .getByRole("button", { name: /Web/ })
    .click();
  await expect(page.getByRole("heading", { name: "当前比较没有文件变化" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "工作区仓库" })).toContainText(
    "未修改 · 上下文",
  );
  await page
    .getByRole("navigation", { name: "工作区仓库" })
    .getByRole("button", { name: /API/ })
    .click();
  await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
  await expect(page.locator(".merge-host")).toContainText("value0 = 1");
  const requests = await page.evaluate(() =>
    (window as unknown as { __WORKSPACE_CALLS__: string[] }).__WORKSPACE_CALLS__.filter(
      (call) => call === "request_file",
    ),
  );
  expect(requests).toHaveLength(2);
  await page.screenshot({ path: "test-results/workspace-review.png", fullPage: true });
  await page.reload();
  await page.getByRole("button", { name: /API \+ SDK \+ Web/ }).click();
  await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
  await expect(page.locator(".merge-host")).toContainText("value0 = 1");
});

test("workspace refresh invalidates reviews and makes manual relations historical", async ({
  page,
}) => {
  await bridge(page);
  await capture(page);
  await page.getByRole("button", { name: "标记已审查", exact: true }).click();
  await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "工作区设置", exact: true }).click();
  await page.locator(".manual-relations summary").click();
  await page.getByLabel("关联目标仓库").selectOption({ label: "SDK" });
  await page.getByLabel("关联目标文件").selectOption({ label: "src/shared.ts" });
  await page.getByLabel("关联备注").fill("调用契约");
  await page.getByRole("button", { name: "保存手工关联", exact: true }).click();
  await expect(page.locator(".manual-relations")).toContainText("当前快照关系");
  await page.getByRole("button", { name: "比较工作区", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("value0 = 2");
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "工作区设置" }).click();
  await page.locator(".manual-relations summary").click();
  await expect(page.locator(".manual-relations")).toContainText("历史关系，待重新确认");
  await page.getByLabel("工作区历史快照").selectOption("workspace-snapshot-1");
  await expect(page.locator(".merge-host")).toContainText("value0 = 1");
  await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "工作区设置" }).click();
  await page.getByRole("button", { name: "导出版本清单", exact: true }).click();
  await expect((await download).suggestedFilename()).toMatch(/^diffkin-workspace-.*\.json$/);
});

test("a second instance switching snapshots cannot redirect a manual relation", async ({
  page,
}) => {
  await bridge(page);
  await capture(page);
  await page.getByRole("button", { name: "工作区设置" }).click();
  await page.locator(".manual-relations summary").click();
  await page.getByLabel("关联目标仓库").selectOption({ label: "SDK" });
  await page.getByLabel("关联目标文件").selectOption({ label: "src/shared.ts" });
  await page.evaluate(async () => {
    const saved = JSON.parse(localStorage.getItem("test.workspace")!) as WorkspaceSession;
    await (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke(cmd: string, args: object): Promise<unknown> };
      }
    ).__TAURI_INTERNALS__.invoke("workspace_capture", {
      config: saved.config,
      jobId: "another-instance",
    });
  });
  await page.getByRole("button", { name: "保存手工关联" }).click();
  await expect(page.getByRole("alert")).toContainText("重新打开工作区");
  expect(
    await page.evaluate(() => JSON.parse(localStorage.getItem("test.workspace")!).relations),
  ).toEqual([]);
  const pendingDownload = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出版本清单" }).click();
  const stream = await (await pendingDownload).createReadStream();
  const chunks = [];
  for await (const chunk of stream) chunks.push(chunk);
  expect(JSON.parse(Buffer.concat(chunks).toString()).snapshotId).toBe("workspace-snapshot-1");
  await page.getByRole("button", { name: "收起设置" }).click();
  await page.getByRole("button", { name: "标记已审查" }).click();
  await expect(page.getByRole("alert")).toContainText("重新打开工作区");
  expect(
    await page.evaluate(() => JSON.parse(localStorage.getItem("test.workspace")!).reviews),
  ).toEqual([]);
  await page.getByRole("button", { name: "刷新工作区" }).click();
  await expect(page.getByRole("alert")).toContainText("重新打开工作区");
  expect(
    await page.evaluate(() => JSON.parse(localStorage.getItem("test.workspace")!).snapshots.length),
  ).toBe(2);
});

test("late workspace file results cannot replace a different repository", async ({ page }) => {
  await bridge(page, true);
  await capture(page);
  await page
    .getByRole("navigation", { name: "工作区仓库" })
    .getByRole("button", { name: /SDK/ })
    .click();
  await expect(page.locator(".merge-host")).toContainText("value1 = 1");
  await page.waitForTimeout(750);
  await expect(page.locator(".merge-host")).toContainText("value1 = 1");
  await expect(page.locator(".merge-host")).not.toContainText("value0");
});

test("workspace repository switch clears refresh status and ignores its late failure", async ({
  page,
}) => {
  await bridge(page);
  await capture(page);
  await page.evaluate(() => {
    Object.assign(window, { delayRefresh: true });
  });
  await page.getByRole("button", { name: "刷新工作区" }).click();
  await expect(page.getByRole("status").filter({ hasText: "上一次比较" })).toBeVisible();
  await page
    .getByRole("navigation", { name: "工作区仓库" })
    .getByRole("button", { name: /SDK/ })
    .click();
  await expect(page.locator(".merge-host")).toContainText("value1 = 1");
  await page.evaluate(() => (window as unknown as { finishRefresh(): void }).finishRefresh());
  await expect(page.getByText(/上一次比较/)).toHaveCount(0);
  await expect(
    page.getByRole("alert").filter({ hasText: "old workspace capture failed" }),
  ).toHaveCount(0);
});

test("refresh keeps the active workspace and the selected repository", async ({ page }) => {
  await bridge(page);
  await capture(page);
  const sdk = page
    .getByRole("navigation", { name: "工作区仓库" })
    .getByRole("button", { name: /SDK/ });
  await sdk.click();
  await page.getByRole("button", { name: "刷新工作区" }).click();
  await expect(page.locator(".merge-host")).toContainText("value1 = 2");
  await expect(sdk).toHaveAttribute("aria-pressed", "true");
  await page.reload();
  await expect(page.getByRole("heading", { name: "我的工作区" })).toBeVisible();
  await page.screenshot({ path: "test-results/workspace-home.png" });
  await page.getByRole("button", { name: /上次打开 · 3 个仓库/ }).click();
  await expect(sdk).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".merge-host")).toContainText("value1 = 2");
  expect(
    await page.evaluate(
      () => (window as unknown as { __WORKSPACE_CALLS__: string[] }).__WORKSPACE_CALLS__,
    ),
  ).not.toContain("workspace_capture");
});

test("commit comparison direction follows the selected workspace repository", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 800 });
  await bridge(page);
  await page.getByRole("button", { name: "新建工作区" }).click();
  await page.getByLabel("仓库 1 路径", { exact: true }).fill("/projects/api");
  await page.getByLabel("仓库 1 模式").selectOption("commits");
  await page.getByLabel("仓库 1 基准（修改前）", { exact: true }).fill("main");
  await page.getByLabel("仓库 1 目标（修改后）", { exact: true }).fill("feature/login");
  await expect(page.locator(".revision-preview")).toHaveText("查看 feature/login 相对 main 的改动");
  await page.screenshot({ path: "test-results/workspace-revision-direction.png" });
  await page.setViewportSize({ width: 760, height: 540 });
  const baseBox = await page.getByLabel("仓库 1 基准（修改前）", { exact: true }).boundingBox();
  const headBox = await page.getByLabel("仓库 1 目标（修改后）", { exact: true }).boundingBox();
  expect(Math.abs(baseBox!.y - headBox!.y)).toBeLessThan(2);
  expect(headBox!.x + headBox!.width).toBeLessThanOrEqual(760);
  await page.getByRole("button", { name: "仓库 1 交换比较方向" }).press("Enter");
  await expect(page.getByLabel("仓库 1 基准（修改前）", { exact: true })).toHaveValue(
    "feature/login",
  );
  await expect(page.locator(".revision-preview")).toHaveText("查看 main 相对 feature/login 的改动");
  await page.getByRole("button", { name: "仓库 1 交换比较方向" }).click();
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.getByRole("button", { name: "比较工作区" }).click();
  const headings = page.locator(".source-headings");
  await expect(headings.locator("strong")).toHaveText(["main", "feature/login"]);
  await page.screenshot({ path: "test-results/workspace-revision-result.png" });
  await page.getByRole("button", { name: "统一", exact: true }).click();
  await expect(headings).toContainText("− 修改前");
  await expect(headings).toContainText("+ 修改后");
  await page.getByRole("button", { name: "工作区设置" }).click();
  await page.getByRole("button", { name: "仓库 1 交换比较方向" }).click();
  await page.getByRole("button", { name: "保存配置" }).click();
  await expect(page.getByText(/配置已保存/)).toBeVisible();
  await page.reload();
  await page.getByRole("list", { name: "已保存工作区" }).getByRole("button").click();
  await expect(headings.locator("strong")).toHaveText(["main", "feature/login"]);
  // A pre-migration snapshot has only frozen IDs, even if its current configuration has refs.
  await page.evaluate(() => {
    const session = JSON.parse(localStorage.getItem("test.workspace")!) as WorkspaceSession;
    delete session.snapshots[0].repositories[0].revisions;
    localStorage.setItem("test.workspace", JSON.stringify(session));
  });
  await page.reload();
  await page.getByRole("list", { name: "已保存工作区" }).getByRole("button").click();
  await expect(headings.locator("strong")).toHaveText(["commit:base-0", "commit:head-0-1"]);
});

test("a saved configuration opens from home before its first comparison", async ({ page }) => {
  await bridge(page);
  await expect(page.getByRole("heading", { name: "我的工作区" })).toBeVisible();
  await page.getByRole("button", { name: "新建工作区" }).click();
  await page.getByLabel("工作区名称").fill("Backend");
  await page.getByLabel("仓库 1 路径", { exact: true }).fill("/projects/api");
  await expect(page.getByLabel("仓库 1 名称")).toHaveValue("api");
  await page.getByLabel("仓库 1 路径范围").fill("services/auth");
  await page.getByRole("button", { name: "保存配置" }).click();
  await page.getByRole("button", { name: /Backend.*尚未比较/ }).click();
  await expect(page.getByLabel("仓库 1 路径范围")).toHaveValue("services/auth");
  await page.getByRole("button", { name: "比较工作区" }).click();
  await expect(page.getByRole("navigation", { name: "工作区仓库" })).toContainText("api");
  await expect(page.getByRole("region", { name: "多仓库工作区" })).toBeHidden();
});

test("a workspace timeout opens the affected repository scope and retries it", async ({ page }) => {
  await bridge(page);
  await page.evaluate(() => {
    Object.assign(window, { timeoutSdk: true });
  });
  await capture(page);
  await page.getByRole("button", { name: /SDK.*不可用/ }).click();
  await expect(page.getByRole("alert")).toContainText("捕获仓库快照超时");
  await page.getByRole("button", { name: "调整此仓库的路径范围" }).click();
  const scope = page.getByLabel("仓库 2 路径范围");
  await expect(scope).toBeFocused();
  await scope.fill("packages/sdk");
  await page.getByRole("button", { name: "比较工作区" }).click();
  await expect(page.locator(".merge-host")).toContainText("value1 = 2");
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("switching repositories resumes a file past the first page within the same snapshot", async ({
  page,
}) => {
  await bridge(page, false, true);
  await capture(page);
  const tabs = page.getByRole("navigation", { name: "工作区仓库" });
  await tabs.getByRole("button", { name: /SDK/ }).click();
  await page.getByRole("button", { name: /加载更多/ }).click();
  const filter = page.getByLabel("筛选文件");
  await filter.fill("src/late.ts");
  await page.getByRole("button", { name: "src/late.ts", exact: true }).click();
  await expect(page.locator(".file-heading h2")).toHaveText("src/late.ts");
  await tabs.getByRole("button", { name: /API/ }).click();
  await expect(page.locator(".file-heading h2")).toHaveText("src/shared.ts");
  await tabs.getByRole("button", { name: /SDK/ }).click();
  await expect(page.locator(".file-heading h2")).toHaveText("src/late.ts");
  await expect(page.locator(".file-item.selected")).toHaveAccessibleName("src/late.ts");
  await page.getByRole("button", { name: "刷新工作区" }).click();
  await expect(page.locator(".merge-host")).toContainText("value1 = 2");
});

test("manual relation target files can be paged past the first 200", async ({ page }) => {
  await bridge(page, false, true);
  await capture(page);
  await page.getByRole("button", { name: "工作区设置", exact: true }).click();
  await page.locator(".manual-relations summary").click();
  await page.getByLabel("关联目标仓库").selectOption({ label: "SDK" });
  await page.getByRole("button", { name: "加载更多目标文件（200/201）", exact: true }).click();
  await page.getByLabel("关联目标文件").selectOption({ label: "src/late.ts" });
  await expect(page.getByLabel("关联目标文件")).toHaveValue("target-201");
  await expect(page.getByRole("button", { name: /加载更多目标文件/ })).toHaveCount(0);
});
