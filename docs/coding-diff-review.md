# Coding Diff Review (Sprint C3.5)

## Goal

Integrate proposed coding patches with Jaymi’s existing **Review Before Action**
system so the user can inspect what Jaymi wants to change before anything is
written.

```text
Jaymi proposes:

planner.rs
+18
−7

context.rs
+9
−2

[Approve]  [Modify]  [Cancel]
```

## Hard constraints

1. Reuse `ExecutionPlan` / `ReviewCard` / `ReviewIntent` — **no** coding-only
   approval system.
2. Every mutation pauses in the Planner-owned review pipeline.
3. No file mutation before Approve.
4. Duplicate approval must not execute twice.
5. Coding UI gestures must not create a hidden approval bypass (still
   `ReviewIntent` → `resolve_review`).
6. Do not implement application beyond the existing approved execution path.

## Surface (same Review Card)

| Section | Source |
|---------|--------|
| **Execution Plan** | Generation steps / ops |
| **Files affected** | Per-file blocks with `+N` / `−M` (C3.5) |
| **Proposed operations** | create / modify / delete / rename labels |
| **Diff Preview** | Multi-file `ActionPreview` (`UnifiedDiff`) |
| **Rationale** | Op + Coding Plan + patch rationales |
| **Dependencies** | Coding Plan deps / step ordering |
| **Validation** | Patch validation status + “no writes until Approve” |
| **Risk** | `EstimatedRisk` (+ coding risk notes folded into why) |
| **Approve / Modify / Cancel** | Existing `ReviewIntent` only |

## Pipeline

```text
CodeGeneration / PatchGeneration (C1.5 / C3.4)
        │
        ▼
 coding_diff_review::build_coding_diff_review   ★ read-only before peek
        │  ActionPreview + CodingDiffReviewSurface
        ▼
 prepare_code_generation_execution
        │  ExecutionPlan + pause (RequiresApproval)
        ▼
 ReviewCardModel::from_plan + apply_coding_diff_surface
        │
        ▼
 ReviewIntent::Approve | Modify | Cancel
        │
        ▼
 resolve_review → tools only on Approve
```

## Ownership

| Concern | Owner |
|---------|--------|
| Multi-file diff stats | Planner (`coding_diff_review`) |
| Pause / resume / revise | Existing ExecutionPlan path |
| Card presentation | ReviewCard (enriched, not forked) |
| Disk writes | Tools after Approve only |

## Diagnostics

`LastPlannerActivity` exposes:

- `coding_diff_review_*` — files / +add / −del summary
- `execution_plan_lifecycle` — plan id, status, revision, parent, awaiting_review, risk

## Related

- [coding-execution-plans.md](coding-execution-plans.md) — C1.6 universal card
- [patch-generation.md](patch-generation.md) — C3.4 proposals → generation ops
- [code-generation.md](code-generation.md) — C1.5 ops → reviewed plans
- [planner.md](planner.md) — Execution Plans / Review Cards

## Out of scope

- Automatic apply without Approve — see [coding-apply.md](coding-apply.md) (C3.6)
- A second coding-only approval modal or bypass path
- Application beyond existing `execute_approved_generation`

## Constitutional audit (C3.5)

### Ownership

```text
Proposed patches
        │
        ▼
 Existing ExecutionPlan + Review Card     ★ no special coding gate
        │
        ▼
 Multi-file Diff Preview (+/− files)      ★ presentation enrichment only
        │
        ▼
 ReviewIntent → resolve_review → Tools    ★ editor / UI never write first
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Mutation gate | Execution Plan + Review | **PASS** |
| Coding-specific approval system | None | **PASS** |
| Pre-approval writes | Forbidden | **PASS** |
| Duplicate Approve | Pause store NotFound | **PASS** |
| Modify child plan | Existing revise_paused_plan | **PASS** |
| Cancel discards mutation | Invalidate pause | **PASS** |
| Conversational Review Cards | Preserved + enriched | **PASS** |
| UI gesture bypass | Still ReviewIntent | **PASS** |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Safe, transparent changes | **PASS** — per-file +/− before write |
| PRINCIPLES | Local, reviewed mutations | **PASS** |
| NON_GOALS | Not silent tool sprawl | **PASS** — same pause path |
| ARCHITECTURE | Planner orchestrates | **PASS** |
| ROADMAP | C3.5 Coding Diff Review | **PASS** — Current |

### Residuals

* `complete_user_initiated` still auto-submits Approve after an **explicit** UI
  gesture; it does not skip the ExecutionPlan / ReviewIntent lifecycle.
* Diff bodies may truncate for large files (`PREVIEW_MAX_*`); summary +/− remains.
* Rename/move ops show path mapping without inventing a full textual diff.
