# Agent Failure and Recovery (Sprint C5.9)

## Goal

Generalize C4 recovery from individual verification failures to entire
multi-step Coding Goals.

Planner owns classification and disposition. This sprint does **not** create a
parallel repair engine and never silently mutates.

```text
Failure → classify → disposition
  READ-ONLY investigate
  | propose new reviewed C3 Coding Plan
  | AwaitUser → Blocked
  | StopFailed → Failed
  | Exhausted (MAX_GOAL_RECOVERY_ATTEMPTS)
```

## Failure categories

| Category | Typical disposition |
|----------|---------------------|
| Investigation failure | Read-only investigate |
| Planning failure | Propose corrective plan |
| Approval rejection | AwaitUser → Blocked |
| Execution failure | Propose corrective plan |
| Partial execution | Propose corrective plan (honest; no rollback) |
| Verification failure | Propose corrective plan |
| Environment failure | AwaitUser → Blocked |
| Missing dependency | AwaitUser → Blocked |
| Ambiguous requirement | AwaitUser → Blocked |
| Stale project state | Propose corrective plan (fresh approval) |

## Soft-path order

```
… → agent_pause_resume_cancel (C5.8; also early-gated)
  → agent_failure_recovery (C5.9; also early-gated)
  → coding_goal_completion (C5.10; also early-gated)
  → agent_session_continuity (C5.11; also early-gated)
  → coding_edit → …
```

Phrases: “Recover this goal.” / “Recover from verification failure.” /
“Why did this goal fail?” / “Propose a corrective plan.” /
“Goal recovery status.”

C5.8 Pause/Resume/Cancel wins over recovery phrases.

## Bounds

| Bound | Value |
|-------|-------|
| Recovery attempts per goal | `MAX_GOAL_RECOVERY_ATTEMPTS` = 3 |

At the cap, disposition is **Exhausted** and the lifecycle moves toward **Failed**.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Classify + disposition (`apply_recovery`) |
| **C5.3 lifecycle** | BeginCorrection / Block / Fail / BeginInvestigation |
| **C5.6 autonomy** | `invalidate_on_corrective_plan` on corrective path |
| **C3 Coding Plan** | Proposal-only corrective plan (Review Before Action) |
| **C4.4** | Remains the verification-grounded soft path for “Fix the build…” |
| **Experience / UI** | Mirrors `agent_recovery` diagnostics |

## Guards

1. Never silently mutate to recover.
2. Never silently retry a mutation.
3. Never reuse approval for a materially changed plan.
4. Preserve completed work.
5. Preserve failed task state.
6. Explain what happened.
7. Corrective path authors a **new** C3 Coding Plan (proposal).
8. User input → Blocked (AwaitingUser semantics).
9. Cannot safely continue → Failed.
10. Partial application remains honest.
11. No automatic rollback.
12. Preserve recovery history on the Coding Goal.
13. Bound recovery attempts.

## Constitutional audit

| Rule | Status |
|------|--------|
| No hidden / silent mutation | PASS |
| No silent mutation retry | PASS |
| No approval reuse for changed plan | PASS |
| Preserve completed work | PASS |
| Preserve failed task state | PASS |
| Explain what happened | PASS |
| Corrective plan is new C3 proposal | PASS |
| AwaitUser → Blocked | PASS |
| Unsafe continue → Failed / Exhausted | PASS |
| Partial honesty / no auto rollback | PASS |
| Recovery history preserved | PASS |
| Bounded attempts (`MAX_GOAL_RECOVERY_ATTEMPTS`) | PASS |
| User can cancel (C5.8) | PASS |
| No parallel repair engine | PASS |
| Planner-owned | PASS |

**Verdict: PASS**
