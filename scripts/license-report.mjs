import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const supplementalDirectory = join(root, "third-party/licenses");

export function supplementalTexts(key, directory = supplementalDirectory) {
  const manifest = JSON.parse(readFileSync(join(directory, "manifest.json"), "utf8"));
  return (manifest[key]?.sources ?? []).map((source) => {
    if (!/^[a-f0-9]{64}\.txt$/.test(source.file))
      throw new Error("Invalid license source filename");
    const bytes = readFileSync(join(directory, source.file));
    if (`${createHash("sha256").update(bytes).digest("hex")}.txt` !== source.file)
      throw new Error(`License text digest mismatch: ${source.file}`);
    return { name: source.path, text: bytes.toString("utf8"), source: source.url };
  });
}

export function licenseFiles(directory, declaredFile) {
  const candidates = readdirSync(directory)
    .filter((name) => /^(licen[sc]e|copying|notice|copyright)([._-]|$)/i.test(name))
    .map((name) => join(directory, name));
  if (declaredFile) candidates.push(resolve(directory, declaredFile));
  const files = new Set();
  function visit(path, depth = 0) {
    if (!existsSync(path) || depth > 2) return;
    const local = relative(directory, path);
    if (local.startsWith("..") || resolve(path) === resolve(directory)) return;
    const stat = statSync(path);
    if (stat.isDirectory()) {
      for (const name of readdirSync(path)) visit(join(path, name), depth + 1);
    } else if (stat.isFile() && stat.size <= 256_000) files.add(path);
  }
  candidates.forEach((path) => visit(path));
  return [...files].sort().map((path) => ({
    name: relative(directory, path).replaceAll("\\", "/"),
    text: readFileSync(path, "utf8"),
  }));
}

function packageDirectory(from, name) {
  let directory = from;
  for (;;) {
    const candidate = join(directory, "node_modules", name, "package.json");
    if (existsSync(candidate)) return dirname(realpathSync(candidate));
    const parent = dirname(directory);
    if (directory === parent) throw new Error(`Installed dependency not found: ${name}`);
    directory = parent;
  }
}

export function javascriptInventory(appDirectory) {
  const seen = new Set();
  const result = [];
  function visit(directory, local = false) {
    const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
    const key = `${manifest.name}@${manifest.version ?? "workspace"}`;
    if (seen.has(key)) return;
    seen.add(key);
    if (!local && !manifest.private) {
      const declared = manifest.license;
      result.push({
        ecosystem: "npm",
        name: manifest.name,
        version: manifest.version,
        license: typeof declared === "string" ? declared : (declared?.type ?? null),
        repository:
          typeof manifest.repository === "string"
            ? manifest.repository
            : (manifest.repository?.url ?? null),
        texts: licenseFiles(directory),
      });
    }
    for (const name of Object.keys(manifest.dependencies ?? {}).sort()) {
      visit(packageDirectory(directory, name));
    }
    for (const name of Object.keys(manifest.optionalDependencies ?? {}).sort()) {
      try {
        visit(packageDirectory(directory, name));
      } catch (error) {
        if (!String(error).includes("Installed dependency not found")) throw error;
      }
    }
  }
  visit(appDirectory, true);
  return result;
}

export function rustInventory(metadata) {
  return metadata.packages
    .filter((pkg) => pkg.source !== null)
    .map((pkg) => ({
      ecosystem: "cargo",
      name: pkg.name,
      version: pkg.version,
      license: pkg.license,
      repository: pkg.repository,
      source: pkg.source,
      sourceArchive: `https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download`,
      texts: (() => {
        const packaged = licenseFiles(dirname(pkg.manifest_path), pkg.license_file);
        return packaged.length ? packaged : supplementalTexts(`cargo/${pkg.name}@${pkg.version}`);
      })(),
    }));
}

export function renderReport(dependencies) {
  const sorted = [...dependencies].sort((a, b) =>
    `${a.ecosystem}/${a.name}/${a.version}`.localeCompare(
      `${b.ecosystem}/${b.name}/${b.version}`,
      "en",
    ),
  );
  const missing = sorted.filter((item) => !item.license || item.texts.length === 0);
  const summary = [
    "# Diffkin third-party dependency inventory",
    "",
    "Generated from the locked Cargo graph and installed JavaScript production dependency graph.",
    "Cargo includes build/test and platform dependencies; this is a conservative inventory, not a runtime-only SBOM.",
    "License expressions are upstream declarations, not a legal assessment or a license choice for Diffkin.",
    "License/notice files supplied in dependency packages or verified upstream sources are reproduced. Supplemental text digests and origins are tracked in third-party/licenses/manifest.json.",
    "Platform libraries, WebView, Git, toolchain components, and installer helper downloads are outside this inventory.",
    "",
    `Dependencies: ${sorted.length}. Packages requiring license-text review: ${missing.length}.`,
    "",
    "| Ecosystem | Package | Version | Declared license | Included license files |",
    "| --- | --- | --- | --- | --- |",
    ...sorted.map(
      (item) =>
        `| ${item.ecosystem} | ${item.name} | ${item.version} | ${item.license ?? "UNKNOWN"} | ${item.texts.map((text) => text.name).join(", ") || "MISSING — review required"} |`,
    ),
    "",
  ].join("\n");
  const notices = sorted
    .map((item) =>
      [
        `${item.ecosystem}: ${item.name}@${item.version}`,
        `Declared license: ${item.license ?? "UNKNOWN"}`,
        `Source: ${item.repository ?? item.source ?? "not declared"}`,
        ...(item.sourceArchive ? [`Source archive: ${item.sourceArchive}`] : []),
        ...item.texts.map(
          (file) => `\n--- ${file.name}${file.source ? ` (${file.source})` : ""} ---\n${file.text}`,
        ),
        ...(item.texts.length ? [] : ["LICENSE TEXT NOT INCLUDED IN PACKAGE — review required"]),
      ].join("\n"),
    )
    .join("\n\n========================================\n\n");
  return { dependencies: sorted, missing, summary, notices: `${notices}\n` };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [
    outputArg = "target/third-party-notices",
    metadataPath = process.env.DIFFKIN_LICENSE_METADATA,
  ] = process.argv.slice(2);
  let metadata;
  if (metadataPath) metadata = JSON.parse(readFileSync(resolve(metadataPath), "utf8"));
  else {
    const run = spawnSync(
      "cargo",
      [
        "metadata",
        "--manifest-path",
        join(root, "Cargo.toml"),
        "--format-version",
        "1",
        "--locked",
      ],
      { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] },
    );
    if (run.error) throw run.error;
    if (run.status !== 0) throw new Error("Cargo dependency metadata failed");
    metadata = JSON.parse(run.stdout);
  }
  const report = renderReport([
    ...rustInventory(metadata),
    ...javascriptInventory(join(root, "apps/desktop")),
  ]);
  const output = resolve(root, outputArg);
  mkdirSync(output, { recursive: true });
  const inventory = {
    projectVersion: JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version,
    inputs: Object.fromEntries(
      ["Cargo.lock", "pnpm-lock.yaml"].map((name) => [
        name,
        createHash("sha256")
          .update(readFileSync(join(root, name)))
          .digest("hex"),
      ]),
    ),
    dependencies: report.dependencies,
    requiresTextReview: report.missing.map(({ ecosystem, name, version }) => ({
      ecosystem,
      name,
      version,
    })),
  };
  writeFileSync(join(output, "inventory.json"), `${JSON.stringify(inventory, null, 2)}\n`);
  writeFileSync(join(output, "README.md"), report.summary);
  writeFileSync(join(output, "THIRD-PARTY-NOTICES.txt"), report.notices);
  console.log(
    `License inventory: ${report.dependencies.length} dependencies; ${report.missing.length} need text review. Output: ${output}`,
  );
  if (report.missing.length)
    throw new Error(
      "Dependency license text is incomplete; review the generated inventory before packaging",
    );
}
