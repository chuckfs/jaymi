# Workspace Intelligence

**Status: Current** — umbrella for Coding observation consumed by Context and
Planner (Sprints B2.1–B2.13.3; documentation synchronized B2.13.4).

Workspace Intelligence is the host-owned, ambient-maintained picture of the
Coding workspace. It is **not** Conversation Memory and **not** a ContextBundle.

## Ownership (constitutional)

```text
CodingState / Monaco / FS / GitProvider (read-only) / Terminal
        │  observation only
        ▼
 Application ContextMaintenance          ★ Workspace owns observation
        │  completed snapshots
        ▼
 prepare_context_session merges
        │
        ▼
 ContextSessionInputs
        │
   ┌────┴────┐
   ▼         ▼
 Planner   Context Providers             ★ Planner owns resolution / orchestration
 (deixis)  propose_candidates            ★ Providers remain passive
   │         │
   ▼         ▼
 AssembleHints → ContextEngine           ★ ContextEngine assembles ContextBundle
```

| Concern | Owner |
|---------|--------|
| Observation (FS / Monaco / ambient LSP enrich / read-only Git / terminal) | Application `ContextMaintenance` + CodingState |
| When to assemble / deixis binding | Planner |
| ContextBundle factory | ContextEngine |
| Providers | Propose candidates only — never assemble, never own observation |

## Shipped surface (all Current)

| Piece | Sprint | Doc | Role |
|-------|--------|-----|------|
| WorkspaceSnapshot | B2.1 / B2.2 / B2.13.2 | [workspace-snapshot.md](workspace-snapshot.md) | Project root, open files, selection, toolchain markers (ambient-only refresh) |
| EditorSnapshot | B2.3 / B2.13.3 | [editor-snapshot.md](editor-snapshot.md) | Cursor, selection text/range, symbols, diagnostics, hover |
| ProjectSnapshot | B2.4 | [project-snapshot.md](project-snapshot.md) | Languages, package manager, layout |
| GitSnapshot | B2.5 | [git-snapshot.md](git-snapshot.md) | Branch / dirty / staged / commits (**Current**) |
| RuntimeSnapshot | B2.6 | [runtime-snapshot.md](runtime-snapshot.md) | Terminal / build / test outcomes (**Current**) |
| Context Candidate Graph | B2.7 / B2.13.1 | [context-candidates.md](context-candidates.md) | `propose_candidates` → Policy → materialize (**Current**) |
| Context Selection | B2.8 | [context-selection.md](context-selection.md) | Deterministic feed profiles |
| Workspace Memory | B2.9 | [workspace-memory.md](workspace-memory.md) | Recent edits / opens / builds / objective |
| Environmental Resolution | B2.10 | [environmental-resolution.md](environmental-resolution.md) | Planner binds `this` / `it` / `why?` |
| Coding Actions | C0.1 | [coding-actions.md](coding-actions.md) | Quick Action Bar → typed Planner intents |
| Coding Understanding | C1.1 | [coding-understanding.md](coding-understanding.md) | Structured understand-before-acting from WI |
| Project Understanding | C1.2 | [project-understanding.md](project-understanding.md) | Whole-project orientation (no tools / plans / edits) |
| Coding Review | C1.3 | [coding-review.md](coding-review.md) | Structured review (no edits / execution / plans) |
| Coding Plan | C1.4 | [coding-plan.md](coding-plan.md) | Generation planning (no codegen / tools / writes) |
| Code Generation | C1.5 | [code-generation.md](code-generation.md) | Ops → Execution Plan → Review → tools |
| Coding Execution Plans | C1.6 | [coding-execution-plans.md](coding-execution-plans.md) | Universal Review Card for coding mutations |
| Workspace Diagnostics | B2.11 | [workspace-diagnostics.md](workspace-diagnostics.md) | Developer Diagnostics only |
| Constitutional Audit | B2.12 | — | Ownership verified; residuals closed in B2.13.x |
| Docs sync | B2.13.4 | this file | Documentation matches implementation |

## Monaco selection (B2.13.3)

Monaco reports selection via IPC into CodingState. Ambient refresh publishes
`current_selection` / `EditorSnapshot.selection` /
`WorkspaceSnapshot.active_selection`. Environmental Resolution binds
`"Explain this."` / `"Rename this."` / `"Clean this up."` from that data.
Planner and ContextEngine never import Monaco types.

## Context sources

**Current** Workspace Intelligence feeds assemble through Context Providers and
session snapshots (see [context.md](context.md)).

**Target** (not WI): Notes / Messages / Browser history as first-class context
feeds. Deep Git history productization belongs to Project Engine Target — not
to be confused with ambient **GitSnapshot** status, which is Current.

## Related

* [context.md](context.md) — Context Engine / providers / bundle sections
* [context-maintenance.md](context-maintenance.md) — ambient refresh ownership
* [providers.md](providers.md) — tool providers vs Context Providers
* [experience.md](experience.md) — Coding Workspace UX
* [performance.md](performance.md) — no sync prepare probes
