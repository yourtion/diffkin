import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import type {
  WorkspaceSession,
  FileAnalysis,
  ValidationPlan,
  ValidationResult,
} from "@diffkin/protocol";
const input = process.env.DIFFKIN_WORKFLOW_FIXTURE;
if (input) {
  const capture = JSON.parse(readFileSync(input, "utf8")) as {
    session: WorkspaceSession;
    files: Record<string, FileAnalysis[]>;
    plan: ValidationPlan;
    results: ValidationResult[];
  };
  test("real workspace persistence and validation results survive the UI contract", async ({
    page,
  }) => {
    const session = capture.session;
    const snapshot = session.snapshots.find((item) => item.id === session.activeSnapshotId)!;
    const candidate = capture.results.find((item) => item.target === "candidate")!;
    const baseline = capture.results.find((item) => item.target === "baseline")!;
    expect(candidate.status).toBe("passed");
    expect(baseline.status).toBe("failed");
    expect(candidate.commandId).toBe(baseline.commandId);
    expect(candidate.sourceId).not.toBe(baseline.sourceId);
    await page.addInitScript((capture) => {
      Object.defineProperty(window, "isTauri", { value: true });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (command: string, args: Record<string, unknown>) => {
            if (command === "workspace_list")
              return [
                {
                  id: capture.session.config.id,
                  name: capture.session.config.name,
                  repositoryCount: 3,
                  snapshotCount: 1,
                  error: null,
                },
              ];
            if (command === "workspace_restore") return capture.session;
            if (command === "request_file")
              return capture.files[args.id as string].find(
                (file) => file.file.summary.id === args.fileId,
              );
            if (command === "cancel_job") return;
            if (command === "validation_history")
              return capture.results.map((result) => ({ ...result, logTail: [] }));
            if (command === "validation_plan") return capture.plan;
            if (command === "validation_result")
              return capture.results.find((result) => result.id === args.id);
            throw new Error(`Unexpected real-workflow bridge call ${command}`);
          },
        },
      });
    }, capture);
    await page.goto("/");
    await page.getByRole("button", { name: "工作区", exact: true }).click();
    await page.getByLabel("已保存工作区").selectOption(session.config.id);
    await page.getByRole("button", { name: "恢复工作区", exact: true }).click();
    await expect(page.getByRole("button", { name: "✓ 已审查此版本", exact: true })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "工作区仓库" })).toContainText(
      "未修改 · 上下文",
    );
    await page.locator(".manual-relations summary").click();
    await expect(page.locator(".manual-relations")).toContainText("当前快照关系");
    await expect(page.locator(".manual-relations")).toContainText("src/shared.ts");
    await page.getByRole("button", { name: "收起设置", exact: true }).click();
    await page
      .getByRole("navigation", { name: "工作区仓库" })
      .getByRole("button", { name: new RegExp(snapshot.repositories[1].label) })
      .click();
    await expect(page.locator(".file-heading h2")).toHaveText("src/shared.ts");
    await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "验证", exact: true }).click();
    const history = page.getByRole("region", { name: "验证历史", exact: true });
    await history.getByRole("button").filter({ hasText: candidate.sourceId }).click();
    const result = page.getByRole("region", { name: "验证执行结果" });
    await expect(result).toContainText(candidate.sourceId);
    await expect(result).toContainText("通过 · 单仓库固定版本");
    await expect(page.getByLabel("验证日志", { exact: true })).toContainText("fixture");
    await expect(result).toContainText("unconfirmed");
    await history.getByRole("button").filter({ hasText: baseline.sourceId }).click();
    await expect(result).toContainText(baseline.sourceId);
    await expect(result).toContainText("失败");
    await expect(result).not.toContainText("通过 · 单仓库固定版本");
  });
}
