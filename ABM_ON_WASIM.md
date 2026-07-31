# Agent-Based Modeling on WaSim — evaluation + a working proof

This documents (1) how ABM maps onto WaSim's execution model, (2) an evaluation of the
krABMaga Rust ABM framework and how much of what it does WaSim can do, and (3) a proof —
`engine/tests/network_sir_v2.rs` — that network contagion runs on today's engine with **no new
primitives**.

## 1. The reframe: agents are array members, not realizations

The apparent tension — "N agents interact, but realizations are independent" — dissolves once
agents are mapped onto the **array axis**, not the realization axis. Realizations stay the
independent, embarrassingly-parallel, bit-reproducible outer loop (the moat). Agents are cells of
a dimensioned element inside one realization, and **interaction is reduction over the agent axis**.

The design question then reduces to one axis: *what kind of interaction, and does it close a cycle
within a single instant?*

| Interaction | Mechanism | Engine work |
|---|---|---|
| Lagged mean-field (respond to population aggregate) | array + `sum`/`mean` reduction | none |
| Lagged network/local (respond to neighbors) | array + adjacency matrix + **axis-selective reduction** | none (shipped) |
| Instantaneous / equilibrium (within-step mutual dependence) | array + **within-step relaxation loop** | one new bounded fixed-point node |
| Heterogeneous types / sequential update | **N orchestrated submodels** | scheduler work; last resort |

`lag` is the step-boundary commit: an agent reading `lag(aggregate)` sees last step's state, so
lagged interaction has no cycle and needs nothing new. Only genuinely instantaneous simultaneity
(market clearing, congestion pricing) needs the within-step fixed-point loop — and even there the
sub-steps are *relaxation iterations*, not per-agent machinery.

## 2. krABMaga evaluation

[krABMaga](https://github.com/krABMaga/krABMaga) is a MASON re-implementation in Rust (MIT). Core:
`Agent` + `State` traits; a **sequential, priority-ordered scheduler** (`Priority { time, ordering }`,
agents run one at a time, self-reschedule at `time + 1`); **double-buffered fields** (continuous
`Field2D`, dense/sparse object grids, `NumberGrid2D` scalar fields, `Network`/`HNetwork` graphs,
`kdtree` spatial index); and an `explore` module (sweep, genetic, random, Bayesian optimization,
MPI-distributed).

**The architectural fault line.** WaSim is synchronous and declarative (`lag` reads the previous
snapshot; the step boundary is the swap). krABMaga is imperative and sequential (agents mutate
shared fields in priority order). But its **double-buffered fields + `update()`** *are* WaSim's
`lag`/step-commit pattern — so the **synchronous subset transfers**. What does not transfer is the
asynchronous, order-dependent, self-rescheduling event model.

This is the classic **"patches vs turtles"** split: WaSim natively does **field ABM and network
ABM** (agents as cells or graph nodes, synchronous, aggregate/neighbor-mediated); it does **not**
natively do **mobile-agent ABM** (turtles in continuous/grid space, immediate mutation,
order-dependent conflict resolution, individual schedules, dynamic populations).

**Scorecard on the six shipped examples:**

| Example | Structure | WaSim today | Why |
|---|---|---|---|
| Forest Fire | scalar grid CA, synchronous | Native | 2-D array + `lag`; spread = neighbor stencil via axis reduction |
| Virus on Network | fixed graph, SIR | Native | adjacency + state vector + axis reduction (**proven below**) |
| Ants Foraging | pheromone field + mobile ants | Half | pheromone diffusion/evaporation natural; ant movement is the turtle half |
| Wolf-Sheep-Grass | grid, 2 species, birth/death, movement | Partial | grass = scalar field ✓; aggregate predator-prey ✓; spatial movement + reproduction + species heterogeneity awkward |
| Schelling | grid, async move-to-empty | Awkward | synchronous approximation changes the dynamics; move-to-empty is sequential |
| Flockers (Boids) | continuous space, radius neighbors | Weak | only as an O(N²) all-pairs matrix; small N, no spatial index |

Net: **2 of 6 land cleanly, 2 contribute their field components, 2 fight the engine** — and the
clean wins are exactly the synchronous field/network models.

**Borrowing code (MIT).** The paradigm impedance is high, so most engine code does not transfer:
fields are imperative mutable containers; WaSim's graph is a *static adjacency array*, not a
mutable `HashMap`. Take the *design*, not the struct. The genuinely valuable borrows are (a) the
six examples as a **conformance/benchmark suite** (port Forest Fire + Virus-on-Network; keep
Boids/Schelling as honest boundary markers), and (b) the **`explore`/optimize module** (sweep, GA,
Bayesian) as a reference for the decision/VOI lens — which isn't ABM at all. The `kdtree` is
borrowable but only matters for the mobile-agent corner WaSim should *not* chase.

## 3. The proof — `engine/tests/network_sir_v2.rs`

Deterministic threshold-SI (susceptible → infected, no recovery) on an explicit 6-node graph: a
path 1–2–3–4–5–6 **plus a chord 2–5**. The rule per step is
`infected[i] ← infected[i] OR (any neighbor infected last step)`, with neighbor aggregation

```
exposure[i] = Σ_j  W[i,j] · infected_prev[j]
```

expressed as an elementwise product over the `[Node, NodeB]` adjacency grid **reduced over axis 2
(the neighbor axis)** — the axis-selective reduction from `axis_reduction_v2`. Two equal-size
dimensions (`Node`, `NodeB`) give the matrix two distinct axes of the same node set; `inf_prev`
(over `Node`) is reindexed onto `NodeB` by a `vector_map` + `index` gather — align-by-name matrix
algebra with **no transpose primitive**. State advances through `lag`.

Every construct is already in the engine: `vector_map`, `index_ref`, `index`, `lag`, elementwise
ops, and axis-selective `sum_array`. **No new primitives.**

Because the dynamics are deterministic, every value is a BFS frontier from the seed (node 1):

```
step:            1    2      3        4      5   6
newly infected:  {1}  {2}    {3,5}    {4,6}  -   -
total infected:   1    2      4        6     6   6
```

The test asserts the exact `total` history `[1,2,4,6,6,6]`, and — the load-bearing check — that
**node 5 infects at step 3 via the chord**. On the bare path node 5 is 4 hops from the seed and
would not infect until step 5; `infected#5 = [0,0,1,1,1,1]` proves the reduction is using the
graph's edge structure, not linear diffusion. All assertions pass.

This closes rows 1–2 of the interaction table end-to-end: **lagged mean-field and network/local
interaction run on today's engine.** The only open engine question for ABM is the within-step
relaxation loop (row 3) for genuine instantaneous simultaneity.

## 4. Strategic conclusion

"ABM" is not one lens — it is at least two, and the krABMaga field taxonomy draws the line:

1. **Field + Network lens** (patches, diffusion, cellular automata, graph contagion, synchronous)
   — fits WaSim natively and, crucially, **composes with stock-and-flow**: stocks-and-flows on
   network nodes is the *metapopulation* model class (spatial epidemiology, supply-chain contagion,
   regional diffusion). This is the natural second lens after stock-and-flow, not a separate
   product.
2. **Mobile-agent lens** (turtles in space, async, dynamic populations) — fits poorly; doing it
   means bolting an imperative, order-dependent sidecar onto the engine, which sacrifices the
   synchronous determinism and diffability that are the moat. Disclaim it, don't chase it.

The answer to "how much of krABMaga's ABM can WaSim do" is: **the synchronous field-and-network
half, cleanly — and that half is the half that composes with the stock-and-flow thesis into
metapopulation modeling.** The mobile-agent half it cannot do without sacrificing the moat, and
should not try.
