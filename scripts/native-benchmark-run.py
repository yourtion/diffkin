"""Run the opt-in native build. Optional process sampling is a separate memory run.

WebKit attribution uses a before/during/after PID lifecycle, since procinfo needs root.
Do not start another WebKit application during a memory run. Surviving new helpers
are reported explicitly; their attribution is uncertain, not a certified app total.
"""
import argparse
import json
import os
import pathlib
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument("binary")
parser.add_argument("config")
parser.add_argument("output")
parser.add_argument("--memory", action="store_true")
args = parser.parse_args()
output = pathlib.Path(args.output).resolve()
output.unlink(missing_ok=True)
def processes():
    rows = subprocess.check_output(["ps", "-axo", "pid,ppid,rss,comm"], text=True).splitlines()[1:]
    return {int(fields[0]): (int(fields[1]), int(fields[2]), fields[3]) for line in rows if len(fields := line.split(None, 3)) == 4}
before = processes() if args.memory else {}
env = dict(os.environ, DIFFKIN_BENCH_CONFIG=pathlib.Path(args.config).read_text(), DIFFKIN_BENCH_OUTPUT=str(output))
with open(str(output) + ".log", "w") as log:
    process = subprocess.Popen([str(pathlib.Path(args.binary).resolve())], env=env, stdout=log, stderr=log)
    samples = []
    helpers = set()
    started = time.monotonic()
    try:
        while process.poll() is None:
            if time.monotonic() - started > 600:
                raise TimeoutError("Native benchmark exceeded 10 minutes")
            if args.memory:
                rows = processes()
                helpers.update(pid for pid, (_, _, name) in rows.items() if pid not in before and "com.apple.WebKit." in name)
                included = {process.pid} | helpers
                for _ in range(5):
                    included.update(pid for pid, (parent, _, _) in rows.items() if parent in included)
                live = sorted(included & rows.keys())
                footprint_file = str(output) + ".footprint.json"
                measured = subprocess.run(["footprint", *[arg for pid in live for arg in ["-p", str(pid)]], "--noCategories", "-j", footprint_file], capture_output=True, text=True)
                footprint = json.loads(pathlib.Path(footprint_file).read_text()) if measured.returncode == 0 else {"errors": [measured.stderr]}
                samples.append({"seconds": time.monotonic() - started, "rssKiB": sum(rows[pid][1] for pid in live), "processes": [{"pid": pid, "rssKiB": rows[pid][1], "name": pathlib.Path(rows[pid][2]).name} for pid in live], "footprintBytes": footprint.get("total footprint"), "footprintProcesses": footprint.get("processes"), "errors": footprint.get("errors")})
            time.sleep(.1 if not args.memory else 1)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
    if args.memory:
        time.sleep(3)
        surviving = sorted(helpers & processes().keys())
        pathlib.Path(str(output) + ".memory.json").write_text(json.dumps({"method": "physical footprint and RSS sum; new WebKit PID lifecycle plus host descendants; RSS may double-count shared pages; 1s sampling can miss short peaks", "helpers": sorted(helpers), "survivingHelpers": surviving, "samples": samples}, indent=2))
    result = json.loads(output.read_text()) if output.exists() else {}
    raise SystemExit(process.returncode or (0 if result.get("complete") else 1))
