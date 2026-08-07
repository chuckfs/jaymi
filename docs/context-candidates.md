# Context Candidate Graph (Sprint B2.7 / B2.13.1)

**Status: Current** — every Context Provider exposes candidates through
`propose_candidates()`; Context Policy selects; only selected candidates become
a `ContextBundle`.

Workspace Intelligence and other Context Providers expose
[`ContextCandidate`](../crates/jaymi-context/src/candidate.rs) nodes instead of
writing finished bundle sections into a parallel assembler. The Context Engine
is still the **sole factory** for `ContextBundle`. Providers never assemble
bundles. Planner ownership is unchanged.

Sprint **B2.13.1** completed the migration: production providers no longer rely
on a contribution→candidate trait fallback. `contribute()` is a convenience that
materializes proposed candidates for tests / diagnostics — not a parallel
assemble path.

## Ownership

| Concern | Owner |
|---------|--------|
| Propose candidates | Context Providers (`propose_candidates`) |
| Score / filter | Context Policy (relevance · recency · importance · privacy) |
| Budget pack | Context Engine (`select_candidates_for_budget`) |
| Materialize → sections | Context Engine (`materialize_candidates`) |
| Assemble `ContextBundle` | Context Engine only |
| Orchestration | Planner (unchanged) |

```text
ContextProvider::propose_candidates
        │  (never builds ContextBundle)
        ▼
 CandidateGraph (nodes ± edges)
        │
        ▼
 Context Policy · evaluate_candidate_item
   relevance · recency · importance · privacy
        │
        ▼
 select_candidates_for_budget
        │
        ▼
 materialize_candidates → ContextContribution
        │
        ▼
 ContextBundleBuilder → ContextBundle
```

## Policy dimensions

| Dimension | Meaning |
|-----------|---------|
| **Relevance** | Provider relevance + candidate importance prior |
| **Recency** | Fresher timestamps score higher (`None` → mid) |
| **Importance** | Provider-declared 0..=100 (`required` prefers packing) |
| **Privacy** | `Sensitivity` must be ≤ request `max_sensitivity` |
| **Budget** | Character packing after policy allow |

## Rules

* Providers propose candidates — they do **not** assemble bundles
* Providers do **not** apply Context Policy or allocate budget
* Only selected candidates enter the `ContextBundle`
* Coarse provider gates (deny / approval / relevance bypass) still run first
* Context Policy evaluates every candidate uniformly via
  `evaluate_candidate_item` regardless of provider
* Fine-grained nodes where practical (diagnostics, file summaries, open tabs)
* Section-level nodes where the feed is naturally atomic (conversation, git,
  runtime, memory results, …)
* Explainability: `PolicyReport.candidate_selection`
* Sprint **B2.8** [Context Selection](context-selection.md) allowlists feeds
  before candidate scoring

## Related

* [context.md](context.md) — Context Engine contract
* [context-selection.md](context-selection.md) — deterministic feed selection
* [context-maintenance.md](context-maintenance.md) — ambient snapshot refresh
* Snapshots B2.1–B2.6 remain observational inputs; candidates are the assemble unit
