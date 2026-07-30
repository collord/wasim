#!/usr/bin/env python3
"""Structural tests for the v2-native emit of `ana_to_wasim.py`.

Complements `test_ana_corpus.py` (which runs the whole corpus through schema
validation). This pins the specific v2 constructs each converter stage adds, on
small real corpus models, so a regression names the exact broken shape.

Run: python3 tools/test_ana_v2_emit.py
"""
import importlib.util
import os
import sys

_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    "ana_to_wasim", os.path.join(_here, "ana_to_wasim.py"))
A = importlib.util.module_from_spec(_spec)
sys.modules["ana_to_wasim"] = A
_spec.loader.exec_module(A)

CORPUS = os.path.join(_here, "fixtures", "ana_corpus")
_fails = []


def check(name, cond, detail=""):
    print(("ok  " if cond else "FAIL") + "  " + name + (f"  — {detail}" if detail and not cond else ""))
    if not cond:
        _fails.append(name)


def convert(fixture):
    text = open(os.path.join(CORPUS, fixture), encoding="utf-8", errors="replace").read()
    A.WARNINGS.clear()
    return A.convert(text)


def by_id(model, eid):
    return next((e for e in model["elements"] if e["id"] == eid), None)


# ── Stage 1: v2 shell + dimensions[] + nested vector_map tables ────────────────

m = convert("Month_to_quarter.ana")

check("v2 wasim_version", m.get("wasim_version") == "0.9.8")
check("first element carries `primitive` (is_v2_native fires)",
      "primitive" in m["elements"][0])
check("no legacy v1 top-level `wasim_version 0.1.0`", m["wasim_version"] != "0.1.0")

dids = [d["id"] for d in m.get("dimensions", [])]
check("declared Indexes → dimensions[]", dids == ["Year", "Month", "Quarter"], str(dids))
year = next((d for d in m["dimensions"] if d["id"] == "Year"), {})
check("dimension carries size + labels",
      year.get("size") == 3 and year.get("labels") == ["2003", "2004", "2005"])

tbl = by_id(m, "Monthly_us_oil_avg_c")
check("Table(Year,Month) → value_rule expression", tbl and tbl.get("value_rule") == "expression")
check("Table output tagged with its dimensions",
      tbl and tbl["outputs"][0].get("dimensions") == ["Year", "Month"])
ast = tbl["expression"]["ast"] if tbl else {}
check("Table emits nested vector_map (Year outer, Month inner)",
      ast.get("op") == "vector_map" and ast.get("over") == "Year"
      and ast.get("body", {}).get("op") == "vector_map"
      and ast["body"].get("over") == "Month")
check("innermost body indexes into a flat values vector",
      ast.get("body", {}).get("body", {}).get("op") == "index")

# `1..12` range materialized (Month index has 12 members)
month = next((d for d in m["dimensions"] if d["id"] == "Month"), {})
check("`a..b` range syntax materializes inclusive members", month.get("size") == 12)

# An Index becomes BOTH a dimension_def and a dimensioned member-carrier node.
year_carrier = by_id(m, "Year")
check("Index emits a member-carrier node dimensioned over itself",
      year_carrier is not None
      and (year_carrier.get("outputs") or [{}])[0].get("dimensions") == ["Year"])

# A node with no dimensioned inputs stays scalar (the deferred Aggregate stub).
scalar = by_id(m, "Quarter_US_Oil_Consu")
check("a node with no dimensioned refs has no output dimensions",
      scalar is not None and not (scalar.get("outputs") or [{}])[0].get("dimensions"))


# ── Stage 2: multi-index tables + axis-selective reducers + shape inference ────

# Elementwise product of two dimensioned arrays → axes UNIONED and sorted by id
# (align-by-name canonical order, mirroring the engine's broadcast_named).
tm = by_id(m, "Transfer_matrix")
check("elementwise mix re-sorts axes by id (align-by-name)",
      tm and (tm.get("outputs") or [{}])[0].get("dimensions") == ["Month", "Quarter", "Year"])

# Sum(Transfer_matrix, month) → axis-selective sum_array with the 1-based POSITION
# of Month in the reduced array's runtime order ([Month,Quarter,Year] → pos 1).
qtr = by_id(m, "Qtr_us_oil_consump")
qast = qtr["expression"]["ast"] if qtr else {}
check("axis reducer emits sum_array (not a bare collapse)", qast.get("fn") == "sum_array")
check("reducer keeps the axis arg as a 1-based position",
      len(qast.get("args", [])) == 2 and qast["args"][1] == {"op": "literal", "value": 1.0})
check("reduced result drops the summed axis (id-sorted remainder)",
      qtr and (qtr.get("outputs") or [{}])[0].get("dimensions") == ["Quarter", "Year"])
check("no leftover `_reduce_axis` markers survive emit",
      "_reduce_axis" not in __import__("json").dumps(m))


# ── Stage 2b: DetermTable scalar-selector + domain/Choice semantics ────────────

c = convert("Compression_Post_Load_Capacity.ana")

# `D = Choice(Self, 3)` over numeric domain [2,3,4,5,…] → the MEMBER VALUE 5
# (used numerically as `D-0.5`), NOT the position.
d = by_id(c, "D")
check("Choice(Self,k) over a numeric domain → selected member value",
      d and d.get("value_rule") == "fixed" and d["value"]["value"] == 5.0)
# `Grade = Choice(Self, 0)` over a label domain → the 1-based position 1.
grade = by_id(c, "Grade")
check("Choice(Self,k) over a label domain → 1-based position",
      grade and grade["value"]["value"] == 1.0)

# `Fc0 = DetermTable(Grade)(…)` → get_element(values, Grade's position 1).
fc0 = by_id(c, "Fc0")
fc0a = fc0["expression"]["ast"] if fc0 else {}
check("single-selector DetermTable → get_element at the selector position",
      fc0a.get("fn") == "get_element"
      and fc0a.get("args", [{}, {}])[1] == {"op": "literal", "value": 1.0})
check("get_element selects the correct first cell (1700)",
      fc0a.get("args", [{}])[0]["elements"][0] == {"op": "literal", "value": 1700.0})

# `Cf = DetermTable(D, Grade)(…)` → a 2-selector flat get_element (multi-index).
cf = by_id(c, "Cf")
cfa = cf["expression"]["ast"] if cf else {}
check("multi-selector DetermTable → single flat get_element",
      cfa.get("fn") == "get_element"
      and cfa.get("args", [{}, {}])[1].get("op") == "literal")


# ── Stage 3: label / positional subscript ─────────────────────────────────────

vm = convert("Vector_Math.ana")


def find_op(node, op):
    if isinstance(node, dict):
        if node.get("op") == op:
            return node
        for v in node.values():
            r = find_op(v, op)
            if r:
                return r
    elif isinstance(node, list):
        for v in node:
            r = find_op(v, op)
            if r:
                return r
    return None

# `Point_Coordinates[Points=1, Coords='Latitude']` → chained
# index(positional 1) then subscript(dim=Coords, label='Latitude').
sub = None
for el in vm["elements"]:
    sub = find_op(el.get("expression", {}).get("ast", {}), "subscript")
    if sub:
        break
check("label subscript emits a `subscript` node with dim + label",
      sub is not None and sub.get("dim") == "Coords" and sub.get("label") == "Latitude")
check("numeric subscript nests as a positional `index`",
      sub is not None and (sub.get("array") or {}).get("op") == "index"
      and (sub["array"].get("indices") or [{}])[0] == {"op": "literal", "value": 1.0})
# The targeted dimension must actually carry labels so the engine resolves it.
coords = next((d for d in vm.get("dimensions", []) if d["id"] == "Coords"), {})
check("subscripted dimension declares matching labels",
      "Latitude" in coords.get("labels", []))


print()
if _fails:
    print(f"{len(_fails)} failure(s): {_fails}")
    sys.exit(1)
print("all passed")
