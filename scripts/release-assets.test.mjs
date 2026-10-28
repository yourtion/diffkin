import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { checksums, expectedAssets, packageCli } from "./release-assets.mjs";

test("CLI archive uses local paths on Windows drives and preserves the executable", () => {
  const directory = mkdtempSync(join(tmpdir(), "diffkin archive "));
  try {
    const executable = join(directory, "diffkin.exe");
    const archive = join(directory, "release.tar.gz");
    const bytes = Buffer.from([0x4d, 0x5a, 0, 10, 13, 255]);
    writeFileSync(executable, bytes);
    const license = join(directory, "MIT.txt");
    const notices = join(directory, "notices.txt");
    writeFileSync(license, "MIT fixture");
    writeFileSync(notices, "Third party fixture");
    if (process.platform === "darwin") {
      const attribute = spawnSync("xattr", [
        "-w",
        "com.diffkin.archive-test",
        "fixture",
        executable,
      ]);
      assert.equal(attribute.status, 0, attribute.stderr.toString());
    }
    packageCli(executable, archive, { license, notices });
    const extracted = spawnSync("tar", ["-xzOf", "release.tar.gz", "diffkin.exe"], {
      cwd: directory,
    });
    assert.equal(extracted.status, 0, extracted.stderr.toString());
    assert.deepEqual(extracted.stdout, bytes);
    const listing = spawnSync("tar", ["-tzf", "release.tar.gz"], {
      cwd: directory,
      encoding: "utf8",
    });
    assert.equal(listing.status, 0, listing.stderr);
    assert.deepEqual(listing.stdout.trim().split(/\r?\n/).sort(), [
      "LICENSE",
      "THIRD-PARTY-NOTICES.txt",
      "diffkin.exe",
    ]);
    const bundledNotice = spawnSync("tar", ["-xzOf", "release.tar.gz", "THIRD-PARTY-NOTICES.txt"], {
      cwd: directory,
      encoding: "utf8",
    });
    assert.equal(bundledNotice.stdout, "Third party fixture");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

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
