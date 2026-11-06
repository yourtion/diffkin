/* Test-build-only driver, embedded by the native-benchmark Cargo feature.
 * Operates the real React UI and Tauri IPC. Two rAFs after the DOM condition
 * conservatively bound a rendering opportunity, not physical pixel presentation.
 */
(async () => {
  if (window.__diffkinBenchmarkRunning) return;
  window.__diffkinBenchmarkRunning = true;
  await new Promise((resolve) => setTimeout(resolve, 100));
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const control = (action, report) => invoke("benchmark_control", { action, report });
  const report = { samples: [], ipc: [], environment: {}, complete: false };
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  const painted = async () => {
    await frame();
    await frame();
  };
  const wait = async (condition, description) => {
    const start = performance.now();
    while (!condition()) {
      if (performance.now() - start > 25000)
        throw new Error(
          `Timeout: ${description}; ${document.querySelector('[role="alert"]')?.textContent ?? ""}`,
        );
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
  };
  const button = (name) =>
    [...document.querySelectorAll("button")].find(
      (node) => (node.getAttribute("aria-label") ?? node.textContent.trim()) === name,
    );
  const input = async (label, value) => {
    const element = document.querySelector(`[aria-label="${label}"]`);
    if (!element) throw new Error(`Missing field: ${label}`);
    const prototype =
      element.tagName === "SELECT" ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(element, value);
    element.dispatchEvent(
      new Event(element.tagName === "SELECT" ? "change" : "input", { bubbles: true }),
    );
    await painted();
  };
  const ready = () => !!button("标记已审查") && !button("标记已审查").disabled;
  const readable = () => !!document.querySelector(".cm-content")?.textContent;
  const measure = async (name, action, condition, extra = {}) => {
    const start = performance.now();
    action();
    await wait(condition, name);
    await painted();
    const ms = performance.now() - start;
    report.samples.push({ name, ms, ...extra });
    return ms;
  };
  try {
    const config = await control("config");
    report.config = config;

    // Resource lifecycle check through real IPC; no visible-latency claim.
    // It can run without foreground rendering and owns two disk snapshots at exit.
    if (config.lifecycleOnly) {
      await control("lifecycle");
      return;
    }

    report.environment = {
      userAgent: navigator.userAgent,
      width: innerWidth,
      height: innerHeight,
      pixelRatio: devicePixelRatio,
      timeOrigin: performance.timeOrigin,
    };
    window.__diffkinBenchmarkIpc = report.ipc;
    await wait(() => button("单仓库比较"), "workspace home");
    button("单仓库比较").click();
    await wait(() => button("开始比较"), "initial form");
    // Let the native preference load settle before editing the isolated profile.
    await new Promise((resolve) => setTimeout(resolve, config.idleMs ?? 1000));
    await input("Git 仓库路径", config.spec.repository);
    await input("比较模式", config.spec.mode);
    if (config.spec.mode === "commits") {
      await input("基准（修改前）", config.spec.base);
      await input("目标（修改后）", config.spec.head);
    }
    for (let iteration = 0; iteration < config.iterations; iteration++) {
      await control("reset");
      const opened = report.ipc.filter((item) => item.command === "open_comparison").length;
      const start = performance.now();
      (button("刷新比较") ?? button("开始比较")).click();
      await wait(
        () =>
          report.ipc.filter((item) => item.command === "open_comparison").length > opened &&
          !!document.querySelector(".review-context"),
        "capture",
      );
      await painted();
      report.samples.push({ name: "capture-list", ms: performance.now() - start, iteration });
      await wait(ready, "first file analysis");
      await painted();
      if (document.querySelector("#review-files").hidden)
        document.querySelector(".review-context button").click();
      await painted();
      const files = [...document.querySelectorAll(".file-item")]
        .slice(0, 3)
        .map((node) => node.getAttribute("aria-label"));
      if (files.length < 3) throw new Error("Benchmark requires three visible changed files");
      // Refresh preserves the selected path. Measure cold visits on the other two files.
      const currentPath = document.querySelector(".file-heading h2")?.title;
      const [firstSwitch, secondSwitch] = files.filter((path) => path !== currentPath);
      for (const path of [firstSwitch, secondSwitch]) {
        await control("reset");
        const selected = () => document.querySelector(".file-heading h2")?.title === path;
        const startFile = performance.now();
        button(path).click();
        await wait(() => selected() && readable(), "uncached text");
        await painted();
        report.samples.push({
          name: "uncached-text",
          ms: performance.now() - startFile,
          iteration,
          path,
        });
        await wait(() => selected() && ready(), "uncached structure");
        await painted();
        report.samples.push({
          name: "uncached-structure",
          ms: performance.now() - startFile,
          iteration,
          path,
        });
      }
      button(firstSwitch).click();
      await wait(
        () => document.querySelector(".file-heading h2")?.title === firstSwitch && ready(),
        "warm cache",
      );
      await painted();
      await measure(
        "cached-switch",
        () => button(secondSwitch).click(),
        () =>
          document.querySelector(".file-heading h2")?.title === secondSwitch &&
          ready() &&
          readable(),
        { iteration },
      );
      if (config.cancellation) {
        for (const operation of ["capture", "analysis"]) {
          await control("reset");
          (operation === "capture" ? button("刷新比较") : button(files[0])).click();
          await wait(() => button("取消"), `${operation} cancellation available`);
          if (!(await control("status")).active)
            throw new Error(`${operation} completed before cancellation; sample invalid`);
          const cancelStart = performance.now();
          button("取消").click();
          await wait(
            () => !button("取消") && document.body.textContent.includes("已取消。"),
            "cancel feedback",
          );
          await painted();
          report.samples.push({
            name: `${operation}-cancel-feedback`,
            ms: performance.now() - cancelStart,
            iteration,
          });
          while ((await control("status")).active) {
            if (performance.now() - cancelStart > 5000)
              throw new Error("Cancelled job did not release");
            await new Promise((resolve) => setTimeout(resolve, 2));
          }
          report.samples.push({
            name: `${operation}-cancel-release`,
            ms: performance.now() - cancelStart,
            iteration,
          });
        }
      }
      await control("report", report);
    }
    if (config.lastPath) {
      const started = performance.now();
      await input("筛选文件", config.lastPath);
      await wait(() => button(config.lastPath), "last catalog path");
      button(config.lastPath).click();
      await wait(
        () =>
          document.querySelector(".file-heading h2")?.title === config.lastPath &&
          ready() &&
          readable(),
        "last file source",
      );
      await painted();
      report.samples.push({
        name: "last-catalog-file",
        ms: performance.now() - started,
        path: config.lastPath,
      });
      report.giant = {
        total: document.querySelector(".panel-heading span")?.textContent,
        selected: document.querySelector(".file-heading h2")?.title,
        visibleRows: document.querySelectorAll(".file-item").length,
        source: document.querySelector('[aria-label="修改后源码"]')?.textContent,
      };
    }
    if (config.layout) {
      if (!document.querySelector("#review-files").hidden)
        document.querySelector(".review-context button").click();
      report.layouts = [];
      for (const [width, height] of [
        [760, 540],
        [1200, 800],
      ]) {
        await control("viewport", { width, height });
        await painted();
        const overflow = document.documentElement.scrollWidth > innerWidth;
        report.layouts.push({ width: innerWidth, height: innerHeight, overflow });
        if (overflow) throw new Error("Native layout horizontally overflows");
      }
    }
    report.complete = true;
  } catch (error) {
    report.error = `${error?.message ?? error}\n${error?.stack ?? ""}`;
  }
  await control("report", report);
  await control("exit");
})();
