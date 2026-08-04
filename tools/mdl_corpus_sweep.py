#!/usr/bin/env python3
"""
mdl_corpus_sweep.py — run tools/mdl_challenge.py over a tree of Vensim `.mdl` files and
triage the results into an actionable importer backlog.

For every `.mdl` under CORPUS it runs the challenge harness and buckets the outcome:
  PASS     — WaSiM matched the pysimlin reference within --tol on every aligned variable
  FAIL     — ran on both sides but drifted; the OFF variables + the unmapped-builtin
             warnings that root-caused the drift are collected
  ERROR    — could not be benchmarked. Sub-split by cause:
               * upstream: pysimlin/xmutil could not load or simulate the model (not a
                 WaSiM signal — the format construct isn't even ingestible)
               * harness:  "no overlapping variables" — WaSiM ran but the harness could not
                 align names (array/subscript members, or constant-only models with no saved
                 trajectory). A measurement gap, not an engine gap.
               * wasim:    the WaSiM engine itself rejected the model (e.g. dependency cycle)

The headline number to read is PASS / (PASS + FAIL + wasim-errors) — the *measurable*
dynamic-scalar models — not PASS / total, which is diluted by upstream + harness buckets.

Usage:
    python3 tools/mdl_corpus_sweep.py path/to/test-models [--tol 1e-2]
Writes mdl_sweep_results.json in the CWD.
"""
import argparse
import collections
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CHALLENGE = os.path.join(HERE, "mdl_challenge.py")
BUILTIN_RE = re.compile(r"XMILE-UNMAPPED-BUILTIN: '([^']+)'")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="Triage a .mdl corpus through the WaSiM challenge harness.")
    ap.add_argument("corpus", help="directory tree of .mdl files (e.g. a clone of SDXorg/test-models)")
    ap.add_argument("--tol", default="1e-2")
    ap.add_argument("--timeout", type=int, default=90)
    args = ap.parse_args(argv)

    mdls = sorted(os.path.join(r, f)
                  for r, _, fs in os.walk(args.corpus) for f in fs if f.endswith(".mdl"))

    results = []
    off_counter = collections.Counter()
    builtin_counter = collections.Counter()

    for i, path in enumerate(mdls):
        rel = os.path.relpath(path, args.corpus)
        try:
            p = subprocess.run([sys.executable, CHALLENGE, path, "--tol", args.tol],
                               capture_output=True, text=True, timeout=args.timeout)
            rc, out, err = p.returncode, p.stdout, p.stderr
        except subprocess.TimeoutExpired:
            rc, out, err = 124, "", "timeout"

        off_vars = [ln.split()[1] for ln in out.splitlines() if ln.strip().startswith("OFF")]
        worst = None
        m = re.search(r"worst max_abs_err = ([\d.eE+-]+)", out)
        if m:
            worst = float(m.group(1))
        for b in set(BUILTIN_RE.findall(out)):
            builtin_counter[b] += 1

        if rc == 0:
            status, cause = "PASS", ""
        elif rc == 1:
            status, cause = "FAIL", ""
            for v in off_vars:
                off_counter[v] += 1
        elif rc == 124:
            status, cause = "ERROR", "timeout"
        else:
            status = "ERROR"
            eline = next((l for l in err.splitlines() if l.startswith("error:")),
                         (err.strip().splitlines() or ["?"])[-1])[:160]
            if "no overlapping" in eline:
                cause = "harness"
            elif "xmutil could not load" in eline or "reference run failed" in eline:
                cause = "upstream"
            elif "engine rejected" in eline or "graph error" in eline:
                cause = "wasim"
            else:
                cause = "other"
            err = eline

        results.append({"status": status, "cause": cause, "model": rel,
                        "worst": worst, "off": off_vars,
                        "err": err if status == "ERROR" else ""})
        tag = f"{status}/{cause}" if cause else status
        extra = f" worst={worst:.2g}" if worst is not None else ""
        print(f"[{i+1}/{len(mdls)}] {tag:14} {rel}{extra}")

    # ---- summary ----
    cats = collections.Counter(r["status"] for r in results)
    causes = collections.Counter(r["cause"] for r in results if r["status"] == "ERROR")
    wasim_err = causes.get("wasim", 0)
    measurable = cats["PASS"] + cats["FAIL"] + wasim_err

    print("\n" + "=" * 70)
    print(f"CORPUS {args.corpus}   models: {len(mdls)}")
    print(f"BY STATUS: {dict(cats)}")
    print(f"ERROR causes: {dict(causes)}")
    if measurable:
        print(f"\nMEASURABLE (dynamic scalar) = {measurable}   "
              f"PASS {cats['PASS']}/{measurable} = {100*cats['PASS']//measurable}%")
    print("\nUNMAPPED BUILTINS (root-cause backlog — # models each appears in):")
    for b, c in builtin_counter.most_common(40):
        print(f"  {c:3}  {b}")
    print("\nTOP OFF VARIABLES (drift symptoms):")
    for v, c in off_counter.most_common(15):
        print(f"  {c:3}  {v}")

    outpath = os.path.join(os.getcwd(), "mdl_sweep_results.json")
    with open(outpath, "w") as fh:
        json.dump(results, fh, indent=1)
    print(f"\nwrote {outpath}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
