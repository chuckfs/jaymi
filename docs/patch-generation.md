# Patch Generation (Sprint C3.4)

## Goal

Generate **precise proposed file modifications** from an approved Coding Plan —
without automatic application.

```
Coding Request
  → Investigation (C3.2)
  → Coding Plan (C3.3)
  → user approval (“Generate patches.” / “Generate the code.”)
  → Patch Generation (C3.4)
  → proposed changes
  → Review Before Action
```

Automatic patch application is **out of scope** for this sprint.

## What a proposed change carries

| Field | Meaning |
|-------|---------|
| file | Target path |
| original location | Line / symbol / note |
| change type | `create_file` / `modify_file` / `delete_file` / `rename_move` |
| original content | Anchor / reference for staleness checks |
| proposed content | Targeted body (not a blind whole-file rewrite) |
| rationale | Why this patch exists |

## Pipeline ownership

| Step | Owner |
|------|--------|
| Draft proposals from Coding Plan | **Planner** (`draft_proposals_from_plan`) |
| Elaborate patch content | **ReasoningEngine** (ConversationStream) when available |
| Read current file state | **Planner**-mediated `read_file` (validation only) |
| Validate / detect stale | **Planner** (`validate_proposal`) |
| Convert valid patches → `GenerationOp` | **Planner** |
| Pause for Review | Existing ExecutionPlan / Review Card path |
| Filesystem writes | Tools **after Approve only** (not this sprint’s success criterion) |

## Hard lines

1. Targeted patches — surgical replace or marked append; never silent whole-file invent.
2. Validate against current file state before becoming executable.
3. Detect stale / mismatched original context.
4. Never write directly to disk during generation.
5. The model never calls filesystem providers directly.
6. Patch elaboration stays behind ReasoningEngine.
7. Planner owns conversion into the existing execution architecture.
8. Preserve Review Before Action.
9. No automatic mutation.
10. On failure: original project unchanged + explicit explanation.

## Mutation vs C1.5

| Coding Plan origin | Path |
|--------------------|------|
| `change_investigation` / `mutation` | **C3.4** Patch Generation |
| `generation_planning` (Build Pong, …) | **C1.5** stub Code Generation |

Both still pause for Review Before Action before tools run.

## Diagnostics

`LastPlannerActivity` exposes `patch_generation_*` from
`PatchGenerationReport::to_diagnostics_summary()`.

## Soft path

Reuses the existing generation detect phrases (`Generate the code.` /
`Generate patches.` / `Apply the plan.` …) after a remembered Coding Plan.

## Out of scope

- Automatic application without Review Approve
- Silent disk writes from Reasoning / providers
- Redesigning C1.5 greenfield generation

## Related

- [coding-diff-review.md](coding-diff-review.md) — C3.5 multi-file +/− Review Card surface
- [coding-apply.md](coding-apply.md) — C3.6 apply after Approve
- [change-coding-plan.md](change-coding-plan.md) — C3.3 mutation Coding Plan
- [code-generation.md](code-generation.md) — C1.5 generation ops

## Constitutional audit

| Line | Status |
|------|--------|
| Planner Over Models | **PASS** — Planner validates + converts; Reasoning elaborates only |
| Conversation First | **PASS** — patch report + review card in conversation |
| Intelligence Through Context | **PASS** — plan + current file state |
| Projects First-Class | **PASS** — project paths grounded |
| Review Before Action | **PASS** — ExecutionPlan awaits Approve |
| Not Fully Autonomous | **PASS** — no automatic mutation |
| Offline First | **PASS** — local read/validate; Reasoning optional |
| No direct FS from model | **PASS** |
| Stale / invalid → unchanged | **PASS** |
| Targeted patches | **PASS** |
