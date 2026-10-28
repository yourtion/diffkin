import { test, expect, type Page } from "@playwright/test";
import type {
  WorkspaceConfig,
  WorkspaceSession,
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
            if (command === "cancel_job") return;
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
              revision++;
              const config = args.config as WorkspaceConfig;
              const id = `workspace-snapshot-${revision}`;
              const snapshot = {
                id,
                workspaceId: config.id,
                capturedAtMs: String(Date.now()),
                repositories: config.repositories.map((repo, index) => ({
                  repositoryId: repo.id,
                  label: repo.label,
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
            if (command === "workspace_basis")
              return basis(args.repositoryId as string, args.fileId as string);
            if (command === "workspace_review") {
              const value = args.basis as EvidenceBasis;
              session!.reviews = session!.reviews.filter(
                (record) => JSON.stringify(record.basis) !== JSON.stringify(value),
              );
              session!.reviews.push({ basis: value, state: args.state as "reviewed" | "pending" });
              save();
              return session;
            }
            if (command === "workspace_relation") {
              session!.relations.push(args.relation as WorkspaceSession["relations"][number]);
              save();
              return session;
            }
            if (command === "workspace_export")
              return {
                schemaVersion: 1,
                workspaceId: session!.config.id,
                snapshotId: session!.activeSnapshotId,
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
                  summary: summary(index, version),
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
  await page.getByRole("button", { name: "工作区", exact: true }).click();
  await page.getByLabel("工作区名称", { exact: true }).fill("API + SDK + Web");
  await page.getByLabel("仓库 1 名称", { exact: true }).fill("API");
  await page.getByLabel("仓库 1 路径", { exact: true }).fill("/fixtures/api");
  await page.getByRole("button", { name: "关联另一个仓库", exact: true }).click();
  await page.getByLabel("仓库 2 名称", { exact: true }).fill("SDK");
  await page.getByLabel("仓库 2 路径", { exact: true }).fill("/fixtures/sdk");
  await page.getByRole("button", { name: "关联另一个仓库", exact: true }).click();
  await page.getByLabel("仓库 3 名称", { exact: true }).fill("Web");
  await page.getByLabel("仓库 3 路径", { exact: true }).fill("/fixtures/web");
  await page.getByRole("button", { name: "捕获工作区", exact: true }).click();
  await expect(page.getByRole("navigation", { name: "工作区仓库" })).toBeVisible();
}

test("workspace keeps same-path files and reviews separate, retains unchanged context", async ({
  page,
}) => {
  await bridge(page);
  await capture(page);
  await page.getByRole("button", { name: "收起设置", exact: true }).click();
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
  await page.screenshot({ path: "test-results/workspace-review.png", fullPage: true });
  await page.reload();
  await page.getByRole("button", { name: "工作区", exact: true }).click();
  await page.getByLabel("已保存工作区").selectOption({ label: "API + SDK + Web · 3 仓库" });
  await page.getByRole("button", { name: "恢复工作区", exact: true }).click();
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
  await page.locator(".manual-relations summary").click();
  await page.getByLabel("关联目标仓库").selectOption({ label: "SDK" });
  await page.getByLabel("关联目标文件").selectOption({ label: "src/shared.ts" });
  await page.getByLabel("关联备注").fill("调用契约");
  await page.getByRole("button", { name: "保存手工关联", exact: true }).click();
  await expect(page.locator(".manual-relations")).toContainText("当前快照关系");
  await page.getByRole("button", { name: "捕获工作区", exact: true }).click();
  await expect(page.locator(".merge-host")).toContainText("value0 = 2");
  await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeVisible();
  await expect(page.locator(".manual-relations")).toContainText("历史关系，待重新确认");
  await page.getByLabel("工作区历史快照").selectOption("workspace-snapshot-1");
  await expect(page.locator(".merge-host")).toContainText("value0 = 1");
  await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出版本清单", exact: true }).click();
  await expect((await download).suggestedFilename()).toMatch(/^diffkin-workspace-.*\.json$/);
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

test("manual relation target files can be paged past the first 200", async ({ page }) => {
  await bridge(page, false, true);
  await capture(page);
  await page.locator(".manual-relations summary").click();
  await page.getByLabel("关联目标仓库").selectOption({ label: "SDK" });
  await page.getByRole("button", { name: "加载更多目标文件（200/201）", exact: true }).click();
  await page.getByLabel("关联目标文件").selectOption({ label: "src/late.ts" });
  await expect(page.getByLabel("关联目标文件")).toHaveValue("target-201");
  await expect(page.getByRole("button", { name: /加载更多目标文件/ })).toHaveCount(0);
});
