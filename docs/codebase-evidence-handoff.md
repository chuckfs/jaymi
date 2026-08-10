# Unified Codebase Evidence → Reasoning Handoff (Sprint C2.7.2)

## Goal

Close the split between Planner-synthesized deterministic reports (C2.1–C2.3)
and evidence-brief → Context → Reasoning (C2.4–C2.5) **without** a second
architecture.

When Reasoning is available and the ask is non-trivial:

```
retrieval → evidence → ContextEngine / ContextPolicy → PromptBuilder → Reasoning
```

When Reasoning is unavailable, unnecessary, or the ask is trivial:

```
retrieval → deterministic markdown report
```

## Shared representation

`crates/jaymi-planner/src/codebase_evidence.rs` converts search / symbol /
relationship / deep / Q&A evidence into `CodebaseEvidenceHit` rows and:

1. Injects them as session `search_hits` for ContextEngine re-assemble
   (**ContextPolicy** selects what enters the `ContextBundle`).
2. Builds a compact evidence brief for conversational Prompt → Reasoning.

C2 modules never call `ReasoningProvider`. Planner owns handoff via
`complete_with_codebase_evidence_handoff` →
`handle_conversational_request_with_observer` (ConversationStream).

## When Reasoning runs

| Origin | Reasoning preferred when |
|--------|--------------------------|
| Code search | NaturalLanguage / Responsibility with hits; **not** ExactSymbol / FindReferences / empty |
| Symbol navigation | Implementations / Dependents with hits; **not** simple location lists |
| Relationship trace | Evidence present (or investigation needed) |
| Deep investigation | Always (when Reasoning implemented) |
| Codebase Q&A | Investigation needed or evidence present |

Trivial lookups stay deterministic even with a live Reasoning backend
(Conversation First + Offline First cost control).

## Soft-path order (unchanged)

1. Relationship (C2.3)
2. Deep (C2.4)
3. Codebase Q&A (C2.5)
4. Symbol navigation (C2.2)
5. Code search (C2.1)

## Constitutional ownership audit

| Component | Ownership in this handoff |
|-----------|---------------------------|
| **Planner** | Owns C2 investigation paths, evidence conversion, inject → assemble, optional Reasoning handoff. Never bypasses Context. Never lets C2 call providers. |
| **ContextPolicy** | Sole selection boundary for what from session `search_hits` / providers enters the final bundle. |
| **ContextEngine** | Sole `ContextBundle` factory (`assemble_with`). No second assembler. |
| **PromptBuilder** | Sole prompt assembly inside ReasoningEngine. No second prompt builder in C2. |
| **ReasoningEngine** | Orchestrates PromptBuilder → provider after Planner conversational path. |
| **ReasoningProvider** | Invoked only via ReasoningEngine / ConversationStream — never from C2 modules. |

### Hard lines preserved

- Planner-owned investigation (C2.1–C2.5 soft paths intact).
- Deterministic fallback when Reasoning is off or not preferred.
- No direct `ReasoningProvider` from C2.
- No second PromptBuilder / ContextBundle assembler.
- ContextPolicy remains selection boundary.
- Bounded retrieval (existing caps + `EVIDENCE_HIT_CAP`).
- Conversation First — no LLM for trivial exact/reference lookups.
- Read-only — no mutation introduced by handoff.
- Model does not invent retrieval; brief instructs honesty about gaps.

## Diagnostics

Structured reports (`code_search`, `symbol_navigation`, `relationship_trace`,
`deep_investigation`, `codebase_qa`) remain attached on both Reasoning and
deterministic responses for C2.7.1 Coding Diagnostics parity.

## Out of scope

C3 editing / mutation. No new workspace shell. No second diagnostics system.
