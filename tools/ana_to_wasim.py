#!/usr/bin/env python3
"""
ana_to_wasim.py — convert an Analytica model (`.ana`) into a WASiM model.json.

Usage:
    python3 tools/ana_to_wasim.py input.ana [-o output.json] [--name "Model name"]
    python3 tools/ana_to_wasim.py input.ana --stdout

The `.ana` format is plain UTF-8 text: a flat sequence of typed nodes, each
introduced by a class keyword header line (`Chance Ab_travel_time`) followed by
attribute lines (`Title:`, `Units:`, `Definition:`, ...). See
docs.analytica.com. This converter targets the WASiM v0.1.0 model format
(`$id https://wasim.dev/schema/model/0.1.0`) that the Rust engine parses and
that `schema_examples_manual/*.json` follow.

Scope. Analytica and WASiM are architecturally different (see
ANALYTICA_ENGINE_GAP_ANALYSIS.md): Analytica centres on Intelligent Arrays +
Monte-Carlo with an optional Time axis, while WASiM is a time-stepping engine
with arrays/sampling as substrate. This converter handles the arithmetic +
probabilistic core that maps cleanly — distributions, scalar arithmetic,
If-Then-Else, comparisons, the common reductions/math builtins, and simple
Table/Sequence indexes. Constructs with no faithful v0.1.0 equivalent
(runtime-dynamic indices, sample-as-axis reductions mid-graph, user Functions,
metaprogramming, GUI logic) degrade to a preserved-but-inert stub
(`{"op":"literal","value":0.0}`, `source:"inferred"`, original text kept in
`display`) and are reported to stderr — the same graceful degradation the
engine's own transpiler uses. The output always validates against
`oldmodel.schema.json` and parses in the engine; unconverted definitions are
visible as warnings + inert stubs rather than silent wrong numbers.

No third-party dependencies (stdlib only).
"""

from __future__ import annotations

import argparse
import copy
import json
import re
import sys
from dataclasses import dataclass, field
from typing import Any, Optional


# --------------------------------------------------------------------------- #
# Warnings collector
# --------------------------------------------------------------------------- #

WARNINGS: list[str] = []


def warn(node: str, msg: str) -> None:
    entry = f"[{node}] {msg}"
    if not WARNINGS or WARNINGS[-1] != entry:  # collapse immediate duplicates
        if entry not in WARNINGS:
            WARNINGS.append(entry)


# --------------------------------------------------------------------------- #
# Stage 1 — .ana lexer: comments, object headers, attribute blocks
# --------------------------------------------------------------------------- #

# Node class keywords that introduce a `<Class> <Identifier>` object header.
OBJECT_CLASSES = {
    "model", "module", "library", "form",
    "variable", "chance", "decision", "objective", "constant",
    "index", "determ", "function", "button", "alias", "text", "picture",
    "metavar", "formnode", "close",
}

# Classes that produce no WASiM element (GUI aliases, presentation, scoping).
SKIP_CLASSES = {
    "model", "module", "library", "form", "text", "picture", "button",
    "alias", "metavar", "formnode", "close",
}

# Attribute keywords we read (others are skipped). GUI/layout attributes
# (nodelocation, nodesize, windowstate, ...) fall through and are ignored.
KNOWN_ATTRS = {
    "title", "units", "description", "definition", "domain", "identifier",
    "value", "defaultsize", "att_prefnodesize",
}

_ATTR_RE = re.compile(r"^([A-Za-z][A-Za-z0-9_]*)\s*:\s?(.*)$")
_HEADER_RE = re.compile(r"^([A-Za-z][A-Za-z0-9_]*)\s+([A-Za-z_][A-Za-z0-9_]*)\s*$")


@dataclass
class AnaNode:
    cls: str                       # lowercased class keyword
    ident: str                     # node identifier
    attrs: dict[str, str] = field(default_factory=dict)
    parent: Optional[str] = None   # enclosing module identifier


def strip_brace_comments(text: str) -> str:
    """Remove Analytica `{ ... }` comments (may span lines, no nesting)."""
    out = []
    depth = 0
    for ch in text:
        if ch == "{":
            depth += 1
        elif ch == "}":
            if depth > 0:
                depth -= 1
        elif depth == 0:
            out.append(ch)
    return "".join(out)


_SYSVAR_RE = re.compile(r"^([A-Za-z][A-Za-z0-9_]*)\s*:=\s*(.+)$")


def normalize_ana(text: str) -> str:
    """Undo Analytica's line-wrap encoding, leaving one statement per `\\n`.

    The `.ana` serialization is classic-Mac CR-delimited. Python text-mode reads
    translate CR/CRLF to `\\n` (universal newlines), so by the time we see the
    text, statements are `\\n`-separated and long values were wrapped with:
      * `~~` before the break  → soft wrap (join with nothing), and
      * `~`  before the break  → hard break inside the value.
    We collapse both to a single-line value (hard breaks become spaces, which is
    safe for both prose and expressions). Defensive `\\r` variants are handled in
    case the file was read in binary mode.
    """
    for br in ("\r\n", "\r", "\n"):
        text = text.replace("~~" + br, "").replace("~" + br, " ")
    return text


def lex_ana(text: str) -> tuple[list[AnaNode], Optional[int]]:
    """Parse the .ana text into (nodes, sample_size).

    Each statement (one per line after `normalize_ana`) is either an object
    header `<Class> <Ident>`, an attribute `Keyword: value`, a system variable
    `Name := value` (only `Samplesize` is read), or a `Close <Ident>` scope pop.
    """
    text = strip_brace_comments(normalize_ana(text))
    nodes: list[AnaNode] = []
    cur: Optional[AnaNode] = None
    module_stack: list[str] = []
    sample_size: Optional[int] = None

    for raw in text.split("\n"):
        line = raw.strip()
        if not line:
            continue

        header = _HEADER_RE.match(line)
        if header and header.group(1).lower() in OBJECT_CLASSES:
            cls = header.group(1).lower()
            ident = header.group(2)
            if cls == "close":
                if module_stack:
                    module_stack.pop()
                cur = None
                continue
            parent = module_stack[-1] if module_stack else None
            cur = AnaNode(cls=cls, ident=ident, parent=parent)
            nodes.append(cur)
            if cls in ("model", "module", "library", "form"):
                module_stack.append(ident)
            continue

        sv = _SYSVAR_RE.match(line)
        if sv:
            if sv.group(1).lower() in ("samplesize", "sampesize", "sample_size"):
                n = _try_number(sv.group(2).strip())
                if n is not None:
                    sample_size = int(n)
            cur = None  # system vars live outside any node
            continue

        m = _ATTR_RE.match(line)
        if m and cur is not None and m.group(1).lower() in KNOWN_ATTRS:
            key = m.group(1).lower()
            # First occurrence wins (later duplicate attrs are GUI variants).
            cur.attrs.setdefault(key, m.group(2).strip())
        # Any other line (GUI attrs, unknown keywords) is ignored.

    return nodes, sample_size


# --------------------------------------------------------------------------- #
# Stage 2 — Analytica expression tokenizer + Pratt parser
# --------------------------------------------------------------------------- #

class Tok:
    NUM = "num"
    IDENT = "ident"
    STR = "str"
    OP = "op"
    LP = "("
    RP = ")"
    LB = "["
    RB = "]"
    COMMA = ","
    COLON = ":"
    EOF = "eof"


_TOKEN_RE = re.compile(
    r"""
      (?P<ws>\s+)
    | (?P<str>'[^']*'|"[^"]*")
    | (?P<num>(?:\d+\.\d+|\d+)[kKmMgG]\b|\d+\.\d+(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|\d+(?:[eE][+-]?\d+)?)
    | (?P<ident>[A-Za-z_][A-Za-z0-9_]*)
    | (?P<op><=|>=|<>|[-+*/^<>=])
    | (?P<lp>\()
    | (?P<rp>\))
    | (?P<lb>\[)
    | (?P<rb>\])
    | (?P<comma>,)
    | (?P<colon>:)
    | (?P<amp>&)
    | (?P<other>.)
    """,
    re.VERBOSE,
)

_SUFFIX = {"k": 1e3, "K": 1e3, "m": 1e6, "M": 1e6, "g": 1e9, "G": 1e9}


def tokenize_expr(s: str) -> list[tuple[str, Any]]:
    toks: list[tuple[str, Any]] = []
    for m in _TOKEN_RE.finditer(s):
        kind = m.lastgroup
        val = m.group()
        if kind == "ws":
            continue
        if kind == "str":
            toks.append((Tok.STR, val[1:-1]))
            continue
        if kind == "num":
            if val[-1] in _SUFFIX:
                num = float(val[:-1]) * _SUFFIX[val[-1]]
            else:
                num = float(val)
            toks.append((Tok.NUM, num))
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
        elif kind == "colon":
            toks.append((Tok.COLON, val))
        elif kind == "amp":
            toks.append((Tok.OP, "&"))
        else:
            toks.append((Tok.OP, val))  # unknown single char, let parser reject
    toks.append((Tok.EOF, None))
    return toks


class ParseError(Exception):
    pass


# Keywords that are control words, not identifiers/refs.
_KEYWORDS = {"if", "then", "else", "and", "or", "not"}

# Bare identifiers with no finite numeric AST form — kept as inert stubs.
_OPAQUE_IDENTS = {"inf", "infinity", "undefined", "null", "nan", "self"}

# Binary operator → WASiM AST op + binding power (left, right).
_BINOPS = {
    "or": ("or", 1, 2),
    "and": ("and", 3, 4),
    "=": ("eq", 5, 6),
    "<>": ("neq", 5, 6),
    "<": ("lt", 5, 6),
    ">": ("gt", 5, 6),
    "<=": ("lte", 5, 6),
    ">=": ("gte", 5, 6),
    "+": ("add", 7, 8),
    "-": ("subtract", 7, 8),
    "*": ("multiply", 9, 10),
    "/": ("divide", 9, 10),
    "^": ("power", 14, 13),  # right-assoc
}


@dataclass
class Call:
    """A raw parsed function call — resolved to AST/distribution later."""
    name: str
    pos: list[Any]                        # positional args (AST nodes)
    named: dict[str, Any]                 # named args (AST nodes)


class ExprParser:
    """Pratt parser producing either a WASiM AST dict or a `Call` sentinel."""

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
        left = self.parse_prefix()
        while True:
            kind, val = self.peek()
            opname = None
            if kind == Tok.OP:
                opname = val
            elif kind == Tok.IDENT and val.lower() in ("and", "or"):
                opname = val.lower()
            if opname is None or opname not in _BINOPS:
                break
            ast_op, lbp, rbp = _BINOPS[opname]
            if lbp < min_bp:
                break
            self.next()
            right = self.parse_expr(rbp)
            left = _as_ast(left)
            right = _as_ast(right)
            left = {"op": ast_op, "left": left, "right": right}
        return left

    def parse_prefix(self) -> Any:
        kind, val = self.peek()
        if kind == Tok.OP and val == "-":
            self.next()
            operand = _as_ast(self.parse_expr(11))  # unary minus binds tightly
            return {"op": "neg", "operand": operand}
        if kind == Tok.OP and val == "+":
            self.next()
            return self.parse_expr(11)
        if kind == Tok.IDENT and val.lower() == "not":
            self.next()
            operand = _as_ast(self.parse_expr(11))
            return {"op": "not", "operand": operand}
        if kind == Tok.IDENT and val.lower() == "if":
            # Functional form `If(cond, then, else)` vs keyword form
            # `If cond Then a Else b`: disambiguate on the next token.
            if self.toks[self.i + 1][0] == Tok.LP:
                self.next()   # 'if'
                self.next()   # '('
                pos, named = self.parse_call_args()
                self.expect(Tok.RP)
                return Call(name="if", pos=pos, named=named)
            return self.parse_if()
        return self.parse_atom()

    def parse_if(self) -> Any:
        self.next()  # 'if'
        cond = _as_ast(self.parse_expr(0))
        self._expect_kw("then")
        then = _as_ast(self.parse_expr(0))
        self._expect_kw("else")
        else_ = _as_ast(self.parse_expr(0))
        return {"op": "if", "cond": cond, "then": then, "else": else_}

    def _expect_kw(self, kw: str) -> None:
        kind, val = self.next()
        if kind != Tok.IDENT or val.lower() != kw:
            raise ParseError(f"expected keyword {kw!r}, got {(kind, val)}")

    def parse_atom(self) -> Any:
        kind, val = self.next()
        if kind == Tok.NUM:
            return {"op": "literal", "value": float(val)}
        if kind == Tok.STR:
            # String literal (e.g. Index labels). No numeric AST form — carry the
            # text so an Index can use it as a label; inert (0.0) in arithmetic.
            return {"op": "literal", "value": 0.0, "_string": val}
        if kind == Tok.LP:
            inner = self.parse_expr(0)
            self.expect(Tok.RP)
            return inner
        if kind == Tok.LB:  # list literal → array
            elements = self.parse_arglist(Tok.RB)
            self.expect(Tok.RB)
            return {"op": "array", "elements": [_as_ast(e) for e in elements]}
        if kind == Tok.IDENT:
            if val.lower() in _KEYWORDS:
                raise ParseError(f"unexpected keyword {val!r}")
            if val.lower() in _OPAQUE_IDENTS:
                # INF / Undefined / NaN etc. — no finite JSON literal; keep the
                # symbol in provenance and evaluate to an inert 0.0.
                return {"op": "literal", "value": 0.0, "_stub_display": val}
            if self.peek()[0] == Tok.LP:
                self.next()  # consume (
                pos, named = self.parse_call_args()
                self.expect(Tok.RP)
                call = Call(name=val, pos=pos, named=named)
                # Analytica double-application: Table(I)(v1, v2, ...)
                while self.peek()[0] == Tok.LP:
                    self.next()
                    pos2, named2 = self.parse_call_args()
                    self.expect(Tok.RP)
                    call = Call(name=val, pos=call.pos + pos2,
                                named={**call.named, **named2})
                return call
            return {"op": "ref", "element_id": val, "output": "value"}
        raise ParseError(f"unexpected token {(kind, val)}")

    def parse_call_args(self) -> tuple[list[Any], dict[str, Any]]:
        pos: list[Any] = []
        named: dict[str, Any] = {}
        if self.peek()[0] == Tok.RP:
            return pos, named
        while True:
            # Named argument: `ident : expr`
            if (self.peek()[0] == Tok.IDENT
                    and self.toks[self.i + 1][0] == Tok.COLON):
                argname = self.next()[1]
                self.next()  # colon
                named[argname.lower()] = self.parse_expr(0)
            else:
                pos.append(self.parse_expr(0))
            if self.peek()[0] == Tok.COMMA:
                self.next()
                continue
            break
        return pos, named

    def parse_arglist(self, close: str) -> list[Any]:
        args: list[Any] = []
        if self.peek()[0] == close:
            return args
        while True:
            args.append(self.parse_expr(0))
            if self.peek()[0] == Tok.COMMA:
                self.next()
                continue
            break
        return args


def _as_ast(node: Any) -> dict:
    """Coerce a parse result into a WASiM AST node, resolving pending Calls."""
    if isinstance(node, Call):
        return resolve_call(node)
    return node


# --------------------------------------------------------------------------- #
# Stage 3 — resolve Analytica function calls to WASiM AST
# --------------------------------------------------------------------------- #

# Analytica fn (lowercased) -> WASiM builtin, with arity expectations.
# 2-arg Min/Max map to the scalar builtins; the array-reduction spellings
# (Sum(x, I), Max(x, I)) are handled specially below.
_SCALAR_BUILTINS = {
    "abs": "abs", "sqrt": "sqrt", "exp": "exp", "ln": "ln", "log": "log",
    "logten": "log", "log10": "log", "sin": "sin", "cos": "cos", "tan": "tan",
    "arcsin": "asin", "arccos": "acos", "arctan": "atan", "arctan2": "atan2",
    "sinh": "sinh", "cosh": "cosh", "tanh": "tanh",
    "floor": "floor", "ceil": "ceil", "round": "round", "mod": "mod",
    "sign": "sign",
}

_ARRAY_REDUCERS = {
    "sum": "sum_array", "average": "mean_array", "mean": "mean_array",
    "min": "min_array", "max": "max_array", "size": "size_array",
}

_DISTRIBUTIONS = {
    "uniform", "normal", "lognormal", "triangular", "beta", "gamma",
    "exponential", "bernoulli", "weibull",
}

# Across-realization ("over Run") sample statistics. In WASiM these live in the
# results/analysis layer (A3, `results_spec`), NOT as a mid-graph value a
# downstream element can consume — the single genuine semantic gap called out in
# ANALYTICA_ENGINE_GAP_ANALYSIS.md §2. They have no faithful v0.1.0 AST form, so
# they degrade to an inert stub + warning.
_SAMPLE_STATS = {
    "probability", "probbands", "getfract", "cdf", "pdf", "sdeviation",
    "variance", "kurtosis", "skewness", "correlation", "frequency",
    "cumulate", "cumproduct", "rank", "dynamic",
}


def _stub(display: str, node_hint: str = "") -> dict:
    """An inert placeholder for an unconvertible construct (kept for provenance)."""
    return {"op": "literal", "value": 0.0, "_stub_display": display}


def resolve_call(call: Call) -> dict:
    name = call.name.lower()
    args = [_as_ast(a) for a in call.pos]
    named = {k: _as_ast(v) for k, v in call.named.items()}

    # Min/Max: 2 positional numeric-ish args -> scalar builtin; else reducer.
    if name in ("min", "max"):
        if len(args) == 2 and not named:
            return {"op": "call", "fn": name, "args": args}
        # Max(x, I) reduction (I is an index ref) or Max(x)
        return {"op": "call", "fn": _ARRAY_REDUCERS[name], "args": args[:1] or args}

    if name in _SCALAR_BUILTINS:
        return {"op": "call", "fn": _SCALAR_BUILTINS[name], "args": args}

    if name in _ARRAY_REDUCERS:
        # Sum(x, Index) -> sum_array(x); drop the index arg (implicit in v0.1.0).
        # Single-arg Mean/Average over a per-realization value is really an
        # across-realization statistic (§2) — map to the array reducer but flag,
        # since the axis is not faithfully represented in v0.1.0.
        if name in ("mean", "average") and len(args) == 1 and not named:
            warn(call.name, "Mean/Average of a single value maps to `mean_array`, "
                            "but Analytica's Mean over the Run sample is a results-layer "
                            "(A3) statistic in WASiM, not a mid-graph array mean — verify axis.")
        return {"op": "call", "fn": _ARRAY_REDUCERS[name], "args": args[:1] or args}

    if name in _SAMPLE_STATS:
        warn(call.name, "across-realization/dynamic statistic has no mid-graph v0.1.0 "
                        "equivalent (see gap analysis §2); preserved as inert stub — "
                        "reconstruct in the results layer or as an accumulator.")
        return _stub(_render_call(call))

    if name in ("if", "ifonly"):
        # If(cond, then, else) functional form.
        if len(args) >= 3:
            return {"op": "if", "cond": args[0], "then": args[1], "else": args[2]}

    if name in ("table", "array", "subscript", "slice"):
        # Table(Index)(v1..vn) collapses to an array literal of the values.
        # (Index dimensioning is not represented in v0.1.0 scalar arrays.)
        vals = args
        return {"op": "array", "elements": vals}

    if name == "sequence":
        # Sequence(start, stop[, step]) -> materialized array literal.
        return _sequence_literal(args, call)

    if name in _DISTRIBUTIONS:
        # A distribution nested inside an expression cannot be a random_variable
        # in v0.1.0 (sampling is per-element). Preserve as a stub + warning; the
        # top-level Chance path (see build_element) handles the common case.
        return _stub(_render_call(call), )

    # Cumulate / cumproduct / dynamic / and friends: no scalar equivalent here.
    return _stub(_render_call(call))


def _sequence_literal(args: list[dict], call: Call) -> dict:
    try:
        start = args[0]["value"]
        stop = args[1]["value"]
        step = args[2]["value"] if len(args) > 2 else 1.0
        if step == 0:
            raise ValueError
        vals = []
        x = start
        # inclusive of stop, matching Analytica Sequence
        n = int(round((stop - start) / step)) + 1
        for k in range(max(n, 0)):
            vals.append({"op": "literal", "value": start + k * step})
        return {"op": "array", "elements": vals}
    except Exception:
        return _stub(_render_call(call))


def _render_call(call: Call) -> str:
    parts = [_render_ast(a) for a in call.pos]
    parts += [f"{k}: {_render_ast(v)}" for k, v in call.named.items()]
    return f"{call.name}({', '.join(parts)})"


def _render_ast(node: Any) -> str:
    if isinstance(node, Call):
        return _render_call(node)
    if not isinstance(node, dict):
        return str(node)
    op = node.get("op")
    if op == "literal":
        return _num(node["value"])
    if op == "ref":
        return node["element_id"]
    if op in ("add", "subtract", "multiply", "divide", "power",
              "lt", "gt", "lte", "gte", "eq", "neq", "and", "or"):
        sym = {"add": "+", "subtract": "-", "multiply": "*", "divide": "/",
               "power": "^", "lt": "<", "gt": ">", "lte": "<=", "gte": ">=",
               "eq": "=", "neq": "<>", "and": "And", "or": "Or"}[op]
        return f"({_render_ast(node['left'])} {sym} {_render_ast(node['right'])})"
    if op == "neg":
        return f"-{_render_ast(node['operand'])}"
    if op == "not":
        return f"Not {_render_ast(node['operand'])}"
    if op == "if":
        return (f"If {_render_ast(node['cond'])} Then {_render_ast(node['then'])} "
                f"Else {_render_ast(node['else'])}")
    if op == "call":
        return f"{node['fn']}({', '.join(_render_ast(a) for a in node['args'])})"
    if op == "array":
        return f"[{', '.join(_render_ast(a) for a in node['elements'])}]"
    return "?"


def _num(x: float) -> str:
    if x == int(x):
        return str(int(x))
    return repr(x)


def strip_stub_markers(node: Any) -> tuple[Any, list[str]]:
    """Remove internal `_stub_display` keys; return cleaned node + stub texts."""
    stubs: list[str] = []

    def walk(n: Any) -> Any:
        if isinstance(n, dict):
            if "_stub_display" in n:
                stubs.append(n["_stub_display"])
                return {"op": "literal", "value": 0.0}
            if "_string" in n:
                return {"op": "literal", "value": float(n.get("value", 0.0))}
            return {k: walk(v) for k, v in n.items()}
        if isinstance(n, list):
            return [walk(v) for v in n]
        return n

    cleaned = walk(node)
    return cleaned, stubs


# A local-variable block: `Var name := expr; [Var …;] final_expr` (also the
# `Var name := expr Do body` spelling). Detected by a leading `var` binding.
_VAR_HEAD_RE = re.compile(r"^\s*var\s+[A-Za-z_][A-Za-z0-9_]*\s*:=", re.IGNORECASE)
_VAR_BIND_RE = re.compile(r"^\s*var\s+([A-Za-z_][A-Za-z0-9_]*)\s*:=\s*(.*)$",
                          re.IGNORECASE | re.DOTALL)


def _split_top_level(text: str) -> list[str]:
    """Split on `;` (and a top-level `Do` keyword) outside parens/brackets/quotes."""
    parts: list[str] = []
    buf: list[str] = []
    depth = 0
    quote: Optional[str] = None
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if quote is not None:
            buf.append(ch)
            if ch == quote:
                quote = None
            i += 1
            continue
        if ch in "'\"":
            quote = ch
            buf.append(ch)
        elif ch in "([":
            depth += 1
            buf.append(ch)
        elif ch in ")]":
            depth -= 1
            buf.append(ch)
        elif ch == ";" and depth == 0:
            parts.append("".join(buf))
            buf = []
        elif (depth == 0 and (ch in " \t") and text[i:i + 4].lower() in (" do ",)
              and (i + 4 <= n)):
            # a top-level " Do " separator (Var x := e Do body)
            parts.append("".join(buf))
            buf = []
            i += 3  # skip "do" plus the leading space already matched
        else:
            buf.append(ch)
        i += 1
    if buf:
        parts.append("".join(buf))
    return [p for p in (s.strip() for s in parts) if p]


def _substitute_refs(node: Any, mapping: dict[str, Any]) -> Any:
    """Replace `ref` nodes whose id is a bound local with a copy of its AST."""
    if isinstance(node, dict):
        if node.get("op") == "ref" and node.get("element_id") in mapping:
            return copy.deepcopy(mapping[node["element_id"]])
        return {k: _substitute_refs(v, mapping) for k, v in node.items()}
    if isinstance(node, list):
        return [_substitute_refs(v, mapping) for v in node]
    return node


def _parse_one(fragment: str) -> Any:
    """Parse a single expression fragment into an AST (resolving pending Calls).

    A fragment that doesn't parse (e.g. it contains a label subscript, §4.1)
    degrades to an inert stub carrying its text — so one unconvertible binding
    stubs on its own instead of failing the whole block.
    """
    try:
        return _as_ast(ExprParser(tokenize_expr(fragment)).parse())
    except (ParseError, Exception):  # noqa: BLE001 - defensive by design
        return _stub(fragment)


def parse_var_block(text: str) -> tuple[Optional[Any], Optional[str]]:
    """Lower an Analytica `Var … := …; … result` block to an inlined AST.

    Bindings are resolved in order (each may reference earlier ones and the
    enclosing scope), then inlined into the final (return) expression. Local
    names shadow model elements only within the block. Unconvertible
    sub-expressions inside the block still degrade to stubs downstream — the win
    is that the *rest* of the definition now survives instead of the whole thing
    stubbing on the leading `var`.
    """
    stmts = _split_top_level(text)
    if not stmts:
        return None, "empty var block"
    resolved: dict[str, Any] = {}
    body_ast: Optional[Any] = None
    last_bound: Optional[str] = None
    try:
        for stmt in stmts:
            m = _VAR_BIND_RE.match(stmt)
            if m:
                name, expr = m.group(1), m.group(2).strip()
                ast = _substitute_refs(_parse_one(expr), resolved)
                resolved[name] = ast
                last_bound = name
            else:
                # A non-binding statement is the return expression (normally last).
                body_ast = _substitute_refs(_parse_one(stmt), resolved)
        if body_ast is None:
            # No bare return expression: the block ends on its last binding.
            if last_bound is None:
                return None, "var block with no body"
            body_ast = resolved[last_bound]
        return body_ast, None
    except (ParseError, Exception) as e:  # noqa: BLE001 - defensive by design
        return None, f"var block: {e}"


def parse_definition(text: str) -> tuple[Optional[Any], Optional[str]]:
    """Parse an Analytica definition into a (parse_result, error) pair.

    The parse_result may be a WASiM AST dict OR a top-level `Call` (so callers
    can special-case a distribution that constitutes the whole definition).
    """
    text = text.strip()
    if not text:
        return None, "empty definition"
    if _VAR_HEAD_RE.match(text):
        return parse_var_block(text)
    try:
        toks = tokenize_expr(text)
        return ExprParser(toks).parse(), None
    except (ParseError, Exception) as e:  # noqa: BLE001 - defensive by design
        return None, str(e)


# --------------------------------------------------------------------------- #
# Stage 4 — distribution mapping (top-level Chance definitions)
# --------------------------------------------------------------------------- #

def _q(value: float, unit: str = "1") -> dict:
    return {"value": float(value), "unit": unit}


def _param_value(node: Any) -> Optional[float]:
    if isinstance(node, dict) and node.get("op") == "literal":
        if "_string" in node or "_stub_display" in node:
            return None
        return float(node["value"])
    if isinstance(node, dict) and node.get("op") == "neg":
        inner = _param_value(node["operand"])
        return -inner if inner is not None else None
    return None


def _param_or_formula(node: Any) -> Any:
    """A distribution parameter: a Quantity if constant, else a formula string."""
    v = _param_value(node)
    if v is not None:
        return _q(v)
    # Formula-valued parameter (references other elements). v0.1.0 accepts a
    # formula string here (quantity_or_formula); the engine stores it.
    return _render_ast(node)


def call_to_distribution(call: Call, unit: str) -> Optional[dict]:
    """Map a top-level Analytica distribution Call to a WASiM distribution dict."""
    name = call.name.lower()
    pos = [_as_ast(a) for a in call.pos]
    named = {k.lower(): _as_ast(v) for k, v in call.named.items()}

    def arg(i: int, *names: str) -> Optional[Any]:
        for nm in names:
            if nm in named:
                return named[nm]
        return pos[i] if i < len(pos) else None

    fam: Optional[str] = None
    params: dict[str, Any] = {}

    if name == "uniform":
        lo, hi = arg(0, "min", "a"), arg(1, "max", "b")
        fam, params = "uniform", {"min": _param_or_formula(lo), "max": _param_or_formula(hi)}
    elif name == "normal":
        mean, sd = arg(0, "mean", "median", "mu"), arg(1, "stddev", "sd", "sigma")
        fam, params = "normal", {"mean": _param_or_formula(mean), "stddev": _param_or_formula(sd)}
    elif name == "lognormal":
        # Analytica Lognormal(median, gsdev) — geometric params. WASiM `lognormal`
        # takes (mean, stddev) of the underlying normal (log-space). Convert:
        #   mu = ln(median), sigma = ln(gsdev).
        median = arg(0, "median", "mean", "mu")
        gsdev = arg(1, "gsdev", "stddev", "sd", "sigma")
        mv, gv = _param_value(median), _param_value(gsdev)
        if mv is not None and gv is not None and mv > 0 and gv > 0:
            import math
            fam = "lognormal"
            params = {"mean": _q(math.log(mv)), "stddev": _q(math.log(gv))}
        else:
            # Non-constant params: fall back to lognormal_moments with formulas,
            # flagged — the geometric→log conversion can't be done symbolically here.
            warn(call.name, "Lognormal with non-constant median/gsdev — emitted "
                            "as lognormal_moments with raw params (verify).")
            fam = "lognormal_moments"
            params = {"mean": _param_or_formula(median), "stddev": _param_or_formula(gsdev)}
    elif name == "triangular":
        lo, mode, hi = arg(0, "min", "a"), arg(1, "mode", "b"), arg(2, "max", "c")
        fam, params = "triangular", {
            "min": _param_or_formula(lo), "mode": _param_or_formula(mode),
            "max": _param_or_formula(hi)}
    elif name == "beta":
        a, b = arg(0, "a", "alpha"), arg(1, "b", "beta")
        fam, params = "beta", {"alpha": _param_or_formula(a), "beta": _param_or_formula(b)}
    elif name == "gamma":
        shape, scale = arg(0, "a", "shape"), arg(1, "b", "scale")
        fam, params = "gamma", {"shape": _param_or_formula(shape), "scale": _param_or_formula(scale)}
    elif name == "exponential":
        rate = arg(0, "r", "rate", "mean")
        rv = _param_value(rate)
        # Analytica Exponential(r) is parameterized by rate; WASiM uses mean=1/rate.
        if rv is not None and rv != 0 and ("mean" not in named):
            params = {"mean": _q(1.0 / rv)}
        else:
            params = {"mean": _param_or_formula(rate)}
        fam = "exponential"
    elif name == "bernoulli":
        p = arg(0, "p", "prob")
        pv = _param_value(p)
        fam, params = "bernoulli", {"prob": _q(pv if pv is not None else 0.5)}
    elif name == "weibull":
        shape, scale = arg(0, "shape", "a"), arg(1, "scale", "b")
        fam, params = "weibull", {"shape": _param_or_formula(shape), "scale": _param_or_formula(scale)}
    else:
        return None

    # Guard: the engine evaluates formula-valued distribution params as 0.0, which
    # makes (log)normal stddev non-finite and crashes sampling — and is silently
    # wrong for the rest. Only emit a random_variable when every parameter is a
    # finite constant; otherwise return None so the caller emits an inert stub.
    if not _all_params_finite(params):
        warn(call.name, "distribution has non-constant / unresolved parameters "
                        "(engine can't evaluate formula params) — emitted inert stub "
                        "instead of a mis-parameterized draw.")
        return None

    dist: dict[str, Any] = {"family": fam, "parameters": params}
    return dist


def _all_params_finite(params: dict[str, Any]) -> bool:
    for v in params.values():
        if isinstance(v, dict) and "value" in v:
            x = v["value"]
            if not isinstance(x, (int, float)) or x != x or x in (float("inf"), float("-inf")):
                return False
        else:
            return False  # a formula string or anything non-Quantity
    return True


# --------------------------------------------------------------------------- #
# Stage 5 — node -> WASiM element
# --------------------------------------------------------------------------- #

def _slug(ident: str) -> str:
    return ident


def build_element(node: AnaNode, container: Optional[str]) -> Optional[dict]:
    ident = node.ident
    name = node.attrs.get("title", ident).strip() or ident
    units = node.attrs.get("units", "").strip() or "1"
    desc = node.attrs.get("description", "").strip() or None
    definition = node.attrs.get("definition", "").strip()

    base: dict[str, Any] = {
        "id": ident,
        "name": name,
        "container": container,
        "description": desc,
    }
    if node.cls in ("index",):
        base["source_type"] = "Index"
    elif node.cls in ("decision", "chance", "objective", "determ", "variable", "constant"):
        base["source_type"] = node.cls.capitalize()

    # Index node -> constant-values array (best-effort; no dynamic indexing).
    if node.cls == "index":
        return _build_index_element(node, base, units)

    if not definition:
        # No formula: a bare input. Decisions/Constants with a numeric Value.
        val = node.attrs.get("value", "").strip()
        num = _try_number(val) if val else None
        base.update({
            "type": "constant",
            "value": _q(num if num is not None else 0.0, units),
            "editable": node.cls in ("decision", "constant"),
            "bounds": {"min": None, "max": None},
        })
        if num is None and node.cls not in ("constant", "decision"):
            warn(ident, "no Definition — emitted as constant 0.")
        return base

    parsed, err = parse_definition(definition)
    if err is not None or parsed is None:
        warn(ident, f"could not parse Definition ({err}); emitted inert stub. "
                    f"Raw: {definition[:120]}")
        base.update({
            "type": "expression",
            "expression": {
                "ast": {"op": "literal", "value": 0.0},
                "display": definition,
                "source": "inferred",
            },
            "inputs": [],
        })
        return base

    # Top-level distribution -> random_variable (Chance / uncertain Variable).
    if isinstance(parsed, Call) and parsed.name.lower() in _DISTRIBUTIONS:
        dist = call_to_distribution(parsed, units)
        if dist is not None:
            base.update({
                "type": "random_variable",
                "distribution": dist,
                "save_results": {"final_value": True, "time_history": False},
            })
            if units != "1":
                base["outputs"] = [{"name": "value", "unit": units, "display_unit": None}]
            return base

    # A bare number -> constant.
    ast = _as_ast(parsed)
    ast, stubs = strip_stub_markers(ast)
    for s in stubs:
        warn(ident, f"unconvertible sub-expression preserved as inert stub: {s}")

    # A genuine bare number -> constant (but NOT a formula that merely stubbed
    # to 0.0 — those stay expressions so the original text survives in `display`).
    if (ast.get("op") == "literal" and not stubs
            and node.cls in ("decision", "constant", "variable", "determ")):
        base.update({
            "type": "constant",
            "value": _q(ast["value"], units),
            "editable": node.cls in ("decision", "constant"),
            "bounds": {"min": None, "max": None},
        })
        return base

    # Everything else -> expression element. For a partially/fully stubbed
    # definition, keep the ORIGINAL Analytica text in `display` (more faithful
    # than the re-rendered, stub-flattened AST).
    inputs = sorted(_collect_refs(ast))
    base.update({
        "type": "expression",
        "expression": {
            "ast": ast,
            "display": definition if stubs else _render_ast(_as_ast(parsed)),
            "source": "explicit" if not stubs else "inferred",
        },
        "inputs": inputs,
    })
    if units != "1":
        base["outputs"] = [{"name": "value", "unit": units, "display_unit": None}]
    return base


def _build_index_element(node: AnaNode, base: dict, units: str) -> Optional[dict]:
    """Map an Analytica Index to a v0.1.0 `array` element (expressions form).

    v0.1.0 arrays are a list of scalar `expressions` (`values_unit` required,
    minItems 1). WASiM has no runtime-dynamic / label indices, so only a
    constant-member index (`Sequence(...)`, a list, or `Table`) maps; anything
    else degrades to an inert scalar stub + warning.
    """
    definition = node.attrs.get("definition", "").strip()
    parsed, err = parse_definition(definition) if definition else (None, "empty")
    values: list[float] = []
    labels: list[str] = []
    if parsed is not None:
        ast = _as_ast(parsed)
        if ast.get("op") == "array":
            for i, e in enumerate(ast["elements"], start=1):
                if isinstance(e, dict) and "_string" in e:
                    # Label index: ordinal position is the value, text is the label.
                    values.append(float(i))
                    labels.append(e["_string"])
                else:
                    v = _param_value(e)
                    if v is not None:
                        values.append(v)
                        labels.append(_num(v))
    if not values:
        warn(node.ident, "Index has no constant members (dynamic/label index?) — "
                         "no v0.1.0 dimension equivalent; emitted inert scalar stub.")
        base.update({
            "type": "expression",
            "expression": {"ast": {"op": "literal", "value": 0.0},
                           "display": definition or "(empty index)",
                           "source": "inferred"},
            "inputs": [],
        })
        return base
    base.update({
        "type": "array",
        "values_unit": units,
        "expressions": [
            {"ast": {"op": "literal", "value": v}, "display": lbl,
             "source": "explicit"}
            for v, lbl in zip(values, labels)
        ],
        "labels": labels,
        "inputs": [],
    })
    return base


def _try_number(s: str) -> Optional[float]:
    try:
        return float(s.replace(",", ""))
    except (ValueError, AttributeError):
        return None


def _collect_refs(ast: Any) -> set[str]:
    refs: set[str] = set()

    def walk(n: Any) -> None:
        if isinstance(n, dict):
            if n.get("op") == "ref":
                refs.add(n["element_id"])
            for v in n.values():
                walk(v)
        elif isinstance(n, list):
            for v in n:
                walk(v)

    walk(ast)
    return refs


# --------------------------------------------------------------------------- #
# Stage 6 — top-level assembly
# --------------------------------------------------------------------------- #

def _sanitize_dangling_refs(elements: list[dict]) -> None:
    """Replace refs to non-emitted elements with inert stubs, in place.

    A converted expression may reference an id that never became an element —
    an Analytica system index (`Time`, `Run`), a user `Function` (skipped), or a
    node the converter dropped. Left alone these are dangling edges the engine's
    graph builder rejects. Rewrite each to `literal 0.0` so the model still
    builds and runs; the original text already survives in the element `display`.
    """
    ids = {e["id"] for e in elements}
    missing: set[str] = set()

    def fix(node: Any) -> Any:
        if isinstance(node, dict):
            if node.get("op") == "ref" and node.get("element_id") not in ids:
                missing.add(node["element_id"])
                return {"op": "literal", "value": 0.0}
            return {k: fix(v) for k, v in node.items()}
        if isinstance(node, list):
            return [fix(v) for v in node]
        return node

    for e in elements:
        if e["type"] == "expression":
            e["expression"]["ast"] = fix(e["expression"]["ast"])
            e["inputs"] = [i for i in e.get("inputs", []) if i in ids]
        elif e["type"] == "array":
            for ex in e.get("expressions", []):
                ex["ast"] = fix(ex["ast"])
            e["inputs"] = [i for i in e.get("inputs", []) if i in ids]

    if missing:
        warn("graph", f"{len(missing)} reference(s) to non-model ids "
                      f"(system indices / skipped nodes) replaced with inert 0.0: "
                      f"{', '.join(sorted(missing)[:8])}"
                      f"{' …' if len(missing) > 8 else ''}")


def convert(text: str, model_name: Optional[str] = None) -> dict:
    nodes, sample_size = lex_ana(text)

    # Containers from Model/Module nodes.
    containers: list[dict] = []
    module_ids: set[str] = set()
    for n in nodes:
        if n.cls in ("module", "model", "library", "form"):
            module_ids.add(n.ident)
            containers.append({
                "id": n.ident,
                "name": n.attrs.get("title", n.ident).strip() or n.ident,
                "parent": n.parent,
                "children": [],
            })

    elements: list[dict] = []
    for n in nodes:
        if n.cls in SKIP_CLASSES:
            continue
        if n.cls == "function":
            warn(n.ident, "Analytica user Function — no v0.1.0 element equivalent; skipped.")
            continue
        # A node named like the sample-size system var isn't a model element.
        if n.ident.lower() in ("samplesize", "sampesize", "sample_size", "time", "run"):
            continue
        container = n.parent if n.parent in module_ids else None
        try:
            el = build_element(n, container)
        except Exception as e:  # noqa: BLE001
            warn(n.ident, f"internal error building element: {e}; skipped.")
            el = None
        if el is not None:
            elements.append(el)

    _sanitize_dangling_refs(elements)

    # populate container children back-refs
    child_map: dict[str, list[str]] = {c["id"]: [] for c in containers}
    for el in elements:
        c = el.get("container")
        if c in child_map:
            child_map[c].append(el["id"])
    for c in containers:
        c["children"] = child_map[c["id"]]

    n_real = sample_size or 1000

    # Choose a sensible top-level name.
    if model_name is None:
        model_node = next((n for n in nodes if n.cls == "model"), None)
        model_name = (model_node.attrs.get("title", model_node.ident)
                      if model_node else "Converted Analytica model")

    model = {
        "wasim_version": "0.1.0",
        "source": {
            "generator": "ana_to_wasim.py",
            "notes": (
                f"Converted from Analytica (.ana): {model_name}. "
                "Analytica is a static Intelligent-Arrays + Monte-Carlo tool; "
                "WASiM is a time-stepping engine. This is a scalar/1-step "
                "translation of the arithmetic + probabilistic core. Constructs "
                "with no v0.1.0 equivalent are preserved as inert stubs — see "
                "the conversion warnings. Review before trusting numbers."
            ),
        },
        "simulation_settings": {
            "duration": {"value": 1.0, "unit": "1"},
            "timestep": {"value": 1.0, "unit": "1"},
            "n_realizations": n_real,
            "seed": 42,
        },
        "containers": containers,
        "elements": elements,
    }
    return model


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #

def main(argv: Optional[list[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="Convert an Analytica .ana model to WASiM model.json")
    ap.add_argument("input", help="path to the .ana file")
    ap.add_argument("-o", "--output", help="output model.json path (default: <input>.wasim.json)")
    ap.add_argument("--name", help="override the model name")
    ap.add_argument("--stdout", action="store_true", help="write JSON to stdout instead of a file")
    args = ap.parse_args(argv)

    with open(args.input, "r", encoding="utf-8", errors="replace") as f:
        text = f.read()

    model = convert(text, model_name=args.name)
    js = json.dumps(model, indent=2)

    if args.stdout:
        print(js)
    else:
        out = args.output or (args.input.rsplit(".", 1)[0] + ".wasim.json")
        with open(out, "w", encoding="utf-8") as f:
            f.write(js + "\n")
        print(f"Wrote {out}  ({len(model['elements'])} elements, "
              f"{len(model['containers'])} containers)", file=sys.stderr)

    if WARNINGS:
        print(f"\n{len(WARNINGS)} conversion warning(s):", file=sys.stderr)
        for w in WARNINGS:
            print("  - " + w, file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
