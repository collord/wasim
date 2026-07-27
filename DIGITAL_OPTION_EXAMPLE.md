# Digital (Binary) Options: A Discontinuous Payoff

**Model:** [`schema_examples_manual/digital_option.json`](schema_examples_manual/digital_option.json)
**Test:** [`engine/tests/digital_option_smoke.rs`](engine/tests/digital_option_smoke.rs)
**Scope / next step:** [`DIGITAL_OPTION_SCOPE.md`](DIGITAL_OPTION_SCOPE.md) (the Greeks gap this motivates)
**Companions:** [`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md) · [`BASKET_OPTION_EXAMPLE.md`](BASKET_OPTION_EXAMPLE.md).

A **digital (binary) option** pays a fixed amount if the underlying finishes past the strike, and
nothing otherwise. It is the canonical **discontinuous** payoff, and the teaching point is exactly that
discontinuity: the *price* is a smooth integral (Monte Carlo converges fine), while its *derivatives*
are not (which is why it motivates the Greeks work in `DIGITAL_OPTION_SCOPE.md`).

Like the [European efficiency example](OPTIONS_PRICING_EFFICIENCY.md), it is priced by **exact terminal**
simulation — one `N(0,1)` draw per realization, `S(T) = S₀·exp((r − σ²/2)T + σ√T·Z)` — so there is no
discretization bias and no time-stepping. It needs **no new engine feature**: a digital is a plain
expression (`gt` + `if`) on constructs that already exist.

---

## 1. The payoffs

| Payoff | Pays | Closed form |
|---|---|---|
| **Cash-or-nothing call** | `1` if `S(T) > K` | `disc·N(d₂)` |
| **Asset-or-nothing call** | `S(T)` if `S(T) > K` | `S₀·N(d₁)` |
| **Cash-or-nothing put** | `1` if `S(T) ≤ K` | `disc·N(−d₂)` |

with `d₁ = [ln(S₀/K) + (r + σ²/2)T]/(σ√T)`, `d₂ = d₁ − σ√T`, all live via `erf`.

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| One shock per realization | `Z` — `random_variable` `N(0,1)` |
| Exact terminal price | `ST = S₀·exp((r − σ²/2)T + σ√T·Z)` |
| In-the-money indicator | `ind = S(T) > K ? 1 : 0` (`gt` + `if`) |
| Cash / asset call | `cash_call = disc·ind`, `asset_call = disc·S(T)·ind` |
| Cash put + parity | `cash_put = disc·(1 − ind)`, `parity = cash_call + cash_put` |
| Closed forms | `cash_bs = disc·N(d₂)`, `asset_bs = S₀·N(d₁)` |

## 3. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `seed = 24680`, `N = 200 000`:

| quantity | value |
|---|---|
| **Cash-or-nothing call — closed form** | **0.5323** |
| Cash-or-nothing call — MC | 0.5323 — **matches** |
| **Asset-or-nothing call — closed form** | **63.68** |
| Asset-or-nothing call — MC | 63.65 — **matches** |
| **Cash parity `cash_call + cash_put`** | **`disc = 0.95123` exactly** (zero MC error) |

Three points:

1. **The discontinuous price converges fine.** The cash and asset digitals match `disc·N(d₂)` and
   `S₀·N(d₁)` to Monte Carlo error — the indicator's jump doesn't hurt the *expectation*, because a
   price is an integral (the jump is smoothed by integration).
2. **Cash parity is exact, per path.** `cash_call + cash_put = disc·ind + disc·(1 − ind) = disc` on
   *every* realization, so the reported mean equals `exp(−rT)` with **zero** Monte Carlo error — a
   nice deterministic identity that falls straight out of the `if`.
3. **This is where MC Greeks break.** The same discontinuity that the *price* shrugs off makes the
   *delta* hard: the pathwise estimator differentiates the payoff and hits a Dirac at `K`, and
   bump-and-revalue is noisy near the strike — the likelihood-ratio method is the clean answer. That
   is the substance of [`DIGITAL_OPTION_SCOPE.md`](DIGITAL_OPTION_SCOPE.md); this example is the setup.

## 4. Takeaway

A digital is a one-line payoff on constructs WASiM already has, and its price validates cleanly against
the closed form with an exact cash-parity identity for free. Its real value is as the motivating case
for Monte Carlo **sensitivities** — the one capability the option family here still lacks — scoped in
`DIGITAL_OPTION_SCOPE.md`.
