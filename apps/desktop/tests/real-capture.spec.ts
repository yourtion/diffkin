import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import type { ComparisonManifest, FileAnalysis } from "@diffkin/protocol";

// Optional local acceptance: real frozen CLI output, mocked transport only.
// Source bodies stay in temporary files and are never committed as test fixtures.
const inputs = (process.env.DIFFKIN_ACCEPTANCE_JSON ?? "").split(",").filter(Boolean);
for (const input of inputs) {
  const capture = JSON.parse(readFileSync(input, "utf8")) as {
    manifest: ComparisonManifest;
    files: FileAnalysis[];
  };
  test(`review every file in real capture ${input.split("/").at(-1)}`, async ({ page }) => {
    test.setTimeout(90_000);
    await page.addInitScript((capture) => {
      Object.defineProperty(window, "isTauri", { value: true });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (command: string, args: Record<string, unknown>) => {
            if (command === "cancel_job") return;
            if (command === "open_comparison") return capture.manifest;
            if (command === "request_file")
              return capture.files.find((item) => item.file.summary.id === args.fileId);
            if (command === "list_changes") return { ...capture.manifest, files: [] };
            throw new Error(`Unexpected command ${command}`);
          },
        },
      });
    }, capture);
    await page.goto("/");
    await page.getByLabel("Git 仓库路径").fill(capture.manifest.repository);
    await page.getByRole("button", { name: "打开比较", exact: true }).click();
    for (const file of capture.manifest.files) {
      const button = page
        .locator(".file-item")
        .filter({ has: page.locator(".file-name", { hasText: file.path }) })
        .first();
      await button.click();
      await expect(page.locator(".file-heading h2")).toHaveText(file.path);
      await expect(page.getByRole("button", { name: "标记已审查", exact: true })).toBeEnabled();
      await page.getByRole("button", { name: "标记已审查", exact: true }).click();
      await expect(page.getByRole("alert")).toHaveCount(0);
    }
    await expect(page.locator("footer")).toContainText(
      `${capture.manifest.totalFiles} / ${capture.manifest.totalFiles} 已审查`,
    );
    await page.screenshot({
      path: `test-results/real-${input.split("/").at(-1)}.png`,
      fullPage: true,
    });
  });
}
