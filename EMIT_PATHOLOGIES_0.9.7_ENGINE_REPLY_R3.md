# Emit pathologies 0.9.7 — engine reply, round 3 (closeout)

**Scope.** Engine response to `EMIT_PATHOLOGIES_0.9.7_RESOLUTION_R3.md` (the on_off top-level lift).
This round is a **verification pass** — no engine code change was needed; the R2 handler + documented
cap default already cover everything. One schema-doc addition (R3 #5). **The controller subsystem is
now closed end-to-end.**

---

## #2 on_off — VERIFIED end-to-end against the regenerated corpus

The R3 lift landed exactly as reported. Verified against `~/openvsim/wasim/schema_examples`:

- **18 on_off controllers** carry top-level `controller_mode: "on_off"`; **17** carry `output_cap`,
  **18** carry `deadband_ref` — `unscheduledtimesteps` is the one without `output_cap`, matching R3.
- **The on_off handler activates and computes a real latched output** (not the old dead 0). Sampled
  controllers (`controlleronoff`, `comparecontrollers`, `deadband`) show the latch toggling —
  nonzero on some steps, zero on others, magnitude = the wired `output_cap` flow rate. e.g.
  `deadband/Outflow_Controller`: nonzero on 31/100 steps, max ≈ `output_cap`.
- **Corpus 220/220 parse + graph-build clean** (was the whole point of this thread — now fully
  resolved across pid/status/species/partition/on_off).

### The `unscheduledtimesteps` edge case (no `output_cap`) — handled, no engine change

The engine latch already defaults a missing `output_cap` to **1.0**, so the controller emits a
**0/1 gate signal** (documented in semantics §2.15). For a controller with genuinely no ON-value in
GoldSim, a normalized on/off signal is the sensible default — it makes the controller a functioning
gate a downstream model can scale, rather than a no-op. (This particular model also has
`duration: 0`, so it barely runs regardless.) No engine-side change warranted; the behavior is
defined and documented. If a future model needs a *physical* cap here, that's a re-gsm decode of an
alternate port, not an engine default question.

---

## #5 (partial) — schema now documents the controller fields

Added the full `[pid]` field set to `wasim-schema-v2.json`'s node definition — the pre-existing
fields (`setpoint`/`kp`/`ki`/`kd`/`output_min`/`output_max`/`deadband`, which had relied on
`additionalProperties: true`) **plus** the three new on_off fields:

- `controller_mode` — enum `pid`/`proportional`/`on_off`
- `output_cap` — `quantity_or_formula` (optional)
- `deadband_ref` — `quantity_or_formula` (optional)

All optional (no `required` block — the additions are documentary; the node stays
`additionalProperties: true`). Corpus re-validates clean. This closes the controller half of #5; the
broader "encode discriminated body requirements as `if`/`then`/`required`" is still a good structural
improvement for the whole node schema (owned in openvsim), tracked separately.

---

## Status of everything

| Item | State |
|---|---|
| **#1** PID gains numeric | Closed (re-gsm R2) |
| **#2** on_off controllers | **Closed end-to-end** — engine latch (R2) + emit lift (R3) + verified |
| **#3** event-driven status → on_event | Open — re-gsm event-link work; engine already consumes `on_event` |
| **#4** species-set as dimension | Parse: closed (220/220). Decay/per-nuclide half-life: deferred schema question |
| **#5** schema if/then | Controller fields now documented; general if/then still open (openvsim) |
| Pathology 1-4 (pid/status/species/partition-Kd) | All closed; corpus 220/220 parse+build |

**Still gated on emit** (not parse blockers, both long-standing): event-link → `on_event` binding
(#3), and cell mass-delivery decode + per-nuclide half-life for decay chains (#4). Neither blocks
parsing or running; both are tracked.

**The controller subsystem (pid / proportional / on_off) is correct end-to-end.** This closes the
0.9.7 emit-pathology thread on everything that was a parse blocker or a wrong-computation bug.

---

*Generated 2026-07-21, engine round-3 closeout to `EMIT_PATHOLOGIES_0.9.7_RESOLUTION_R3.md`. Corpus
re-verified at `~/openvsim/wasim/schema_examples` (220 files, 220/220 parse+build clean).*
