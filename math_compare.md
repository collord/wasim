# WASiM vs DAE/DOF Systems

## What WASiM does

Pure explicit-Euler ODE integration. At each step, every expression is already solved for its output — the graph is a DAG, evaluation is a single forward pass in topo order, and accumulator updates are just `state += rate × dt`. There's no equation-solving happening at runtime at all. The "hard work" of figuring out what depends on what was done once at graph-build time.

---

## DAEs — the gap

A DAE mixes differential equations with algebraic constraints that must hold *simultaneously* at every timestep. The classic example is a mechanical system with a rigid rod: you have ODEs for position and velocity, plus an algebraic constraint that the rod length is fixed. The constraint couples the variables in a way that can't be written as an explicit rate.

The distinguishing feature: you cannot solve for all unknowns by just evaluating expressions in order. At each timestep you have a system of equations — some involving derivatives, some not — and you have to *solve* that system jointly. For linear DAEs this is a matrix solve. For nonlinear DAEs it's Newton iteration (or similar) at every single timestep.

The index of a DAE matters too. Index-1 DAEs are relatively tractable. Index-2 and above require differentiation of the constraints to reduce them to index-1 before you can even apply a solver — this is what Pantelides' algorithm does. ASCEND4 handles this. WASiM has no Newton solver, no matrix factorization, nothing — it's a pure feed-forward computation graph.

How far is the gap? In terms of code: you'd need an implicit integrator (e.g. BDF or implicit Runge-Kutta), a nonlinear solver with Jacobian support, and index-reduction preprocessing. That's roughly the complexity of a small numerical library — think the core of Sundials/CVODE. It's not a feature addition, it's a different computational paradigm.

---

## Degree-of-freedom solver — the gap

ASCEND4's DOF system means the model doesn't specify *which variables are inputs and which are outputs* — it specifies a set of equations and lets you designate any consistent subset of variables as fixed (`FIX`). The solver then figures out which variables are determined by which equations (the matching problem), confirms the system is neither under- nor over-determined, and solves accordingly.

WASiM's graph has a fixed causal direction baked in at model-author time: expressions flow from inputs to outputs, accumulators have explicit rate formulas. You cannot ask WASiM "given that the output is 5, what should the input be?" — causality is one-directional.

What would it take? The structural analysis part (Dulmage-Mendelsohn decomposition, or ASCEND4's variant) is a graph matching algorithm — not trivial, but implementable in a few hundred lines. The harder part is that once you've established the matching, you need to *solve* each matched block, which for nonlinear equations means Newton iteration again. And for systems with tearing (where blocks are solved iteratively) you need a fixed-point or Newton loop over the whole system.

---

## Concrete distance

| Capability | WASiM | DAE solver | DOF solver |
|---|---|---|---|
| Evaluation model | Feed-forward DAG | Implicit joint solve | Equation matching + solve |
| Per-step work | O(n) expressions | O(n³) or Newton iters | Same as DAE |
| Solver needed | None | Newton/GMRES | Newton/GMRES |
| Jacobian needed | No | Yes | Yes |
| Index reduction | N/A | Pantelides | Pantelides |
| Causal direction | Fixed at build time | Flexible | Fully flexible |

The ODE-only WASiM you have now is perhaps 5% of the implementation complexity of a full DAE/DOF system. The other 95% is numerical linear algebra and symbolic preprocessing that general-purpose tools like Modelica/OpenModelica, Sundials, or ASCEND4 itself have taken decades to mature.
