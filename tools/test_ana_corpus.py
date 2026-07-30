#!/usr/bin/env python3
"""Corpus smoke-test for ana_to_wasim.py against real third-party .ana models.

Runs every file in tools/fixtures/ana_corpus/ through the converter and asserts
it parses without crashing and yields a structurally sane WASiM model. Where
`jsonschema` is installed, it also validates against oldmodel.schema.json and
checks that the set of schema-invalid conversions matches EXPECTED_INVALID (the
known converter bugs documented in ana_corpus/SOURCE.md) — no more, no fewer.

Run: python3 tools/test_ana_corpus.py     (stdlib; schema check needs jsonschema)
"""
import glob
import importlib.util
import json
import os
import sys

_here = os.path.dirname(os.path.abspath(__file__))
_root = os.path.dirname(_here)
_spec = importlib.util.spec_from_file_location(
    "ana_to_wasim", os.path.join(_here, "ana_to_wasim.py"))
A = importlib.util.module_from_spec(_spec)
sys.modules["ana_to_wasim"] = A
_spec.loader.exec_module(A)

CORPUS = os.path.join(_here, "fixtures", "ana_corpus")

# Real models whose conversion is schema-INVALID. Empty: every fixture here now
# validates. This set previously pinned two schema/engine-drift bugs the corpus
# surfaced (a `gather` op and the nullary `null()` call missing from
# oldmodel.schema.json's `call` node); both were fixed by syncing the schema
# with the engine's builtins. Keep it as a live regression tracker — if a new
# fixture converts to invalid output, the check below fails and names it.
EXPECTED_INVALID = set()

_fails = []


def check(name, cond):
    print(("ok  " if cond else "FAIL") + "  " + name)
    if not cond:
        _fails.append(name)


try:
    import jsonschema
    _schema = json.load(open(os.path.join(_root, "oldmodel.schema.json")))
except Exception:
    jsonschema = None
    _schema = None
    print("note: jsonschema not installed — schema validation skipped\n")

files = sorted(glob.glob(os.path.join(CORPUS, "*.ana")))
check("corpus is non-empty", len(files) > 0)

got_invalid = set()
for path in files:
    name = os.path.basename(path)
    text = open(path, encoding="utf-8", errors="replace").read()
    A.WARNINGS.clear()
    try:
        model = A.convert(text)
        crashed = None
    except Exception as e:  # noqa: BLE001 — the point is that it must not
        crashed = e
        model = None
    check(f"{name}: converts without crashing", crashed is None)
    if model is None:
        continue

    # Structural sanity: the emitted model is the expected shape.
    check(f"{name}: emits a well-formed model shell",
          isinstance(model.get("elements"), list)
          and "wasim_version" in model
          and "simulation_settings" in model)

    if jsonschema is not None:
        try:
            jsonschema.validate(model, _schema)
        except jsonschema.ValidationError:
            got_invalid.add(name)

if jsonschema is not None:
    check("schema-invalid set matches the known-bug list "
          f"(expected {sorted(EXPECTED_INVALID)}, got {sorted(got_invalid)})",
          got_invalid == EXPECTED_INVALID)

print()
if _fails:
    print(f"{len(_fails)} failure(s): {_fails}")
    sys.exit(1)
print("all passed")
