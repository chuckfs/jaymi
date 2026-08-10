# Coding Plan from Change Investigation (Sprint C3.3)

## Goal

Turn a completed [`Change Investigation`](change-investigation.md) into a
concrete Planner-owned [`CodingPlan`](coding-plan.md).

C1.4 greenfield generation planning remains. C3.3 reuses the **same**
`CodingPlan` type so a later slice can translate it into the existing
ExecutionPlan / generation path — without inventing a second plan system.

## Example

User: “Add image generation.”

Investigation touches Settings, Model Registry, providers, capabilities,
configuration.

Coding Plan (proposal only):

1. Add image provider abstraction  
2. Register provider  
3. Add configuration  
4. Add Settings UI  
5. Add tests  
6. Update documentation  

## Required sections

| Section | Role |
|---------|------|
| Objective | User goal |
| Rationale | Why these steps — grounded in investigation |
| Plan | Ordered steps |
| Step ordering | Explicit dependencies between steps |
| Files to Create / Modify / Remove | Proposal paths only |
| Symbols affected | From investigation |
| Dependencies | From investigation / WI |
| Tests | Observed or proposed coverage |
| Documentation | Doc candidates |
| Estimated Risk | Observational |
| Unresolved questions | Ambiguity stays visible |
| Constitutional conflicts | Flagged — never silently designed around |

## Hard lines

1. Ground the plan in C3.2 evidence.  
2. Do **not** generate file contents.  
3. Do **not** modify files.  
4. Do **not** execute tools while authoring the plan.  
5. Do **not** create an executable mutation ExecutionPlan in this sprint.  
6. Prefer `CodingPlan` (translatable later) over a parallel plan type.  
7. Planner owns the plan; Reasoning may elaborate later but cannot execute.  
8. If the request conflicts with constitutional architecture, **flag** it.

## Pipeline

```
Coding Edit Intent (C3.1)
  → Change Investigation (C3.2)
  → plan_from_change_investigation (C3.3)
  → conversation markdown + PlannerResponse.coding_plan
  → (later) reviewed generation / ExecutionPlan
```

No new soft-path detect — C3.3 runs inside `complete_coding_edit_intent`.

| Mode | Plan behavior |
|------|----------------|
| Understood + Full investigation | Full mutation plan grounded in affected files/symbols |
| NeedsClarification / Light | Deferred steps; unresolved questions only — no invented paths |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Authors `CodingPlan` via `plan_from_change_investigation` |
| **CodingPlan** | Shared C1.4 / C3.3 structure (`origin=change_investigation`) |
| **ReasoningProvider** | Optional later elaboration — never executes the plan |
| **Tools / writes** | Forbidden in this turn |

## Diagnostics

`LastPlannerActivity` exposes `coding_plan_*` from `to_diagnostics_summary()`.

## Soft-path order

```
… → coding_edit (C3.1 + C3.2 + C3.3) → review → …
```

## Out of scope

- Automatic application without Review Approve
- Generating / applying patches without C3.4 validation — see [patch-generation.md](patch-generation.md)
- C3.7+ Git undo / compensating rollback tooling beyond recovery reports

## Constitutional audit

| Line | Status |
|------|--------|
| Planner Over Models | **PASS** — Planner authors the plan |
| Conversation First | **PASS** — markdown plan in conversation |
| Intelligence Through Context | **PASS** — grounded in investigated Context |
| Projects First-Class | **PASS** — project paths from evidence |
| Review Before Action | **PASS** — no writes; bypass phrases flagged |
| Not Fully Autonomous | **PASS** — planning only; ambiguities visible |
| Offline First | **PASS** — no cloud-required plan authorship |
| Grounded in C3.2 | **PASS** — files/symbols from investigation |
| No file contents / tools / mutation | **PASS** |
| Same CodingPlan type | **PASS** — origin distinguishes C1.4 vs C3.3 |
| Conflicts flagged | **PASS** — `constitutional_conflicts` |
