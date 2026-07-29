#!/usr/bin/env python3
"""Self-tests for xmile_to_wasim.py (stdlib only for the converter; this test file
uses `jsonschema` if available for the schema-validation check).

Run: python3 tools/test_xmile_to_wasim.py
"""
import importlib.util
import json
import os
import sys

_here = os.path.dirname(os.path.abspath(__file__))
_repo = os.path.dirname(_here)
_spec = importlib.util.spec_from_file_location(
    "xmile_to_wasim", os.path.join(_here, "xmile_to_wasim.py"))
X = importlib.util.module_from_spec(_spec)
sys.modules["xmile_to_wasim"] = X
_spec.loader.exec_module(X)

_fails = []


def check(name, cond):
    print(("ok  " if cond else "FAIL") + "  " + name)
    if not cond:
        _fails.append(name)


def parse(eqn, dt=1.0):
    """Parse + lower an eqn to a WaSiM AST (IdentRefs left unresolved, Calls lowered
    via a fresh LowerCtx). Returns (ast, err). Extras (from element-producing
    builtins) are discarded here — see parse_ctx for tests that need them."""
    X.reset_warnings()
    ast, err = X.parse_eqn(eqn)
    if err is not None:
        return None, err
    ctx = X.LowerCtx(who="test", dt=dt, scope=("Main",))
    return X.lower_ast(ast, ctx), None


def parse_ctx(eqn, dt=1.0):
    """Like parse() but returns (ast, ctx) so tests can inspect ctx.extras."""
    X.reset_warnings()
    ast, err = X.parse_eqn(eqn)
    ctx = X.LowerCtx(who="test", dt=dt, scope=("Main",))
    if err is not None:
        return None, ctx
    return X.lower_ast(ast, ctx), ctx


# ── eqn parser: precedence & associativity ────────────────────────────────────
ast, err = parse("2 + 3 * 4")
check("precedence: * binds tighter than +",
      err is None and ast["op"] == "add" and ast["right"]["op"] == "multiply")

ast, err = parse("2 ^ 3 ^ 2")
# right-assoc: 2 ^ (3 ^ 2)
check("power is right-associative",
      err is None and ast["op"] == "power" and ast["right"]["op"] == "power")

ast, err = parse("-2 ^ 2")
# unary minus looser than ^  →  -(2^2)
check("unary minus looser than power (-2^2 = -(2^2))",
      err is None and ast["op"] == "neg" and ast["operand"]["op"] == "power")

ast, err = parse("(a - b) / c")
check("parenthesized grouping",
      err is None and ast["op"] == "divide" and ast["left"]["op"] == "subtract")

# ── quoted identifiers ────────────────────────────────────────────────────────
ast, err = parse('"Room Temperature" + 1')
check("quoted identifier parses as a ref name",
      err is None and ast["op"] == "add"
      and isinstance(ast["left"], X.IdentRef) and ast["left"].name == "Room Temperature")

# ── comparisons / logic / IF (Phase 2 grammar, parser supports now) ───────────
ast, err = parse("IF a > b THEN 1 ELSE 0")
check("IF/THEN/ELSE",
      err is None and ast["op"] == "if" and ast["cond"]["op"] == "gt")

ast, err = parse("a <> b")
check("<> maps to neq", err is None and ast["op"] == "neq")

# ── parse failure degrades, never raises ──────────────────────────────────────
ast, err = parse("3 +")
check("incomplete eqn returns error, does not raise", ast is None and err is not None)

# ── builtin lowering ──────────────────────────────────────────────────────────
ast, err = parse("SQRT(x)")
check("SQRT → call sqrt", err is None and ast["op"] == "call" and ast["fn"] == "sqrt")

ast, err = parse("MAX(a, 0)")
check("MAX → call max", err is None and ast["op"] == "call" and ast["fn"] == "max")

X.reset_warnings()
ast, err = parse("WEIRDFN(a, b)")
check("unknown builtin → extern_call + warning",
      err is None and ast["op"] == "extern_call" and ast["fn"] == "WEIRDFN"
      and any("XMILE-UNMAPPED-BUILTIN" in w for w in X.WARNINGS))

# ── full teacup conversion (emitter shapes) ───────────────────────────────────
X.reset_warnings()
with open(os.path.join(_here, "fixtures", "teacup.xmile"), encoding="utf-8") as f:
    teacup = X.convert(f.read())

els = {e["id"]: e for e in teacup["elements"]}

check("teacup: 4 elements, zero warnings",
      len(teacup["elements"]) == 4 and not X.WARNINGS)

stock = els.get("Main/Teacup Temperature")
check("teacup stock: primitive + initial_value 180 (NOT a rate)",
      stock and stock["primitive"] == "stock" and stock["initial_value"]["value"] == 180.0)
check("teacup stock: outflow qualified to Main/Heat Loss to Room",
      stock and stock["outflows"] == ["Main/Heat Loss to Room"] and stock["inflows"] == [])

flow = els.get("Main/Heat Loss to Room")
check("teacup flow: expression node with divide-at-root AST",
      flow and flow["value_rule"] == "expression"
      and flow["expression"]["ast"]["op"] == "divide")
check("teacup flow: refs resolved to qualified ids",
      flow and set(flow["inputs"]) == {
          "Main/Teacup Temperature", "Main/Room Temperature", "Main/Characteristic Time"})

aux = els.get("Main/Room Temperature")
check("teacup aux: constant eqn → fixed node value 70",
      aux and aux["value_rule"] == "fixed" and aux["value"]["value"] == 70.0)

check("teacup sim_settings: duration 30, timestep 0.125, n_realizations 1",
      teacup["simulation_settings"]["duration"]["value"] == 30.0
      and teacup["simulation_settings"]["timestep"]["value"] == 0.125
      and teacup["simulation_settings"]["n_realizations"] == 1)

check("teacup top-level: wasim_version 0.9.8",
      teacup["wasim_version"] == "0.9.8")

# ── graceful degradation: rk4 + start-offset warn, model still builds ─────────
X.reset_warnings()
rk = X.convert('''<xmile xmlns="http://docs.oasis-open.org/xmile/ns/XMILE/v1.0">
  <sim_specs method="rk4"><start>1990</start><stop>2000</stop><dt>1</dt></sim_specs>
  <model><variables><aux name="a"><eqn>1</eqn></aux></variables></model></xmile>''')
check("rk4 → INTEGRATOR-DOWNCONVERT warning, model still built",
      any("XMILE-INTEGRATOR-DOWNCONVERT" in w for w in X.WARNINGS)
      and len(rk["elements"]) == 1)
check("start≠0 → START-OFFSET warning; duration = stop − start",
      any("XMILE-START-OFFSET" in w for w in X.WARNINGS)
      and rk["simulation_settings"]["duration"]["value"] == 10.0)

# ── dangling reference degrades to 0 + warning ────────────────────────────────
X.reset_warnings()
dangle = X.convert('''<xmile xmlns="http://docs.oasis-open.org/xmile/ns/XMILE/v1.0">
  <sim_specs><start>0</start><stop>1</stop><dt>1</dt></sim_specs>
  <model><variables><aux name="a"><eqn>nonexistent + 1</eqn></aux></variables></model></xmile>''')
check("dangling ref → DANGLING-REF warning",
      any("XMILE-DANGLING-REF" in w for w in X.WARNINGS))

# ── Phase 3: builtin lowering (composed + element-producing) ──────────────────
ast, _ = parse("TIME()")
check("TIME() → time_ref elapsed", ast["op"] == "time_ref" and ast["property"] == "elapsed")

ast, _ = parse("DT()")
check("DT() → time_ref timestep", ast["op"] == "time_ref" and ast["property"] == "timestep")

ast, _ = parse("PI()")
check("PI() → literal", ast["op"] == "literal" and abs(ast["value"] - 3.14159265) < 1e-6)

ast, _ = parse("LOG(x, 2)")
check("LOG(x, base) → ln(x)/ln(base)",
      ast["op"] == "divide" and ast["left"]["fn"] == "ln" and ast["right"]["fn"] == "ln")

ast, _ = parse("STEP(5, 3)")
check("STEP(h,t0) → if(elapsed>=t0, h, 0)",
      ast["op"] == "if" and ast["cond"]["op"] == "gte"
      and ast["cond"]["left"]["op"] == "time_ref")

ast, _ = parse("RAMP(2, 4)")
check("RAMP(slope,t0) → slope*max(0, elapsed-t0)",
      ast["op"] == "multiply" and ast["right"]["op"] == "call" and ast["right"]["fn"] == "max")

ast, _ = parse("PULSE(10, 5)", dt=0.5)
check("PULSE(mag,first) → one-step if with area mag/dt",
      ast["op"] == "if" and ast["then"]["op"] == "divide"
      and ast["then"]["right"]["value"] == 0.5)

# element-producing: PREVIOUS → lag node (needs a resolved ref input)
ref = {"op": "ref", "element_id": "Main/x"}
X.reset_warnings()
_ctx = X.LowerCtx(who="Main/host", dt=1.0, scope=("Main",))
res = X.lower_call("previous", [ref, {"op": "literal", "value": 3.0}], _ctx)
check("PREVIOUS → lag node emitted + host refs it",
      res["op"] == "ref" and len(_ctx.extras) == 1
      and _ctx.extras[0]["value_rule"] == "lag"
      and _ctx.extras[0]["input"] == "Main/x"
      and _ctx.extras[0]["initial"]["value"] == 3.0)

# SMTH1 → filter/ema with τ→steps window
X.reset_warnings()
_ctx = X.LowerCtx(who="Main/host", dt=0.25, scope=("Main",))
res = X.lower_call("smth1", [ref, {"op": "literal", "value": 1.0}], _ctx)
check("SMTH1 → filter/ema, window = round(τ/dt)",
      res["op"] == "ref" and _ctx.extras[0]["value_rule"] == "filter"
      and _ctx.extras[0]["statistic"] == "ema" and _ctx.extras[0]["window"] == 4)

# per-step random → sample node with resampling always
X.reset_warnings()
_ctx = X.LowerCtx(who="Main/host", dt=1.0, scope=("Main",))
res = X.lower_call("normal", [{"op": "literal", "value": 0.0},
                              {"op": "literal", "value": 1.0}], _ctx)
check("NORMAL(mean,sd) → sample node, resampling always",
      res["op"] == "ref" and _ctx.extras[0]["value_rule"] == "sample"
      and _ctx.extras[0]["distribution"]["family"] == "normal"
      and _ctx.extras[0]["resampling"]["mode"] == "always")

# ── Phase 2: embedded gf, non_negative, MOD, IF ───────────────────────────────
X.reset_warnings()
with open(os.path.join(_here, "fixtures", "phase2_features.xmile"), encoding="utf-8") as f:
    p2 = X.convert(f.read())
p2els = {e["id"]: e for e in p2["elements"]}

check("embedded gf → host + companion __gf lookup node",
      "Main/effect_of_backlog" in p2els and "Main/effect_of_backlog__gf" in p2els)
host = p2els.get("Main/effect_of_backlog")
check("embedded gf host output is a lookup_call",
      host and host["expression"]["ast"]["op"] == "lookup_call"
      and host["expression"]["ast"]["element_id"] == "Main/effect_of_backlog__gf")

bl = p2els.get("Main/Backlog")
check("non_negative stock → floor 0",
      bl and bl.get("floor", {}).get("value") == 0.0)

oc = p2els.get("Main/orders_completed")
check("non_negative flow → max(expr, 0) wrap",
      oc and oc["expression"]["ast"]["op"] == "call"
      and oc["expression"]["ast"]["fn"] == "max")

par = p2els.get("Main/parity")
past = par["expression"]["ast"] if par else {}
check("IF + MOD lowering (a MOD b → call mod)",
      past.get("op") == "if" and past["cond"]["op"] == "eq"
      and past["cond"]["left"].get("op") == "call" and past["cond"]["left"]["fn"] == "mod")

# ── Phase 4: apply-to-all arrays + modules ────────────────────────────────────
X.reset_warnings()
with open(os.path.join(_here, "fixtures", "phase4_arrays.xmile"), encoding="utf-8") as f:
    p4a = X.convert(f.read())
p4aels = {e["id"]: e for e in p4a["elements"]}

check("array: top-level dimension with labels emitted",
      p4a.get("dimensions") == [{"id": "Region", "name": "Region", "size": 3,
                                 "labels": ["North", "South", "East"]}])
bp = p4aels.get("Main/base_pop")
check("apply-to-all array → vector_map over the dimension + dimensioned output",
      bp and bp["expression"]["ast"]["op"] == "vector_map"
      and bp["expression"]["ast"]["over"] == "Region"
      and bp["outputs"][0]["dimensions"] == ["Region"])
tot = p4aels.get("Main/total")
check("SUM(arrayvar) → call sum_array",
      tot and tot["expression"]["ast"]["op"] == "call"
      and tot["expression"]["ast"]["fn"] == "sum_array")

X.reset_warnings()
with open(os.path.join(_here, "fixtures", "phase4_modules.xmile"), encoding="utf-8") as f:
    p4m = X.convert(f.read())
conts = {c["id"]: c for c in p4m.get("containers", [])}
sub = conts.get("Main/sub")
check("module → submodel container with interior elements",
      sub and sub["kind"] == "submodel"
      and set(sub["elements"]) == {"Main/sub/input_signal", "Main/sub/doubled"})
check("module <connect> → interface input binding {input, from}",
      sub and sub["interface"]["inputs"] == [
          {"input": "Main/sub/input_signal", "from": "Main/driver"}])
check("module emits MODULE-SEMANTICS caveat warning",
      any("XMILE-MODULE-SEMANTICS" in w for w in X.WARNINGS))

# Parent reference to an interior variable (dotted `sub.doubled`) → submodel_stat.
p4mels = {e["id"]: e for e in p4m["elements"]}
res = p4mels.get("Main/result")
res_ast = res["expression"]["ast"] if res else {}
check("parent read of module interior (sub.doubled) → submodel_stat(mean)",
      res_ast.get("op") == "add"
      and res_ast["left"].get("op") == "submodel_stat"
      and res_ast["left"]["submodel_id"] == "Main/sub"
      and res_ast["left"]["output"] == "Main/sub/doubled"
      and res_ast["left"]["statistic"] == "mean")
check("no dangling-ref warning for the module interior read",
      not any("XMILE-DANGLING-REF" in w for w in X.WARNINGS))

# ── Feature B: non-constant stock initial → expression (no warning) ───────────
X.reset_warnings()
binit = X.convert('''<xmile xmlns="http://docs.oasis-open.org/xmile/ns/XMILE/v1.0">
  <sim_specs><start>0</start><stop>2</stop><dt>1</dt></sim_specs>
  <model><variables>
    <aux name="total"><eqn>1000</eqn></aux>
    <stock name="pool"><eqn>total</eqn></stock>
  </variables></model></xmile>''')
binit_els = {e["id"]: e for e in binit["elements"]}
pool = binit_els.get("Main/pool")
check("non-constant stock initial → expression initial_value (not scalar-0)",
      pool and isinstance(pool["initial_value"], dict) and "ast" in pool["initial_value"]
      and pool["initial_value"]["ast"]["op"] == "ref")
check("non-constant stock initial emits no XMILE-STOCK-INITIAL-EXPR warning",
      not any("XMILE-STOCK-INITIAL-EXPR" in w for w in X.WARNINGS))

# constant stock initial stays scalar (regression)
X.reset_warnings()
cinit = X.convert('''<xmile xmlns="http://docs.oasis-open.org/xmile/ns/XMILE/v1.0">
  <sim_specs><start>0</start><stop>2</stop><dt>1</dt></sim_specs>
  <model><variables><stock name="s"><eqn>180</eqn></stock></variables></model></xmile>''')
cs = {e["id"]: e for e in cinit["elements"]}["Main/s"]
check("constant stock initial stays a scalar quantity",
      cs["initial_value"] == {"value": 180.0, "unit": "1"})

# ── robustness: bad input + alternate namespace ───────────────────────────────
try:
    X.convert("this is not xml")
    check("non-XML input raises ConvertError", False)
except X.ConvertError:
    check("non-XML input raises ConvertError", True)

try:
    X.convert("<html><body>404</body></html>")
    check("non-<xmile> root raises ConvertError", False)
except X.ConvertError:
    check("non-<xmile> root raises ConvertError", True)

# The systemdynamics.org namespace (used by SDXorg models) must parse too.
X.reset_warnings()
alt_ns = X.convert('''<xmile xmlns="http://www.systemdynamics.org/XMILE">
  <sim_specs><start>0</start><stop>2</stop><dt>1</dt></sim_specs>
  <model><variables><aux name="a"><eqn>42</eqn></aux></variables></model></xmile>''')
check("alternate (systemdynamics.org) namespace is stripped and parsed",
      len(alt_ns["elements"]) == 1
      and alt_ns["elements"][0]["value"]["value"] == 42.0)

# ── schema validation (requires jsonschema; skip cleanly if absent) ───────────
try:
    import jsonschema
    schema = json.load(open(os.path.join(_repo, "schema", "wasim-schema-v2.json")))
    jsonschema.Draft7Validator(schema).validate(teacup)
    check("teacup validates against wasim-schema-v2.json (0.9.8)", True)
    jsonschema.Draft7Validator(schema).validate(p2)
    check("phase2 model validates against wasim-schema-v2.json (0.9.8)", True)
    with open(os.path.join(_here, "fixtures", "phase3_builtins.xmile"), encoding="utf-8") as f:
        p3 = X.convert(f.read())
    jsonschema.Draft7Validator(schema).validate(p3)
    check("phase3 model validates against wasim-schema-v2.json (0.9.8)", True)
    jsonschema.Draft7Validator(schema).validate(p4a)
    check("phase4 arrays model validates against wasim-schema-v2.json (0.9.8)", True)
    jsonschema.Draft7Validator(schema).validate(p4m)
    check("phase4 modules model validates against wasim-schema-v2.json (0.9.8)", True)
except ImportError:
    print("skip  schema validation (jsonschema not installed)")
except Exception as e:  # noqa: BLE001
    check(f"teacup validates against wasim-schema-v2.json: {e}", False)


# ── OASIS canonical corpus: every example converts to schema-valid v2 with no
#    dangling references (a dangling ref is a converter name-resolution bug, not a
#    model gap). See fixtures/xmile_oasis/SOURCE.md. ────────────────────────────
import glob
_oasis = sorted(glob.glob(os.path.join(_here, "fixtures", "xmile_oasis", "*.xml")))
check("OASIS corpus present", len(_oasis) >= 10)
try:
    import jsonschema
    _schema = json.load(open(os.path.join(_repo, "schema", "wasim-schema-v2.json")))
    _validator = jsonschema.Draft7Validator(_schema)
except ImportError:
    _validator = None

for path in _oasis:
    name = os.path.basename(path)
    X.reset_warnings()
    try:
        model = X.convert(open(path, encoding="utf-8").read())
    except Exception as e:  # noqa: BLE001
        check(f"OASIS {name}: converts", False)
        continue
    dangling = [w for w in X.WARNINGS if "XMILE-DANGLING-REF" in w]
    check(f"OASIS {name}: no dangling references", not dangling)
    polluted = [e["id"] for e in model["elements"] if "\\n" in e["id"] or "\n" in e["id"]]
    check(f"OASIS {name}: element ids are clean", not polluted)
    if _validator is not None:
        errs = list(_validator.iter_errors(model))
        check(f"OASIS {name}: schema-valid", not errs)


if _fails:
    print(f"\n{len(_fails)} FAILED: {_fails}")
    sys.exit(1)
print("\nall passed")
