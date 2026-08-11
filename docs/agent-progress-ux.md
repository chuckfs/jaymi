# Agent Progress UX (Sprint C5.7)

## Goal

Give users clear visibility into what an active Coding Goal is doing **without**
exposing chain-of-thought or private reasoning.

Experience / Conversation / Coding Workspace **mirror** Planner state. This
sprint does **not** create a second agent state.

```text
CodingGoal + lifecycle + tasks  (Planner-owned authority)
        │
        ▼
  AgentProgressView  (derive-only read model)
        │
        ▼
Conversation markdown + SuggestedAction chips
Experience / Coding Workspace panel (never invent)
```

Diagnostics remain a **separate** operational surface (`LastPlannerActivity` /
Coding Diagnostics). The user Goal Progress panel is not Diagnostics.

## Surfaces

| Field | Source |
|-------|--------|
| Goal title | `CodingGoal.user_objective` (shortened) |
| Current phase | `AgentLifecycleState` → `ProgressPhase` |
| Current / completed / pending tasks | `GoalTask` statuses + `active_task_id` |
| Approval required | `AwaitingApproval` / task awaiting approval |
| Verification status | `GoalVerificationState` |
| Blockers / failures | Goal blockers + failed tasks / recent fail workflow |
| Actions | Pause, Stop, Review, Continue / Start / Resume (conversation phrases) |

## Soft-path order

```
… → agent_context_maintenance (C5.5)
  → approval_autonomy_policy (C5.6; also early-gated)
  → agent_progress_ux (C5.7; also early-gated)
  → agent_pause_resume_cancel (C5.8; also early-gated)
  → agent_failure_recovery (C5.9; also early-gated)
  → coding_goal_completion (C5.10; also early-gated)
  → agent_session_continuity (C5.11; also early-gated)
  → coding_edit → …
```

Phrases: “Show goal progress.” / “How is the goal going?” / “Goal progress status.”

`finalize_response` also attaches a derive-only `agent_progress` whenever an
**active non-terminal** Coding Goal is in play, so progress stays current without
blocking the UI or inventing state.

## Actions

Chips reuse existing C5.3 / C5.4 conversation phrases:

| Chip | Conversation text |
|------|-------------------|
| Pause | `Pause this goal.` |
| Resume | `Resume this goal.` |
| Stop | `Cancel this goal.` |
| Review | `Review my changes.` |
| Continue / Start | `Continue the goal.` / `Start the goal.` |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns Coding Goal + lifecycle; derives `AgentProgressView` |
| **Experience / UI** | Mirrors `PlannerResponse.agent_progress`; never invents |
| **Diagnostics** | Compact `agent_progress_*` lines only — not the Progress panel |
| **LLM** | Must not author hidden CoT into the progress surface |

## Guards

1. No second agent state / AgentProgress authority.
2. No chain-of-thought, private reasoning, or internal deliberation in the UX.
3. Progress always derived from Planner-owned Coding Goal fields.
4. Experience mirrors Planner payloads only.
5. Diagnostics ≠ user Progress panel.
6. Actions route through existing conversation detectors (no parallel control plane).

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner owns goal / lifecycle / tasks | PASS |
| Derive-only progress view (not authority) | PASS |
| Experience/UI mirrors Planner | PASS |
| No second agent state | PASS |
| No CoT / private reasoning in progress UX | PASS |
| Diagnostics separate from Progress panel | PASS |
| Pause / Stop / Review / Continue reuse existing phrases | PASS |
| Progress updates without blocking UI (`finalize_response` attach) | PASS |
| Conversation First actions | PASS |
