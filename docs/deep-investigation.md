# Deep Codebase Investigation (Sprint C2.4)

Planner-owned read-only investigation for questions that need evidence from
multiple parts of a codebase.

## Goal

Answer questions like:

- “Why is this slow?”
- “How does authentication work?”
- “Where should this feature be implemented?”
- “Why does this state sometimes disappear?”
- “What would be affected if I changed this?”

by planning required evidence, then combining Search, symbols, project context,
file reads, and relationship following — without letting the model invent
retrieval actions.

## Investigation lifecycle

```
Question
→ Planner determines required evidence
→ Search / symbols / project context
→ Read relevant files
→ Follow relationships when necessary
→ Context Policy selects evidence
→ ContextEngine assembles
→ Reasoning produces answer (when implemented)
```

Every retrieval is Planner-mediated. Reasoning never calls Search / LSP / FS
directly. ContextEngine remains the sole `ContextBundle` factory. Workspace
Intelligence stays observational. Providers stay passive.

## Limits (anti-loop)

| Budget | Cap |
|--------|-----|
| Steps | `MAX_STEPS` (12) |
| Files inspected | `MAX_FILES` (10) |
| Symbols followed | `MAX_SYMBOLS` (8) |
| Relationship depth | `MAX_DEPTH` (3) |
| Elapsed time | `MAX_ELAPSED_MS` (3500) |
| Conversation evidence | `CONVERSATION_EVIDENCE_CAP` (14) |
| Search hits | `SEARCH_HIT_LIMIT` (8) |
| File reads | `MAX_FILE_READS` (3) |

Stop reasons and budget snapshots live on the report for diagnostics.

## Ownership

| Concern | Owner |
|---------|--------|
| Detect multi-source investigation phrasing | Planner `deep_investigation` |
| Evidence plan (what to gather) | Planner `plan_required_evidence` |
| Ambient symbols / refs / project / git | Workspace Intelligence in ContextBundle |
| Lexical extension | Search Knowledge tool (Planner dispatch) |
| File contents | `read_file` tool (Planner dispatch, planned only) |
| Live refs | `language_server` (read-only, when plan includes follow-refs) |
| Bundle assembly | ContextEngine only |
| Conversation findings + gaps | `DeepInvestigationReport` on `PlannerResponse` |
| Diagnostics (steps / searches / files / symbols / context / timing / budget) | Report + Coding Diagnostics / `LastPlannerActivity` — **not** the chat body |

Narrower modes keep priority when they match first at soft-path order:

1. Relationship Tracing (C2.3)
2. Deep Investigation (C2.4)
3. Symbol Navigation (C2.2)
4. Code Search (C2.1)

## Honesty

If evidence is thin, Jaymi states **what could not be established**. Conversation
markdown includes findings and gaps; it does not dump raw budget / step
diagnostics. Unsupported architectural claims are avoided when evidence is thin.

## Constraints

- Read-only; no file modification; no destructive tools; no Rename.
- Model must not independently invent retrieval actions.
- Planner remains responsible for orchestration.
- ContextEngine remains the sole ContextBundle factory.
- Workspace Intelligence remains observational.
- Providers remain passive.
- No second context assembler.
- Bounded and observable investigation.

## Constitutional audit

| Rule | Status |
|------|--------|
| Read-only only | PASS — Search / read_file / LSP refs; no write/manage/terminal/rename |
| Model does not invent retrieval | PASS — evidence plan + Planner dispatch only |
| Planner owns orchestration | PASS — `complete_deep_investigation` |
| ContextEngine sole bundle factory | PASS — re-assemble via `assemble_with` after search hits |
| WI observational / providers passive | PASS |
| Bounded investigation | PASS — steps/files/symbols/depth/time |
| Explicit gaps when insufficient | PASS — gaps section + honest answer synthesis |
| No unsupported architectural claims | PASS — thin-evidence wording |
| Diagnostics not in normal conversation | PASS — `to_markdown` findings/gaps only; chain/budget via Coding Diagnostics + logs |
| Tests | PASS |

**Verdict: PASS**

### Remaining limitations

- Soft-path quality depends on ambient WI enrichment and registered tools.
- Search / read / LSP extensions only run when those tools are registered and
  the evidence plan / budgets allow.
- When Reasoning is unimplemented, the conversation body is the Planner report;
  when Reasoning is available, it answers from a Planner-authored evidence brief.

## Module

`crates/jaymi-planner/src/deep_investigation.rs`

## Related

- [docs/relationship-tracing.md](relationship-tracing.md)
- [docs/symbol-navigation.md](symbol-navigation.md)
- [docs/code-search.md](code-search.md)
- [docs/planner.md](planner.md)
