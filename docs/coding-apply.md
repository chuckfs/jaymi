# Apply Approved Coding Changes (Sprint C3.6)

## Goal

Execute **approved** Coding Execution Plans through the existing mutation tools.
This is the first sprint where Jaymi may modify the project as a result of
conversational coding.

```text
User Request
  → Planner
  → C2 Investigation
  → CodingPlan
  → Patch Generation
  → Review
  → Approved ExecutionPlan
  → Tool
  → Provider
  → Project
```

## Hard constraints

1. Only execute after Planner approval (`ReviewIntent::Approve`).
2. Reuse `ExecutionPlan`, Permission Engine, ToolRisk, Review, `write_file`,
   `manage_path`, and the existing provider architecture.
3. Do **not** create a coding mutation bypass.
4. LLM never writes files directly.
5. Provider never decides whether a change is approved.
6. Planner remains the sole production execution owner.
7. Multi-file plans run in order; stop on first failure (no silent continue).
8. Capture per-op results and produce an `ExecutionSummary`.
9. No autonomous retry loops.
10. Git mutations remain on their own policy/review path.

## Ownership

| Step | Owner |
|------|--------|
| Approve | User → `ReviewIntent` → `Planner::resolve_review` |
| Resume frozen batch | Planner (`resume_paused`) |
| Ordered tool calls | Planner (`execute_approved_generation`) |
| Per-op / partial summary | Planner (`coding_apply`) |
| Disk I/O | Tools → Filesystem Provider |
| Memory / history | Experience stores `ExecutionSummary`; Approval History records result |

## Failure semantics

| Case | Behavior |
|------|----------|
| First op fails | Plan `Failed`, `partial=false`, project unchanged for that batch |
| Mid-batch failure | Plan `Failed`, `partial=true`, earlier ops remain; later ops **not** run |
| Cancel before Approve | Plan `Cancelled`, no tools |
| Duplicate Approve | Error — pause already consumed |
| Policy / permission Denied at prepare | Blocked before review; no pause; no writes |

## Conversation surface

On apply (success, failed, or partial), the Planner response content includes
`ExecutionSummary::render_conversation()` so the transcript shows goal, actions,
files edited, errors, and next suggested actions.

## Diagnostics

Coding Diagnostics continue to expose `execution_plan_lifecycle` and
`coding_diff_review_*` from the review turn; apply turns surface via
`execution_summary` / approval history (`execution_result`).

## Related

- [coding-diff-review.md](coding-diff-review.md) — C3.5 review surface
- [coding-mutation-recovery.md](coding-mutation-recovery.md) — C3.7 stale / partial / cancel recovery
- [patch-generation.md](patch-generation.md) — C3.4 proposals
- [code-generation.md](code-generation.md) — C1.5 ops → plans
- [coding-execution-plans.md](coding-execution-plans.md) — C1.6 universal card
- [planner.md](planner.md) — Execution Plans / Review Cards

## Out of scope

- Autonomous retries / self-healing loops
- Git commit/push as part of coding apply
- Provider- or model-owned writes
- A second coding-only execution engine

## Constitutional audit (C3.6)

### Required mutation chain

```text
Planner
  → Permission
  → Review
  → Approved ExecutionPlan
  → Tool
  → Provider
```

| Check | Verdict |
|-------|---------|
| Only after approval | **PASS** — `resume_paused` after `ReviewIntent::Approve` |
| Reuse ExecutionPlan / Review / tools | **PASS** |
| No coding mutation bypass | **PASS** |
| LLM never writes | **PASS** |
| Provider never approves | **PASS** — gate is Planner-owned |
| Multi-file ordered apply | **PASS** |
| Stop + report on failure | **PASS** — `partial` + errors, no continue |
| ExecutionSummary + history | **PASS** — conversation + approval history + memory store |
| No retry loops | **PASS** |
| Git separate | **PASS** |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Safe, transparent changes | **PASS** |
| PRINCIPLES | Local, reviewed mutations | **PASS** |
| NON_GOALS | Not silent autonomy | **PASS** — no retries / bypass |
| ARCHITECTURE | Planner orchestrates tools | **PASS** |
| ROADMAP | C3.6 Apply Approved Coding Changes | **PASS** — Current |

### Residuals

* Explicit UI gestures may auto-submit Approve via `complete_user_initiated`;
  they still use `ReviewIntent` on the same paused plan.
* Partial apply leaves earlier files changed; Jaymi reports that honestly and
  does not invent a compensating rollback in this sprint.
* Rename/delete ops still use `manage_path` and its Trash/permanent policy.
