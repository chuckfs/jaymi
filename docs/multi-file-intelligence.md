# Multi-file Intelligence (Sprint C1.7)

**Status: Current** — Reason across the project for coordinated coding changes
using Workspace Intelligence only. Mutations still go through GenerationOps →
Execution Plans → Review Before Action. No direct LLM editing.

## Examples

| Ask | Planner kind | Typical WI inputs |
|-----|--------------|-------------------|
| Rename symbol | `rename_symbol` | Current file + editor references / open tabs |
| Move module | `move_module` | Active file + project layout + open/recent imports |
| Extract class | `extract_class` | Active file + enclosing type + destination beside source |
| Split file | `split_file` | Active file → source + companion module |

```text
Rename symbol / Move module / Extract class / Split file
        │
        ▼
 Planner detects MultiFileKind
        │
        ▼
 Workspace Intelligence → affected files   ★ paths never LLM-invented
        │
        ▼
 MultiFilePlan (summary + risk)
        │
        ▼
 GenerationOp[] → Execution Plan           ★ same C1.5 / C1.6 gate
        │
        ▼
 Review Card (Files · Diff · Risk · A/M/C)
```

## Constraints

| Forbidden | Required |
|-----------|----------|
| LLM invents / edits file paths | Affected files from ContextBundle / WI only |
| Provider or Monaco writes | Tools after Review Approve only |
| Skipping Review Before Action | Universal Execution Plan pause |
| New coding-only approval path | Shared C1.6 Review Card |
| Filesystem scan outside WI | Editor / project / memory sections already assembled |

## Pipeline

```text
UserRequest (multi-file phrasing)
        │
        ▼
Context assemble (coding workspace → editor provider)
        │
        ▼
plan_from_bundle(kind, goal, ContextBundle)
        │  AffectedFile[] + change_summary
        ▼
code_generation_from_plan → begin_code_generation
        │  CreateFile / ModifyFile / DeleteFile stubs
        ▼
PausedExecution + Review Card
   Approve → tools    Cancel → nothing written
```

When WI has no concrete paths, the Planner returns the Multi-file markdown
scaffold **without** an Execution Plan (gather context first) — still no writes.

## API surface

| Entry | Behavior |
|-------|----------|
| `"Rename symbol"` / `"Move module"` / `"Extract class"` / `"Split file"` | Detect → WI plan → reviewed generation when paths exist |
| `PlannerResponse.multi_file` | Observational `MultiFilePlan` until Approve |
| Coding Plan detect | Yields to multi-file when phrasing matches C1.7 |

## Ownership

| Concern | Owner |
|---------|--------|
| Kind detect + affected-file selection | **Planner** (`multi_file`) |
| Context assemble / sections | **Context Engine** (WI) |
| Ops + Execution Plan + pause | **Planner** (`code_generation` / prepare) |
| Review Approve / Cancel / Modify | **Planner** (`resolve_review`) |
| Mutations | Tools → Providers after Approve |
| Intent / Capability catalog | Unchanged |

## Related

- [workspace-intelligence.md](workspace-intelligence.md) — WI source of truth
- [code-generation.md](code-generation.md) — C1.5 ops → reviewed plans
- [coding-execution-plans.md](coding-execution-plans.md) — C1.6 Review Card
- [coding-plan.md](coding-plan.md) — C1.4 planning (yields to C1.7)
- [planner.md](planner.md) — orchestration

## Constitutional audit (C1.7)

### Ownership

```text
Rename / Move / Extract / Split
        │
        ▼
 Planner + Workspace Intelligence         ★ affected files from WI only
        │
        ▼
 GenerationOps → Execution Plan           ★ no LLM file edits
        │
        ▼
 Review Before Action (C1.6 card)         ★ no coding bypass
        │
   Approve
        │
        ▼
 Tool → Provider                          ★ editor never writes
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Affected-file determination | Planner from WI | Pass |
| Path invention by LLM | Forbidden | Pass |
| Direct LLM / editor edits | Forbidden | Pass |
| Review Before Action | Universal Execution Plan | Pass |
| Intent / Capability ownership | Unchanged | Pass |
| New context / FS scan system | None — uses ContextBundle | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Transparent, reviewed multi-file change | Pass |
| PRINCIPLES | Local WI + reviewed mutations | Pass |
| NON_GOALS | No silent codegen / editor sprawl | Pass |
| ARCHITECTURE | Planner orchestrates; Context assembles | Pass |
| ROADMAP | C1.7 Multi-file Intelligence | Pass — Current |

### Residuals

* Editor Context Provider needs coding-workspace relevance (≥ threshold) to
  contribute `current_file`; multi-file asks outside Coding Workspace may
  return the empty-path scaffold until the coding surface is active.
* Rename stubs prefer WI-observed paths; live LSP workspace-edit rename remains
  the richer path when a structured LSP rename request is present.
* Destination paths for move/extract/split are **proposed** from observed layout
  (parent dir / `src/` …) — still Review-gated stubs, not silent moves.
