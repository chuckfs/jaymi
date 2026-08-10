# Coding Mutation Recovery (Sprint C3.7)

## Goal

Make coding mutations safe when something goes wrong — without inventing a
parallel transaction system.

```text
Failure / cancel / stale
        │
        ▼
 Existing ExecutionPlan status + ExecutionSummary
        │
        ▼
 CodingMutationRecovery (actionable report)
        │
        ▼
 Further mutation ⇒ new Planner-owned plan + Review
```

## Failure scenarios covered

| Scenario | Detection | Behavior |
|----------|-----------|----------|
| File changed since investigation / review | Baseline hash / existence at Approve | Refuse batch before writes |
| Patch no longer applies | Original snippet missing / hash drift | Refuse; report `patch_no_longer_applies` |
| Write fails | Tool / provider error | Stop; report succeeded vs failed |
| Directory disappears | Parent missing at preflight | Refuse before writes |
| Generated patch invalid | Empty write content at apply | Refuse op; no silent invent |
| Second file fails after first succeeds | Ordered apply stop | `partial` + recovery; no continue |
| User cancels multi-file plan | Cancel while paused | No writes; skipped paths listed |

## Lifecycle vocabulary

Maps onto existing `ExecutionStatus` (+ `ExecutionSummary.partial`):

| Recovery phase | Plan status |
|----------------|-------------|
| planned | Draft / Ready / AwaitingReview |
| approved | Approved |
| executing | Executing |
| succeeded | Completed |
| failed | Failed (`partial=false`) |
| partially_completed | Failed (`partial=true`) |
| cancelled | Cancelled |

## Hard constraints

1. Detect stale source state **before** mutation (batch preflight + per-op recheck).
2. Never silently overwrite unrelated user changes.
3. Preserve review-time original snippets / hashes for recovery display.
4. Actionable recovery summary in conversation.
5. No automatic destructive rollback (unless a future tool safely supports it).
6. Never silently retry mutations.
7. Preserve Approval History and Execution Summary.
8. Further mutation requires a **new** Planner-owned plan + Review.
9. No parallel transaction system.

## Ownership

| Concern | Owner |
|---------|--------|
| Baselines at pause | Planner (`GenerationBaseline` on `PausedExecution`) |
| Preflight / refuse | Planner (`coding_mutation_recovery`) |
| Ordered apply + stop | Existing `execute_approved_generation` |
| Recovery report | `CodingMutationRecovery` on `PlannerResponse` |
| History | Approval History + Execution Summary (unchanged store path) |

## Pipeline

```text
Pause generation (C1.5 / C3.4–C3.6)
  → capture baselines (hash / existence / snippet)
Approve
  → preflight baselines (read-only)
  → if blocked: Failed + recovery (zero writes)
  → else apply ops in order with per-op recheck
  → on stop: ExecutionSummary (+ partial) + recovery
Cancel
  → Cancelled + recovery (skipped paths)
```

## Related

- [coding-apply.md](coding-apply.md) — C3.6 apply path
- [coding-diff-review.md](coding-diff-review.md) — C3.5 review surface
- [patch-generation.md](patch-generation.md) — C3.4 validation
- [planner.md](planner.md) — Execution Plans / Review Cards

## Out of scope

- Automatic compensating rollback of successful earlier ops
- Silent retry / heal loops
- A second coding transaction engine
- Git undo as part of coding recovery

## Constitutional audit (C3.7)

### Mutation safety chain

```text
Planner
  → Permission
  → Review
  → Approved ExecutionPlan
  → Preflight (C3.7)
  → Tool
  → Provider
```

| Check | Verdict |
|-------|---------|
| Stale detection before write | **PASS** |
| No silent overwrite | **PASS** |
| Clear phase vocabulary | **PASS** — maps ExecutionStatus |
| Actionable recovery | **PASS** — markdown + diagnostics |
| No auto destructive rollback | **PASS** |
| No silent retry | **PASS** |
| Approval History / Execution Summary preserved | **PASS** |
| New mutation ⇒ new reviewed plan | **PASS** — `requires_new_plan` |
| No parallel transaction system | **PASS** |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Safe, transparent changes | **PASS** |
| PRINCIPLES | Local, reviewed mutations | **PASS** |
| NON_GOALS | Not silent autonomy | **PASS** |
| ARCHITECTURE | Planner orchestrates | **PASS** |
| ROADMAP | C3.7 Coding Mutation Recovery | **PASS** — Current |

### Residuals

* Successful earlier ops in a partial batch are left in place and reported;
  compensating edits need a new reviewed plan.
* Mid-execute cooperative cancel is not a separate control plane — cancel is
  the Review Cancel of a paused multi-file plan (before tools run).
* Provider errors that look like missing parents are classified as
  `directory_missing` when the message matches; otherwise `write_failed`.
