import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import {
  javascriptInventory,
  licenseFiles,
  renderReport,
  supplementalTexts,
} from "./license-report.mjs";

test("license inventory traverses production dependencies and preserves notices", () => {
  const directory = mkdtempSync(join(tmpdir(), "diffkin-licenses-"));
  try {
    const dependency = join(directory, "node_modules", "example");
    mkdirSync(dependency, { recursive: true });
    writeFileSync(
      join(directory, "package.json"),
      JSON.stringify({
        name: "app",
        dependencies: { example: "1" },
        devDependencies: { absent: "1" },
      }),
    );
    writeFileSync(
      join(dependency, "package.json"),
      JSON.stringify({ name: "example", version: "1.0.0", license: "MIT" }),
    );
    writeFileSync(join(dependency, "LICENSE"), "Copyright fixture\nPermission fixture\n");
    const inventory = javascriptInventory(directory);
    assert.equal(inventory.length, 1);
    assert.equal(inventory[0].texts[0].text, "Copyright fixture\nPermission fixture\n");
    const report = renderReport(inventory);
    assert.equal(report.missing.length, 0);
    assert.match(report.notices, /Copyright fixture/);
    assert.doesNotMatch(report.summary, new RegExp(directory));
    assert.equal(licenseFiles(dependency, "../../package.json").length, 1);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("missing upstream text is explicitly reported without inventing a license", () => {
  const report = renderReport([
    {
      ecosystem: "cargo",
      name: "unknown",
      version: "1",
      license: null,
      repository: null,
      texts: [],
    },
  ]);
  assert.equal(report.missing.length, 1);
  assert.match(report.summary, /UNKNOWN/);
  assert.match(report.summary, /MISSING — review required/);
  assert.match(report.notices, /LICENSE TEXT NOT INCLUDED/);
});

test("supplemental upstream license text is version pinned and digest checked", () => {
  const directory = mkdtempSync(join(tmpdir(), "diffkin-license-source-"));
  try {
    const text = "Upstream license text\n";
    const file = `${createHash("sha256").update(text).digest("hex")}.txt`;
    writeFileSync(join(directory, file), text);
    writeFileSync(
      join(directory, "manifest.json"),
      JSON.stringify({
        "cargo/example@1.0.0": {
          sources: [{ file, path: "LICENSE", url: "https://example.org/source/commit/LICENSE" }],
        },
      }),
    );
    assert.equal(supplementalTexts("cargo/example@1.0.0", directory)[0].text, text);
    assert.deepEqual(supplementalTexts("cargo/example@2.0.0", directory), []);
    writeFileSync(join(directory, file), "changed");
    assert.throws(() => supplementalTexts("cargo/example@1.0.0", directory), /digest mismatch/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
