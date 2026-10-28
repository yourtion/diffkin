import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { root, checkVersions } from "./version.mjs";

export const targets = {
  "aarch64-apple-darwin": ["dmg"],
  "x86_64-apple-darwin": ["dmg"],
  "x86_64-unknown-linux-gnu": ["deb", "AppImage"],
  "x86_64-pc-windows-msvc": ["exe"],
};

function filesBelow(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? filesBelow(join(directory, entry.name)) : [join(directory, entry.name)],
  );
}

export function expectedAssets(version) {
  return Object.entries(targets)
    .flatMap(([target, extensions]) => [
      ...extensions.map((extension) => `Diffkin-${version}-${target}.${extension}`),
      `diffkin-cli-${version}-${target}.tar.gz`,
    ])
    .sort();
}

function collect(target, output) {
  const extensions = targets[target];
  if (!extensions) throw new Error(`Unknown target: ${target}`);
  const { version } = checkVersions();
  const release = join(root, "target", target, "release");
  mkdirSync(output, { recursive: true });
  const bundleDirectories = { dmg: "dmg", deb: "deb", AppImage: "appimage", exe: "nsis" };
  for (const extension of extensions) {
    const bundles = filesBelow(join(release, "bundle", bundleDirectories[extension]));
    const candidates = bundles.filter((file) => file.endsWith(`.${extension}`));
    if (candidates.length !== 1)
      throw new Error(`Expected one ${extension} installer, found ${candidates.length}`);
    copyFileSync(candidates[0], join(output, `Diffkin-${version}-${target}.${extension}`));
  }
  const executable = target.includes("windows") ? "diffkin.exe" : "diffkin";
  const stage = mkdtempSync(join(tmpdir(), "diffkin-package-"));
  try {
    copyFileSync(join(release, executable), join(stage, executable));
    const archive = join(output, `diffkin-cli-${version}-${target}.tar.gz`);
    const run = spawnSync("tar", ["-czf", archive, "-C", stage, executable], { stdio: "inherit" });
    if (run.error) throw run.error;
    if (run.status !== 0) throw new Error("CLI packaging failed");
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

export function checksums(directory, version) {
  const files = readdirSync(directory)
    .filter((name) => name !== "SHA256SUMS")
    .sort();
  if (JSON.stringify(files) !== JSON.stringify(expectedAssets(version)))
    throw new Error("Release asset set is incomplete or contains unexpected files");
  const lines = files.map(
    (name) =>
      `${createHash("sha256")
        .update(readFileSync(join(directory, name)))
        .digest("hex")}  ${basename(name)}`,
  );
  writeFileSync(join(directory, "SHA256SUMS"), `${lines.join("\n")}\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, argument] = process.argv.slice(2);
  if (command === "collect") collect(argument, join(root, "artifacts"));
  else if (command === "checksums") checksums(resolve(argument), checkVersions().version);
  else throw new Error("Usage: release-assets.mjs collect <target> | checksums <directory>");
}
