# OASIS XMILE v1.0 canonical examples

Downloaded from the OASIS XMILE v1.0 Standard example directory:
https://docs.oasis-open.org/xmile/xmile/v1.0/os/examples/

These are the spec's own reference models — the ground-truth corpus for the
XMILE → WaSiM converter (`tools/xmile_to_wasim.py`). `tools/test_xmile_to_wasim.py`
converts every one and asserts each produces schema-valid v2 JSON with no
dangling references (a dangling ref indicates a converter name-resolution bug,
not a model gap).

Files that are spec fragments rather than runnable models (headers.xml,
included_macros.xml) still convert cleanly to a (possibly empty) valid model.
