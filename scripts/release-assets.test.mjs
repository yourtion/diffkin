import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checksums, expectedAssets } from "./release-assets.mjs";

test("release requires all four targets and produces repeatable SHA256SUMS", () => {
  const directory = mkdtempSync(join(tmpdir(), "diffkin-assets-"));
  try {
    assert.throws(() => checksums(directory, "0.1.0"), /incomplete/);
    for (const name of expectedAssets("0.1.0")) writeFileSync(join(directory, name), "fixture");
    checksums(directory, "0.1.0");
    const first = readFileSync(join(directory, "SHA256SUMS"), "utf8");
    assert.equal(first.trim().split("\n").length, 9);
    checksums(directory, "0.1.0");
    assert.equal(readFileSync(join(directory, "SHA256SUMS"), "utf8"), first);
    writeFileSync(join(directory, "unexpected.exe"), "fixture");
    assert.throws(() => checksums(directory, "0.1.0"), /unexpected/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
