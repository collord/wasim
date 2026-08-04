#!/usr/bin/env python3
"""
mdl_challenge.py — challenge the WaSiM engine with a real Vensim `.mdl` (or XMILE) model
by round-tripping it through the existing XMILE importer and diffing the WaSiM trajectory
against a reference simulator (pysimlin), the same way simlin's build_notebook.py exercises
the simlin engine.

Pipeline (no native `.mdl` parser — reuse the XMILE path, which is how simlin ingests
Vensim too):

    model.mdl ──pysimlin.load──▶ Project.to_xmile() ──▶ xmile_to_wasim.py ──▶ model.json
                                                                                    │
    pysimlin reference: model.run().results (the oracle) ◀── diff ──▶ wasim-validate --trajectories

Requires:
    pip install pysimlin
    a release build of the engine binary: (cd engine && cargo build --release --bin wasim-validate)

Usage:
    python3 tools/mdl_challenge.py path/to/model.mdl
    python3 tools/mdl_challenge.py path/to/model.xmile --tol 1e-3
    python3 tools/mdl_challenge.py model.mdl --keep out_dir/   # keep intermediate xmile+json

Exit code is 0 when every aligned variable is within tolerance, 1 otherwise — so this drops
straight into CI as a conformance gate over a corpus (e.g. SDXorg/test-models).

Alignment notes (surfaced empirically; see VENSIM_MDL_IMPORT_SCOUT.md §3):
  * Names. WaSiM ids are module-qualified + slugged (`Main/teacup_temperature`); pysimlin
    columns are the same slug unqualified. We match on the trailing slug.
  * Time. WaSiM's saved history row `i` spans the step `[t_i, t_i+dt)`, and the initial point
    is not emitted (N steps vs the reference's N+1 rows). Within that row a STOCK is sampled at
    the END of the step, while a FLOW/AUX is evaluated at the START (Euler reads the pre-step
    state). So we compare a stock's `series[i]` against the reference at `t_i + dt`, and a
    flow/aux's `series[i]` against the reference at `t_i`. The primitive type is read from the
    generated model.json.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
XMILE_TO_WASIM = os.path.join(HERE, "xmile_to_wasim.py")
ENGINE_BIN = os.path.join(REPO, "engine", "target", "release", "wasim-validate")


def die(msg: str) -> "None":
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(2)


def slug(name: str) -> str:
    """Trailing identifier, lowercased, non-alnum → underscore — the common denominator
    between a WaSiM id (`Main/teacup_temperature`) and a pysimlin column (`teacup_temperature`)."""
    tail = name.split("/")[-1]
    return re.sub(r"[^a-z0-9]+", "_", tail.lower()).strip("_")


def load_reference(mdl_path: str):
    """Return (xmile_text, {slug: {time: value}}, dt). pysimlin is both the .mdl→XMILE bridge
    and the reference oracle — exactly simlin's own ingestion path for Vensim."""
    try:
        import simlin  # noqa
    except ImportError:
        die("pysimlin not installed — `pip install pysimlin`")

    model = simlin.load(mdl_path)
    xmile = model.project.to_xmile()
    if isinstance(xmile, bytes):
        xmile = xmile.decode("utf-8")

    df = model.run().results  # pandas DataFrame indexed by time
    ref: dict[str, dict[float, float]] = {}
    times = [float(t) for t in df.index]
    for col in df.columns:
        ref[slug(col)] = {t: float(v) for t, v in zip(times, df[col].tolist())}
    dt = float(df["dt"].iloc[0]) if "dt" in df.columns else (times[1] - times[0] if len(times) > 1 else 1.0)
    return xmile, ref, dt


def run_wasim(model_json_path: str):
    """Invoke the engine binary and return {slug: [mean per step]}, time_axis."""
    if not os.path.exists(ENGINE_BIN):
        die(f"engine binary not found at {ENGINE_BIN}\n"
            f"       build it: (cd engine && cargo build --release --bin wasim-validate)")
    out = subprocess.run([ENGINE_BIN, model_json_path, "--trajectories"],
                         capture_output=True, text=True)
    if not out.stdout.strip():
        die(f"engine produced no output:\n{out.stderr}")
    rep = json.loads(out.stdout)
    if not rep.get("ok"):
        die(f"engine rejected the model: {rep.get('errors')}")
    traj = rep.get("trajectories") or {}
    series = {slug(k): v for k, v in traj.get("series", {}).items()}
    return series, traj.get("time_axis", []), rep.get("warnings", [])


def nearest(ref_series: dict[float, float], t: float, tol: float = 1e-6):
    """Reference value at time t (exact key, else nearest within a small window)."""
    if t in ref_series:
        return ref_series[t]
    best_k = min(ref_series, key=lambda k: abs(k - t))
    return ref_series[best_k] if abs(best_k - t) <= max(tol, 1e-6) else None


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="Differential-test the WaSiM engine against pysimlin on a Vensim/XMILE model.")
    ap.add_argument("model", help="path to a .mdl (Vensim) or .xmile file")
    ap.add_argument("--tol", type=float, default=1e-2, help="max abs error allowed per variable (default 1e-2)")
    ap.add_argument("--keep", metavar="DIR", help="keep intermediate .xmile and .json in DIR")
    args = ap.parse_args(argv)

    if not os.path.exists(args.model):
        die(f"no such file: {args.model}")

    workdir = args.keep or tempfile.mkdtemp(prefix="mdl_challenge_")
    os.makedirs(workdir, exist_ok=True)
    base = os.path.splitext(os.path.basename(args.model))[0]
    xmile_path = os.path.join(workdir, base + ".xmile")
    json_path = os.path.join(workdir, base + ".wasim.json")

    # 1. .mdl (or .xmile) → XMILE + reference oracle via pysimlin.
    if args.model.lower().endswith(".xmile") or args.model.lower().endswith(".xml"):
        # Still route through pysimlin so the reference and the imported XMILE agree.
        xmile_text, ref, dt = load_reference(args.model)
    else:
        xmile_text, ref, dt = load_reference(args.model)
    with open(xmile_path, "w") as fh:
        fh.write(xmile_text)

    # 2. XMILE → WaSiM JSON via the existing importer.
    conv = subprocess.run([sys.executable, XMILE_TO_WASIM, xmile_path, "-o", json_path],
                          capture_output=True, text=True)
    if conv.returncode != 0:
        die(f"xmile_to_wasim failed:\n{conv.stderr}")
    import_warnings = [ln for ln in conv.stderr.splitlines() if ln.strip()]

    # 3. WaSiM engine run → trajectories. Classify primitives for time alignment.
    with open(json_path) as fh:
        model = json.load(fh)
    stock_slugs = {slug(e["id"]) for e in model.get("elements", []) if e.get("primitive") == "stock"}
    wasim_series, time_axis, engine_warnings = run_wasim(json_path)

    # 4. Align + diff. WaSiM series[i] ↔ reference at time_axis[i] + dt (end-of-step convention).
    print(f"\n=== {base}: WaSiM vs pysimlin ===")
    print(f"    dt={dt}  steps={len(time_axis)}  ref_vars={len(ref)}  wasim_vars={len(wasim_series)}")
    common = sorted(set(wasim_series) & set(ref))
    skipped = sorted(set(wasim_series) - set(ref))
    if not common:
        die("no overlapping variables between WaSiM output and reference")

    worst = 0.0
    rows = []
    for name in common:
        w = wasim_series[name]
        # Stocks sampled end-of-step (t+dt); flows/auxes evaluated start-of-step (t).
        offset = dt if name in stock_slugs else 0.0
        max_abs = 0.0
        max_rel = 0.0
        n = 0
        for i, t in enumerate(time_axis):
            if i >= len(w):
                break
            r = nearest(ref[name], t + offset)
            if r is None:
                continue
            ae = abs(w[i] - r)
            max_abs = max(max_abs, ae)
            denom = max(abs(r), 1e-9)
            max_rel = max(max_rel, ae / denom)
            n += 1
        worst = max(worst, max_abs)
        status = "ok " if max_abs <= args.tol else "OFF"
        rows.append((status, name, max_abs, max_rel, n))

    w = max((len(n) for _, n, *_ in rows), default=8)
    print(f"    {'':3} {'variable':<{w}}  {'max_abs_err':>12}  {'max_rel_err':>12}  pts")
    for status, name, ma, mr, n in rows:
        print(f"    {status} {name:<{w}}  {ma:>12.6g}  {mr:>12.6g}  {n}")
    if skipped:
        print(f"    (no reference column for: {', '.join(skipped)})")

    all_warnings = import_warnings + [f"engine: {x}" for x in engine_warnings]
    if all_warnings:
        print(f"\n    warnings ({len(all_warnings)}):")
        for x in all_warnings[:20]:
            print(f"      - {x}")

    ok = worst <= args.tol
    print(f"\n    worst max_abs_err = {worst:.6g}   tol = {args.tol}   → {'PASS' if ok else 'FAIL'}")
    if args.keep:
        print(f"    intermediates kept in {workdir}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
