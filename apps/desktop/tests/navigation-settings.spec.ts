import { expect, test, type Page } from "@playwright/test";

async function bridge(
  page: Page,
  options: { corrupt?: boolean; delayLoad?: boolean; slowSave?: boolean } = {},
) {
  await page.addInitScript((options) => {
    Object.defineProperty(window, "isTauri", { value: true });
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    Object.assign(window, { testCalls: calls, pickerResults: [null, "/selected/repository"] });
    let saved = JSON.parse(localStorage.getItem("test.native.settings") ?? "null");
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {
        invoke: async (command: string, args: Record<string, unknown> = {}) => {
          calls.push({ command, args });
          if (command === "preferences_load") {
            if (options.delayLoad) await new Promise((resolve) => setTimeout(resolve, 700));
            if (options.corrupt) throw { code: "corrupt", message: "Settings JSON is damaged" };
            return saved;
          }
          if (command === "preferences_save") {
            if (options.slowSave) await new Promise((resolve) => setTimeout(resolve, 500));
            saved = args.preferences;
            localStorage.setItem("test.native.settings", JSON.stringify(saved));
            return;
          }
          if (command === "pick_repository_directory")
            return (
              (window as unknown as { pickerResults: (string | null)[] }).pickerResults.shift() ??
              null
            );
          if (command === "cancel_job") return;
          if (command === "list_repository_refs") {
            if (args.repository === "/slow")
              await new Promise((resolve) => setTimeout(resolve, 700));
            const refs = args.offset
              ? [
                  {
                    name: "refs/tags/v1",
                    label: "v1",
                    kind: "tag",
                    target: "c".repeat(40),
                    symbolicTarget: null,
                  },
                ]
              : [
                  {
                    name: "refs/heads/main",
                    label: args.repository === "/slow" ? "obsolete" : "main",
                    kind: "branch",
                    target: "a".repeat(40),
                    symbolicTarget: null,
                  },
                  {
                    name: "refs/remotes/origin/feature",
                    label: "origin/feature",
                    kind: "remote",
                    target: "b".repeat(40),
                    symbolicTarget: null,
                  },
                ];
            return {
              repository: args.repository,
              refs: refs.filter((ref) => ref.label.includes(String(args.query))),
              currentBranch: "refs/heads/main",
              offset: args.offset,
              nextOffset: args.offset || args.query ? null : 2,
              truncated: false,
              warnings: [],
            };
          }
          if (command === "open_comparison")
            return {
              id: "snapshot",
              repository: "/repo",
              mode: "commits",
              baseLabel: "base",
              headLabel: "head",
              files: [],
              totalFiles: 0,
              warnings: [],
            };
          if (command === "workspace_list") return [];
          throw new Error(`Unexpected ${command}`);
        },
      },
    });
  }, options);
  await page.goto("/");
}
const calls = (page: Page, command: string) =>
  page.evaluate(
    (command) =>
      (
        window as unknown as {
          testCalls: Array<{ command: string; args?: Record<string, unknown> }>;
        }
      ).testCalls.filter((call) => call.command === command),
    command,
  );

test("directory selection and cancellation never open a repository automatically", async ({
  page,
}) => {
  await bridge(page);
  const path = page.getByLabel("Git 仓库路径");
  await path.fill("/original");
  await page.getByRole("button", { name: "选择仓库目录", exact: true }).click();
  await expect(path).toHaveValue("/original");
  await page.keyboard.press("ControlOrMeta+o");
  await expect(path).toHaveValue("/selected/repository");
  expect(await calls(page, "open_comparison")).toEqual([]);
  await page.getByRole("button", { name: "工作区", exact: true }).click();
  await page.evaluate(() => {
    (window as unknown as { pickerResults: string[] }).pickerResults = ["/workspace/other"];
  });
  await page.getByRole("button", { name: "选择仓库 1 目录", exact: true }).click();
  await expect(page.getByLabel("仓库 1 路径", { exact: true })).toHaveValue("/workspace/other");
});

test("branch choices are paged, while arbitrary commit IDs remain editable", async ({ page }) => {
  await bridge(page);
  await page.getByLabel("Git 仓库路径").fill("/repo");
  await page.getByLabel("比较模式").selectOption("commits");
  await page.getByRole("button", { name: "选择来源分支或提交", exact: true }).click();
  await expect(page.getByRole("button", { name: /本地 main/ })).toBeVisible();
  await page.screenshot({ path: "test-results/repository-selection.png" });
  await page.getByRole("button", { name: /本地 main/ }).click();
  await expect(page.getByRole("textbox", { name: "来源分支或提交", exact: true })).toHaveValue(
    "refs/heads/main",
  );
  await page.getByRole("button", { name: "选择目标分支或提交", exact: true }).click();
  await page.getByRole("button", { name: "更多引用" }).click();
  await page.getByRole("button", { name: /标签 v1/ }).click();
  const target = page.getByRole("textbox", { name: "目标分支或提交", exact: true });
  await expect(target).toHaveValue("refs/tags/v1");
  await target.fill("deadbeef1234567890");
  await page.getByRole("button", { name: "打开比较" }).click();
  expect((await calls(page, "open_comparison"))[0].args?.spec).toMatchObject({
    base: "refs/heads/main",
    head: "deadbeef1234567890",
  });
});

test("late branch lists cannot replace the current repository choices", async ({ page }) => {
  await bridge(page);
  await page.getByLabel("Git 仓库路径").fill("/slow");
  await page.getByLabel("比较模式").selectOption("commits");
  await page.getByRole("button", { name: "选择来源分支或提交", exact: true }).click();
  await expect.poll(async () => (await calls(page, "list_repository_refs")).length).toBe(1);
  await page.getByLabel("Git 仓库路径").fill("/new");
  if (
    !(
      (await page
        .getByRole("button", { name: "选择来源分支或提交", exact: true })
        .getAttribute("aria-expanded")) === "true"
    )
  )
    await page.getByRole("button", { name: "选择来源分支或提交", exact: true }).click();
  await expect(page.getByRole("button", { name: /本地 main/ })).toBeVisible();
  await page.waitForTimeout(800);
  await expect(page.getByRole("button", { name: /obsolete/ })).toHaveCount(0);
  expect((await calls(page, "cancel_job")).length).toBeGreaterThan(0);
});

test("native settings persist paths, refs and view preferences across reload without capture", async ({
  page,
}) => {
  await bridge(page);
  await page.getByLabel("Git 仓库路径").fill("/persisted");
  await page.getByLabel("比较模式").selectOption("commits");
  await page
    .getByRole("textbox", { name: "来源分支或提交", exact: true })
    .fill("refs/heads/release");
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByLabel("默认差异视图").selectOption("unified");
  await page.getByLabel("自动收起已证明的格式变化").uncheck();
  await expect(page.getByText("已自动保存到本机", { exact: true })).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => JSON.parse(localStorage.getItem("test.native.settings") ?? "null")?.spec.repository,
      ),
    )
    .toBe("/persisted");
  await page.evaluate(() => localStorage.removeItem("diffkin.preferences.v1"));
  await page.reload();
  await expect(page.getByLabel("Git 仓库路径")).toHaveValue("/persisted");
  await expect(page.getByRole("textbox", { name: "来源分支或提交", exact: true })).toHaveValue(
    "refs/heads/release",
  );
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(page.getByLabel("默认差异视图")).toHaveValue("unified");
  await expect(page.getByLabel("自动收起已证明的格式变化")).not.toBeChecked();
  expect(await calls(page, "open_comparison")).toEqual([]);
});

test("corrupt native settings are preserved until an explicit replacement", async ({ page }) => {
  await bridge(page, { corrupt: true });
  await page.getByLabel("Git 仓库路径").fill("/recovery");
  await page.getByRole("button", { name: "设置 · 未保存", exact: true }).click();
  await expect(page.getByText(/已保留原文件/)).toBeVisible();
  await page.waitForTimeout(400);
  expect(await calls(page, "preferences_save")).toEqual([]);
  await page.getByRole("button", { name: "保存当前设置并替换本地文件" }).click();
  await expect(page.getByText("已自动保存到本机", { exact: true })).toBeVisible();
  expect((await calls(page, "preferences_save"))[0].args?.preferences).toMatchObject({
    spec: { repository: "/recovery" },
  });
});

test("slow settings reads and writes never replace a newer user edit", async ({ page }) => {
  await bridge(page, { delayLoad: true, slowSave: true });
  await page.getByLabel("Git 仓库路径").fill("/first");
  await expect.poll(async () => (await calls(page, "preferences_save")).length).toBe(1);
  await page.getByLabel("Git 仓库路径").fill("/latest");
  await expect
    .poll(() =>
      page.evaluate(
        () => JSON.parse(localStorage.getItem("test.native.settings") ?? "null")?.spec.repository,
      ),
    )
    .toBe("/latest");
  await expect(page.getByLabel("Git 仓库路径")).toHaveValue("/latest");
});

test("legacy session preferences migrate without opening a repository", async ({ page }) => {
  await page.addInitScript(() => {
    if (!localStorage.getItem("diffkin.preferences.v1"))
      localStorage.setItem(
        "diffkin.session.v1",
        JSON.stringify({
          spec: {
            repository: "/legacy/repo",
            mode: "commits",
            base: "release",
            head: "feature",
            includeUntracked: false,
          },
          recent: ["/legacy/repo", "/another"],
        }),
      );
  });
  await bridge(page);
  await expect(page.getByLabel("Git 仓库路径")).toHaveValue("/legacy/repo");
  await expect
    .poll(() =>
      page.evaluate(
        () => JSON.parse(localStorage.getItem("test.native.settings") ?? "null")?.spec.base,
      ),
    )
    .toBe("release");
  expect(await calls(page, "open_comparison")).toEqual([]);
});

test("unsaved settings survive restart and replace older native settings after a successful load", async ({
  page,
}) => {
  await page.addInitScript(() => {
    if (localStorage.getItem("test.pending.seeded")) return;
    localStorage.setItem("test.pending.seeded", "true");
    const original = {
      schemaVersion: 1,
      spec: {
        repository: "/older",
        mode: "worktree",
        base: "HEAD~1",
        head: "HEAD",
        includeUntracked: true,
      },
      recent: [],
      view: { layout: "split", foldFormats: true, collapseUnchanged: true },
    };
    localStorage.setItem("test.native.settings", JSON.stringify(original));
    const pending = {
      ...original,
      spec: { ...original.spec, repository: "/pending/newest" },
      view: { ...original.view, layout: "unified" },
    };
    localStorage.setItem("diffkin.preferences.pending.v1", JSON.stringify(pending));
    localStorage.setItem("diffkin.preferences.v1", JSON.stringify(pending));
  });
  await bridge(page);
  await expect(page.getByLabel("Git 仓库路径")).toHaveValue("/pending/newest");
  await expect
    .poll(() =>
      page.evaluate(
        () => JSON.parse(localStorage.getItem("test.native.settings") ?? "null")?.spec.repository,
      ),
    )
    .toBe("/pending/newest");
  await expect
    .poll(() =>
      page.evaluate(() => {
        const pending = localStorage.getItem("diffkin.preferences.pending.v1");
        return pending === null || pending === localStorage.getItem("diffkin.preferences.ack.v1");
      }),
    )
    .toBe(true);
  expect(await calls(page, "open_comparison")).toEqual([]);
});

test("complete raw verification is an explicit, persistent comparison choice", async ({ page }) => {
  await bridge(page);
  await page.getByLabel("Git 仓库路径").fill("/verify-all");
  const verification = page.getByRole("checkbox", { name: "完整原文核验", exact: true });
  await expect(verification).not.toBeChecked();
  await verification.check();
  await page.getByRole("button", { name: "打开比较" }).click();
  expect((await calls(page, "open_comparison"))[0].args?.spec).toMatchObject({
    repository: "/verify-all",
    verifyAll: true,
  });
  await expect
    .poll(() =>
      page.evaluate(
        () => JSON.parse(localStorage.getItem("test.native.settings") ?? "null")?.spec.verifyAll,
      ),
    )
    .toBe(true);
  await page.reload();
  await expect(page.getByRole("checkbox", { name: "完整原文核验", exact: true })).toBeChecked();
  expect(await calls(page, "open_comparison")).toEqual([]);
});
