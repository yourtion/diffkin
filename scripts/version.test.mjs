import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { root, tagInfo, checkVersions, setVersion } from "./version.mjs";

test("release tags must exactly match stable or supported prerelease versions", () => {
  assert.equal(tagInfo("1.2.3", "v1.2.3").prerelease, false);
  assert.equal(tagInfo("0.1.0-alpha.0", "v0.1.0-alpha.0").prerelease, true);
  for (const version of [
    "01.2.3",
    "1.2",
    "1.2.3-beta.01",
    "1.2.3+meta",
    "1.2.3\n",
    "1.2.3\nother=value",
  ]) {
    assert.throws(() => tagInfo(version, `v${version}`));
  }
  assert.throws(() => tagInfo("1.2.3", "v1.2.4"));
});

test("version bump updates inherited Rust packages and lockfile, and detects drift", () => {
  const directory = mkdtempSync(join(tmpdir(), "diffkin-version-"));
  try {
    for (const file of [
      "package.json",
      "Cargo.toml",
      "Cargo.lock",
      "crates",
      "apps/cli",
      "apps/desktop/src-tauri",
    ]) {
      cpSync(join(root, file), join(directory, file), {
        recursive: true,
        filter: (path) => !path.includes("/gen/") && !path.includes("/icons/"),
      });
    }
    setVersion("0.2.0-beta.1", directory);
    assert.equal(checkVersions(directory, "v0.2.0-beta.1").version, "0.2.0-beta.1");
    const path = join(directory, "Cargo.toml");
    writeFileSync(
      path,
      readFileSync(path, "utf8").replace('version = "0.2.0-beta.1"', 'version = "0.3.0"'),
    );
    assert.throws(() => checkVersions(directory), /differs/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
