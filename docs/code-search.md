# Intelligent Code Search (Sprint C2.1)

Planner-owned natural-language code search across the current project.

## Goal

Answer questions like:

- “Where is authentication handled?”
- “Find where the score gets updated.”
- “Where is ContextBundle created?”
- “Show me all the places that call this function.”
- “Find the code responsible for opening Settings.”

without a special search workspace, without dumping the repository into the model,
and without a parallel search/context stack.

## Ownership

| Concern | Owner |
|---------|--------|
| Decide to search + extract query | Planner (`code_search` + Decision Engine) |
| Retrieve hits | Search Knowledge Tool → Search Engine |
| Scope to project | Planner `scope_search_request` |
| Citations / evidence | Existing Search → Citation pipeline |
| Conversation evidence (capped) | `CodeSearchReport` on `PlannerResponse` |
| Diagnostics (what was searched / selected) | Report diagnostics + Coding Diagnostics / activity |

UI never calls Search or providers directly.

## Paths

1. **Lexical / project search** — NL phrases map to `Intent::SearchKnowledge` with
   `files_only` + hit limit. Results become citations; Planner rewrites the
   conversation body as a capped `CodeSearchReport` markdown block.
2. **Find references (deixis)** — Prefer Symbol Navigation (C2.2) for
   “who calls this” / “places that call this function”. Code Search still
   supports lexical responsibility questions and structured search.

Semantic / hybrid ranking remains whatever the Search Engine already does for
`search_knowledge` — no second index.

## Constraints

- Read-only (Safe search tool; no writes, no manage_path, no terminal).
- Conversation evidence capped (`CONVERSATION_EVIDENCE_CAP`); excess stays in diagnostics.
- Context Policy / budgets unchanged — search hits enter via citations / Context assemble.
- No UI → provider bypass.
- Conversation First — ordinary chat questions trigger search when phrasing matches.

## Constitutional audit

| Rule | Status |
|------|--------|
| Read-only search | PASS — `search_knowledge` (Safe) or WI soft path; no write/manage/terminal |
| Planner owns search decision | PASS — Decision Engine + `code_search::detect` |
| Results via existing context / citations | PASS — Tool citations → `CodeSearchReport`; no parallel index |
| Lexical + semantic where infra exists | PASS — Search Engine strategies unchanged; WI refs for call sites |
| Evidence (file / line / match / relevance / preview) | PASS — from Citation / EditorReference |
| No repo dump / respect budgets | PASS — hit limit + conversation cap |
| No mutation / no extra tools | PASS — tests assert tool id |
| No UI → provider bypass | PASS — Application → Planner → tool only |
| Diagnostics without polluting chat | PASS — activity + report diagnostics; chat gets capped markdown |

**Verdict: PASS** (PARTIAL only if Search Engine has no content indexed — empty honest report).

## Module

`crates/jaymi-planner/src/code_search.rs`

## Related

- [docs/planner.md](planner.md)
- [docs/workspace-intelligence.md](workspace-intelligence.md)
- Layer 3 Search Engine
