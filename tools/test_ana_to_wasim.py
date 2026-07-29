#!/usr/bin/env python3
"""Self-tests for ana_to_wasim.py (stdlib only). Run: python3 tools/test_ana_to_wasim.py"""
import importlib.util
import os
import sys

_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("ana_to_wasim", os.path.join(_here, "ana_to_wasim.py"))
A = importlib.util.module_from_spec(_spec)
sys.modules["ana_to_wasim"] = A
_spec.loader.exec_module(A)

_fails = []


def check(name, cond):
    print(("ok  " if cond else "FAIL") + "  " + name)
    if not cond:
        _fails.append(name)


def lower(defn):
    """Parse a definition, strip stubs, return (rendered, refs, n_stubs)."""
    res, err = A.parse_definition(defn)
    if res is None:
        return None, [], -1, err
    resolved = A._as_ast(res)  # resolve pending Calls (Checkbox, Choice, builtins…)
    ast, stubs = A.strip_stub_markers(resolved)
    return A._render_ast(resolved), sorted(A._collect_refs(ast)), len(stubs), None


# ── var-block lowering ────────────────────────────────────────────────────────
r, refs, ns, _ = lower("var delta := (a - b)/(c); delta * 2")
check("var-block inlines bindings", r == "(((a - b) / c) * 2)" and refs == ["a", "b", "c"] and ns == 0)

r, refs, ns, _ = lower("Var x := p + q; Var y := x * 2; y - p")
check("var-block chains bindings", refs == ["p", "q"] and ns == 0)

# million-suffix inside a binding, inlined into the body
r, refs, ns, _ = lower("var k := 1M; var result := k * base; result")
check("million-suffix + trailing binding", "1000000" in (r or "") and "base" in refs)

# one unconvertible fragment stubs locally, the rest survives
r, refs, ns, _ = lower("var bad := A[Dim=lbl]; var good := m + n; good + 1")
check("bad binding stubs locally, good survives", set(["m", "n"]).issubset(set(refs)))

# ── Choice / Checkbox decisions ───────────────────────────────────────────────
r, _, ns, _ = lower("Checkbox(1)")
check("Checkbox(1) -> 1", r == "1" and ns == 0)
r, _, ns, _ = lower("Choice(Platform_regions, 2)")
check("Choice(index, 2) -> selected position 2", r == "2" and ns == 0)
r, refs, ns, _ = lower("Table(Scenarios)(Checkbox(1), Checkbox(0), Checkbox(1))")
check("Checkbox inside Table lowers to 0/1 array", "1" in (r or "") and "0" in (r or ""))

# ── Table(Index)(values): the index dimension must not leak into the value list ─
_res, _ = A.parse_definition("Table(Action)(8,15,10,20,5,5,35,50)")
_ast = A._as_ast(_res)
check("Table array has exactly the value cells (index not leaked as element 0)",
      _ast.get("op") == "array" and len(_ast["elements"]) == 8
      and [c.get("value") for c in _ast["elements"]] == [8, 15, 10, 20, 5, 5, 35, 50])
_r, _refs, _ns, _ = lower("Table(Action)(8,15,10,20,5,5,35,50)")
check("Table index name is not treated as a data ref", "Action" not in _refs and _ns == 0)

# ── numeric suffixes ──────────────────────────────────────────────────────────
r, _, _, _ = lower("2K + 3M")
check("K/M suffixes tokenize", r == "(2000 + 3000000)")

# ── control cases still work ──────────────────────────────────────────────────
r, refs, ns, _ = lower("If a > b Then a Else b")
check("plain if/then/else unaffected", ns == 0 and refs == ["a", "b"])

d = A.call_to_distribution(A.Call("Lognormal", [A.lit(40) if hasattr(A, "lit") else {"op": "literal", "value": 40.0},
                                                {"op": "literal", "value": 1.3}], {}), "1")
check("lognormal(median,gsdev) -> log space", d is not None and abs(d["parameters"]["mean"]["value"] - __import__("math").log(40)) < 1e-9)

# non-constant distribution params degrade to a stub (None)
d2 = A.call_to_distribution(A.Call("Normal", [{"op": "ref", "element_id": "mu"}, {"op": "literal", "value": 1.0}], {}), "1")
check("formula-param distribution -> stub", d2 is None)

# ── array-language Phase 1 ops (sort/cumulate) map to builtins, drop index arg ──
for _defn, _fn in [("SortIndex(cost)", "sort_index"), ("Sort(x, I)", "sort_array"),
                   ("Rank(x)", "rank_array"), ("cumulate(x, Time)", "cumulate"),
                   ("cumproduct(x)", "cumproduct")]:
    _res, _ = A.parse_definition(_defn)
    _ast = A._as_ast(_res)
    check(f"{_defn} -> {_fn}(x) (index arg dropped, no stub)",
          _ast.get("op") == "call" and _ast.get("fn") == _fn and len(_ast.get("args", [])) == 1)

# ── Phase 2: reindex-by-index `x[Dim=idx]` → gather; `@Index` → ordinal ──
_res, _ = A.parse_definition("Amount[Action=Sorted]")
_ast = A._as_ast(_res)
check("x[Dim=idx] -> gather(x, idx)",
      _ast.get("op") == "call" and _ast.get("fn") == "gather"
      and [a.get("element_id") for a in _ast["args"]] == ["Amount", "Sorted"])

_res, _ = A.parse_definition("cumulate(Amount[Action=Sorted], Sorted)")
_ast = A._as_ast(_res)
check("cumulate(x[Dim=idx]) -> cumulate(gather(x, idx))",
      _ast.get("fn") == "cumulate" and _ast["args"][0].get("fn") == "gather")

_res, _ = A.parse_definition("@Sorted_action")
_ast = A._as_ast(_res)
check("@Index -> ordinal(Index)",
      _ast.get("op") == "call" and _ast.get("fn") == "ordinal"
      and _ast["args"][0].get("element_id") == "Sorted_action")

_r, _refs, _ns, _ = lower("@Cumulative = @Sorted + 1")
check("@ binds tighter than +/= (ordinal on each index, no stub)",
      _ns == 0 and set(_refs) == {"Cumulative", "Sorted"})

# a label subscript (string RHS) still degrades rather than mis-gathering
_res, _err = A.parse_definition("x[Dim='label']")
check("x[Dim='label'] (non-index RHS) does not silently gather", _res is None or
      (A._as_ast(_res).get("fn") != "gather"))

# ── Phase 3: null / Undefined → null() (→ NaN), not an inert 0 stub ──
for _defn in ["null", "Undefined"]:
    _res, _ = A.parse_definition(_defn)
    _ast = A._as_ast(_res)
    check(f"{_defn} -> null() builtin",
          _ast.get("op") == "call" and _ast.get("fn") == "null" and _ast.get("args") == [])
_r, _refs, _ns, _ = lower("if a > b then a else null")
check("if … else null keeps null() (no stub)", _ns == 0 and "null()" in (_r or ""))

print()
if _fails:
    print(f"{len(_fails)} failure(s): {_fails}")
    sys.exit(1)
print("all passed")
