#!/usr/bin/env python3
"""
xmile_to_wasim.py — convert an XMILE system-dynamics model into a WaSiM v2 model.json.

Usage:
    python3 tools/xmile_to_wasim.py input.xmile [-o output.json] [--name "Model name"]
    python3 tools/xmile_to_wasim.py input.xmile --stdout

XMILE is the OASIS v1.0 system-dynamics interchange standard (stocks, flows,
auxiliaries, graphical functions, modules). This converter targets the WaSiM
**v2** model format (`$id https://wasim.dev/schema/model/0.9.8`, `primitive`-based)
that the Rust engine parses. It is the first converter to emit v2.

Mapping. A `<stock>` is a WaSiM `stock` primitive (its `<eqn>` is the INITIAL
value, never a rate); a `<flow>` is a rate `node` (`value_rule:"expression"`); an
`<aux>` is a derived node (`fixed` for a constant eqn, `expression` otherwise); a
graphical function `<gf>` is a `lookup` node invoked via the `lookup_call` AST op.
The `<eqn>` infix language is parsed to WaSiM's op-discriminated AST. See
XMILE_MAPPING_SCOUT.md for the full construct-by-construct mapping.

Scope. The clean stock/flow/aux/gf/Euler core plus converter-side lowering of the
common SD builtins. XMILE features WaSiM cannot yet express — RK2/RK4 integration,
conveyor/queue stocks, >=3-D arrays, complex macros — degrade gracefully: the
model still loads, but the construct becomes an inert stub or an `extern_call`
(which the engine evaluates to 0.0 while preserving name + args), and a warning is
emitted to stderr. Never abort, never silent wrong numbers. Numeric fidelity is
"same trajectory under Euler," not bit-identical to Stella/Vensim.

No third-party dependencies (stdlib only). `jsonschema` is used only by the tests.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from typing import Any, Optional


WASIM_VERSION = "0.9.8"
XMILE_NS = "{http://docs.oasis-open.org/xmile/ns/XMILE/v1.0}"

# Warnings are collected globally and printed once at the end (mirrors
# ana_to_wasim.py). Each entry is `[<node>] <message>`; duplicates collapse.
WARNINGS: list[str] = []


def warn(node: str, msg: str) -> None:
    entry = f"[{node}] {msg}"
    if entry not in WARNINGS:
        WARNINGS.append(entry)


def reset_warnings() -> None:
    """Clear the module-level warning list (used by tests between conversions)."""
    WARNINGS.clear()


# --------------------------------------------------------------------------- #
# Stage 1 — XML parse: <sim_specs> and the <model>/<variables> block
# --------------------------------------------------------------------------- #

def _tag(elem: ET.Element) -> str:
    """Local tag name, namespace-stripped. XMILE files in the wild carry different
    namespace URIs (the OASIS `docs.oasis-open.org/...` one, the older
    `systemdynamics.org/XMILE` one, or none), so strip ANY `{ns}` prefix rather
    than a single hardcoded URI."""
    t = elem.tag
    if t.startswith("{"):
        return t[t.index("}") + 1:]
    return t


def _find(elem: ET.Element, name: str) -> Optional[ET.Element]:
    """Find a direct child by local name, namespace-agnostic."""
    for child in elem:
        if _tag(child) == name:
            return child
    return None


def _findall(elem: ET.Element, name: str) -> list[ET.Element]:
    return [c for c in elem if _tag(c) == name]


def _text(elem: Optional[ET.Element]) -> str:
    return (elem.text or "").strip() if elem is not None else ""


@dataclass
class IRVar:
    """One XMILE variable (stock / flow / aux / gf) before lowering."""
    kind: str                       # "stock" | "flow" | "aux" | "gf"
    raw_name: str                   # XMILE name attribute (may contain spaces)
    scope: tuple[str, ...]          # module path, e.g. ("Main",)
    eqn: Optional[str] = None
    inflows: list[str] = field(default_factory=list)   # raw (possibly quoted) names
    outflows: list[str] = field(default_factory=list)
    gf: Optional["IRGf"] = None     # embedded or standalone graphical function
    non_negative: bool = False
    units: Optional[str] = None
    doc: Optional[str] = None
    dims: list[str] = field(default_factory=list)      # dimension names (arrayed var)
    # filled in by build_scope_table:
    qual_id: str = ""
    name: str = ""


@dataclass
class IRGf:
    x: list[float]
    y: list[float]
    interpolation: str              # "linear" | "step"
    extrapolate: bool = False


@dataclass
class IRDim:
    """A top-level <dimensions>/<dim> declaration."""
    name: str
    size: int
    labels: list[str] = field(default_factory=list)


@dataclass
class IRModule:
    """An XMILE <module> instance: a reference to another <model> + <connect> edges."""
    raw_name: str
    scope: tuple[str, ...]
    ref_model: Optional[str]                     # the referenced <model> name
    connects: list[tuple[str, str]] = field(default_factory=list)  # (to, from) raw
    qual_id: str = ""
    name: str = ""


@dataclass
class IRSimSpecs:
    start: float = 0.0
    stop: float = 1.0
    dt: float = 1.0
    method: Optional[str] = None
    time_units: Optional[str] = None


@dataclass
class IRModel:
    variables: list[IRVar]
    sim: IRSimSpecs
    name: str
    dims: list[IRDim] = field(default_factory=list)
    modules: list[IRModule] = field(default_factory=list)
    # {model_name -> its variables} for resolving <module> references (Phase 4).
    submodels: dict = field(default_factory=dict)


def _parse_gf(gf_elem: ET.Element) -> Optional[IRGf]:
    """Parse a <gf> into (x[], y[], interpolation). Handles <xpts>/<ypts> and
    the evenly-spaced <xscale min max>/<ypts> form."""
    ypts_el = _find(gf_elem, "ypts")
    if ypts_el is None:
        return None
    y = _parse_num_list(_text(ypts_el))
    xpts_el = _find(gf_elem, "xpts")
    if xpts_el is not None:
        x = _parse_num_list(_text(xpts_el))
    else:
        # xscale min/max evenly spaced across len(y) knots.
        xscale = _find(gf_elem, "xscale")
        if xscale is None or not y:
            return None
        xmin = float(xscale.get("min", "0"))
        xmax = float(xscale.get("max", "1"))
        n = len(y)
        x = [xmin + (xmax - xmin) * i / (n - 1) for i in range(n)] if n > 1 else [xmin]
    gtype = (gf_elem.get("type") or "continuous").lower()
    interp = "step" if gtype == "discrete" else "linear"
    extrapolate = gtype == "extrapolate"
    return IRGf(x=x, y=y, interpolation=interp, extrapolate=extrapolate)


def _parse_num_list(text: str) -> list[float]:
    """Parse a comma- (or whitespace-) separated numeric list."""
    parts = re.split(r"[,\s]+", text.strip())
    out: list[float] = []
    for p in parts:
        if not p:
            continue
        try:
            out.append(float(p))
        except ValueError:
            pass
    return out


def _var_dims(child: ET.Element) -> list[str]:
    """Dimension names on a variable's <dimensions><dim name=.../></dimensions>."""
    dims_el = _find(child, "dimensions")
    if dims_el is None:
        return []
    return [d.get("name", "") for d in _findall(dims_el, "dim") if d.get("name")]


def _parse_variables(vars_elem: ET.Element, scope: tuple[str, ...],
                     out: list[IRVar], modules: list[IRModule]) -> None:
    for child in vars_elem:
        kind = _tag(child)
        if kind == "module":
            name = child.get("name", "")
            connects = [(_norm_ref(c.get("to", "")), _norm_ref(c.get("from", "")))
                        for c in _findall(child, "connect")]
            # A module references a model of the same name unless it says otherwise.
            ref = child.get("model_name") or name
            modules.append(IRModule(raw_name=name, scope=scope, ref_model=ref,
                                    connects=connects))
            continue
        if kind not in ("stock", "flow", "aux", "gf"):
            continue
        name = child.get("name", "")
        if kind == "gf":
            gf = _parse_gf(child)
            v = IRVar(kind="gf", raw_name=name, scope=scope, gf=gf,
                      doc=_text(_find(child, "doc")) or None)
            out.append(v)
            continue
        v = IRVar(
            kind=kind,
            raw_name=name,
            scope=scope,
            eqn=_text(_find(child, "eqn")) or None,
            inflows=[_text(e) for e in _findall(child, "inflow")],
            outflows=[_text(e) for e in _findall(child, "outflow")],
            non_negative=_find(child, "non_negative") is not None,
            units=_text(_find(child, "units")) or None,
            doc=_text(_find(child, "doc")) or None,
            dims=_var_dims(child),
        )
        gf_elem = _find(child, "gf")
        if gf_elem is not None:
            v.gf = _parse_gf(gf_elem)   # embedded gf on a flow/aux
        if v.dims and _findall(child, "element"):
            warn(name, "XMILE-ARRAY-ELEMENTWISE: element-specific array equations "
                       "(<element subscript=...>) are not supported; only the "
                       "apply-to-all <eqn> is used (per-element values dropped).")
        # (≥3-D apply-to-all arrays are supported as of schema 0.9.9 — the converter
        #  emits one nested vector_map per dimension and the engine builds an N-D array.)
        out.append(v)


def _norm_ref(name: str) -> str:
    """Strip a module-qualified connect endpoint 'sub.x' to its trailing name, and
    unquote. XMILE <connect> uses dotted paths; we keep the last segment."""
    n = name.strip().strip('"')
    return n


def _parse_dimensions(root: ET.Element) -> list[IRDim]:
    dims_el = _find(root, "dimensions")
    if dims_el is None:
        return []
    out: list[IRDim] = []
    for d in _findall(dims_el, "dim"):
        name = d.get("name", "")
        if not name:
            continue
        elems = _findall(d, "elem")
        if elems:
            labels = [e.get("name", "") for e in elems]
            out.append(IRDim(name=name, size=len(labels), labels=labels))
        else:
            size = int(d.get("size", "0") or "0")
            if size >= 1:
                out.append(IRDim(name=name, size=size))
            else:
                warn(name, f"XMILE-ARRAY: dimension '{name}' has no size or members; skipped.")
    return out


def parse_xmile(root: ET.Element) -> IRModel:
    # Header name.
    header = _find(root, "header")
    model_name = _text(_find(header, "name")) if header is not None else ""

    # sim_specs.
    sim = IRSimSpecs()
    ss = _find(root, "sim_specs")
    if ss is not None:
        sim.method = (ss.get("method") or "").lower() or None
        sim.time_units = ss.get("time_units")
        start_el, stop_el, dt_el = _find(ss, "start"), _find(ss, "stop"), _find(ss, "dt")
        if start_el is not None:
            sim.start = _num_or(_text(start_el), 0.0)
        if stop_el is not None:
            sim.stop = _num_or(_text(stop_el), 1.0)
        if dt_el is not None:
            sim.dt = _num_or(_text(dt_el), 1.0)
    else:
        warn("sim_specs", "no <sim_specs>; defaulting duration=1, timestep=1.")

    dims = _parse_dimensions(root)

    # The first <model> is the root (scope "Main"); subsequent NAMED models are
    # submodel templates referenced by <module>. Root variables + module instances
    # live at top level; each referenced template's variables are emitted under the
    # module's scope.
    variables: list[IRVar] = []
    modules: list[IRModule] = []
    submodels: dict[str, ET.Element] = {}   # model-name -> <model> element
    models = _findall(root, "model")
    for idx, m in enumerate(models):
        mname = m.get("name")
        if idx == 0 or not mname:
            root_scope = (mname or "Main",)
            vars_elem = _find(m, "variables")
            if vars_elem is not None:
                _parse_variables(vars_elem, root_scope, variables, modules)
        else:
            submodels[mname] = m

    return IRModel(variables=variables, sim=sim, name=model_name or "XMILE model",
                   dims=dims, modules=modules, submodels=submodels)


def _num_or(text: str, default: float) -> float:
    try:
        return float(text)
    except (ValueError, TypeError):
        return default


# --------------------------------------------------------------------------- #
# Stage 2 — scope & id qualification
# --------------------------------------------------------------------------- #

def _norm_name(name: str) -> str:
    """Normalize an XMILE name/reference for matching. Per the XMILE spec (§3.1.1),
    identifiers are matched case-insensitively with **spaces and underscores
    equivalent**, and display names may carry embedded newlines (`\\n`) for
    multi-line rendering that references write as spaces/underscores. So we treat any
    run of whitespace (incl. newlines) or underscores as a single canonical
    separator and casefold. E.g. `Sales\\nForce`, `Sales Force`, and `Sales_Force`
    all key the same."""
    n = name.strip()
    if len(n) >= 2 and n[0] == '"' and n[-1] == '"':
        n = n[1:-1]
    # XMILE display names embed line breaks as the literal two-character escape `\n`
    # (backslash + n), which references write as a space/underscore. Fold the literal
    # `\n` / `\r` escapes first, then any run of real whitespace or underscores → one
    # canonical separator. E.g. `indicated\nsales_force` == `indicated_sales_force`.
    n = re.sub(r"\\[nrt]", "_", n)
    n = re.sub(r"[\s_]+", "_", n.strip())
    return n.casefold()


def _reserved_ident(name: str) -> Optional[dict]:
    """XMILE reserved identifiers referenceable without parentheses. Returns the
    lowered AST node, or None if `name` is not reserved."""
    key = re.sub(r"[\s_]+", "", name.strip()).casefold()
    if key == "time":
        return {"op": "time_ref", "property": "elapsed"}
    if key in ("dt", "timestep"):
        return {"op": "time_ref", "property": "timestep"}
    if key in ("starttime", "initialtime"):
        return {"op": "time_ref", "property": "start"}
    if key == "pi":
        return {"op": "literal", "value": 3.141592653589793}
    return None


def _id_segment(name: str) -> str:
    """A clean id segment from an XMILE name: fold the literal `\\n`/`\\r`/`\\t`
    escapes and real whitespace to single spaces (so ids don't carry raw line-break
    escapes), keeping the readable words. `Sales\\nForce` → `Sales Force`."""
    n = re.sub(r"\\[nrt]", " ", name.strip())
    n = re.sub(r"\s+", " ", n).strip()
    return n


def build_scope_table(model: IRModel) -> dict[tuple[str, ...], dict[str, str]]:
    """Assign each var a globally-unique qualified id ("<scope>/<name>") and a
    display name; build a per-scope {normalized name -> qual_id} resolver map."""
    resolver: dict[tuple[str, ...], dict[str, str]] = {}
    seen_ids: set[str] = set()
    for v in model.variables:
        display = _id_segment(v.raw_name)
        qual = "/".join(v.scope + (display,))
        if qual in seen_ids:
            i = 2
            while f"{qual}#{i}" in seen_ids:
                i += 1
            warn(v.raw_name, f"duplicate id '{qual}' — disambiguated as '{qual}#{i}'.")
            qual = f"{qual}#{i}"
        seen_ids.add(qual)
        v.qual_id = qual
        v.name = display
        resolver.setdefault(v.scope, {})[_norm_name(display)] = qual
    return resolver


def resolve_ref(raw: str, scope: tuple[str, ...],
                resolver: dict[tuple[str, ...], dict[str, str]]) -> Optional[str]:
    """Resolve a raw (possibly quoted, possibly dotted) XMILE name to a qualified id.
    A bare name searches the current scope then outward. A dotted module path
    (`sub.interior`) descends: each leading segment is a module name appended to the
    scope, and the final segment is resolved within that child scope."""
    raw = raw.strip().strip('"')
    if "." in raw:
        *mods, leaf = raw.split(".")
        child = scope + tuple(m.strip() for m in mods)
        got = resolver.get(child, {}).get(_norm_name(leaf))
        if got is not None:
            return got
        # fall through to a plain lookup if the dotted path didn't resolve
    key = _norm_name(raw)
    s = scope
    while True:
        got = resolver.get(s, {}).get(key)
        if got is not None:
            return got
        if not s:
            return None
        s = s[:-1]


# --------------------------------------------------------------------------- #
# Stage 3 — <eqn> tokenizer + Pratt parser  →  WaSiM AST (with Call sentinels)
# --------------------------------------------------------------------------- #

class Tok:
    NUM = "num"
    IDENT = "ident"     # bare identifier
    QIDENT = "qident"   # "quoted identifier" — a name with spaces
    OP = "op"
    LP = "("
    RP = ")"
    LB = "["
    RB = "]"
    COMMA = ","
    EOF = "eof"


_TOKEN_RE = re.compile(
    r"""
      (?P<ws>\s+)
    | (?P<qident>"[^"]*")
    | (?P<num>\d+\.\d+(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|\d+(?:[eE][+-]?\d+)?)
    | (?P<ident>[A-Za-z_][A-Za-z0-9_$]*(?:\.[A-Za-z_][A-Za-z0-9_$]*)*)
    | (?P<op><=|>=|<>|[-+*/^<>=])
    | (?P<lp>\()
    | (?P<rp>\))
    | (?P<lb>\[)
    | (?P<rb>\])
    | (?P<comma>,)
    | (?P<other>.)
    """,
    re.VERBOSE,
)


def strip_brace_comments(text: str) -> str:
    """Remove XMILE `{ ... }` comments (non-nesting)."""
    out, depth = [], 0
    for ch in text:
        if ch == "{":
            depth += 1
        elif ch == "}":
            if depth > 0:
                depth -= 1
        elif depth == 0:
            out.append(ch)
    return "".join(out)


def tokenize_eqn(s: str) -> list[tuple[str, Any]]:
    s = strip_brace_comments(s)
    toks: list[tuple[str, Any]] = []
    for m in _TOKEN_RE.finditer(s):
        kind = m.lastgroup
        val = m.group()
        if kind == "ws":
            continue
        if kind == "qident":
            toks.append((Tok.QIDENT, val[1:-1]))
        elif kind == "num":
            toks.append((Tok.NUM, float(val)))
        elif kind == "ident":
            toks.append((Tok.IDENT, val))
        elif kind == "op":
            toks.append((Tok.OP, val))
        elif kind == "lp":
            toks.append((Tok.LP, val))
        elif kind == "rp":
            toks.append((Tok.RP, val))
        elif kind == "lb":
            toks.append((Tok.LB, val))
        elif kind == "rb":
            toks.append((Tok.RB, val))
        elif kind == "comma":
            toks.append((Tok.COMMA, val))
        else:
            toks.append((Tok.OP, val))  # unknown char → let parser reject
    toks.append((Tok.EOF, None))
    return toks


class ParseError(Exception):
    pass


@dataclass
class Call:
    """Deferred function call — resolved to an AST after parsing, so the lowering
    stage can inspect name + arity before committing (mirrors ana_to_wasim.py)."""
    name: str
    args: list[Any]


@dataclass
class IdentRef:
    """An unresolved identifier (bare or quoted). Rewritten to a `ref` AST once the
    scope resolver runs; kept distinct so resolution can qualify the id."""
    name: str


# (op-name -> (ast_op, lbp, rbp)); higher binding power = tighter.
_BINOPS = {
    "or":  ("or", 1, 2),
    "and": ("and", 3, 4),
    "=":   ("eq", 5, 6),
    "<>":  ("neq", 5, 6),
    "<":   ("lt", 5, 6),
    "<=":  ("lte", 5, 6),
    ">":   ("gt", 5, 6),
    ">=":  ("gte", 5, 6),
    "+":   ("add", 7, 8),
    "-":   ("subtract", 7, 8),
    "*":   ("multiply", 9, 10),
    "/":   ("divide", 9, 10),
    "mod": ("__mod__", 9, 10),  # XMILE `a MOD b` → call mod(a, b) (see parse_expr)
    "^":   ("power", 14, 13),   # right-associative
}
_UNARY_BP = 11   # unary -, +, NOT bind looser than ^ (so -2^2 = -(2^2) = -4)


class EqnParser:
    """Pratt parser producing WaSiM AST dicts, `Call` sentinels, or `IdentRef`."""

    def __init__(self, toks: list[tuple[str, Any]]):
        self.toks = toks
        self.i = 0

    def peek(self) -> tuple[str, Any]:
        return self.toks[self.i]

    def next(self) -> tuple[str, Any]:
        t = self.toks[self.i]
        self.i += 1
        return t

    def expect(self, kind: str) -> Any:
        t = self.next()
        if t[0] != kind:
            raise ParseError(f"expected {kind}, got {t}")
        return t[1]

    def parse(self) -> Any:
        node = self.parse_expr(0)
        if self.peek()[0] != Tok.EOF:
            raise ParseError(f"trailing tokens at {self.peek()}")
        return node

    def parse_expr(self, min_bp: int) -> Any:
        left = self.parse_postfix(self.parse_prefix())
        while True:
            kind, val = self.peek()
            opname = None
            if kind == Tok.OP:
                opname = val
            elif kind == Tok.IDENT and val.lower() in ("and", "or", "mod"):
                opname = val.lower()
            if opname is None or opname not in _BINOPS:
                break
            ast_op, lbp, rbp = _BINOPS[opname]
            if lbp < min_bp:
                break
            self.next()
            right = _as_ast(self.parse_expr(rbp))
            left_ast = _as_ast(left)
            if ast_op == "__mod__":   # a MOD b → call mod(a, b)
                left = {"op": "call", "fn": "mod", "args": [left_ast, right]}
            else:
                left = {"op": ast_op, "left": left_ast, "right": right}
        return left

    def parse_prefix(self) -> Any:
        kind, val = self.peek()
        if kind == Tok.OP and val == "-":
            self.next()
            return {"op": "neg", "operand": _as_ast(self.parse_expr(_UNARY_BP))}
        if kind == Tok.OP and val == "+":
            self.next()
            return self.parse_expr(_UNARY_BP)
        if kind == Tok.IDENT and val.lower() == "not":
            self.next()
            return {"op": "not", "operand": _as_ast(self.parse_expr(_UNARY_BP))}
        if kind == Tok.IDENT and val.lower() == "if":
            return self.parse_if()
        return self.parse_atom()

    def parse_if(self) -> Any:
        self.next()  # consume IF
        cond = _as_ast(self.parse_expr(0))
        kind, val = self.next()
        if kind != Tok.IDENT or val.lower() != "then":
            raise ParseError(f"expected THEN, got ({kind},{val})")
        then = _as_ast(self.parse_expr(0))
        kind, val = self.next()
        if kind != Tok.IDENT or val.lower() != "else":
            raise ParseError(f"expected ELSE, got ({kind},{val})")
        els = _as_ast(self.parse_expr(0))
        return {"op": "if", "cond": cond, "then": then, "else": els}

    def parse_atom(self) -> Any:
        kind, val = self.next()
        if kind == Tok.NUM:
            return {"op": "literal", "value": val}
        if kind == Tok.QIDENT:
            return IdentRef(val)
        if kind == Tok.IDENT:
            if self.peek()[0] == Tok.LP:
                return self.parse_call(val)
            return IdentRef(val)
        if kind == Tok.LP:
            inner = self.parse_expr(0)
            self.expect(Tok.RP)
            return inner
        raise ParseError(f"unexpected token ({kind}, {val})")

    def parse_call(self, name: str) -> Call:
        self.expect(Tok.LP)
        args: list[Any] = []
        if self.peek()[0] != Tok.RP:
            args.append(self.parse_expr(0))
            while self.peek()[0] == Tok.COMMA:
                self.next()
                args.append(self.parse_expr(0))
        self.expect(Tok.RP)
        return Call(name, args)

    def parse_postfix(self, node: Any) -> Any:
        while True:
            kind, _ = self.peek()
            if kind == Tok.LB:   # subscript v[i,j] — kept for Phase 4
                self.next()
                indices = [self.parse_expr(0)]
                while self.peek()[0] == Tok.COMMA:
                    self.next()
                    indices.append(self.parse_expr(0))
                self.expect(Tok.RB)
                node = {"op": "index", "array": _as_ast(node),
                        "indices": [_as_ast(x) for x in indices]}
            else:
                break
        return node


def _as_ast(node: Any) -> Any:
    """Coerce a parse result into an AST-ish node. `Call` and `IdentRef` sentinels
    are left in place — they're resolved by later passes (`lower_ast` resolves
    Calls with emission context; `resolve_refs` qualifies IdentRefs)."""
    return node


def parse_eqn(text: str) -> tuple[Optional[Any], Optional[str]]:
    """Parse an <eqn> to an AST tree that may still contain `Call`/`IdentRef`
    sentinels. Returns (ast, None) on success or (None, error) on failure — never
    raises. Callers run `lower_ast` (Call → WaSiM AST + extras) then `resolve_refs`
    (IdentRef → qualified `ref`)."""
    try:
        toks = tokenize_eqn(text)
        ast = EqnParser(toks).parse()
        return ast, None
    except (ParseError, ZeroDivisionError, ValueError) as e:
        return None, str(e)


# --------------------------------------------------------------------------- #
# Stage 5 — builtin lowering (Call → WaSiM AST, possibly emitting extra elements)
# --------------------------------------------------------------------------- #

# XMILE builtin (lowercased) -> WaSiM `call` fn. All are engine-implemented.
_DIRECT_FN = {
    "abs": "abs", "exp": "exp", "sqrt": "sqrt", "ln": "ln",
    "log10": "log", "sin": "sin", "cos": "cos", "tan": "tan",
    "arcsin": "asin", "arccos": "acos", "arctan": "atan",
    "min": "min", "max": "max", "int": "int", "sign": "sign",
    "abs": "abs",
}

# XMILE array reductions over an arrayed variable -> WaSiM array-reducer fn.
_ARRAY_REDUCER = {
    "sum": "sum_array", "mean": "mean_array", "size": "size_array",
}
# MIN/MAX are arity-sensitive: MIN(a,b) scalar vs MIN(arrayvar) reduction.
_ARRAY_REDUCER_1ARG = {"min": "min_array", "max": "max_array"}

# XMILE per-step random builtin -> (WaSiM distribution family, [param names]).
_RANDOM_FN = {
    "normal": ("normal", ["mean", "stddev"]),
    "random": ("uniform", ["min", "max"]),
    "uniform": ("uniform", ["min", "max"]),
    "poisson": ("poisson", ["lambda"]),
    "exprnd": ("exponential", ["mean"]),
    "lognormal": ("lognormal", ["mean", "stddev"]),
}


@dataclass
class LowerCtx:
    """Emission context for the lowering pass. `who` names the host variable (for
    warnings + synth ids); `dt` is the model timestep; `extras` collects sibling
    elements produced by element-generating builtins (PREVIOUS, DELAY1, SMTH1,
    random draws). `scope` is the host's module path (extras inherit it)."""
    who: str
    dt: float
    scope: tuple[str, ...]
    extras: list[dict] = field(default_factory=list)
    _n: int = 0

    def synth_id(self, suffix: str) -> str:
        self._n += 1
        return f"{self.who}__{suffix}{self._n}"

    def container(self) -> Optional[str]:
        return "/".join(self.scope) if self.scope else None


def _lit(v: float) -> dict:
    return {"op": "literal", "value": float(v)}


def _elapsed() -> dict:
    return {"op": "time_ref", "property": "elapsed"}


def _const_arg(node: Any) -> Optional[float]:
    """If a lowered arg is a bare literal, return its value (for τ→steps etc.)."""
    if isinstance(node, dict) and node.get("op") == "literal" and "value" in node:
        return float(node["value"])
    return None


def _safe_div(a: Any, b: Any, default: Any) -> dict:
    """Guarded division `b==0 ? default : a/b` — the shared body of ZIDZ/XIDZ/SAFEDIV.
    The divisor AST `b` is referenced twice (guard + quotient); harmless for the ref/const
    args these builtins take in practice."""
    return {"op": "if",
            "cond": {"op": "eq", "left": b, "right": _lit(0.0)},
            "then": default,
            "else": {"op": "divide", "left": a, "right": b}}


def lower_ast(node: Any, ctx: LowerCtx) -> Any:
    """Resolve `Call` sentinels to WaSiM AST, recursing into children first. May
    append sibling elements to `ctx.extras`. Non-Call nodes are walked so nested
    calls (and IdentRefs, left untouched) are handled."""
    if isinstance(node, Call):
        args = [lower_ast(a, ctx) for a in node.args]
        return lower_call(node.name, args, ctx)
    if isinstance(node, dict):
        return {k: lower_ast(v, ctx) for k, v in node.items()}
    if isinstance(node, list):
        return [lower_ast(x, ctx) for x in node]
    return node


def lower_call(name_raw: str, args: list[Any], ctx: LowerCtx) -> Any:
    """Lower one XMILE builtin (args already lowered) to a WaSiM AST node,
    optionally emitting sibling elements into ctx.extras."""
    name = name_raw.lower()

    # --- array reducers (XMILE SUM/MEAN/SIZE over an arrayed var) ---
    if name in _ARRAY_REDUCER and len(args) >= 1:
        return {"op": "call", "fn": _ARRAY_REDUCER[name], "args": [args[0]]}
    # MIN/MAX: 1 arg → array reduction; 2+ args → scalar min/max.
    if name in _ARRAY_REDUCER_1ARG and len(args) == 1:
        return {"op": "call", "fn": _ARRAY_REDUCER_1ARG[name], "args": args}

    # --- direct math ---
    if name in _DIRECT_FN:
        return {"op": "call", "fn": _DIRECT_FN[name], "args": args}
    if name == "pi":
        return _lit(3.141592653589793)
    if name == "log":
        # LOG10(x) already handled above; LOG(x) = log10; LOG(x, b) = ln x / ln b.
        if len(args) == 1:
            return {"op": "call", "fn": "log", "args": args}
        if len(args) == 2:
            return {"op": "divide",
                    "left": {"op": "call", "fn": "ln", "args": [args[0]]},
                    "right": {"op": "call", "fn": "ln", "args": [args[1]]}}

    # --- time ---
    if name == "time":
        return _elapsed()
    if name == "dt":
        return {"op": "time_ref", "property": "timestep"}

    # --- composed step functions ---
    if name == "step" and len(args) == 2:      # STEP(height, t0)
        height, t0 = args
        return {"op": "if",
                "cond": {"op": "gte", "left": _elapsed(), "right": t0},
                "then": height, "else": _lit(0.0)}
    if name == "ramp" and len(args) >= 2:       # RAMP(slope, t0)
        slope, t0 = args[0], args[1]
        return {"op": "multiply", "left": slope,
                "right": {"op": "call", "fn": "max",
                          "args": [_lit(0.0),
                                   {"op": "subtract", "left": _elapsed(), "right": t0}]}}
    if name == "pulse" and len(args) >= 2:      # PULSE(magnitude, first) — one-step spike
        mag, first = args[0], args[1]
        area = {"op": "divide", "left": mag, "right": _lit(ctx.dt)}
        return {"op": "if",
                "cond": {"op": "and",
                         "left": {"op": "gte", "left": _elapsed(), "right": first},
                         "right": {"op": "lt", "left": _elapsed(),
                                   "right": {"op": "add", "left": first, "right": _lit(ctx.dt)}}},
                "then": area, "else": _lit(0.0)}

    # --- rounding ---
    if name == "integer" and len(args) == 1:    # Vensim INTEGER(x) → truncate toward zero
        return {"op": "call", "fn": "int", "args": args}
    if name in ("modulo", "mod") and len(args) == 2:  # MODULO(a, b)
        return {"op": "call", "fn": "mod", "args": args}

    # --- safe divide (Vensim ZIDZ/XIDZ, sometimes surfaced as SAFEDIV) ---
    # ZIDZ(a,b)   = b==0 ? 0 : a/b
    # XIDZ(a,b,x) = b==0 ? x : a/b   (SAFEDIV(a,b[,x]) — x defaults to 0)
    if name == "zidz" and len(args) == 2:
        return _safe_div(args[0], args[1], _lit(0.0))
    if name in ("xidz", "safediv") and len(args) >= 2:
        default = args[2] if len(args) >= 3 else _lit(0.0)
        return _safe_div(args[0], args[1], default)

    # --- graphical-function call: LOOKUP(table, x) → lookup_call on the table node ---
    if name in ("lookup", "lookup_extrapolate", "with_lookup") and len(args) == 2:
        tbl = args[0]
        if isinstance(tbl, dict) and tbl.get("op") == "ref":
            return {"op": "lookup_call", "element_id": tbl["element_id"], "input": args[1]}
        warn(ctx.who, "XMILE-UNMAPPED-BUILTIN: LOOKUP's first argument is not a table "
                      "reference; extern_call fallback.")
        return {"op": "extern_call", "fn": name_raw, "args": args}

    # --- element-producing builtins ---
    if name == "previous" and len(args) >= 1:   # PREVIOUS(x[, init]) → lag node
        return _emit_lag(args, ctx)
    if name in ("smth1", "smooth", "smooth1") and len(args) >= 2:  # SMTH1(in, τ[, init])
        return _emit_smooth(args, ctx)
    if name in _RANDOM_FN:                       # per-step random draw → sample node
        return _emit_random(name, args, ctx)

    # --- not yet mapped: preserve as extern_call (engine → 0.0 + warn) ---
    warn(ctx.who, f"XMILE-UNMAPPED-BUILTIN: '{name_raw}' has no mapping; emitted as "
                  f"extern_call (engine evaluates 0.0, args preserved).")
    return {"op": "extern_call", "fn": name_raw, "args": args}


def _emit_lag(args: list[Any], ctx: LowerCtx) -> dict:
    """PREVIOUS(x, init) → a `lag` node (one-step delay); host refs it."""
    inp = args[0]
    if not (isinstance(inp, dict) and inp.get("op") == "ref"):
        warn(ctx.who, "XMILE-UNMAPPED-BUILTIN: PREVIOUS of a non-reference argument "
                      "is unsupported; extern_call fallback.")
        return {"op": "extern_call", "fn": "previous", "args": args}
    init = _const_arg(args[1]) if len(args) > 1 else None
    node = {"id": ctx.synth_id("prev"), "name": "previous", "primitive": "node",
            "value_rule": "lag", "input": inp["element_id"]}
    if ctx.container():
        node["container"] = ctx.container()
    if init is not None:
        node["initial"] = _q(init)
    ctx.extras.append(node)
    return {"op": "ref", "element_id": node["id"]}


def _emit_smooth(args: list[Any], ctx: LowerCtx) -> dict:
    """SMTH1(input, averaging_time[, init]) → a `filter` node (EMA). window is an
    integer step count, so averaging_time τ (in time units) → max(1, round(τ/dt))."""
    inp = args[0]
    if not (isinstance(inp, dict) and inp.get("op") == "ref"):
        warn(ctx.who, "XMILE-UNMAPPED-BUILTIN: SMTH1 of a non-reference argument is "
                      "unsupported; extern_call fallback.")
        return {"op": "extern_call", "fn": "smth1", "args": args}
    tau = _const_arg(args[1])
    if tau is None:
        warn(ctx.who, "XMILE-DELAY-CASCADE: SMTH1 averaging time is not a constant; "
                      "cannot fix an integer EMA window; extern_call fallback.")
        return {"op": "extern_call", "fn": "smth1", "args": args}
    window = max(1, round(tau / ctx.dt)) if ctx.dt > 0 else 1
    node = {"id": ctx.synth_id("smth"), "name": "smooth", "primitive": "node",
            "value_rule": "filter", "input": inp["element_id"],
            "window": window, "statistic": "ema"}
    if ctx.container():
        node["container"] = ctx.container()
    ctx.extras.append(node)
    return {"op": "ref", "element_id": node["id"]}


def _emit_random(name: str, args: list[Any], ctx: LowerCtx) -> dict:
    """Per-step random builtin → a `sample` node that redraws every timestep."""
    family, pnames = _RANDOM_FN[name]
    vals = [_const_arg(a) for a in args[:len(pnames)]]
    if any(v is None for v in vals) or len(vals) < len(pnames):
        warn(ctx.who, f"XMILE-UNMAPPED-BUILTIN: {name}() needs constant parameters to "
                      f"map to a sample node; extern_call fallback.")
        return {"op": "extern_call", "fn": name, "args": args}
    params = {pn: _q(v) for pn, v in zip(pnames, vals)}
    node = {"id": ctx.synth_id("rand"), "name": name, "primitive": "node",
            "value_rule": "sample",
            "distribution": {"family": family, "parameters": params},
            "resampling": {"mode": "always"}}
    if ctx.container():
        node["container"] = ctx.container()
    warn(ctx.who, "XMILE-RANDOM-SEED-GLOBAL: per-step random mapped to a resampling "
                  "sample node; any XMILE per-call seed becomes the run seed.")
    ctx.extras.append(node)
    return {"op": "ref", "element_id": node["id"]}


# --------------------------------------------------------------------------- #
# Stage 4/6 — resolve IdentRefs and emit elements
# --------------------------------------------------------------------------- #

def resolve_refs(ast: Any, scope: tuple[str, ...],
                 resolver: dict[tuple[str, ...], dict[str, str]],
                 who: str,
                 interior: Optional[dict[str, str]] = None) -> Any:
    """Walk an AST, rewriting IdentRef placeholders to `ref` nodes with qualified
    ids. Unresolved names become an inert literal 0.0 + a warning.

    `interior` maps a submodel-interior element id -> its submodel container id. A
    reference from OUTSIDE that submodel to one of its interior outputs is rewritten
    to `submodel_stat{mean, submodel_id, output}` — the WaSiM way a parent reads a
    submodel output. With the submodel at n_realizations:1, mean() is the exact
    value, so this reproduces XMILE's inline deterministic module read."""
    interior = interior or {}
    if isinstance(ast, IdentRef):
        qid = resolve_ref(ast.name, scope, resolver)
        if qid is None:
            # XMILE reserved identifiers usable without parens (TIME, DT, PI, …).
            reserved = _reserved_ident(ast.name)
            if reserved is not None:
                return reserved
            warn(who, f"XMILE-DANGLING-REF: '{ast.name}' does not resolve to a "
                      f"variable; replaced with 0.")
            return {"op": "literal", "value": 0.0}
        sub_id = interior.get(qid)
        if sub_id is not None and not _within(scope, sub_id):
            # qid is interior to submodel `sub_id` and the referrer is outside it →
            # read it as a submodel statistic rather than a (non-resolving) plain ref.
            return {"op": "submodel_stat", "submodel_id": sub_id,
                    "output": qid, "statistic": "mean"}
        return {"op": "ref", "element_id": qid}
    if isinstance(ast, Call):
        # Resolve refs inside call args, keeping the Call sentinel for lower_ast.
        return Call(ast.name, [resolve_refs(a, scope, resolver, who, interior)
                               for a in ast.args])
    if isinstance(ast, dict):
        return {k: resolve_refs(v, scope, resolver, who, interior) for k, v in ast.items()}
    if isinstance(ast, list):
        return [resolve_refs(x, scope, resolver, who, interior) for x in ast]
    return ast


def _within(scope: tuple[str, ...], container_id: str) -> bool:
    """True if an element at `scope` is inside (or is) the container `container_id`
    (container ids are '/'-joined scope paths)."""
    return "/".join(scope) == container_id or "/".join(scope).startswith(container_id + "/")


def _collect_refs(ast: Any, out: set[str]) -> None:
    if isinstance(ast, dict):
        if ast.get("op") == "ref" and "element_id" in ast:
            out.add(ast["element_id"])
        for v in ast.values():
            _collect_refs(v, out)
    elif isinstance(ast, list):
        for x in ast:
            _collect_refs(x, out)


def _q(value: float, unit: str = "1") -> dict:
    return {"value": float(value), "unit": unit}


def _const_value(ast: Any) -> Optional[float]:
    """If an AST is a bare literal, return its value; else None (for aux fixed vs
    expression, and stock initial_value)."""
    if isinstance(ast, dict) and ast.get("op") == "literal" and "value" in ast:
        return float(ast["value"])
    return None


def emit_element(v: IRVar, scope_resolver, sim: IRSimSpecs,
                 interior: Optional[dict[str, str]] = None,
                 dimensioned: Optional[set[str]] = None) -> tuple[dict, list[dict]]:
    """Emit the WaSiM element(s) for one IRVar. Returns (primary, extras).
    `interior` (interior-id -> submodel-id) lets cross-submodel reads become
    `submodel_stat` (see resolve_refs). `dimensioned` is the set of qualified ids
    that are themselves arrays — used to decide whether an apply-to-all array eqn
    needs a `vector_map` wrap (constant/scalar body) or should broadcast (body
    already references a dimensioned array)."""
    dimensioned = dimensioned or set()
    unit = v.units or "1"
    extras: list[dict] = []

    # Parse the equation once (stocks/flows/auxes all have one). Pipeline:
    #   parse → resolve_refs (IdentRef → qualified ref) → lower_ast (Call → AST,
    #   possibly emitting sibling elements into `extras`). Refs are resolved before
    #   lowering so element-producing builtins (PREVIOUS/SMTH1) see resolved inputs.
    ast: Any = None
    if v.eqn is not None:
        ast, err = parse_eqn(v.eqn)
        if err is not None:
            warn(v.qual_id, f"XMILE-EQN-PARSE: could not parse eqn '{v.eqn}': {err}; "
                            f"replaced with 0.")
            ast = {"op": "literal", "value": 0.0, "_stub_display": v.eqn}
        else:
            ast = resolve_refs(ast, v.scope, scope_resolver, v.qual_id, interior)
            ctx = LowerCtx(who=v.qual_id, dt=sim.dt, scope=v.scope)
            ast = lower_ast(ast, ctx)
            extras.extend(ctx.extras)

    base = {"id": v.qual_id, "name": v.name, "primitive": ""}
    if len(v.scope) > 0:
        base["container"] = "/".join(v.scope)
    if v.doc:
        base["description"] = v.doc

    if v.kind == "stock":
        base["primitive"] = "stock"
        init = _const_value(ast) if ast is not None else None
        if init is not None:
            base["initial_value"] = _q(init, unit)          # constant initial
        elif ast is not None:
            # Non-constant initial: WaSiM 0.9.9 accepts an expression here (evaluated at t=0,
            # ordered among other stock/aux initials). Emit the same expression_field shape as
            # aux expressions.
            expr = {"ast": ast, "source": "explicit"}
            if v.eqn:
                expr["display"] = v.eqn
            base["initial_value"] = expr
        else:
            base["initial_value"] = _q(0.0, unit)           # truly empty <eqn>
        base["inflows"] = [resolve_ref(x, v.scope, scope_resolver) or x for x in v.inflows]
        base["outflows"] = [resolve_ref(x, v.scope, scope_resolver) or x for x in v.outflows]
        if v.non_negative:
            base["floor"] = _q(0.0, unit)
        base["save_results"] = {"final_value": True, "time_history": True}
        return base, extras

    # A standalone named graphical function → a `lookup` node. This covers both an explicit
    # `v.kind == "gf"` and the shape xmutil emits for a bare Vensim lookup table: an
    # `<aux name=…><gf>…</gf></aux>` with NO `<eqn>` (so `ast is None`). Without this second
    # case the table would fall through to an empty aux (literal 0) and any `LOOKUP(table, x)`
    # calling it would read 0. It must have a `<gf>` payload — a no-eqn/no-gf aux is a real
    # empty variable, not a table.
    if v.kind == "gf" or (v.gf is not None and ast is None):
        # Standalone named graphical function → lookup node.
        base["primitive"] = "node"
        base["value_rule"] = "lookup"
        if v.gf is not None:
            base["table"] = {"x": v.gf.x, "y": v.gf.y}
            base["interpolation"] = v.gf.interpolation
            if v.gf.extrapolate:
                warn(v.qual_id, "XMILE-GF-EXTRAPOLATE: extrapolate mode → linear "
                                "(values differ only outside the x-range).")
        else:
            warn(v.qual_id, "empty <gf>; emitting a trivial identity lookup.")
            base["table"] = {"x": [0.0, 1.0], "y": [0.0, 1.0]}
            base["interpolation"] = "linear"
        return base, extras

    # flow or aux (a node)
    base["primitive"] = "node"

    # Embedded graphical function: eqn result → x → lookup. Two elements.
    if v.gf is not None and ast is not None:
        gf_id = v.qual_id + "__gf"
        gf_node = {
            "id": gf_id, "name": v.name + " (gf)", "primitive": "node",
            "value_rule": "lookup",
            "table": {"x": v.gf.x, "y": v.gf.y},
            "interpolation": v.gf.interpolation,
        }
        if v.scope:
            gf_node["container"] = "/".join(v.scope)
        extras.append(gf_node)
        ast = {"op": "lookup_call", "element_id": gf_id, "input": ast}

    if v.non_negative and v.kind == "flow" and ast is not None:
        ast = {"op": "call", "fn": "max",
               "args": [ast, {"op": "literal", "value": 0.0}]}

    # Apply-to-all array: wrap the (uniform) body in nested vector_maps over each
    # declared dimension and mark the output dimensioned. Only supports the
    # apply-to-all case (one eqn for the whole array); element-specific arrays are
    # a gap (warned in convert()).
    if v.dims:
        body = ast if ast is not None else {"op": "literal", "value": 0.0}
        refs = set()
        _collect_refs(body, refs)
        # If the body already references a dimensioned array, emit the PLAIN expression
        # and let the engine's align-by-name broadcast produce the dimensioned result
        # (wrapping it in vector_map would double-count the axis). Only a body with no
        # dimensioned input (a constant, or one over index_ref) needs vector_map to
        # materialize across the dimension.
        if refs & dimensioned:
            wrapped = body
        else:
            wrapped = body
            for dim in reversed(v.dims):
                wrapped = {"op": "vector_map", "over": dim, "body": wrapped}
        base["value_rule"] = "expression"
        expr = {"ast": wrapped, "source": "explicit"}
        if v.eqn:
            expr["display"] = v.eqn
        base["expression"] = expr
        base["outputs"] = [{"name": v.name, "unit": unit, "dimensions": list(v.dims)}]
        if refs:
            base["inputs"] = sorted(refs)
        return base, extras

    const = _const_value(ast) if (ast is not None and not v.non_negative and v.gf is None) else None
    if const is not None:
        base["value_rule"] = "fixed"
        base["value"] = _q(const, unit)
        return base, extras

    base["value_rule"] = "expression"
    expr: dict = {"ast": ast if ast is not None else {"op": "literal", "value": 0.0}}
    if v.eqn:
        expr["display"] = v.eqn
    expr["source"] = "explicit"
    base["expression"] = expr
    refs = set()
    _collect_refs(ast, refs)
    if refs:
        base["inputs"] = sorted(refs)
    return base, extras


# --------------------------------------------------------------------------- #
# Stage 7 — sim_specs + assembly
# --------------------------------------------------------------------------- #

def build_sim_settings(sim: IRSimSpecs) -> dict:
    if sim.method and sim.method not in ("euler", ""):
        warn("sim_specs", f"XMILE-INTEGRATOR-DOWNCONVERT: method='{sim.method}' → "
                          f"Euler (WaSiM integrates explicit-Euler only; results "
                          f"may diverge for stiff/oscillatory models).")
    if sim.start != 0.0:
        warn("sim_specs", f"XMILE-START-OFFSET: start={sim.start} ≠ 0; WaSiM's clock "
                          f"is t=0-relative. duration set to stop−start; absolute TIME "
                          f"references are not re-based in this phase.")
    duration = sim.stop - sim.start
    unit = sim.time_units or "1"
    return {
        "duration": _q(duration, unit),
        "timestep": _q(sim.dt, unit),
        "n_realizations": 1,
        "seed": 0,
    }


def _expand_modules(model: IRModel) -> list[dict]:
    """Expand each <module> instance: clone the referenced submodel template's
    variables under the module's scope and build a container_def (kind=submodel)
    with interface bindings from <connect> edges. Returns the container_def list;
    the cloned variables are appended to model.variables so they emit normally."""
    containers: list[dict] = []
    for mod in model.modules:
        template = model.submodels.get(mod.ref_model or "")
        if template is None:
            warn(mod.raw_name, f"XMILE-MODULE: referenced model '{mod.ref_model}' not "
                               f"found; module skipped.")
            continue
        warn(mod.raw_name, "XMILE-MODULE-SEMANTICS: <module> mapped to a WaSiM "
                           "submodel container running at n_realizations:1 (a deterministic "
                           "single run). Parent references to interior variables (`mod.var`) "
                           "are read via `submodel_stat(mean, …)`, which at n_realizations:1 "
                           "returns the exact interior value — so this reproduces the XMILE "
                           "inline sub-call. Caveat: interior variables that the parent never "
                           "references are not surfaced in top-level results, and any "
                           "randomness inside the submodel is collapsed to a single draw.")
        sub_scope = mod.scope + (mod.raw_name,)
        vars_elem = _find(template, "variables")
        sub_vars: list[IRVar] = []
        sub_mods: list[IRModule] = []
        if vars_elem is not None:
            _parse_variables(vars_elem, sub_scope, sub_vars, sub_mods)
        if sub_mods:
            warn(mod.raw_name, "XMILE-MODULE: nested modules inside a submodel are not "
                               "supported; inner modules skipped.")
        model.variables.extend(sub_vars)
        cont = {
            "id": "/".join(sub_scope),
            "name": mod.raw_name,
            "kind": "submodel",
            "elements": [],   # filled after ids are assigned
            "simulation_settings": {
                "duration": build_sim_settings(model.sim)["duration"],
                "timestep": build_sim_settings(model.sim)["timestep"],
                "n_realizations": 1,
            },
        }
        if mod.scope:
            cont["parent"] = "/".join(mod.scope)
        containers.append((cont, sub_scope, mod))
    return containers


class ConvertError(Exception):
    """Raised for input that isn't parseable XMILE at all (bad XML, not an <xmile>
    document). Distinct from per-construct degradation, which warns and continues."""


def convert(xml_text: str, model_name: Optional[str] = None) -> dict:
    try:
        root = ET.fromstring(xml_text)
    except ET.ParseError as e:
        raise ConvertError(f"input is not well-formed XML: {e}") from e
    if _tag(root) != "xmile":
        raise ConvertError(
            f"root element is <{_tag(root)}>, not <xmile>; not an XMILE document.")
    # ET keeps the namespace on tags; our _tag() strips it, so this works for both
    # namespaced and (rare) un-namespaced XMILE.
    model = parse_xmile(root)

    # Expand module instances into submodel variables + container_defs (Phase 4).
    container_specs = _expand_modules(model)

    resolver = build_scope_table(model)

    # Map each submodel-interior element id -> its submodel container id, so that a
    # parent expression referencing an interior output is emitted as `submodel_stat`
    # (see resolve_refs). Longest (deepest) container id wins for nested scopes.
    interior: dict[str, str] = {}
    sub_ids = ["/".join(sub_scope) for _, sub_scope, _ in container_specs]
    for v in model.variables:
        vid = v.qual_id
        owning = [s for s in sub_ids if vid == s or vid.startswith(s + "/")]
        if owning:
            interior[vid] = max(owning, key=len)

    # Qualified ids of dimensioned (array) variables — used to decide vector_map wrap.
    dimensioned = {v.qual_id for v in model.variables if v.dims}

    elements: list[dict] = []
    for v in model.variables:
        try:
            primary, extras = emit_element(v, resolver, model.sim, interior, dimensioned)
        except Exception as e:  # noqa: BLE001
            warn(v.qual_id or v.raw_name, f"internal error emitting element: {e}; skipped.")
            continue
        elements.append(primary)
        elements.extend(extras)

    # Build container_defs: interface bindings from <connect>, and member ids.
    containers: list[dict] = []
    for cont, sub_scope, mod in container_specs:
        cont["elements"] = [e["id"] for e in elements
                            if e.get("container") == "/".join(sub_scope)]
        inputs = []
        for to_raw, from_raw in mod.connects:
            # `to` is an interior input of the submodel; `from` is an outer element.
            to_id = resolve_ref(to_raw.split(".")[-1], sub_scope, resolver)
            from_id = resolve_ref(from_raw, mod.scope, resolver)
            if to_id and from_id:
                inputs.append({"input": to_id, "from": from_id})
            else:
                warn(mod.raw_name, f"XMILE-MODULE: <connect to='{to_raw}' "
                                   f"from='{from_raw}'> did not fully resolve; skipped.")
        cont["interface"] = {"inputs": inputs, "outputs": []}
        containers.append(cont)

    # Top-level dimensions.
    dimensions = [{"id": d.name, "name": d.name, "size": d.size,
                   **({"labels": d.labels} if d.labels else {})}
                  for d in model.dims]

    name = model_name or model.name
    out = {
        "wasim_version": WASIM_VERSION,
        "source": {
            "generator": "xmile_to_wasim.py",
            "notes": (
                f"Converted from XMILE: {name}. XMILE is a deterministic "
                "system-dynamics interchange format; WaSiM is a probabilistic "
                "time-stepping engine. This is a structural translation of the "
                "stock/flow/aux core; integration is explicit-Euler only (RK "
                "methods are down-converted). Constructs with no WaSiM equivalent "
                "become extern_call stubs — see the conversion warnings. Numeric "
                "fidelity is 'same trajectory under Euler', not bit-identical to "
                "Stella/Vensim."
            ),
        },
        "simulation_settings": build_sim_settings(model.sim),
        "elements": elements,
    }
    if dimensions:
        out["dimensions"] = dimensions
    if containers:
        out["containers"] = containers
    return out


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #

def main(argv: Optional[list[str]] = None) -> int:
    ap = argparse.ArgumentParser(
        description="Convert an XMILE system-dynamics model to a WaSiM v2 model.json")
    ap.add_argument("input", help="path to the .xmile / .stmx file")
    ap.add_argument("-o", "--output", help="output model.json path (default: <input>.wasim.json)")
    ap.add_argument("--name", help="override the model name")
    ap.add_argument("--stdout", action="store_true", help="write JSON to stdout instead of a file")
    args = ap.parse_args(argv)

    reset_warnings()
    with open(args.input, "r", encoding="utf-8", errors="replace") as f:
        text = f.read()

    try:
        model = convert(text, model_name=args.name)
    except ConvertError as e:
        print(f"error: {args.input}: {e}", file=sys.stderr)
        return 2
    js = json.dumps(model, indent=2)

    if args.stdout:
        print(js)
    else:
        out = args.output or (args.input.rsplit(".", 1)[0] + ".wasim.json")
        with open(out, "w", encoding="utf-8") as f:
            f.write(js + "\n")
        print(f"Wrote {out}  ({len(model['elements'])} elements)", file=sys.stderr)

    if WARNINGS:
        print(f"\n{len(WARNINGS)} conversion warning(s):", file=sys.stderr)
        for w in WARNINGS:
            print("  - " + w, file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
