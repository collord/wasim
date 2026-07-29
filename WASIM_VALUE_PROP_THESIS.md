# WaSim — The Value Proposition

*A spiral document: the thesis in one paragraph, then a two-page executive summary, then
the full treatment. Each layer is self-contained; each expands the one before it.*

---

## I. The thesis, in one paragraph

WaSim is a general probabilistic dynamic-simulation engine — realization-major Monte Carlo,
time-stepped recurrences, named-dimension arrays, condition-triggered failure state machines,
bit-identical reproducibility, and models stored as diffable JSON. Its generality is real
leverage but a fatal *sales* surface: a tool that can express anything asks every modeler to
first invent the vocabulary, and a tool that turns off everyone by being for no one. The move is
to stop selling the substrate and start selling **lenses** — thin, domain-specific authoring
surfaces, each with a pre-committed vocabulary and its own validation invariants, projected onto
one unchanged general engine. The first lens is **stock-and-flow**, the 65-year-proven, most
teachable abstraction in all of dynamic modeling (Forrester → Sterman), delivered as the
**diffable, probabilistic, git-native successor to XMILE** — the stock-flow interchange standard
that stalled in 2018 as un-versionable XML with no uncertainty in it. WaSim enforces stock-flow
*consistency* as checkable governance (the accounting invariants of Godley-Lavoie), simulates
accumulation rather than asking users to reason about it (the empirically documented thing humans
cannot do), and carries per-realization uncertainty natively — the exact axis classical system
dynamics is criticized for lacking. The pitch is therefore not "a more general modeling tool" but
"**auditable, reproducible, probabilistic stock-and-flow models your regulator can review** —
on an engine general enough to grow a second and third lens without a rewrite."

---

## II. Executive summary (~two pages)

### The problem we kept circling

WaSim can express an enormous problem space: cash and inventory accumulating under noise, fleet
reliability with repair queues, path-dependent option valuation, contaminant transport,
policy dynamics. That breadth was verified against the source, not assumed. But breadth is the
liability, not the asset, at the point of sale. Every modeling tool that won its market won by
**pre-committing a vocabulary** — Analytica's typed nodes, Vensim/Stella's stocks and flows,
GoldSim's containers, the spreadsheet's cells. None is less general underneath; they simply never
show the modeler the substrate. A general schema, exposed directly, forces the domain expert to
also be a language designer — a job they neither want nor are equipped for. **The generality is
the moat; it must never be the sales surface.**

### The answer: lenses, not a platform

The resolution is a clean architectural line:

- A **lens** changes *what nouns you author and how they are validated*. It is a restricted,
  semantically-typed sublanguage with evaluation-relevant invariants. Stock-and-flow (a stock
  integrates; a flow conserves), reliability block diagrams (series vs parallel availability),
  decision/value-of-information (declare decisions + an objective; the engine sweeps and
  optimizes). Test: **delete the visualization — if the model and the authoring change, it was a
  lens.**
- A **view** merely re-renders the same model with domain coloring. The "influence diagram," as
  actually shipped by Analytica, is a view: it is the dataflow DAG your canvas already draws, with
  node types colored in. Views are free — ship all of them — but they are not products.

So: **build one lens that changes authoring; ship every view for free.** A general tool wearing
three half-lenses still reads as a general tool — the trap. One complete lens reads as a product.

### Why stock-and-flow is the first lens

Four converging bodies of well-cited work make this the highest-leverage choice:

1. **It is the proven manageable abstraction.** Forrester's *Industrial Dynamics* (1961) reduced
   all industrial activity to five stock-flow networks plus information feedback; Sterman's
   *Business Dynamics* (2000) made it the lingua franca of an entire field (Scholar total ~65,600).
   Sixty-five years of durability is the strongest possible evidence that this is *the* answer to
   "how do you make dynamic modeling communicable." We spend zero novelty budget on the metaphor.
2. **The engine already fits it exactly.** A stock is a `lag` accumulator under explicit-Euler
   stepping; a flow is a rate expression. The lens is thin — it is a typed projection of
   constructs the engine already has, not new physics.
3. **There is a stalled standard to succeed.** XMILE became an OASIS standard in 2015 and its
   technical committee **closed in 2018** — an un-diffable XML dialect, deterministic, with no
   native uncertainty (Monte Carlo lives in the tools, not the format). WaSim's diffable JSON +
   realization-major Monte Carlo is precisely that standard modernized for version control and
   uncertainty. We revive a standardized category rather than inventing one.
4. **There is a rigorous invariant set to enforce.** Stock-flow *consistent* macroeconomics
   (Godley & Lavoie 2007, credited with anticipating the 2008 crisis) is built on hard accounting
   invariants — every flow leaves one place and enters another; all stocks reconcile. Those
   invariants survive deleting the diagram, which is exactly what makes stock-and-flow a lens and
   not a view — and they hand us a checkable governance claim: *"guaranteed stock-flow
   consistent."*

### The differentiator that is already built

The standing critique of classical system dynamics is that it is **aggregate and deterministic** —
it flattens heterogeneity and carries no per-entity uncertainty; the acknowledged frontier
(hybrid SD + agent-based modeling) exists to patch exactly that. WaSim's realization-major engine,
with independent per-realization streams, answers the critique *natively*. The differentiated
product is not "reimplement Vensim" but **stock-and-flow on a probabilistic, per-realization
substrate** — the direction the literature is already straining toward. The reliability fleet work
already built this without naming it: accumulated damage (a stock) with per-truck heterogeneity and
Monte-Carlo failure is a hybrid model in all but label.

### One empirical result that dictates the UI

Shown a bathtub's inflow and outflow graphs, **fewer than half of MIT graduate students** could
sketch the water level (Booth Sweeney & Sterman 2000; Sterman & Booth Sweeney 2007). Humans cannot
mentally integrate a flow into a stock. Therefore the lens must **simulate accumulation and show
the trajectory**, never ask the user to reason about it symbolically. This is a direct argument for
WaSim's simulate-first substrate and against any purely-declarative surface.

### What this asks of the schema (almost nothing)

The lens lives in the authoring environment: palette, canonical templates, validation, diagram
semantics. The engine calculus stays general and hidden. The **one** schema-adjacent change is a
lightweight, engine-ignored `role`/`lens` annotation on elements so a lens can *round-trip* —
so a stock knows it is a stock and the JSON parses back into the right vocabulary. Additive,
cheap, non-breaking. Do not generalize the calculus further; annotate it.

### Markets, ranked by fit × winnability

| Market | Fit | Incumbent | WaSim's edge |
|---|---|---|---|
| System dynamics / policy / business & sustainability | Highest | Vensim, Stella, AnyLogic | Best engine fit, widest and least-technical audience, untouched so far |
| Reliability / RAM | Proven | GoldSim (proprietary, costly) | Same power, git-native, auditable, cheaper |
| Model-risk / validation (finance, insurance) | Proven | In-house libs, opaque spreadsheets | The **governance** buyer: diffable, reproducible, reviewable |
| Probabilistic risk assessment (nuclear, pharma, environmental) | High | GoldSim, bespoke | Reproducibility + auditability is regulatory gold |

The common thread in the winnable column is **not modeling power** — every incumbent has enough.
It is **model governance**: diffable, bit-reproducible, PR-reviewable models, a concept borrowed
from software engineering and genuinely novel in this space. That is the wedge that does not
require out-modeling anyone.

### The one-line positioning

**Auditable, reproducible, probabilistic stock-and-flow — the diffable successor to a stalled
standard — on an engine general enough to grow the next lens without a rewrite.**

---

## III. The full treatment

### 1. Starting conditions, verified not assumed

The engine's capabilities were checked against source during this research, not taken from a
brochure. What is real today:

- **Realization-major Monte Carlo** with per-realization ChaCha8 streams keyed on `(seed, k)`,
  yielding **bit-identical reproducibility** across repeats and machines.
- **Time-stepped recurrences**: `lag` + explicit-Euler stepping express any accumulator.
- **Named-dimension arrays** with align-by-name broadcast (disjoint axes outer-product into
  multi-axis arrays) and, as of this branch, **axis-selective reduction** (reduce one named axis,
  keep the rest).
- **Condition-triggered failure state machines** (`event` primitive, condition-basis processes,
  status latch) — repairable-system reliability without bespoke code.
- **Reductions at three levels** — across realizations at the output boundary, mid-graph via
  submodels, and across array axes — plus **sweep composition** (a decision axis becomes a swept
  dimension).
- **Models as diffable JSON**, reviewable like source.

Problem *shapes* this cleanly expresses: accumulation-under-uncertainty over time; availability of
repairable systems; path-dependent valuation; sweep-composed decision risk. Two of these are
already proven end-to-end in the repo (the RAM fleet arc; the options-pricing corpus).

### 2. The strategic disease and its diagnosis

A general substrate is a compilation target, not a UX. Exposed directly, it fails commercially for
a specific reason: it makes the user do language design. The domain expert opens the tool and must
decide what counts as a stock, a rate, a failure, a decision — inventing vocabulary before
expressing anything. Every winner in this category removed that burden by pre-committing the
vocabulary and hiding the engine:

- Analytica → typed nodes over a dataflow DAG.
- Vensim / Stella → stocks and flows.
- GoldSim → containers and typed elements.
- @RISK / Crystal Ball → "a spreadsheet where cells can be distributions" (near-zero new
  vocabulary — and, not coincidentally, the widest adoption).

None is less general underneath. The lesson is exact: **keep the general schema; never sell it.**

### 3. Lens vs view — the load-bearing distinction

The single most useful discriminator this research produced:

> A **lens** changes what nouns you author and how they are validated — a restricted, typed
> sublanguage with evaluation-relevant invariants. A **view** re-renders the same model with
> domain coloring. Delete the visualization: if the model and the authoring are unchanged, it was
> a view.

Worked examples:

- **Stock-and-flow** fails the delete test → **lens.** A stock carries an integration invariant;
  flows carry conservation structure. Strip the picture and author it as a raw DAG and you lose
  those invariants — you would have to reconstruct them by hand.
- **Reliability block diagram** fails the delete test → **lens.** Series vs parallel is a distinct
  availability-composition semantics, not a color.
- **Decision / value-of-information** fails the delete test → **lens.** You declare decision
  variables and an objective; the engine sweeps the decision axis, reduces to an expected
  objective, and picks the best (and can compute VOI as the objective delta across information
  structures). WaSim already has the load-bearing half of this via sweep composition; the missing
  half is an optimize/VOI reduction as a named operation.
- **Analytica-style "influence diagram"** *passes* the delete test → **view.** Analytica's
  evaluation is plain dataflow over a typed DAG; the decision/chance/objective node classes are
  coloring and organization on the same DAG the canvas already renders. Useful as a hint (typed
  nodes seed the decision/VOI lens) but not itself a distinct model style.

Consequence: **build one lens that changes authoring; ship every view for free.** Three half-lenses
still read as a general tool. One complete lens reads as a product.

### 4. Why stock-and-flow is the first lens — the evidence in depth

**4.1 The abstraction is proven and teachable.**
Forrester's *Industrial Dynamics* (1961) postulated that essentially all industrial activity is
five stock-flow networks (materials, orders, money, capital equipment, personnel) tied together by
a sixth, information, network — feedback among stocks and flows as a universal grammar for dynamic
systems. It seeded *Urban Dynamics* (1969), *World Dynamics* (1971), and thence *Limits to Growth*
(1972). Sterman's *Business Dynamics* (2000) standardized the modern vocabulary and became one of
the most-cited works in management science. The abstraction has been the field's lingua franca for
sixty-five years. That durability is the asset: we adopt stock, flow, auxiliary, connector verbatim
and spend no credibility on renaming.

**4.2 The engine already fits it.**
A stock is a `lag` accumulator under explicit-Euler integration; a flow is a rate expression that
adds to and subtracts from stocks each step. Auxiliaries are ordinary derived nodes. The lens is a
*typed projection* of constructs already present, not new engine work. This is why the first lens
is cheap: most of the cost is authoring-environment palette and validation, not the calculus.

**4.3 There is a stalled standard to succeed — XMILE.**
XMILE (built on the SMILE/DYNAMO lineage) became an **OASIS standard in December 2015**; its
technical committee **closed in September 2018.** It represents stocks that accumulate via
inflows/outflows, flows as rates, auxiliaries, and connectors, with a subscript/arrays spec. Two
facts define the opening:
- It is **XML** — verbose, awkward to diff, never designed for pull-request review.
- It is **deterministic** — uncertainty and Monte Carlo live in the tools (Vensim/Stella run
  modes), not in the interchange format.

WaSim's diffable JSON + native realization-major Monte Carlo is that standard carried into the era
of version control and uncertainty. The story is "modernize a standardized, dormant category," not
"invent a paradigm" — a far easier sale, and one that inherits XMILE's conceptual legitimacy while
fixing its two structural weaknesses.

**4.4 There is a rigorous invariant set to enforce — stock-flow consistency.**
Godley & Lavoie's *Monetary Economics* (2007) is the reference for stock-flow-consistent (SFC)
modeling, an approach credited with anticipating the 2007–09 crisis. Its machinery is four
accounting invariants — flow consistency, stock consistency, stock-flow consistency, and quadruple
book-keeping: every flow leaves one sector and enters another; all stocks reconcile; nothing
leaks. These invariants are exactly what survive deleting the diagram — the reason stock-and-flow
is a lens and not a view — and they give the authoring layer a **checkable governance claim**:
*"this model is guaranteed stock-flow consistent."* No incumbent spreadsheet or opaque binary can
make that claim structurally.

### 5. The differentiator that is already built — probabilistic, per-realization dynamics

Classical system dynamics is criticized on two counts: it is **aggregate** (it flattens
heterogeneity across a population into a single stock) and **deterministic** (it carries no
per-entity uncertainty). The recognized research frontier — hybrid system-dynamics + agent-based
modeling (e.g. Nguyen, Howick & Megiddo 2020; Swinerd & McNaught's design classes) — exists
specifically to patch those two gaps by coupling aggregate SD with agent-level heterogeneity.

WaSim's realization-major engine, with independent per-realization streams, answers both natively.
The product is not a Vensim clone; it is **stock-and-flow on a probabilistic, per-realization
substrate** — heterogeneity across realizations and genuine uncertainty are first-class, not
bolted on. The reliability fleet arc already demonstrates this without naming it: accumulated
damage is a stock, per-truck wear is heterogeneity, and failure is Monte-Carlo — a hybrid model in
substance. The lens simply gives that capability a vocabulary the SD market already speaks.

### 6. The empirical constraint on the authoring environment

Booth Sweeney & Sterman's "Bathtub Dynamics" (2000) and Sterman & Booth Sweeney's "conservation of
matter" study (Climatic Change, 2007) established a robust, uncomfortable finding: even highly
educated adults cannot infer a stock's trajectory from graphs of its inflow and outflow — fewer
than half of MIT graduate students got the bathtub right. Humans do not integrate in their heads.

The design consequence is not optional: the stock-and-flow lens must **simulate accumulation and
render the trajectory**, never present the stock as a symbolic expression to be reasoned about.
This is a first-class argument for WaSim's simulate-first substrate — the engine's core competence
is exactly the operation users' intuition fails at — and against any competitor whose surface is
declarative-only.

### 7. The schema/authoring split — where the work actually lands

- **Authoring environment (≈80% of the work):** the stock/flow/auxiliary palette; canonical
  templates; the simulate-first trajectory view; and the SFC validation rules (conservation,
  reconciliation) enforced at author time. Views (sankey of flows, tornado of sensitivities,
  feedback-loop highlighting) ride the existing DAG canvas for free.
- **Schema (one additive change):** an engine-ignored `role`/`lens` annotation on elements so the
  lens round-trips — a stock is tagged a stock, so re-opening the JSON reconstructs the right
  vocabulary rather than a raw DAG. Non-breaking, cheap, and the *only* schema change the lens
  requires. The calculus stays general and hidden.

This is the concrete answer to the question that opened the thread ("is it a schema problem or an
authoring problem?"): **mostly authoring, with a single small schema annotation to make lenses
round-trip.** Do not simplify the schema; annotate it and hide it.

### 8. Markets and go-to-market

**8.1 Ranked by fit × winnability** (not fit alone):

1. **System dynamics / policy / business & sustainability.** Best engine fit, widest and
   least-technical audience, and untouched by the repo so far. The stock-and-flow lens opens it
   directly. Incumbents: Vensim, Stella, AnyLogic.
2. **Reliability / RAM.** Already proven end-to-end. Incumbent GoldSim is powerful but proprietary,
   expensive, and un-diffable — beatable on governance and price. Mining, aerospace, energy,
   defense.
3. **Model-risk / model-validation groups** (finance, insurance). Not the front office — the
   governance buyer, trapped in opaque spreadsheets and un-auditable binaries. Options/exposure
   work already proven.
4. **Probabilistic risk assessment** (nuclear, pharma, environmental). Reproducibility and
   auditability are regulatory requirements, not nice-to-haves.

**8.2 The wedge is governance, not power.** Every incumbent has enough modeling power. What none
offers is **model governance**: diffable, bit-reproducible, PR-reviewable models — an idea imported
from software engineering, novel in this space, and *already built* (diffable JSON + deterministic
reproducibility) rather than promised. Every pitch leads with "auditable, reproducible, reviewable,"
not "more expressive."

**8.3 Sequencing.** Ship **one** beachhead lens. The default recommendation is stock-and-flow
(widest market, best fit, clean XMILE-succession narrative); the alternative is reliability/RBD
(converts proven work into a vertical against a beatable incumbent). Do not ship both first — a
general tool with two half-lenses is still a general tool. Prove the lens pattern round-trips on
one, then the second and third lens are incremental, not rewrites — which is itself the closing
argument of the value prop: *the generality you were tempted to sell is what lets you not rewrite.*

### 9. Risks and honest counter-arguments

- **The SD market is mature and habituated to Vensim/Stella.** Mitigation: do not compete on
  modeling breadth; compete on governance + probabilistic dynamics + git-native diffability, none
  of which the incumbents offer, and inherit XMILE's legitimacy as its successor.
- **"Governance" may be a thin wedge if buyers do not feel the audit pain.** Mitigation: target the
  regulated verticals (model-risk, PRA) where the pain is a compliance line item, not a preference.
- **A lens that leaks the substrate fails.** The round-trip `role` annotation and strict
  author-time validation are what keep the substrate hidden; if the lens ever forces users back to
  raw ASTs, it has failed the whole thesis. This is the primary build risk and must be the primary
  acceptance test.
- **The lens might be thinner or thicker than believed.** The single most decisive next artifact is
  a concrete mapping of XMILE's stock/flow/auxiliary schema onto WaSim's `primitive`/`value_rule`/
  `lag` constructs, plus one end-to-end lens round-trip. That mapping converts this thesis from a
  reading into a build estimate.

### 10. The thesis restated

WaSim's generality is a moat to hide, not a product to sell. Sell **lenses** — typed authoring
surfaces with real validation invariants — over one general engine. Ship the **stock-and-flow**
lens first: the proven, teachable abstraction of dynamic modeling, delivered as the **diffable,
probabilistic, git-native successor to XMILE**, enforcing **stock-flow-consistency** as checkable
governance, **simulating** the accumulation humans cannot compute in their heads, and carrying the
**per-realization uncertainty** classical system dynamics lacks. Lead with governance. Keep the
substrate hidden. Let generality do the one job it is genuinely good at — letting the second and
third lens ship without a rewrite.

---

## References

- Forrester, J. W. (1958). "Industrial Dynamics—A Major Breakthrough for Decision Makers,"
  *Harvard Business Review*.
- Forrester, J. W. (1961). *Industrial Dynamics.* MIT Press / Pegasus.
- Forrester, J. W. (1968). *Principles of Systems*; (1969) *Urban Dynamics*; (1971) *World Dynamics.*
- Sterman, J. D. (2000). *Business Dynamics: Systems Thinking and Modeling for a Complex World.*
  McGraw-Hill.
- Booth Sweeney, L. & Sterman, J. D. (2000). "Bathtub Dynamics: Initial Results of a Systems
  Thinking Inventory," *System Dynamics Review* 16(4): 249–286.
- Sterman, J. D. & Booth Sweeney, L. (2007). "Understanding Public Complacency about Climate
  Change: adults' mental models of climate change violate conservation of matter," *Climatic
  Change* 80(3–4): 213–238.
- Eberlein, R. & Chichakly, K. (2013–2015). XMILE — XML Interchange Language for System Dynamics.
  OASIS Standard v1.0 (Dec 2015); TC closed Sept 2018.
- Godley, W. & Lavoie, M. (2007). *Monetary Economics: An Integrated Approach to Credit, Money,
  Income, Production and Wealth.* Palgrave Macmillan. (Stock-flow-consistent modeling.)
- Nguyen, L., Howick, S. & Megiddo, I. (2020). A hybrid simulation modelling framework combining
  system dynamics and agent-based models. (Frontier: hybrid SD + ABM.)
