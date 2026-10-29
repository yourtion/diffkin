"""Create a disposable, deterministic 10k tracked / 20 modified file fixture."""
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve()
functions = int(sys.argv[2]) if len(sys.argv) > 2 else 200
changes = int(sys.argv[4]) if len(sys.argv) > 4 else 20
if not 1 <= changes <= 10000 or not 1 <= functions <= 1000:
    raise SystemExit("Expected 1..10000 changes and 1..1000 functions")
root.mkdir(parents=True, exist_ok=False)
def git(*args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()
git("init", "-q")
def source(i, updated):
    text = "\n".join(
        f"/** {'documented behavior ' * 5} */\nexport function value_{i}_{j}(input: number) {{ return input {'>=' if updated and j == min(75, functions - 1) else '>'} {j} ? {j + 1} : {j}; }}"
        for j in range(functions)
    ) + "\n"
    return text.replace("return input", "return   input") if updated and len(sys.argv) > 3 and sys.argv[3] == "format" else text
for i in range(10000):
    target = root / "packages" / f"module-{i // 100:03d}" / f"file-{i:05d}.ts"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(source(i, False) if i < changes else f"export const value = {i};\n")
git("add", ".")
git("-c", "user.name=Diffkin benchmark", "-c", "user.email=benchmark@example.invalid", "commit", "-qm", "fixed baseline")
base = git("rev-parse", "HEAD")
for i in range(changes):
    (root / "packages" / f"module-{i // 100:03d}" / f"file-{i:05d}.ts").write_text(source(i, True))
git("add", ".")
git("-c", "user.name=Diffkin benchmark", "-c", "user.email=benchmark@example.invalid", "commit", "-qm", "fixed comparison")
print(json.dumps({"spec": {"repository": str(root), "mode": "commits", "base": base, "head": git("rev-parse", "HEAD"), "includeUntracked": False}, "iterations": 30}))
