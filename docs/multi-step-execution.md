# Multi-Step Coding Execution (Sprint C5.4)

## Goal

Connect Coding Goals, Goal Tasks, C3 mutation, and C4 verification into a
**Planner-owned** multi-step coding workflow — the first major Agentic Coding
milestone.

This is **orchestration, not unrestricted autonomy**.

```text
Model proposes → Planner decides → Review authorizes → Tools execute → Verification evaluates
```

Example:

```text
User: "Build a login system."

Goal
→ Task 1 → Investigation → Coding Plan → Review → Apply → Verify
→ Task 2 → …
→ Goal Complete
```

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Orchestrates ready tasks; owns workflow history and advance/continue |
| **C2** | Investigation (reused) |
| **C3** | Coding Plan → Review → Apply (reused; no second plan/review) |
| **C4** | Verification (reused; evidence-based) |
| **C5.1–C5.3** | Goal / tasks / agent lifecycle |
| **LLM** | Proposes only — never direct tool execution or silent approve |

## Soft-path order

```
… → coding_goal (C5.1) → goal_decomposition (C5.2)
  → agent_lifecycle (C5.3; also early-gated)
  → multi_step_execution (C5.4; also early-gated)
  → agent_context_maintenance (C5.5; also early-gated)
  → approval_autonomy_policy (C5.6; also early-gated)
  → agent_progress_ux (C5.7; also early-gated)
  → agent_pause_resume_cancel (C5.8; also early-gated)
  → agent_failure_recovery (C5.9; also early-gated)
  → coding_goal_completion (C5.10; also early-gated)
  → agent_session_continuity (C5.11; also early-gated)
  → coding_edit → …
```

Phrases: “Start the goal.” / “Continue the goal.” / “Advance to the next task.” /
“Multi-step execution status.”

## Bounds

| Bound | Value |
|-------|-------|
| In-progress tasks | `MAX_IN_PROGRESS_TASKS` = 1 |
| Task advances per turn | `MAX_TASK_ADVANCES_PER_TURN` = 1 |
| Workflow history / transitions | `MAX_WORKFLOW_TRANSITIONS` = 128 |

Next task starts only on explicit continue/advance after the prior task leaves
`InProgress`. Failed tasks do **not** unlock dependents.

## Behavior

| Event | Effect |
|-------|--------|
| Start the goal | First ready task → InProgress; hint C2→C3→Review→C4 |
| Continue / Advance | Hint if in progress; else start next ready task |
| Approved apply (hook) | Sync agent lifecycle to Executing (observes C3 Review) |
| Verification pass (hook) | May complete active task with `TaskCompletionEvidence` |
| Verification fail (hook) | Task Failed → Correcting; no auto-mutation; dependents blocked |
| Pause / Cancel | Existing C5.3 events |
| Goal complete | Still requires C5.3 `CompletionEvidence` |

## Guards

1. Every mutation still requires existing Review/Approval.
2. Risky tools retain Permission / ToolRisk behavior.
3. Verification remains explicit and evidence-based.
4. Task complete needs appropriate `TaskCompletionEvidence`.
5. No second ExecutionPlan or Review system.
6. No silent approve; no automatic corrective mutation.
7. No parallel coding execution; no hidden filesystem access; no LLM direct tools.
8. Workflow transitions are bounded (no infinite orchestration loop).
9. Full `workflow_history` preserved on the Coding Goal.

## Out of scope

- Long-running session persistence
- Autonomy modes
- Advanced progress UX

## Constitutional audit

| Rule | Status |
|------|--------|
| Reuse C2 investigation | PASS |
| Reuse C3 mutation | PASS |
| Reuse C4 verification | PASS |
| Planner owns orchestration | PASS |
| Review/Approval before mutation | PASS |
| Permission / ToolRisk retained | PASS |
| Verification evidence-based | PASS |
| Task complete needs evidence | PASS |
| Failed task does not unlock dependents | PASS |
| Dependents blocked until prereqs complete | PASS |
| User can stop/cancel | PASS |
| User can pause | PASS |
| No hidden filesystem access | PASS |
| No LLM direct tool execution | PASS |
| No parallel coding execution architecture | PASS |
| No second ExecutionPlan system | PASS |
| No second Review system | PASS |
| No silent approve | PASS |
| No automatic corrective mutation | PASS |
| Bounded active tasks / transitions | PASS |
| Full execution/verification history | PASS |
| Orchestration ≠ unrestricted autonomy | PASS |

**Verdict: PASS**
