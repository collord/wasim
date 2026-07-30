#!/usr/bin/env python3
"""
ana_to_wasim.py — convert an Analytica model (`.ana`) into a WASiM model.json.

Usage:
    python3 tools/ana_to_wasim.py input.ana [-o output.json] [--name "Model name"]
    python3 tools/ana_to_wasim.py input.ana --stdout

The `.ana` format is plain UTF-8 text: a flat sequence of typed nodes, each
introduced by a class keyword header line (`Chance Ab_travel_time`) followed by
attribute lines (`Title:`, `Units:`, `Definition:`, ...). See
docs.analytica.com. This converter targets the WASiM **v2-native** model format
(`schema/wasim-schema-v2.json`) that the Rust engine parses: `primitive:"node"`
elements, top-level `dimensions[]` from Analytica Indexes, and `vector_map`
comprehensions for dimensioned arrays.

Scope. Analytica and WASiM are architecturally different (see
ANALYTICA_ENGINE_GAP_ANALYSIS.md): Analytica centres on Intelligent Arrays +
Monte-Carlo with an optional Time axis, while WASiM is a time-stepping engine
with arrays/sampling as substrate. This converter handles the arithmetic +
probabilistic core that maps cleanly — distributions, scalar arithmetic,
If-Then-Else, comparisons, the common reductions/math builtins, named
dimensions, `Table(I,J)`/`DetermTable(selector)` data, axis-selective reducers
(`Sum(x, Index)`), and label/positional subscript (`x[Dim='label']`, `x[I=3]`).
Constructs with no faithful WASiM equivalent (runtime-dynamic indices,
`Self[Time-1]` recurrence, sample-as-axis reductions mid-graph, user Functions,
metaprogramming, GUI logic) degrade to a preserved-but-inert stub
(`{"op":"literal","value":0.0}`, `source:"inferred"`, original text kept in
`display`) and are reported to stderr. The output validates against
`schema/wasim-schema-v2.json` and parses in the engine; unconverted definitions
are visible as warnings + inert stubs rather than silent wrong numbers.

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
    RANGE = ".."
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
    | (?P<range>\.\.)
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
        elif kind == "range":
            toks.append((Tok.RANGE, val))
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
    index: Optional[list[Any]] = None     # index-dimension args from a `Table(I…)(…)`
                                          # double-application (kept out of `pos`)


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
        left = self.parse_postfix(self.parse_prefix())
        # Analytica `a..b` inclusive integer range → materialized array literal
        # (index members: `2003..2005`, `1..12`). Lower binding than arithmetic.
        if self.peek()[0] == Tok.RANGE and min_bp <= 1:
            self.next()
            right = _as_ast(self.parse_expr(2))
            left = _range_literal(_as_ast(left), right)
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
        if kind == Tok.OP and val == "@":
            # Analytica ordinal `@Index` → the 1-based positions along that index.
            self.next()
            operand = _as_ast(self.parse_expr(11))
            return {"op": "call", "fn": "ordinal", "args": [operand]}
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

    def parse_postfix(self, node: Any) -> Any:
        # Analytica postfix subscript `x[Dim = rhs, Dim2 = rhs2, …]`. Each clause
        # is dispatched on its RHS:
        #   - string label  `x[Coords='Lat']`   → subscript(x, dim=Coords, label='Lat')
        #   - numeric literal `x[I=3]`           → index(x, [3])   (1-based positional)
        #   - index ref     `x[Dim=anIndex]`     → gather(x, anIndex)  (reindex, Phase 2)
        # Multiple comma-separated clauses chain left-to-right. An RHS shape we
        # don't model (arithmetic, `@I=…`, `Domain of …`) raises, so the whole
        # definition falls back to the existing stub path.
        while self.peek()[0] == Tok.LB:
            self.next()  # consume '['
            node = _as_ast(node)
            while True:
                if self.peek()[0] != Tok.IDENT:
                    raise ParseError("unsupported subscript (expected `Dim = …`)")
                dim = self.next()[1]  # dimension name (kept for label subscript)
                if not (self.peek()[0] == Tok.OP and self.peek()[1] == "="):
                    raise ParseError("unsupported subscript (expected `=`)")
                self.next()  # '='
                rhs = _as_ast(self.parse_expr(0))
                if isinstance(rhs, dict) and "_string" in rhs:
                    node = {"op": "subscript", "array": node,
                            "dim": dim, "label": rhs["_string"]}
                elif isinstance(rhs, dict) and rhs.get("op") == "literal" \
                        and "_stub_display" not in rhs:
                    node = {"op": "index", "array": node,
                            "indices": [{"op": "literal", "value": rhs["value"]}]}
                elif isinstance(rhs, dict) and rhs.get("op") == "ref":
                    node = {"op": "call", "fn": "gather", "args": [node, rhs]}
                else:
                    raise ParseError("unsupported subscript RHS")
                if self.peek()[0] == Tok.COMMA:
                    self.next()
                    continue
                break
            self.expect(Tok.RB)
        return node

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
            if val.lower() in ("null", "nan", "undefined"):
                # Analytica null / empty cell → the `null()` builtin (→ NaN). Marks
                # staircase/plot gaps and propagates through arithmetic (Phase 3).
                return {"op": "call", "fn": "null", "args": []}
            if val.lower() in _OPAQUE_IDENTS:
                # INF / Infinity / self — no finite JSON literal and no builtin; keep the
                # symbol in provenance and evaluate to an inert 0.0.
                return {"op": "literal", "value": 0.0, "_stub_display": val}
            if self.peek()[0] == Tok.LP:
                self.next()  # consume (
                pos, named = self.parse_call_args()
                self.expect(Tok.RP)
                call = Call(name=val, pos=pos, named=named)
                # Analytica double-application `Table(I…)(v1, v2, …)`: the first
                # parenthesis group names the index dimension(s), the second the cell
                # values. Keep them separate — folding the index args into the value
                # list (the old bug) leaked the index name in as a spurious element 0.
                while self.peek()[0] == Tok.LP:
                    self.next()
                    pos2, named2 = self.parse_call_args()
                    self.expect(Tok.RP)
                    call = Call(name=val, pos=pos2,
                                named={**call.named, **named2},
                                index=(call.index or []) + call.pos)
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

# Shape-preserving array→array ops (WASIM_ARRAY_LANGUAGE_SCOPE Phase 1). Like the
# reducers, an optional trailing index arg is dropped (`cumulate(x, I)` → `cumulate(x)`);
# unlike them, the result keeps the array's axis.
_ARRAY_MAP_OPS = {
    "sortindex": "sort_index", "sort": "sort_array", "rank": "rank_array",
    "cumulate": "cumulate", "cumproduct": "cumproduct",
}

_DISTRIBUTIONS = {
    "uniform", "normal", "lognormal", "triangular", "beta", "gamma",
    "exponential", "bernoulli", "weibull",
}

# Across-realization ("over Run") sample statistics. In WASiM these live in the
# results/analysis layer (A3, `results_spec`), NOT as a mid-graph value a
# downstream element can consume — the single genuine semantic gap called out in
# ANALYTICA_ENGINE_GAP_ANALYSIS.md §2. They have no faithful mid-graph AST form,
# so they degrade to an inert stub + warning.
_SAMPLE_STATS = {
    "probability", "probbands", "getfract", "cdf", "pdf", "sdeviation",
    "variance", "kurtosis", "skewness", "correlation", "frequency",
    "dynamic",
}


def _stub(display: str, node_hint: str = "") -> dict:
    """An inert placeholder for an unconvertible construct (kept for provenance)."""
    return {"op": "literal", "value": 0.0, "_stub_display": display}


def _reducer(fn: str, args: list[dict]) -> dict:
    """A reducer call `Sum(x, Index)` → `{op:call, fn, args:[x]}` carrying a
    `_reduce_axis` marker naming the reduced index (when args[1] is an index ref).

    The axis position can't be resolved until x's runtime axis ORDER is known
    (align-by-name re-sorts axes by id), so the shape-inference pass in `convert`
    later rewrites `_reduce_axis` → a 1-based position literal `args[1]`. With no
    named axis, the reducer stays a full collapse (single arg)."""
    node: dict[str, Any] = {"op": "call", "fn": fn, "args": args[:1] or args}
    if len(args) >= 2 and isinstance(args[1], dict) and args[1].get("op") == "ref":
        node["_reduce_axis"] = args[1]["element_id"]
    return node


def resolve_call(call: Call) -> dict:
    name = call.name.lower()
    args = [_as_ast(a) for a in call.pos]
    named = {k: _as_ast(v) for k, v in call.named.items()}

    # Min/Max: 2 positional numeric-ish args -> scalar builtin; else reducer.
    if name in ("min", "max"):
        if len(args) == 2 and not named:
            return {"op": "call", "fn": name, "args": args}
        # Max(x, I) reduction (I is an index ref) or Max(x)
        return _reducer(_ARRAY_REDUCERS[name], args)

    if name in _SCALAR_BUILTINS:
        return {"op": "call", "fn": _SCALAR_BUILTINS[name], "args": args}

    if name in _ARRAY_REDUCERS:
        # Sum(x, Index) -> sum_array(x, <axis pos>): keep WHICH axis is reduced (as
        # a `_reduce_axis` marker naming the index) so the shape-inference pass can
        # resolve it to a 1-based axis position against x's runtime axis order.
        if name in ("mean", "average") and len(args) == 1 and not named:
            warn(call.name, "Mean/Average of a single value maps to `mean_array`, "
                            "but Analytica's Mean over the Run sample is a results-layer "
                            "(A3) statistic in WASiM, not a mid-graph array mean — verify axis.")
        return _reducer(_ARRAY_REDUCERS[name], args)

    if name in _ARRAY_MAP_OPS:
        # `SortIndex(x)` / `cumulate(x, I)` etc. → array→array builtin; drop any
        # trailing index arg (the axis is implicit), keep the array's shape.
        return {"op": "call", "fn": _ARRAY_MAP_OPS[name], "args": args[:1] or args}

    if name in _SAMPLE_STATS:
        warn(call.name, "across-realization/dynamic statistic has no mid-graph "
                        "equivalent (see gap analysis §2); preserved as inert stub — "
                        "reconstruct in the results layer or as an accumulator.")
        return _stub(_render_call(call))

    if name in ("if", "ifonly"):
        # If(cond, then, else) functional form.
        if len(args) >= 3:
            return {"op": "if", "cond": args[0], "then": args[1], "else": args[2]}

    if name == "checkbox":
        # Checkbox(v) is a GUI boolean toggle; its value is v (0/1). Keep the
        # value, drop the GUI — faithful for a headless engine.
        return args[0] if args else {"op": "literal", "value": 0.0}

    if name == "choice":
        # Choice(index, default[, multi]) is a GUI selector; its value is the
        # chosen 1-based position (the `default`). The index arg is presentational.
        # NOTE: faithful `DetermTable(thisVar)(…)` selection needs the `domain`
        # self-index + member-value-vs-position semantics — deferred to a later
        # stage; here `default` is passed through unchanged.
        if len(args) >= 2:
            return args[1]
        return {"op": "literal", "value": 1.0}

    if name in ("table", "determtable", "probtable"):
        # `Table(I…)(v1..vn)` / `DetermTable(I…)(…)`: a dimensioned constant. The
        # index dimension refs live in `call.index` (the first paren group), the
        # row-major cell values in `call.pos` (→ `args`). Emit a `_table` marker
        # carrying both; the v2 emit layer materializes it as nested `vector_map`
        # over the named indexes (Stage 1: 1-D; Stage 2: N-D). Fall back to a bare
        # array literal when no index dimensions were given.
        index_ids = [ix.get("element_id") for ix in (call.index or [])
                     if isinstance(ix, dict) and ix.get("op") == "ref"]
        if index_ids and all(index_ids):
            return {"op": "_table", "dims": index_ids,
                    "elements": [_as_ast(a) for a in args]}
        return {"op": "array", "elements": args}

    if name in ("array", "subscript", "slice"):
        # Array/subscript/slice constructors collapse to an array literal of values.
        return {"op": "array", "elements": args}

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


def _range_literal(lo: dict, hi: dict) -> dict:
    """Analytica `a..b` → inclusive integer array literal [a, a+1, …, b].

    Falls back to a 2-element array of the endpoints if either bound isn't a
    constant number (best-effort; keeps the definition convertible)."""
    if lo.get("op") == "literal" and hi.get("op") == "literal":
        a, b = lo["value"], hi["value"]
        if b >= a:
            vals = [{"op": "literal", "value": float(k)}
                    for k in range(int(a), int(b) + 1)]
            return {"op": "array", "elements": vals}
    return {"op": "array", "elements": [lo, hi]}


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


WASIM_VERSION = "0.9.8"

# --------------------------------------------------------------------------- #
# Stage 5 — v2-native emit: dimensions + vector_map + dimensioned nodes
# --------------------------------------------------------------------------- #
#
# The engine's ONLY node that mints a dimension-tagged NamedArray is `vector_map`
# (via `over`). A constant `array` literal is anonymous (untagged), so a
# `Table(I,J)` must emit as nested `vector_map over I (vector_map over J (index
# into a flat row-major values vector)))` to acquire named axes that downstream
# axis-`sum_array` / label-`subscript` / align-by-name can see.


def collect_dimensions(nodes: list["AnaNode"]) -> dict[str, dict]:
    """Every Analytica Index whose members are constant → a v2 `dimension_def`.

    Returns `{id: {"id","name","size","labels","values"}}`. `labels` are the
    stringified members; `values` are the numeric members (for `@I` / member
    arithmetic, materialized lazily by the emit layer)."""
    dims: dict[str, dict] = {}
    for n in nodes:
        if n.cls != "index":
            continue
        definition = n.attrs.get("definition", "").strip()
        parsed, _ = parse_definition(definition) if definition else (None, "empty")
        if parsed is None:
            continue
        ast = _as_ast(parsed)
        if ast.get("op") != "array":
            continue
        values: list[float] = []
        labels: list[str] = []
        for i, e in enumerate(ast["elements"], start=1):
            if isinstance(e, dict) and "_string" in e:
                values.append(float(i))
                labels.append(e["_string"])
            else:
                v = _param_value(e)
                if v is not None:
                    values.append(v)
                    labels.append(_num(v))
        if not values:
            continue
        name = n.attrs.get("title", n.ident).strip() or n.ident
        dims[n.ident] = {"id": n.ident, "name": name, "size": len(values),
                         "labels": labels, "values": values}
    return dims


def collect_selectors(nodes: list["AnaNode"]) -> dict[str, dict]:
    """Variables defined `Choice(Self, k[, …])` with a `domain` self-index.

    Analytica `Choice(Self, k)` selects the domain member at 0-based UI position
    `k`; its 1-based position is `k+1`. Such a variable serves two roles:
      - numeric use (`D-0.5`) → its selected MEMBER VALUE `domain[k]` (numeric
        domains only; a label domain has no numeric value → falls back to k+1);
      - `DetermTable(thisVar)(…)` index → the domain POSITION `k+1`.

    Returns `{ident: {"pos": k+1, "value": <member value or k+1>, "size": n}}`.
    Only constant `Choice(Self, k)` selections are captured (the corpus case)."""
    out: dict[str, dict] = {}
    for n in nodes:
        definition = n.attrs.get("definition", "").strip()
        domain = n.attrs.get("domain", "").strip()
        if not domain or not re.match(r"(?i)\s*choice\s*\(", definition):
            continue
        parsed, _ = parse_definition(definition)
        if not isinstance(parsed, Call) or parsed.name.lower() != "choice":
            continue
        # Choice(Self, default[, multi]); default is the 0-based UI position.
        if len(parsed.pos) < 2:
            continue
        k = _param_value(_as_ast(parsed.pos[1]))
        if k is None:
            continue
        k = int(k)
        dparsed, _ = parse_definition(domain)
        dast = _as_ast(dparsed) if dparsed else None
        members: list[dict] = dast.get("elements", []) if isinstance(dast, dict) and dast.get("op") == "array" else []
        pos = k + 1  # 1-based position of the selected member
        # numeric member value if the domain is numeric, else the position
        val = float(pos)
        if 0 <= k < len(members):
            mv = _param_value(members[k])
            if mv is not None:
                val = mv
        out[n.ident] = {"pos": pos, "value": val, "size": len(members)}
    return out


def _index_at(depth: int) -> dict:
    """The 1-based iteration index of the `depth`-th enclosing `vector_map`
    (0 = innermost)."""
    return {"op": "index_ref", "depth": depth}


def _flat_index_expr(dim_ids: list[str], sizes: list[int]) -> dict:
    """1-based flat row-major offset into a Table(I,J,…) values vector, from the
    enclosing vector_map iteration indices. For dims [I,J,…] (I outermost), the
    innermost `index` selects `((iI-1)*nJ + (iJ-1))*nK + … + iLast`.

    `depth` maps to nesting: the LAST dim is innermost (depth 0), the FIRST is
    outermost (depth len-1)."""
    n = len(dim_ids)
    # i_k is index_ref at depth (n-1-k); accumulate row-major with 1-based result.
    expr: dict = {"op": "literal", "value": 0.0}
    for k in range(n):
        depth = n - 1 - k
        ik = _index_at(depth)
        if k == 0:
            # (i0 - 1)
            expr = {"op": "subtract", "left": ik, "right": {"op": "literal", "value": 1.0}}
        else:
            # expr * n_k + (i_k - 1)
            expr = {"op": "add",
                    "left": {"op": "multiply",
                             "left": expr,
                             "right": {"op": "literal", "value": float(sizes[k])}},
                    "right": {"op": "subtract", "left": ik,
                              "right": {"op": "literal", "value": 1.0}}}
    # convert 0-based accumulator to the 1-based `index` selector
    return {"op": "add", "left": expr, "right": {"op": "literal", "value": 1.0}}


def materialize_table(table: dict, dims: dict[str, dict],
                      selectors: dict[str, dict]) -> Optional[dict]:
    """A `_table` marker → an engine-tagged array or a scalar lookup:

      - all index args are DECLARED dimensions → nested `vector_map` over them
        (mints a NamedArray with named axes; the shape align-by-name/reducers need);
      - all index args are Choice/`domain` SELECTORS → a scalar `get_element` into
        the flat row-major values at the selectors' combined 1-based position
        (`DetermTable(D, Grade)(…)`).

    Returns None (caller falls back to a bare array literal) when the shape can't
    be reconstructed (unknown index, value-count mismatch, or a mix of the two)."""
    dim_ids = table.get("dims", [])
    values = table.get("elements", [])
    if not dim_ids:
        return None

    # Selector-indexed DetermTable → flat row-major get_element at the combined
    # 1-based position (all selectors are constant, so the position is static).
    if all(d in selectors for d in dim_ids):
        sizes = [selectors[d]["size"] for d in dim_ids]
        offset = 0
        for k, d in enumerate(dim_ids):
            offset = offset * sizes[k] + (selectors[d]["pos"] - 1)
        expect = 1
        for s in sizes:
            expect *= s
        if expect and len(values) != expect:
            return None
        return {"op": "call", "fn": "get_element",
                "args": [{"op": "array", "elements": values},
                         {"op": "literal", "value": float(offset + 1)}]}

    if any(d not in dims for d in dim_ids):
        return None
    sizes = [dims[d]["size"] for d in dim_ids]
    expect = 1
    for s in sizes:
        expect *= s
    if len(values) != expect:
        return None
    body = {"op": "index",
            "array": {"op": "array", "elements": values},
            "indices": [_flat_index_expr(dim_ids, sizes)]}
    for dim_id in reversed(dim_ids):
        body = {"op": "vector_map", "over": dim_id, "body": body}
    return body


def _lower_tables(ast: Any, dims: dict[str, dict],
                  selectors: dict[str, dict]) -> tuple[Any, list[str]]:
    """Recursively turn `_table` markers into `vector_map` chains; return the
    rewritten AST and the source-declared dim ids of any TOP-LEVEL table (used
    to tag `outputs[].dimensions`). Non-top-level tables still lower correctly;
    only the outermost one determines the element's declared dimensions."""
    top_dims: list[str] = []
    if isinstance(ast, dict):
        if ast.get("op") == "_table":
            lowered = materialize_table(ast, dims, selectors)
            if lowered is not None:
                # Only DECLARED dimensions become output axes; a selector lookup
                # collapses to a scalar (no axes).
                top_dims = [d for d in ast["dims"] if d in dims]
                return lowered, top_dims
            return {"op": "array", "elements": ast.get("elements", [])}, []
        out = {}
        for k, v in ast.items():
            nv, td = _lower_tables(v, dims, selectors)
            out[k] = nv
            if td and not top_dims:
                top_dims = td
        return out, top_dims
    if isinstance(ast, list):
        out_list = []
        for v in ast:
            nv, _ = _lower_tables(v, dims, selectors)
            out_list.append(nv)
        return out_list, top_dims
    return ast, top_dims


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


def build_element(node: AnaNode, container: Optional[str],
                  dims: dict[str, dict], selectors: dict[str, dict]) -> Optional[dict]:
    """Emit ONE v2-native element (`primitive:"node"`).

    `dims` is the model's collected dimensions (see `collect_dimensions`); it lets
    `_table` markers lower to `vector_map`. `selectors` maps Choice/`domain`
    variables to their selected member value/position (see `collect_selectors`),
    so a `Choice(Self,k)` var emits its member value and `DetermTable(thisVar)(…)`
    resolves to a `get_element` lookup."""
    ident = node.ident
    name = node.attrs.get("title", ident).strip() or ident
    units = node.attrs.get("units", "").strip() or "1"
    desc = node.attrs.get("description", "").strip() or None
    definition = node.attrs.get("definition", "").strip()

    base: dict[str, Any] = {
        "id": ident,
        "name": name,
        "primitive": "node",
    }
    if container is not None:
        base["container"] = container
    if desc is not None:
        base["description"] = desc

    # An Index becomes a top-level dimension_def (handled in `convert`). Here it
    # emits its numeric-member CARRIER node so `@I` / member arithmetic resolve.
    if node.cls == "index":
        return _build_index_carrier(node, base, units, dims, selectors)

    # A Choice(Self,k) selector with a `domain`: emit its selected MEMBER VALUE as
    # a fixed constant (numeric use). `DetermTable(thisVar)(…)` separately resolves
    # via the selector's position (see materialize_table).
    if node.ident in selectors:
        base.update({"value_rule": "fixed", "value": _q(selectors[node.ident]["value"], units)})
        return base

    if not definition:
        # No formula: a bare input. Decisions/Constants with a numeric Value.
        val = node.attrs.get("value", "").strip()
        num = _try_number(val) if val else None
        base.update({"value_rule": "fixed", "value": _q(num if num is not None else 0.0, units)})
        if num is None and node.cls not in ("constant", "decision"):
            warn(ident, "no Definition — emitted as fixed 0.")
        return base

    parsed, err = parse_definition(definition)
    if err is not None or parsed is None:
        warn(ident, f"could not parse Definition ({err}); emitted inert stub. "
                    f"Raw: {definition[:120]}")
        base.update({
            "value_rule": "expression",
            "expression": {"ast": {"op": "literal", "value": 0.0},
                           "display": definition, "source": "inferred"},
        })
        return base

    # Top-level distribution -> sample node (Chance / uncertain Variable).
    if isinstance(parsed, Call) and parsed.name.lower() in _DISTRIBUTIONS:
        dist = call_to_distribution(parsed, units)
        if dist is not None:
            base.update({
                "value_rule": "sample",
                "distribution": dist,
                "save_results": {"final_value": True, "time_history": False},
            })
            if units != "1":
                base["outputs"] = [{"name": "value", "unit": units}]
            return base

    ast = _as_ast(parsed)
    ast, stubs = strip_stub_markers(ast)
    for s in stubs:
        warn(ident, f"unconvertible sub-expression preserved as inert stub: {s}")

    # Lower any `_table` markers to nested vector_map; capture the top-level
    # table's dims so we can tag `outputs[].dimensions`.
    ast, table_dims = _lower_tables(ast, dims, selectors)

    # A genuine bare number -> fixed constant (but NOT a formula that merely
    # stubbed to 0.0 — those stay expressions so the original text survives).
    if (ast.get("op") == "literal" and not stubs and not table_dims
            and node.cls in ("decision", "constant", "variable", "determ")):
        base.update({"value_rule": "fixed", "value": _q(ast["value"], units)})
        return base

    inputs = sorted(_collect_refs(ast))
    base.update({
        "value_rule": "expression",
        "expression": {
            "ast": ast,
            "display": definition if stubs else _render_ast(_as_ast(parsed)),
            "source": "explicit" if not stubs else "inferred",
        },
    })
    if inputs:
        base["inputs"] = inputs
    out: dict[str, Any] = {"name": "value", "unit": units}
    if table_dims:
        out["dimensions"] = table_dims
    if table_dims or units != "1":
        base["outputs"] = [out]
    return base


def _build_index_carrier(node: AnaNode, base: dict, units: str,
                         dims: dict[str, dict], selectors: dict[str, dict]) -> Optional[dict]:
    """A constant-member Index → a dimensioned node carrying its member VALUES
    (so `@I` ordinals and member arithmetic resolve). The dimension_def itself is
    emitted separately in `convert`. A dynamic/label-only index with no numeric
    members degrades to the Phase-4 "demote to computed expression" fallback."""
    dim = dims.get(node.ident)
    definition = node.attrs.get("definition", "").strip()
    if dim is not None:
        # vector_map over I (index into [members] at index_ref depth 0)
        vals = [{"op": "literal", "value": v} for v in dim["values"]]
        body = {"op": "index", "array": {"op": "array", "elements": vals},
                "indices": [_index_at(0)]}
        base.update({
            "value_rule": "expression",
            "expression": {"ast": {"op": "vector_map", "over": node.ident, "body": body},
                           "display": definition, "source": "explicit"},
            "outputs": [{"name": "value", "unit": units, "dimensions": [node.ident]}],
        })
        return base

    # No constant members: demote to a computed expression variable if convertible.
    parsed, _ = parse_definition(definition) if definition else (None, "empty")
    if parsed is not None:
        ast = _as_ast(parsed)
        cleaned, stubs = strip_stub_markers(ast)
        if not stubs:
            ast, _td = _lower_tables(ast, dims, selectors)
            base.update({
                "value_rule": "expression",
                "expression": {"ast": ast, "display": definition, "source": "explicit"},
            })
            refs = sorted(_collect_refs(cleaned))
            if refs:
                base["inputs"] = refs
            return base
    warn(node.ident, "Index has no constant members and its definition is not "
                     "convertible (dynamic/label index); emitted inert scalar stub.")
    base.update({
        "value_rule": "expression",
        "expression": {"ast": {"op": "literal", "value": 0.0},
                       "display": definition or "(empty index)", "source": "inferred"},
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
        expr = e.get("expression")
        if isinstance(expr, dict) and "ast" in expr:
            expr["ast"] = fix(expr["ast"])
        if "inputs" in e:
            kept = [i for i in e["inputs"] if i in ids]
            if kept:
                e["inputs"] = kept
            else:
                del e["inputs"]

    if missing:
        warn("graph", f"{len(missing)} reference(s) to non-model ids "
                      f"(system indices / skipped nodes) replaced with inert 0.0: "
                      f"{', '.join(sorted(missing)[:8])}"
                      f"{' …' if len(missing) > 8 else ''}")


def _infer_shapes(elements: list[dict], dims: dict[str, dict]) -> None:
    """Compute each element's RUNTIME axis-id list and, in place:
      (a) tag `outputs[].dimensions` for dimensioned expression results, and
      (b) rewrite reducer `_reduce_axis` markers to 1-based axis positions.

    Mirrors the engine EXACTLY (WASIM_NAMEDARRAY_DESIGN / eval.rs):
      - A `vector_map over I (…)` result carries `I` as its OUTERMOST axis.
      - An elementwise op over ≥2 dimensioned operands unions their axes and
        RE-SORTS by dimension id (align-by-name canonical order, `broadcast_named`).
      - A reducer removes its named axis; the reduced axis's 1-based POSITION is
        computed against the input's runtime order (post align-by-name sort).

    `ELEM_DIMS[id]` = the ordered dim-id list an element's value carries. Computed
    by walking each element's AST bottom-up; refs resolve via `ELEM_DIMS` in the
    element order (Analytica's constant/expression lane is acyclic and mostly
    topologically ordered; a not-yet-known ref contributes no axes — a safe under-
    approximation that self-corrects on a second pass)."""
    elem_dims: dict[str, list[str]] = {}

    def axes_of(node: Any) -> list[str]:
        """The runtime axis-id list an AST node evaluates to (order matters)."""
        if not isinstance(node, dict):
            return []
        op = node.get("op")
        if op == "ref":
            return list(elem_dims.get(node.get("element_id"), []))
        if op == "vector_map":
            inner = axes_of(node.get("body"))
            over = node.get("over")
            return [over] + [a for a in inner if a != over]
        if op == "call":
            fn = node.get("fn", "")
            cargs = node.get("args", [])
            if fn.endswith("_array") or fn in ("gather", "ordinal", "cumulate",
                                               "cumproduct", "sort_array", "sort_index",
                                               "rank_array"):
                base = axes_of(cargs[0]) if cargs else []
                # a reducer with a resolved/known axis drops it (case-insensitive:
                # Analytica identifiers fold case, dimension ids are exact)
                red = node.get("_reduce_axis")
                if fn in ("sum_array", "mean_array", "min_array", "max_array",
                          "argmin_array", "argmax_array") and red is not None \
                        and red.lower() in [a.lower() for a in base]:
                    return [a for a in base if a.lower() != red.lower()]
                if fn in ("sum_array", "mean_array", "min_array", "max_array",
                          "argmin_array", "argmax_array", "size_array", "get_element",
                          "dot_product") and red is None:
                    return []  # full collapse → scalar
                return base
            # scalar/elementwise builtin: union of arg axes (sorted by id)
            return _sorted_union(axes_of(a) for a in cargs)
        if op in ("add", "subtract", "multiply", "divide", "pow", "mod",
                  "lt", "gt", "lte", "gte", "eq", "neq", "and", "or"):
            return _sorted_union([axes_of(node.get("left")), axes_of(node.get("right"))])
        if op in ("neg", "not"):
            return axes_of(node.get("operand"))
        if op == "if":
            return _sorted_union([axes_of(node.get("cond")), axes_of(node.get("then")),
                                  axes_of(node.get("else"))])
        if op == "index":
            # positional index into an array literal → scalar (Table cell selection)
            return []
        if op == "subscript":
            base = axes_of(node.get("array"))
            return [a for a in base if a != node.get("dim")]
        return []

    def resolve_reduce_axes(node: Any) -> None:
        """Rewrite `_reduce_axis` markers to a 1-based position literal arg.

        Analytica identifiers are case-insensitive, so the reduced-axis name
        (`month`) is matched against the array's axis ids (`Month`) case-folded."""
        if isinstance(node, dict):
            red = node.get("_reduce_axis")
            if red is not None and node.get("op") == "call":
                base = axes_of(node["args"][0]) if node.get("args") else []
                fold = [a.lower() for a in base]
                if red.lower() in fold:
                    pos = fold.index(red.lower()) + 1
                    node["args"] = [node["args"][0], {"op": "literal", "value": float(pos)}]
                del node["_reduce_axis"]
            for v in node.values():
                resolve_reduce_axes(v)
        elif isinstance(node, list):
            for v in node:
                resolve_reduce_axes(v)

    def elem_ast(e: dict) -> Any:
        expr = e.get("expression")
        return expr.get("ast") if isinstance(expr, dict) else None

    # Two passes over element order so forward refs converge (acyclic lane).
    for _ in range(2):
        for e in elements:
            ast = elem_ast(e)
            if ast is None:
                # fixed/sample: dims from its declared output, if any
                out = (e.get("outputs") or [{}])[0]
                elem_dims[e["id"]] = list(out.get("dimensions", []))
                continue
            elem_dims[e["id"]] = axes_of(ast)

    # Apply: resolve reducer axes, then tag outputs[].dimensions.
    for e in elements:
        ast = elem_ast(e)
        if ast is None:
            continue
        resolve_reduce_axes(ast)
        ed = elem_dims.get(e["id"], [])
        if ed:
            outs = e.get("outputs")
            if not outs:
                unit = "1"
                e["outputs"] = [{"name": "value", "unit": unit, "dimensions": ed}]
            else:
                outs[0]["dimensions"] = ed


def _sorted_union(axis_lists) -> list[str]:
    """Union of several axis-id lists, sorted by id — the engine's align-by-name
    canonical order (`broadcast_named`, BTreeMap key order)."""
    s: set[str] = set()
    for lst in axis_lists:
        s.update(lst)
    return sorted(s)


def convert(text: str, model_name: Optional[str] = None) -> dict:
    nodes, sample_size = lex_ana(text)

    # Pass A — collect dimensions from Index nodes and Choice/domain selectors.
    dims = collect_dimensions(nodes)
    selectors = collect_selectors(nodes)

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

    # Pass B — emit v2-native elements.
    elements: list[dict] = []
    for n in nodes:
        if n.cls in SKIP_CLASSES:
            continue
        if n.cls == "function":
            warn(n.ident, "Analytica user Function — no v2 element equivalent; skipped.")
            continue
        # A node named like a system index isn't a model element.
        if n.ident.lower() in ("samplesize", "sampesize", "sample_size", "time", "run"):
            continue
        container = n.parent if n.parent in module_ids else None
        try:
            el = build_element(n, container, dims, selectors)
        except Exception as e:  # noqa: BLE001
            warn(n.ident, f"internal error building element: {e}; skipped.")
            el = None
        if el is not None:
            elements.append(el)

    _sanitize_dangling_refs(elements)

    # Pass C — infer runtime array shapes: tag dimensioned expression outputs and
    # resolve reducer axis positions (must run after refs are settled).
    _infer_shapes(elements, dims)

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

    model: dict[str, Any] = {
        "wasim_version": WASIM_VERSION,
        "source": {
            "generator": "ana_to_wasim.py",
            "notes": (
                f"Converted from Analytica (.ana): {model_name}. "
                "Analytica is a static Intelligent-Arrays + Monte-Carlo tool; "
                "WASiM is a time-stepping engine. This is a v2-native "
                "translation of the arithmetic + probabilistic core, with named "
                "dimensions from Analytica Indexes. Constructs with no WASiM "
                "equivalent are preserved as inert stubs — see the conversion "
                "warnings. Review before trusting numbers."
            ),
        },
        "simulation_settings": {
            "duration": {"value": 1.0, "unit": "1"},
            "timestep": {"value": 1.0, "unit": "1"},
            "n_realizations": n_real,
            "seed": 42,
        },
        "elements": elements,
    }
    if dims:
        model["dimensions"] = [
            {"id": d["id"], "name": d["name"], "size": d["size"],
             **({"labels": d["labels"]} if d["labels"] else {})}
            for d in dims.values()
        ]
    if containers:
        model["containers"] = containers
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
