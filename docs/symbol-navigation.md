# Symbol Navigation (Sprint C2.2)

Planner-owned deep semantic navigation through Workspace Intelligence and the
existing language_server Tool → LSP Provider path.

## Goal

Answer questions like:

- “Where is this function defined?”
- “Who calls this?”
- “What implements this trait?”
- “Where is this type used?”
- “Show me everything that depends on this.”

using Selection / cursor deixis (“this”, “here”, “this function”) from Workspace
Intelligence when present.

## Ownership

| Concern | Owner |
|---------|--------|
| Decide to navigate + extract kind | Planner (`symbol_navigation`) |
| Ambient symbol / refs / enclosing | Workspace Intelligence (EditorSnapshot) |
| Live definition / references / implementation | `language_server` Tool → LSP Provider |
| Conversation evidence (capped) | `SymbolNavigationReport` on `PlannerResponse` |
| Diagnostics | Report + Coding Diagnostics / activity |

Reasoning never calls LSP. UI never calls LSP providers directly.

## Paths

1. **WI soft path** — enclosing symbol, cursor symbol location, ambient references
   (callers / usages / dependents) without tools when Editor Intelligence has
   observations.
2. **LSP tool path** — when WI is insufficient and cursor/path are known, Planner
   dispatches read-only `LspOperation::{Definition,References,Implementation}`
   through the existing Intent → Tool route.
3. **Honest empty** — missing symbol / no LSP / no ambient data.

Lexical “where is authentication handled?” remains Intelligent Code Search (C2.1).
Call-site deixis (“places that call this function”) is owned by Symbol Navigation
(C2.2) as Callers — see [docs/symbol-navigation.md](symbol-navigation.md).

## Constraints

- Read-only only (no Rename, no edits, no mutation).
- No parallel symbol database.
- Results become evidence via Planner reports + existing Context assemble (WI
  candidates already in the Context pipeline).
- Conversation evidence capped (`CONVERSATION_EVIDENCE_CAP`).

## Constitutional audit

| Rule | Status |
|------|--------|
| Use existing LSP architecture | PASS — Tool → Provider only |
| Planner owns request | PASS — detect + soft/tool dispatch |
| LSP not exposed to Reasoning | PASS |
| No parallel symbol DB | PASS |
| Results via Context / evidence | PASS — WI candidates + report |
| Read-only / no rename / no mutation | PASS — Rename excluded; tests assert |
| Deixis from WI | PASS — cursor / symbol / references |
| Tests (def / refs / impls / missing / multi-file) | PASS |

**Verdict: PASS**

### Remaining limitations

- Callees require call hierarchy — not on the current LSP tool surface; honest empty.
- Implementations need a live language server for real trait-impl graphs (mock returns definition stand-ins).
- Ambient WI quality depends on host maintenance / LSP enrichment.

## Module

`crates/jaymi-planner/src/symbol_navigation.rs`

## Related

- [docs/code-search.md](code-search.md)
- [docs/editor-snapshot.md](editor-snapshot.md)
- [docs/planner.md](planner.md)
