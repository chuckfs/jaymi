# Agent Pause, Resume, and Cancellation (Sprint C5.8)

## Goal

Give users reliable control over active Coding Goals.

Planner owns transitions. Tool / stream interruption reuses **existing**
cancellation semantics — this sprint does **not** create a parallel cancellation
engine and never silently undoes successful mutations.

```text
Pause  → stop new ops; preserve goal / task / plan / verification; resumable
Resume → revalidate project fingerprint; refuse stale approved mutation
Cancel → stop future work; invalidate approvals; keep history; no rollback
```

## Soft-path order

```
… → agent_progress_ux (C5.7)
  → agent_pause_resume_cancel (C5.8; also early-gated in handle)
  → agent_failure_recovery (C5.9; also early-gated in handle)
  → coding_goal_completion (C5.10; also early-gated in handle)
  → agent_session_continuity (C5.11; also early-gated in handle)
  → coding_edit → …
```

Phrases: “Pause this goal.” / “Resume this goal.” / “Cancel this goal.” /
“Goal control status.”

Early-gated ahead of OpenProject / session intents (same pattern as C5.3).
**C5.14:** that early gate **assembles via ContextEngine for Pause/Resume**
(not `empty_bundle`) so fingerprints are real or honestly unavailable.
Cancel/Status keep the lightweight empty bundle (no fingerprint needed).

## Pause

| Behavior | Detail |
|----------|--------|
| Stop new operations | `allows_new_operations` false; multi-step rejects |
| Preserve | Goal, tasks, pending plan id, approval flags, verification state |
| Checkpoint | `ProjectFingerprint` (HEAD / branch / dirty / conflicts / root / current file) |
| Resumable | Yes — lifecycle `Paused` ≠ `Cancelled` |
| In-flight | Surfaced honestly when generation/tools cannot interrupt immediately |

## Resume

| Behavior | Detail |
|----------|--------|
| Revalidate | Compare pause fingerprint to current ContextBundle (ContextEngine-assembled) |
| Stale | HEAD / branch / conflicts / dirty tree / workspace root / current file |
| Unavailable | Missing ambient signal → `FingerprintUnavailable` (**unknown/stale**, never “unchanged”) |
| Approvals | Invalidate when stale or binding unusable — **no blind Execute** |
| Landing state | Stale Executing / AwaitingApproval / Verifying → resume to **Planning** |
| Correcting | Resume after verification failure returns to Correcting |

## C5.14 — Resume Integrity Hardening

| Rule | Behavior |
|------|----------|
| Real pause fingerprint | Pause/resume soft path calls `ContextEngine::assemble_with` (not `empty_bundle`) |
| Real resume fingerprint | Same assemble path; compare against checkpoint |
| Unavailable ≠ unchanged | `ProjectFingerprint::is_available()`; else `StaleReason::FingerprintUnavailable` |
| Revalidate before continue | Stale or unavailable → invalidate approvals (C5.6) before mutation |
| No second assembler | ContextEngine remains the sole ContextBundle factory |

## Cancel

| Behavior | Detail |
|----------|--------|
| Future work | Stopped (`control.cancelled`, coarse `Cancelled`) |
| Approvals | Invalidated (C5.6) — including Err-path cancel bookkeeping |
| History | Lifecycle + workflow history preserved |
| Mutations | **Never** silently rolled back |
| Phases | Investigation, planning, approval, execution, verification, correction |
| Repeated | Idempotent (`CancelRepeated`) |
| Continue after cancel | Forbidden |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Sole control orchestration (`pause_goal` / `resume_goal` / `cancel_active_goal`) |
| **C5.3 lifecycle** | Transition authority (`Pause` / `Resume` / `Cancel` events) |
| **C5.6 autonomy** | Approval invalidate on cancel / stale resume |
| **Tool / stream cancel** | Existing Tool + `StreamingLifecycle` / `cancel_generation` |
| **Experience / UI** | Mirrors `agent_control` diagnostics — never invents transitions |

## Guards

1. Planner owns state transitions.
2. No parallel cancellation engine (`parallel_cancel_engine=false`).
3. Never silently undo successful changes.
4. Never continue after explicit cancellation.
5. Cancel allowed in every non-hard-terminal phase.
6. In-flight honesty when interrupt is not immediate.
7. Resume detects stale project state and requires fresh approval when needed.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner-owned transitions | PASS |
| Reuse Tool / stream cancellation | PASS |
| No parallel cancellation engine | PASS |
| No silent rollback of mutations | PASS |
| No continue after cancel | PASS |
| Cancel during investigation…correction | PASS |
| In-flight honesty | PASS |
| Resume detects stale project state | PASS |
| Unavailable fingerprint → unknown/stale (C5.14) | PASS |
| Pause/resume use ContextEngine assemble (C5.14) | PASS |
| Stale / invalid plan → fresh approval | PASS |
| Pause preserves goal/task/plan/verification | PASS |
| Pause stops new operations | PASS |
| Repeated cancel idempotent | PASS |
| Resume after verification failure | PASS |

**Verdict: PASS** (including C5.14 resume integrity)
