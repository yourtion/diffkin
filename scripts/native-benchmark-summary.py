"""Summarize actual native samples; fail if any measured gate is exceeded."""
import json
import math
import sys

report = json.load(open(sys.argv[1]))
if not report.get("complete"):
    raise SystemExit(report.get("error", "Incomplete native run"))
limits = {"capture-list": 1000, "uncached-text": 300, "uncached-structure": 1000, "cached-switch": 100, "cancel-feedback": 100, "cancel-release": 250, "capture-cancel-feedback": 100, "analysis-cancel-feedback": 100, "capture-cancel-release": 250, "analysis-cancel-release": 250}
summary = {}
for name in sorted({sample["name"] for sample in report["samples"]}):
    values = sorted(sample["ms"] for sample in report["samples"] if sample["name"] == name)
    summary[name] = {"count": len(values), "p50": values[math.ceil(len(values) * .5) - 1], "p95": values[math.ceil(len(values) * .95) - 1], "max": max(values), "limit": limits.get(name)}
print(json.dumps(summary, indent=2))
if any(item["count"] < 30 or item["limit"] is not None and item["p95"] > item["limit"] for item in summary.values()):
    raise SystemExit(1)
