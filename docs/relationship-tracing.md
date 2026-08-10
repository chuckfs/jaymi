# Code Relationship Tracing (Sprint C2.3)

Planner-owned read-only investigation that traces how functionality moves
through a codebase.

## Goal

Answer questions like:

- “What happens when I click Save?”
- “Trace this request through the application.”
- “How does this value get here?”
- “Where does this event eventually end up?”
- “What calls this provider?”

by combining Workspace Intelligence, Search, LSP symbols, Project Intelligence,
and Git/context — without a second context assembler and without letting the LLM
navigate the filesystem or LSP.

## Investigation shape

```
Question
→ identify symbol
→ find definition
→ inspect related symbols
→ find callers / references
→ (optional) Planner-mediated Search Knowledge
→ (optional) Planner-mediated LSP references
→ project / git context when relevant
→ assemble evidence
→ answer
```

Every retrieval is Planner-mediated. Reasoning never calls Search/LSP/FS directly.

## Limits (anti-loop)

| Budget | Cap |
|--------|-----|
| Depth (symbol hops) | `MAX_DEPTH` (4) |
| Files inspected | `MAX_FILES` (12) |
| Symbols followed | `MAX_SYMBOLS` (8) |
| Steps | `MAX_STEPS` (10) |
| Elapsed time | `MAX_ELAPSED_MS` (2500) |
| Conversation evidence | `CONVERSATION_EVIDENCE_CAP` (16) |
| Search hits (extension) | `SEARCH_HIT_LIMIT` (8) |

Stop reasons are recorded on the report (`complete`, `max_depth`, `max_files`, …).

## Ownership

| Concern | Owner |
|---------|--------|
| Detect multi-hop / flow phrasing | Planner `relationship_tracing` |
| Ambient hops | Workspace Intelligence in ContextBundle |
| Lexical extension | Search Knowledge tool (Planner dispatch) |
| Live refs/defs | `language_server` tool (read-only) |
| Evidence + chain | `RelationshipTraceReport` on `PlannerResponse` |
| Diagnostics | Report chain + Coding Diagnostics / activity |

Simple “who calls this?” remains Symbol Navigation (C2.2). Lexical “where is
authentication handled?” remains Intelligent Code Search (C2.1).

## Constraints

- Read-only; no file modification; no destructive tools; no Rename.
- No second context assembler — uses the existing Context assemble + WI candidates.
- No unbounded investigation loops (hard budgets above).
- Diagnostics expose the investigation chain without dumping it as the sole UX.

## Constitutional audit

| Rule | Status |
|------|--------|
| LLM does not navigate FS/LSP | PASS — Planner mediates every retrieval |
| No second context assembler | PASS |
| No mutation / destructive tools | PASS |
| Bounded investigation | PASS — depth/files/symbols/steps/time |
| Diagnostics show chain | PASS — report + activity |
| Reuses WI / Search / LSP / Project / Git | PASS |
| Tests | PASS |

**Verdict: PASS**

### Remaining limitations

- Soft path quality depends on ambient WI enrichment.
- Search/LSP extensions only run when those tools are registered and budgets allow.
- Deterministic answer synthesis is evidence-based; conversational Reasoning can
  still deepen answers in later turns without owning navigation.

## Module

`crates/jaymi-planner/src/relationship_tracing.rs`

## Related

- [docs/symbol-navigation.md](symbol-navigation.md)
- [docs/code-search.md](code-search.md)
- [docs/planner.md](planner.md)
