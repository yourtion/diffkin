import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { root } from "./version.mjs";

const path = resolve(root, "packages/protocol/src/generated.ts");
const original = readFileSync(path, "utf8");
const run = spawnSync(
  "cargo",
  [
    "run",
    "--locked",
    "--manifest-path",
    resolve(root, "Cargo.toml"),
    "-p",
    "diffkin-domain",
    "--example",
    "export_protocol",
  ],
  { stdio: "inherit" },
);
if (run.error) throw run.error;
if (run.status !== 0) process.exit(run.status ?? 1);
if (original !== readFileSync(path, "utf8")) {
  console.error("Generated protocol changed. Run pnpm protocol:generate and commit the result.");
  process.exitCode = 1;
}
