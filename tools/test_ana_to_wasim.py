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
    ast, stubs = A.strip_stub_markers(A._as_ast(res))
    return A._render_ast(res), sorted(A._collect_refs(ast)), len(stubs), None


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

print()
if _fails:
    print(f"{len(_fails)} failure(s): {_fails}")
    sys.exit(1)
print("all passed")
