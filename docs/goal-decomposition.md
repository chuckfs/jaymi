# Goal Decomposition (Sprint C5.2)

## Goal

Allow Jaymi to turn a Coding Goal into a structured set of **proposed** tasks.

C5.1 provides the Coding Goal. C0–C4 remain closed.
This sprint does **not** implement autonomous task execution or an agent state machine.

Example:

> Goal: “Build a login system.”
>
> Proposed tasks: investigate auth → design flow → implement module → login UI →
> session handling → tests → verify.

## Behavior

| User says | Jaymi does |
|-----------|------------|
| “Break this goal into tasks.” | Proposes tasks, attaches them to the active `CodingGoal` |
| “Decompose the goal.” / “Propose tasks for this goal.” | Same |
| “Complete task \<id\>.” | Status → Completed (deps must be Completed) |
| “Cancel task \<id\>.” | Status → Cancelled |
| “Start task \<id\>.” | Status → InProgress (deps must be Completed) |
| “Break things down.” (no goal) | Ambiguous / no-active-goal — Conversation First |
| Constraint “one task only” | Single-task decomposition |

Decomposition **never** executes tasks, modifies files, or auto-approves plans.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns task graph + goal state updates |
| **CodingGoal (C5.1)** | Receives `tasks`; constraints preserved |
| **Reasoning** | May propose structure later; Planner owns resulting state (templates here) |
| **C2 / C3 / C4** | Referenced as reuse hints only — not invoked on decompose |
| **Tool Engine** | Not invoked |

## Soft-path order

```
… → fix_from_verification → coding_goal (C5.1)
  → goal_decomposition (C5.2) → agent_lifecycle (C5.3)
  → multi_step_execution (C5.4) → agent_context_maintenance (C5.5)
  → approval_autonomy_policy (C5.6) → agent_progress_ux (C5.7)
  → agent_pause_resume_cancel (C5.8)
  → agent_failure_recovery (C5.9)
  → coding_goal_completion (C5.10)
  → agent_session_continuity (C5.11)
  → coding_edit → …
```

## Task model

Each `GoalTask` includes:

- task ID, description, objective
- dependencies (task IDs)
- relevant files / context when known
- expected verification (proposal)
- status: Pending · InProgress · AwaitingApproval · Completed · Failed · Blocked · Cancelled
- risk · mutation_expected
- reuse hints (C2 / C3 / C4) for later Review Before Action

Dependencies are validated with a topological sort. **Circular dependencies are
detected and refused** (tasks not applied). Completing / starting a task while
dependencies are incomplete is refused — prevents impossible ordering.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner owns decomposition | PASS |
| No task execution / file mutation | PASS |
| No automatic approval | PASS |
| Reuse C2/C3 (hints; no parallel task runner) | PASS |
| Preserve goal constraints | PASS |
| Circular dependency detection | PASS |
| Ambiguous / no-goal stays conversational | PASS |
| Details may be refined later | PASS |
| No agent state machine / autonomy | PASS |
| C0–C4 / C5.1 closed | PASS |

**Verdict: PASS**

## Out of scope (later C5)

- Autonomous task execution
- Agent state machine / goal-seeking loop
- LLM-required decomposition (optional enrichment later)
- Progress UX beyond diagnostics
