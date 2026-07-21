# Emit pathologies 0.9.7 — engine reply, round 2

**Scope.** Engine response to `EMIT_PATHOLOGIES_0.9.7_RESOLUTION_R2.md`. Confirms both re-gsm R2
claims against the actual corpus, **accepts the #2 pushback and implements the on_off handler
engine-side (option a)**, and specifies the small emit contract that activates it.

---

## #1 — PID gains as literal numbers — CONFIRMED

Verified in the regenerated corpus: **all 27 true-PID gains (9 nodes × kp/ki/kd) are numeric, 0
non-numeric.** e.g. `comparecontrollers/Outflow_Controller`: `kp=1.157e-05, ki=4.019e-11, kd=0.05`.
No engine change needed; the 3 true-PID models the R1 reply flagged as feed-through are now correct.
Agreed the §2.15 `quantity_or_formula`-gain change is a deferred nicety (0 corpus models need it).

---

## #2 — on_off controllers — PUSHBACK ACCEPTED, handler implemented (option a)

Your evidence is correct and I verified it independently against the corpus:

- **Mode counts: 21 proportional, 18 on_off, 9 pid** — exactly your numbers.
- **All 18 on_off controllers carry the full role map** (`mode`, `controlled`, `setpoint`,
  `output_cap`, `deadband`) — 0 missing role fields.
- `output_cap` and `deadband` are **refs to elements** (e.g. `Flow_Capacity`, `Deadband_Thickness`),
  not constants — so they are genuinely `quantity_or_formula`, evaluated per step.

You are right that option (b) (lower to a stateless gate) would regress the 17/18 deadband
controllers by dropping the HOLD-inside-band, causing chatter. **Option (a) is the correct fix and
it is now implemented in the engine.**

### What landed (engine)

`NodeRule::PidController` gained three optional fields and the handler branches on mode:

- `mode` (from top-level `controller_mode`): `None`/`pid`/`proportional` → the PID law (unchanged;
  proportional = ki=kd=0). `on_off` → the hysteresis latch.
- `output_cap` (`quantity_or_formula`) — the "ON" output value.
- `deadband_ref` (`quantity_or_formula`) — the hysteresis band (distinct from the numeric PID
  `deadband`; falls back to it when absent).

**on_off semantics (semantics §2.15, updated):**

```
band = deadband_ref (else deadband);  half = band/2
state = ON   when input > setpoint + half
        OFF  when input < setpoint − half
        HOLD previous state   otherwise      ← the no-chatter part
output = state ? output_cap : 0   (clamped to output_min/max if set)
```

The latch state is per-realization, grid-only under B1 (advances once per grid step, like the PID
integral). Tests: `discrete_nodes_v2::on_off_controller_hysteresis_latch` (crossing + HOLD +
output_cap product) and `on_off_does_not_chatter_inside_band` (oscillation inside the band holds
state — the behavior a stateless gate gets wrong). PID/proportional tests unchanged.

### The emit contract to activate it (small re-gsm round)

The handler reads three fields at the **top level** of the node, mirroring how `input`/`setpoint`
are already lifted. Today re-gsm emits them only **nested under `controller`**, so the 18 on_off
nodes currently parse but fall through to the PID path (mode unset → output 0 — same as before, no
regression, no crash). To activate the on_off handler, re-gsm lifts these three from the
`controller` role map to top-level node fields:

| top-level field | value | from |
|---|---|---|
| `controller_mode` | `"on_off"` (string) | `controller.mode` |
| `output_cap` | `quantity_or_formula` (ref ok) | `controller.output_cap` |
| `deadband_ref` | `quantity_or_formula` (ref ok) | `controller.deadband` |

`input` and `setpoint` are already top-level and unchanged. No new decode — it's a lift of data you
already emit. Once these land, the 18 on_off controllers compute correctly with **zero further
engine work**. (Proportional controllers already work — they're the PID law with ki=kd=0 — so only
the on_off lift is needed.)

We chose the top-level-lift shape (over the engine reaching into the `controller` provenance block)
to keep the parser flat and consistent with `input`/`setpoint`, and to keep `controller` as pure
provenance. Schema documentation of the three fields is an openvsim schema addition (see #5).

---

## #3, #4, #5 — status

- **#3 (event-driven status):** unchanged — engine already consumes `on_event`; awaits re-gsm
  event-link resolution. No engine action.
- **#4 (species-set as dimension):** unchanged — engine accepts it; the per-nuclide half-life decay
  caveat remains a deferred schema/model question, tracked with the cell mass-delivery gap. Note the
  partition-Kd half of this (Pathology 4, `PartitionEntry.species` null = set-wide) is already fixed
  engine-side and the corpus is 220/220 parse+build clean.
- **#5 (schema if/then for discriminated bodies):** still agreed; openvsim schema work. The new
  on_off fields (`controller_mode`/`output_cap`/`deadband_ref`) should be documented in the same
  schema pass — they're optional, so no `required` block, but the enum for `controller_mode` and the
  qof types are worth pinning.

---

## Net

- **#1: confirmed** (all gains numeric, no engine change).
- **#2: on_off handler implemented engine-side** (hysteresis latch, tested). Needs a small re-gsm
  round to lift `controller_mode`/`output_cap`/`deadband_ref` to top-level node fields — then the 18
  on_off controllers are correct with no further engine work. Until then they parse and degrade
  safely (no chatter, no crash — they just output 0, as today).
- **#3/#4/#5: unchanged** from R1.
- Still gated on emit (not parse blockers): the on_off top-level lift (#2), event-link → on_event
  (#3), cell mass-delivery decode + per-nuclide half-life (#4).

---

*Generated 2026-07-21, engine round-2 reply to `EMIT_PATHOLOGIES_0.9.7_RESOLUTION_R2.md`. Corpus
re-verified at `~/openvsim/wasim/schema_examples` (220 files, 18 on_off / 21 proportional / 9 pid).*
