# Code Generation (Sprint C1.5)

**Status: Current** — Generate file ops from approved Coding Plans via reviewed Execution Plans.

## Goal

After a Coding Plan (C1.4), generation produces typed operations and the Planner
converts them into Execution Plans. Review Before Action remains mandatory.
Providers and LLMs never write files directly.

## Operations

| Op | Tool binding | Permission |
|----|--------------|------------|
| `CreateFile` | `write_file` | filesystem write |
| `ModifyFile` | `write_file` | filesystem write |
| `DeleteFile` | `manage_path` delete | filesystem delete |

Ops are Planner-owned (`GenerationOp`). Content stubs are Planner-authored from
the Coding Plan — not live LLM edits of the workspace.

## Constraints

| Forbidden | Required |
|-----------|----------|
| Provider writes | Planner → Tool → Provider only |
| LLM edits of files | Ops → Execution Plan → Review → Tool |
| Skipping review | Review Before Action for every generation batch |
| Silent codegen on C1.4 plan turns | Separate “Generate the code.” turn |

## Pipeline

```text
Coding Plan (C1.4) remembered
        │
        ▼
UserRequest ("Generate the code." / begin_code_generation)
        │
        ▼
Planner materializes CreateFile / ModifyFile / DeleteFile
        │  CodeGeneration { operations }
        ▼
ExecutionPlan (multi-step, tools = write_file / manage_path)
        │  ReviewRequirement::Required (always)
        ▼
PausedExecution + Review Card
        │
   Approve ──► Planner executes generation batch (tools only)
   Cancel  ──► nothing written
```

## API surface

| Entry | Behavior |
|-------|----------|
| `"Generate the code."` / `"Implement the coding plan."` / `"Apply the plan."` | Uses last Coding Plan → ops → reviewed plan |
| `Planner::begin_code_generation(CodeGeneration)` | Explicit ops batch → reviewed plan |
| `PlannerResponse.code_generation` | Proposed ops (observational until Approve) |

## Ownership

| Concern | Owner |
|---------|--------|
| Op materialization | **Planner** (`code_generation`) |
| Execution Plan + pause | **Planner** |
| Review Approve / Cancel / Modify | **Planner** (`resolve_review`) |
| Tool execution | Tool Orchestrator (after Approve only) |
| Filesystem I/O | Filesystem Provider (via tools) |
| Intent / Capability catalog | Unchanged |

## Related

- [coding-plan.md](coding-plan.md) — C1.4 planning only
- [planner.md](planner.md) — Execution Plans / Review Cards
- [coding-actions.md](coding-actions.md) — toolbar intents (no direct writes)
- [tools.md](tools.md) — `write_file` / `manage_path`
- [coding-execution-plans.md](coding-execution-plans.md) — C1.6 Review Card surface

## Constitutional audit (C1.5)

### Ownership

```text
"Generate the code."
        │
        ▼
 Planner materializes GenerationOp[]     ★ owns ops (not LLM / provider)
        │
        ▼
 Planner ExecutionPlan + pause           ★ Review Before Action mandatory
        │
   Approve
        │
        ▼
 Tool Orchestrator → write_file /
 manage_path → Filesystem Provider       ★ providers never self-start writes
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Create/Modify/Delete ops | Planner | Pass |
| Conversion to Execution Plan | Planner | Pass |
| Review Before Action | Mandatory gate | Pass |
| Provider direct writes | Forbidden | Pass |
| LLM direct edits | Forbidden | Pass |
| Mutations | Planner-owned pipeline | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Help users change work safely | Pass — generate after plan + review |
| PRINCIPLES | Local, transparent, reviewed mutations | Pass |
| NON_GOALS | Not silent tool sprawl / not LLM product | Pass — typed ops + Review Card |
| ARCHITECTURE | Planner orchestrates; tools execute | Pass |
| ROADMAP | C1.5 Code Generation | Pass — Current |

### Residuals

* Soft-path stubs are Planner templates, not full product implementations.
* Multi-op Approve runs the whole batch under one reviewed plan (steps listed).
* Delete via Trash may be unavailable in restricted environments — Permanent remains Planner-chosen.
* Coding Actions Edit/Refactor still proposal-only until they emit GenerationOps.
