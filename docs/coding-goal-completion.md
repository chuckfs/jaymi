# Coding Goal Completion (Sprint C5.10)

## Goal

Teach Jaymi to determine when a Coding Goal is **actually complete**.

Completion is **evidence-based**. A goal is not complete merely because the last
task ran, the model says it is done, files were modified, or no immediate error
occurred.

```text
Acceptance criteria + tasks + verification + blockers
  → Planner assesses each criterion:
      Satisfied | Unsatisfied | Unknown | Waived (user-explicit)
  → Ready → SuccessfullyCompleted
  | Incomplete / FailedVerification / Blocked → stay open
  | User end incomplete → UserEnded (Cancelled; ≠ SuccessfullyCompleted)
```

## Soft-path order

```
… → agent_pause_resume_cancel (C5.8; also early-gated)
  → agent_failure_recovery (C5.9; also early-gated)
  → coding_goal_completion (C5.10; also early-gated)
  → agent_session_continuity (C5.11; also early-gated)
  → coding_goal (C5.1 create/status/block; Complete yields to C5.10)
  → …
```

Phrases: “Assess goal completion.” / “Is this goal complete?” /
“Mark this goal complete.” / “Waive remaining criteria because …” /
“Waive failed verification because …” /
“End this goal incomplete because …”

C5.8 Cancel/Pause and C5.9 Recover win over completion phrases.

## What completion considers

| Signal | Role |
|--------|------|
| Acceptance criteria | Each must be Satisfied, Unsatisfied, Unknown, or Waived |
| Task completion | Open / failed tasks block Satisfied |
| Execution results | Completed work / remaining work |
| Verification results | Failed verification blocks success unless waived |
| Known failures | Failed tasks → Unsatisfied |
| Blockers | Blocked goals cannot become Completed |
| User requirements | Explicit waivers and user-end |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns criterion evaluation + completion disposition |
| **Model** | May propose; **cannot** declare authoritative completion |
| **C5.3 lifecycle** | `Complete` only with sufficient `CompletionEvidence` |
| **C5.8** | Cancel remains user cancel (not SuccessfullyCompleted) |
| **Experience / UI** | Mirrors `goal_completion` diagnostics |

## Ending kinds

| Kind | Coarse status | Meaning |
|------|---------------|---------|
| `successfully_completed` | Completed | Evidence-based success |
| `user_ended` | Cancelled | User explicitly ended while incomplete |

## Guards

1. Planner owns completion state.
2. Model cannot declare authoritative completion.
3. Every criterion is Satisfied / Unsatisfied / Unknown / Waived.
4. Unknown is never represented as Satisfied.
5. Failed verification blocks success unless resolved/waived.
6. Blocked goals cannot become Completed.
7. Completion summary uses actual evidence.
8. Preserve complete goal history (`GoalCompletionState.history`).
9. Do not invent success.
10. User may end incomplete; UserEnded ≠ SuccessfullyCompleted.
11. No silent mutation on assess / waive / complete / end.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner owns completion | PASS |
| Model not authoritative | PASS |
| Criterion states (4-way) | PASS |
| Unknown ≠ Satisfied | PASS |
| Failed verification blocks (unless waived) | PASS |
| Blocked cannot Complete | PASS |
| Evidence-based summary | PASS |
| History preserved | PASS |
| No invented success | PASS |
| User-ended ≠ successfully completed | PASS |
| No silent mutation | PASS |
| C5.8 cancel still works | PASS |

**Verdict: PASS**
