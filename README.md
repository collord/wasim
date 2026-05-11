# WASiM

Open probabilistic simulation standard and browser runtime. A versioned JSON model format (`model.json`) plus a Rust/WASM engine and React frontend that execute models entirely client-side.

The model format is the primary artifact — a transparent, diffable JSON document that can be version-controlled, diffed, and reviewed like any other text file.

## Repository layout

```
schema/                 JSON Schema (draft-07) for model.json v0.1.0
schema_examples_manual/ Example models
engine/                 Rust simulation engine (native + WASM)
frontend/               React/TypeScript UI
```

## Model format

A `model.json` file contains simulation settings and a flat list of typed elements.

```json
{
  "wasim_version": "0.1.0",
  "simulation_settings": {
    "duration": { "value": 30, "unit": "yr" },
    "timestep": { "value": 1, "unit": "yr" },
    "n_realizations": 1000,
    "sampling_method": "monte_carlo",
    "seed": 42
  },
  "containers": [],
  "elements": []
}
```

**Sampling methods:** `monte_carlo`, `lhs` (Latin hypercube).

### Element types

| Type | Description |
|------|-------------|
| `constant` | Fixed scalar value; optionally editable with min/max bounds |
| `random_variable` | Sampled once per realization from a distribution |
| `expression` | Computed value via expression AST |
| `accumulator` | State variable with a rate expression; integrates via explicit Euler |
| `timeseries` | Time-varying input with interpolation (linear, step, cubic) |
| `lookup` | 1D or 2D table interpolation with configurable extrapolation |
| `stochastic_process` | GBM resampled every timestep (drift + volatility) |
| `delay` | Time-lagged signal |
| `array` | Named vector of scalar expressions |
| `script` | Reserved (not yet executed) |

### Distributions

Supported by `random_variable`: `uniform`, `normal`, `lognormal`, `lognormal_moments`, `triangular`, `exponential`, `gamma`, `beta`, `weibull`, `pearson_v`, `pearson_iii`, `discrete_uniform`, `bernoulli`, `discrete`.

All distributions support optional truncation (`min`, `max`). The `correlation_group` field is present in the schema for rank-correlation grouping but is not yet implemented in the engine.

### Expression language

Expressions are encoded as an AST with these node types:

- **Literal** — scalar value with optional unit
- **Reference** — `element_id` + optional output port; a reference to an accumulator from within its own rate expression resolves to the previous-timestep value
- **Time** — `elapsed`, `timestep`, `year`, `month`, `day_of_year`, `day_of_month`, `days_in_month`
- **Binary ops** — arithmetic (`add`, `subtract`, `multiply`, `divide`, `power`), comparison (`lt`, `gt`, `lte`, `gte`, `eq`, `neq`), logical (`and`, `or`)
- **Unary ops** — `neg`, `not`
- **If/then/else**
- **Function call** — 52 builtins (see below)
- **Lookup call** — 1D and 2D table lookup with interpolation
- **Array literal**

**Scalar builtins:** `min`, `max`, `abs`, `sqrt`, `exp`, `ln`, `log`, `log2`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `floor`, `ceil`, `round`, `mod`, `sign`, `int`, `step`

**Array builtins:** `sum_array`, `mean_array`, `min_array`, `max_array`, `size_array`, `get_element`, `dot_product`, `interp_array`

## Engine

The Rust engine (`engine/`) runs as both a native library (for testing) and a WASM module (for the browser).

**Execution model:**

1. Parse `model.json` into typed element structs.
2. Build a dependency graph; topological-sort evaluation order (cycle detection included). Accumulator rate inputs are excluded from the topo edges so they evaluate against the previous-step state.
3. For each realization:
   - Sample all `random_variable` elements once (ChaCha8 RNG, per-realization stream offset from seed).
   - For each timestep, evaluate all elements in topological order; advance accumulators, delays, and stochastic processes.
4. Aggregate results: mean and percentiles (p05, p25, p50, p75, p95) per element per timestep.

**WASM API** (exposed via `wasm-bindgen`):

```js
const engine = new WasmEngine(modelJson);   // parse + validate
engine.model_summary();                     // JSON summary of elements
engine.run_json('{"n_realizations":1000,"seed":42}');  // → SimulationResults JSON
```

**Not yet implemented in the engine:**
- Autocorrelated random variables (parsed, treated as iid at runtime)
- Rank-correlation sampling (`correlation_group`)
- `script` element execution

## Frontend

React 18 + TypeScript + Vite + Tailwind. Runs the engine in a Web Worker to keep the UI non-blocking.

**Tabs:**
- **Model** — element list, dependency graph (Dagre layout), container hierarchy
- **Dashboard** — edit `editable` constant values and re-run
- **Results** — time-history charts (mean + percentile bands) and final-value distributions per element

Load a model by file drop or upload. The frontend calls the WASM engine through a typed worker protocol.

## Example models

| File | Description |
|------|-------------|
| `schema_examples_manual/retirement_planning.json` | 30-year Monte Carlo retirement accumulation with three account types (401k, Roth, taxable), lognormal return distributions, tax-aware withdrawals |
| `schema_examples_manual/two_tank_hydraulic.json` | Two-tank hydraulic system with bistable pipe-flow hysteresis; Darcy-Weisbach friction, 1-hour simulation at 5-second timesteps |

## Building

**Prerequisites:** Rust toolchain, `wasm-pack`, Node ≥ 18.

```sh
# Build WASM engine
cd engine
cargo install wasm-pack          # once
./build-wasm.sh                  # outputs engine/pkg/

# Build and run the frontend
cd ../frontend
npm install
npm run dev
```

**Native engine tests:**

```sh
cd engine
cargo test
```

**Build WASM from the frontend:**

```sh
cd frontend
npm run build:engine   # runs engine/build-wasm.sh
npm run build          # production bundle
```

## License

The model schema, Rust engine, and frontend are MIT licensed.
