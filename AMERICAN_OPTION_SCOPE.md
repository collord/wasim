# American / Bermudan Options — Least-Squares Monte Carlo (Longstaff–Schwartz)

**Status:** **Phases 1–4 built** ([`AMERICAN_OPTION_EXAMPLE.md`](AMERICAN_OPTION_EXAMPLE.md),
[`AMERICAN_MAX_CALL_EXAMPLE.md`](AMERICAN_MAX_CALL_EXAMPLE.md)).
- **Phase 1 — `lsm` node:** post-run backward induction over the stored `time_history` panel,
  ITM-filtered covariance regression, per-realization cashflow injection.
- **Phase 2 — out-of-sample cross-fit** (`folds` on `lsm`): fit the policy on the complementary folds
  and price each fold under it → a nearly unbiased **lower** bound (removes the in-sample bias).
- **Phase 3 — `lsm_dual` node:** the dual **upper** bound with the underlying as the hedging martingale
  (`M_t = θ·(discᵗ·S_t − S_0)`; `discᵗ·S_t` is a true martingale, so valid for any `θ`). Together they
  **bracket** the true price: OOS 6.015 ≤ binomial 6.045 ≤ dual 9.704.
- **Phase 4 — multi-asset state** (`states` list on `lsm`): the continuation regression uses a
  **multivariate monomial basis** over several state panels (total degree ≤ `basis`). Prices an
  **American max-call on two correlated dividend-paying assets** — the first multi-asset early-exercise
  option — against a Boyle–Evnine–Gibbs 2-asset binomial lattice (OOS 9.35 ≤ lattice 9.56, a valid
  lower bound). A single-asset model (empty `states`) is byte-identical to before.

The dual is rigorous but **loose** (one hedge can't replicate the option); a *tight* upper bound needs
the nested-simulation Andersen–Broadie martingale — the one remaining piece (a non-nested regression
martingale was tried and collapses to the primal; not shipped). See §8 for the full tight-dual
investigation. Richer/orthogonal bases also remain, below.
**Motivation:** completes the Glasserman arc — the only major class left is **optimal stopping**
(§8), the hardest and most valuable Monte Carlo capability. This was the one candidate needing a
genuinely new engine capability (a **backward, time-recursive, cross-path regression**); the enablers
turned out to be already present (`run_regress`'s solver + `hist_store`'s panel), so it landed as a
post-run backward analogue of the terminal two-pass.

---

## 1. The problem

An American (or Bermudan) option can be exercised at any time (or on a set of dates) before `T`; its
price is a **supremum over stopping times** `sup_τ E[disc(τ)·payoff(S_τ)]`. There is no forward-payoff
formula — you must decide, at each date and on each path, whether to exercise now or continue, and that
decision depends on the *conditional expectation of continuing*, which is what makes it hard for Monte
Carlo (a forward simulation only knows the future in hindsight).

**Longstaff–Schwartz (LSM)** solves it by backward induction with regression:

```
at maturity:            V_T = payoff(S_T)                       on every path
for each earlier date t (backward):
    among IN-THE-MONEY paths, regress the discounted future value V_{t+1}
        on basis functions of the state:   E[V | S_t] ≈ Σ_k β_k φ_k(S_t)
    exercise where immediate payoff > fitted continuation; else carry V forward
price = mean over paths of the discounted value at each path's optimal stopping time
```

---

## 2. Why the engine can't express this today (verified)

Two structural facts:

- **The run loop is forward and per-realization.** `engine_v2` folds one realization at a time; a
  realization never sees the others *during* the run. Cross-path information is available only in the
  **two-pass** reduction, which fires **once, at the terminal** (`run_stat` / `run_stat2` / `run_regress`
  all reduce over *final* values). LSM needs cross-path regression at **every exercise date**, and it
  needs to run **backward**.
- **`run_regress` is exactly the right primitive — but terminal-only.** `reduce_run_regress(finals(y),
  control_cols, …)` already does a least-squares fit of one element's finals on control elements' finals
  across the Run axis. LSM is that same fit, done at each date on each date's state, recursively — i.e.
  `run_regress` generalized from "the terminal" to "a backward sweep over the grid."

There is, however, a crucial enabler already present: **`time_history` stores the full path panel.**
`hist_store[id]` is `[step][realization]` — so after a forward run, the complete `[dates × paths]` matrix
of any saved state variable (and immediate payoff) is in hand. LSM does **not** need a new *forward*
mechanism; it needs a new **post-run backward pass** over that stored panel. That reframes the gap from
"a new simulator" to "a backward analogue of the existing terminal two-pass."

## 3. Design — an LSM post-run construct

A new element/mode, e.g. `least_squares_mc`, declaring:

- **`exercise_payoff`** — the immediate-exercise value as a per-step expression (e.g. `(K − S)⁺` for an
  American put);
- **`state`** — the regression state variable(s) (usually `S`; the basis is built from these);
- **`basis`** — polynomial degree (default) or an explicit list of basis expressions `φ_k(state)`;
- **`exercise_dates`** — all grid steps (American limit) or a subset (Bermudan);
- **`discount`** — per-step discount factor.

**Execution (post-run, reusing existing pieces).** After the normal forward run has populated
`hist_store` for `state` and `exercise_payoff`:

1. Initialize `V = exercise_payoff` at the last exercise date, per path.
2. Walk dates **backward**. At each date `t`: select in-the-money paths; build the basis matrix `Φ` from
   `state[t]`; solve `β = (ΦᵀΦ)⁻¹ Φᵀ (disc·V)` with the **existing regression solver**
   (`solve_linear_or_zero`, already used by `reduce_run_regress`); set `continuation = Φβ`; on paths where
   `exercise_payoff[t] > continuation`, set `V ← exercise_payoff[t]` (exercise), else `V ← disc·V` (carry).
3. Price `= mean(disc·V)` at `t₀`.

This is a **backward K-pass** analogue of the forward two-pass — same cross-path regression math, same
stored panel, applied recursively over the grid instead of once at the end. No change to the forward
loop.

## 4. Known subtleties (must be designed in, not discovered)

- **In-sample low bias.** Using the same paths to fit the policy *and* price it biases the estimate
  **low** (the fitted policy is sub-optimal). Standard fixes: (a) report it as a lower bound; (b) fit on
  one path set, price on an independent set (out-of-sample); (c) add the **Andersen–Broadie dual upper
  bound** for a genuine confidence interval bracketing the true price. Phase the dual in later.
- **Basis conditioning.** `ΦᵀΦ` can be ill-conditioned (raw monomials of `S`); use scaled/orthogonal
  bases (Laguerre/Chebyshev in a normalized state) and the existing solver's zero-fallback for rank
  deficiency.
- **In-the-money filtering.** Regress only on ITM paths (out-of-the-money continuation is irrelevant and
  adds noise) — Longstaff–Schwartz's key variance point.
- **Memory.** Storing `state` + `payoff` histories is `O(dates × paths)` — exactly what `time_history`
  already allocates; large runs may need a state-only panel rather than saving every element.

## 5. Phasing

| Phase | Deliverable |
|---|---|
| **1** | Bermudan put, coarse date set, polynomial basis in `S`, backward pass over `hist_store`; validate against a **binomial tree** (or a published LSM benchmark, e.g. Longstaff–Schwartz Table 1). Low-biased point estimate. |
| **2** | American limit (all grid dates), scaled/orthogonal basis, ITM filtering; out-of-sample pricing to remove in-sample bias. |
| **3** | Andersen–Broadie **dual upper bound** → a true confidence interval `[LSM_low, dual_high]` around the price. |
| **4** ✅ | Multi-factor state — an American **max-call on two correlated assets**; a multivariate monomial basis over both state panels, validated against a Boyle–Evnine–Gibbs 2-asset lattice. Reuses the basket correlated-process work. |

## 6. Test plan

- **Convergence:** LSM price → binomial-tree American put value as paths/dates increase; the American
  price ≥ its European counterpart (early-exercise premium ≥ 0) and ≥ immediate payoff.
- **Bias direction:** in-sample LSM ≤ out-of-sample LSM ≤ dual upper bound (the sandwich).
- **Determinism:** the backward pass is a pure function of the stored panel + seed — a fixed seed gives a
  bit-stable price (pin the regression ordering).
- **Degenerate check:** with exercise disabled (or deep OTM), LSM collapses to the European price.

## 7. Risk

**High — the only high-risk item of the three.** It adds a new post-run *backward* execution mode, a new
declarative construct, and regression numerics with a documented estimator bias that needs the dual bound
to fully resolve. Mitigants: the cross-path regression math and the stored path panel already exist
(`reduce_run_regress` + `hist_store`), so the work is orchestration and numerics, not a new simulator;
and Phase 1 (Bermudan vs. a binomial tree) is a self-contained, checkable milestone before committing to
the American limit and the dual.

**Bottom line:** early exercise is the one remaining capability that is a genuine engine project, not an
example. The good news is the two hard ingredients — cross-path least squares and the full path panel —
are already in the codebase; LSM is a backward, recursive re-use of them. Scope it as its own effort
(Phase 1 first, validated against a tree), separate from the drop-in digital and basket examples.

## 8. The tight dual — why it needs nested simulation (investigation)

Phase 3 ships a **valid but loose** upper bound (`lsm_dual`): the Rogers/Haugh–Kogan/Andersen–Broadie
duality says `price = inf_M E[maxₜ(Zₜ − Mₜ)]` over all martingales `M`, and *any* martingale gives a
valid upper bound. `lsm_dual` uses the single hedging martingale `M_t = θ·(discᵗ·S_t − S_0)` (the
discounted underlying is a true martingale) and minimizes over the one scalar `θ`. One instrument cannot
replicate the option's payoff, so the max-deviation is large and the bound is loose (dual ≈ 9.70 vs a
true price ≈ 6.05 for the put).

The *tight* dual is the **Andersen–Broadie** martingale: the Doob martingale of the LSM value process,
`M_t = Σ (V̂_k − E_{k−1}[V̂_k])`, whose increments are the value's innovations. When `M` is close to the
option's own value process, `maxₜ(Zₜ − Mₜ)` shrinks to near-zero duality gap. Building `E_{k−1}[V̂_k]`
(the one-step conditional expectation of the continuation value) is exactly what requires **nested
simulation** — an inner Monte Carlo of sub-paths launched from each `(date, path)` node — which this
engine's forward-once / backward-once execution model does not provide.

Non-nested substitutes were tried and **none is both tight and valid**:

- **Regression martingale (innovations from the fitted `V̂`).** Reconstructing the Doob increments from
  the same LSM regression that produced the primal makes `M` collapse toward the primal value process,
  and the estimated dual gap collapses with it — *below* the true price, i.e. an **invalid** "upper"
  bound. It is only unbiased with genuinely independent inner samples for the conditional expectation.
- **Exact-moment / control-variate martingale.** Using the GBM's known conditional moments to build an
  analytic `M` for the state gives a valid but even **looser** bound (≈ 12.6) — it hedges the state, not
  the option value.
- **Gauss–Hermite quadrature** for the one-step conditional expectation. Valid in principle, but the
  quadrature is over the *fitted* continuation, so it inherits the regression's bias: loose (≈ 9.6 at 16
  nodes) or, when the node count and basis degree interact badly, **invalid** (0.18 *below* the price).

The pattern is consistent: without a true, independent inner expectation, the martingale either stays
far from the value process (loose) or is contaminated by the primal fit (invalid). A robust tight dual is
therefore **gated on a nested-simulation capability** — an inner sampler the engine would have to grow.
We deliberately did **not** ship a fragile "tight" dual that violates the upper-bound guarantee; the
loose-but-rigorous `lsm_dual` is the honest Phase-3 deliverable, and the tight bound is documented here
as future engine work.
