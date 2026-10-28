import { readFileSync, writeFileSync, appendFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "smol-toml";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const releaseVersion =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(alpha|beta|rc)\.(0|[1-9]\d*))?$/;

export function tagInfo(version, tag) {
  if (typeof version !== "string" || version.trim() !== version || !releaseVersion.test(version))
    throw new Error(`Unsupported release version: ${version}`);
  if (tag !== `v${version}`) throw new Error(`Tag ${tag} does not match v${version}`);
  return { version, tag, prerelease: version.includes("-") };
}

export function checkVersions(directory = root, tag) {
  const read = (path) => readFileSync(resolve(directory, path), "utf8");
  const pkg = JSON.parse(read("package.json"));
  const cargo = parse(read("Cargo.toml"));
  const lock = parse(read("Cargo.lock"));
  const config = JSON.parse(read("apps/desktop/src-tauri/tauri.conf.json"));
  const info = tagInfo(pkg.version, tag ?? `v${pkg.version}`);
  if (!pkg.private || cargo.workspace.package.publish !== false)
    throw new Error("Workspace packages must remain private");
  if (cargo.workspace.package.version !== pkg.version)
    throw new Error("Cargo workspace version differs from package.json");
  if (
    resolve(directory, "apps/desktop/src-tauri", config.version) !==
    resolve(directory, "package.json")
  )
    throw new Error("Tauri must read the root package.json version");
  for (const member of cargo.workspace.members) {
    const manifest = parse(read(`${member}/Cargo.toml`));
    if (manifest.package.version.workspace !== true)
      throw new Error(`${member} must inherit workspace version`);
    const locked = lock.package.find(
      (entry) => entry.name === manifest.package.name && !entry.source,
    );
    if (locked?.version !== pkg.version)
      throw new Error(`Cargo.lock version mismatch: ${manifest.package.name}`);
  }
  return info;
}

export function setVersion(version, directory = root) {
  tagInfo(version, `v${version}`);
  checkVersions(directory);
  const packagePath = resolve(directory, "package.json");
  const cargoPath = resolve(directory, "Cargo.toml");
  const lockPath = resolve(directory, "Cargo.lock");
  const pkg = JSON.parse(readFileSync(packagePath, "utf8"));
  const cargoText = readFileSync(cargoPath, "utf8");
  const lockText = readFileSync(lockPath, "utf8");
  const cargo = parse(cargoText);
  const names = new Set(
    cargo.workspace.members.map(
      (member) =>
        parse(readFileSync(resolve(directory, member, "Cargo.toml"), "utf8")).package.name,
    ),
  );
  const nextCargo = cargoText.replace(
    /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"[^"]+"/,
    `$1"${version}"`,
  );
  const nextLock = lockText
    .split("[[package]]")
    .map((block, index) => {
      if (!index) return block;
      const entry = parse(block);
      return names.has(entry.name) && !entry.source
        ? block.replace(/(\nversion = )"[^"]+"/, `$1"${version}"`)
        : block;
    })
    .join("[[package]]");
  pkg.version = version;
  writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);
  writeFileSync(cargoPath, nextCargo);
  writeFileSync(lockPath, nextLock);
  return checkVersions(directory);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const command = process.argv[2];
    if (command !== "set" && command !== "check")
      throw new Error("Usage: version.mjs check [tag] | set <version>");
    const info =
      command === "set" ? setVersion(process.argv[3]) : checkVersions(root, process.argv[3]);
    if (process.env.GITHUB_OUTPUT)
      appendFileSync(
        process.env.GITHUB_OUTPUT,
        `version=${info.version}\nprerelease=${info.prerelease}\n`,
      );
    console.log(`Version verified: ${info.tag} (${info.prerelease ? "prerelease" : "stable"})`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
