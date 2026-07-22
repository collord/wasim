# Streaming results from WaSim — design note

Companion to `src/engine/streaming.ts` (the contract) and
`src/engine/streamingRef.ts` (a working reference implementation).

## The governing rule

```
PartialRunResult is structurally assignable to RunResult.
```

A partial result is not a different shape. It is the *same* shape with fewer
realizations folded in. `RunResult` is simply the last value of a stream of
partials. **The renderer needs no changes to gain streaming** — `Diagram`,
`glyphs`, and `Transport` consume a partial exactly as they consume a final
result.

This is why the batch API can be dropped entirely: a batch run is a stream
whose caller ignores every emission but the last.

```ts
const handle = engine.runStreaming(opts, () => {});
const result = await handle.done;   // identical to the old run()
```

## What streams, and what deliberately doesn't

WaSim has two nested loops with opposite characteristics:

```
for realization in 1..N:      ← OUTER: independent, long  → STREAM THIS
    for step in 1..T:         ← INNER: sequential, sub-ms → DO NOT STREAM
        evaluate graph
```

**Don't stream the inner loop.** It's already fast, and streaming it would
couple frame rate to solver internals while making backward scrub impossible.
The playhead stays decoupled: animation always replays retained history. This
was the Layer-1 choice and it remains correct.

**Do stream the outer loop.** Realizations arrive *whole*. The uncertainty band
visibly tightens as they accumulate, the CI shrinking at 1/√N. Three payoffs:

1. **Convergence becomes legible.** "Have I run enough realizations?" is a
   question every Monte-Carlo modeler answers by guessing. Watching the band
   stop moving *answers* it.
2. **Early termination.** If the CI on the target metric is tight enough at
   3,000 realizations, stop — don't burn the other 7,000. A performance feature
   disguised as a visualization. (The reference impl stops at **350 of 5,000**
   at a 2% relative tolerance — a 14× saving on that model.)
3. **It demos in fifteen seconds** and no competitor can tell the same story.

For rare-event work with importance sampling, watching a tail probability
stabilize is exactly the diagnostic you want — and plain-MC vs IS convergence
side by side is benchmark 5.4 rendered live.

## What crosses the boundary

**Never full traces for every realization.** At R × E × T × 8 bytes, retaining
everything is hundreds of MB, and postMessage serialization would dominate the
run. Instead, three things:

| What | Why | Memory |
|---|---|---|
| **Running moments** (count, mean, M2) per (element, step) | Exactly incremental, exactly mergeable across workers. Gives mean + CI for free. | O(E×T) |
| **Percentile sketches** per (element, step) | Percentiles are *not* incrementally computable in constant memory. Use t-digest (production choice), P², or fixed bins (shown in the ref impl). | O(E×T) |
| **Bounded trace sample** — first K + pinned exemplars | All the single-realization replay view needs. | O(K×E×T) |

That split — **sketches for the cloud, a bounded sample for the stories** — is
the load-bearing decision. Settle it before the results layer hardens, because
it determines what the engine retains.

Two consequences for the view layer: `realizations` is *sparse*, so the
realization selector must key off `retainedIndices` rather than assume dense
indexing; and `bandsApproximate` lets the UI mark a band provisional while
streaming (honest, and it clears on the final emission if exact percentiles
are recomputed).

Use Welford/Chan pairwise merging rather than naive sum-of-squares — numerical
conditioning bites at 1e5+ realizations with large-magnitude values.

## Implementation shapes, in order

1. **Chunked yield on the main thread.** `run_chunk(from, to)` driven from JS,
   accumulating between chunks. Chunk sized so one call is ~16–50ms. No
   threading, no SAB. This is what `streamingRef.ts` implements.
2. **Web Worker + postMessage.** The right default: UI never janks, user can
   interact with arrived results while more stream in, clean cancellation.
   Message protocol is in `streaming.ts` (`WorkerRequest`/`WorkerResponse`).
   Retained traces are sent *once* when first retained, not re-serialized
   every chunk.
3. **SharedArrayBuffer + worker pool.** Realizations are independent, so fan
   across `hardwareConcurrency` with a shared typed-array and a monotonic
   completion counter. Genuine parallel speedup — but requires cross-origin
   isolation (COOP/COEP), which complicates deployment and can break embeds.
   Worth it for 10k-realization models; overkill before that.

Cancellation is **cooperative and realization-aligned**: the engine polls the
abort flag *between* realizations, never mid-realization. A cancelled run
therefore always ends on a realization boundary and its partial aggregate stays
statistically valid — no half-integrated realization pollutes the statistics.

## The determinism invariant

This is the one that protects the brand.

> **Streaming and parallelism must not change results.**

The `fixed` timebase's bit-identical guarantee has to survive chunking, worker
fan-out, and early termination. That requires realization *k*'s random stream to
be a **pure function of (seedRoot, k)** — a counter-based or splittable PRNG
(PCG, ChaCha, SplitMix-style jump), *not* one sequential generator advanced
across realizations.

If realization *k*'s stream depends on *k−1* having run first, results become
order-dependent, and a run split across 8 workers silently differs from a
single-threaded one. That failure mode — reproducible single-threaded, subtly
different multi-threaded — is precisely the bug that would destroy trust in a
defensibility-focused tool. It's also the kind of bug that survives casual
testing, because single-threaded dev runs always agree with each other.

`event_accurate` sub-steps consume no randomness, so the guarantee extends
across both timebase modes. Preserve that property.

Get it right and the claim is strong and rare:

> Identical results whether run on one core or sixteen, streamed or batched,
> cancelled and resumed or run straight through.

**This is asserted, not assumed.** `src/engine/determinism.test.ts` runs the
same model at chunk sizes 1 / 7 / 33 / 200, hashes the aggregate, and requires
every hash to match. Verified output:

```
--- chunk-size invariance ---
  chunkSize=  1  aggregate=c94b53cd
  chunkSize=  7  aggregate=c94b53cd
  chunkSize= 33  aggregate=c94b53cd
  chunkSize=200  aggregate=c94b53cd
  PASS: chunking does not change results

--- realization-order invariance (worker fan-out proxy) ---
  PASS: realization k is a pure function of (seedRoot, k)

--- early termination leaves valid statistics ---
  stopped at 350/5000 realizations, relative CI half-width = 0.0199

ALL INVARIANTS HELD
```

Run it with `npx tsx src/engine/determinism.test.ts`. This belongs in CI from
day one — alongside the benchmark suite's bit-identical output hashing, it's
the mechanical backing for the reproducibility claim.
