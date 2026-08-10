# Codebase Q&A (Sprint C2.5)

Integrates Intelligent Code Search, Symbol Navigation, Relationship Tracing, and
Deep Investigation into natural conversational codebase questions.

## Goal

Answer questions like:

- “How does Jaymi handle permissions?”
- “Where does Settings get its models?”
- “Why is this request slow?”
- “What happens after I press Enter?”
- “Where would image generation belong?”
- “What would break if I removed this provider?”

in ordinary conversation — without a special Codebase Q&A workspace.

## Behavior

```
Question
→ Planner classifies complexity (simple / search / flow / investigate)
→ Route to ambient WI, Code Search, Relationship Tracing, or Deep Investigation
→ Gather evidence (Planner-mediated only)
→ Detect conflicts across sources
→ ContextEngine assembles (sole factory)
→ Reasoning answers from Planner evidence brief (when implemented)
```

| Complexity | When | Route |
|------------|------|-------|
| Simple | Ambient WI sufficient | Ambient |
| Search | Focused lexical / placement | Code Search (+ light reads) |
| Flow | Event / after-press style | Relationship Tracing |
| Investigate | How-handles / impact / multi-source | Deep Investigation |

Simple / focused questions **do not** trigger unnecessary deep investigation.
Complex questions gather evidence **before** Reasoning.

## Honesty

- Substantive claims are grounded in retrieved project evidence when it exists.
- When evidence **conflicts**: surface the conflict, identify files/symbols, do
  **not** silently choose one side.
- When evidence is **unavailable**: say so; do not invent project behavior.
- Conversation markdown includes Answer / Evidence / Conflicts / Gaps — not raw
  diagnostics.

## Ownership

| Concern | Owner |
|---------|--------|
| Detect natural Q&A + complexity | Planner `codebase_qa` |
| Narrow C2.1–C2.4 exact phrasing | Existing soft paths (Q&A yields) |
| Retrieval | Planner-mediated Search / read / relationship / deep |
| Bundle assembly | ContextEngine only |
| Conversation report | `CodebaseQaReport` on `PlannerResponse` |
| Diagnostics | Report + Coding Diagnostics / `LastPlannerActivity` (not chat body) |

Soft-path order:

1. Relationship Tracing (C2.3)
2. Deep Investigation (C2.4)
3. **Codebase Q&A (C2.5)**
4. Symbol Navigation (C2.2)
5. Code Search (C2.1)

## Constraints

- Conversation remains the primary UX.
- No special Codebase Q&A workspace.
- Read-only — no modification.
- No autonomous execution.
- Model does not invent retrieval actions.
- Workspace Intelligence remains observational; providers remain passive.

## Constitutional audit

| Rule | Status |
|------|--------|
| Conversation First / no special workspace | PASS |
| Auto decide when investigation is necessary | PASS — `QaComplexity` |
| Simple ≠ deep investigation | PASS — Search/Ambient for settings / belong |
| Complex gathers evidence before Reasoning | PASS — Investigate/Flow routes |
| Claims grounded in retrieved evidence | PASS — evidence rows + honest gaps |
| Conflicts surfaced, not silently chosen | PASS — `EvidenceConflict` |
| Unavailable evidence stated; no hallucination | PASS — gaps + answer wording |
| No modification / no autonomous execution | PASS |
| Planner orchestrates; ContextEngine sole factory | PASS |
| Diagnostics not dumped into chat | PASS — activity + Coding Diagnostics; chat gets answer/evidence/gaps |
| Tests | PASS |

**Verdict: PASS**

### Remaining limitations

- Natural-language coverage is rule-based; novel phrasings may fall through to
  Understanding / plain conversation.
- Conflict detection is heuristic (shared topic + divergent previews / polarity).
- Soft-path quality still depends on WI enrichment and registered tools.

## Module

`crates/jaymi-planner/src/codebase_qa.rs`

## Related

- [docs/deep-investigation.md](deep-investigation.md)
- [docs/relationship-tracing.md](relationship-tracing.md)
- [docs/symbol-navigation.md](symbol-navigation.md)
- [docs/code-search.md](code-search.md)
- [docs/planner.md](planner.md)
