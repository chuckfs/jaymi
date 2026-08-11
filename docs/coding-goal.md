# Coding Goal Model (Sprint C5.1)

## Goal

Introduce a Planner-owned representation of a **multi-step coding objective** —
answering “what is Jaymi trying to accomplish?” across turns — without treating
every coding message as an isolated request.

C0–C4 remain closed. This sprint does not reopen or redesign them.
C5 begins the Agentic Coding chapter.

A Coding Goal is **not**:

| Concept | Meaning |
|---------|---------|
| **CodingGoal** | What the user wants accomplished |
| **CodingPlan** | How Jaymi proposes accomplishing it |
| **ExecutionPlan** | Approved operations that may execute |
| **VerificationResult** | Evidence about whether work succeeded |

## Behavior

| User says | Jaymi does |
|-----------|------------|
| “Build a login system.” | Creates an Active `CodingGoal` (no tools / no mutation) |
| “Refactor the authentication architecture.” | Same |
| “Add image generation to the application.” | Same |
| “Fix all the compiler errors in this project.” | Same (C4.4 yields multi-step “fix all…” to C5.1) |
| “Rename this variable.” / “Write tests.” | No goal — one-step / C1.4 stay as before |
| “Explain this…” / “Help with auth.” | No goal — Conversation First / ambiguous |
| “Cancel this goal.” | Cancels the active goal |
| “Mark this goal complete.” | Completes the active goal |
| “This goal is blocked because …” | Blocks with a recorded reason |
| “What’s my current goal?” | Shows the active goal (persistence across turns) |

Creating a goal **never** executes tools or modifies files.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns `CodingGoal` / `CodingGoalStore`; soft path + session Mutex |
| **Conversation / Memory** | Conversation id scopes goals when available |
| **Workspace Intelligence / ContextEngine** | Ambient project / open file only — no second discovery |
| **C2 / C3 / C4** | Reused later; not invoked on goal create |
| **Tool Engine** | Not invoked |

Do **not** create an AgentContextEngine, a second Planner, or a second coding
orchestration system.

## Soft-path order

After C4.4; before coding_edit / review / plan / understanding:

```
… → test_execution → build_awareness → fix_from_verification
  → coding_goal (C5.1) → goal_decomposition (C5.2)
  → agent_lifecycle (C5.3; also early-gated in handle)
  → multi_step_execution (C5.4; also early-gated in handle)
  → agent_context_maintenance (C5.5; also early-gated in handle)
  → approval_autonomy_policy (C5.6; also early-gated in handle)
  → agent_progress_ux (C5.7; also early-gated in handle)
  → agent_pause_resume_cancel (C5.8; also early-gated in handle)
  → agent_failure_recovery (C5.9; also early-gated in handle)
  → coding_goal_completion (C5.10; also early-gated in handle)
  → agent_session_continuity (C5.11; also early-gated in handle)
  → agentic_coding_experience (C5.12; status early-gated; begin after assemble)
  → coding_goal (create usually via C5.12) → …
  → coding_edit → …
```

C1.4 Coding Plan and C3.1 Coding Edit **yield** multi-step goal phrases via
`steals_from_*`. C5.12 Begin wraps multi-step creates with experience narration.

## Representation

Where applicable a goal carries:

- goal ID, user objective, project/workspace, conversation id
- constraints, acceptance criteria (explicit vs **proposed**)
- status (`active` / `completed` / `cancelled` / `blocked`)
- current task/step (empty until later decomposition sprints)
- completed / remaining work, blockers
- relevant files/symbols (ambient + mentioned only)
- verification state (observational; starts `not_started`)
- created/updated timestamps
- `mutated=false`, `tools_executed=false` on create

Proposed acceptance criteria paraphrase the user’s objective and are labeled
**proposed** — unsupported inventing is forbidden.

## Diagnostics

Observational Coding Diagnostics lines via `to_diagnostics_summary()` /
`LastPlannerActivity.coding_goal_*`. Diagnostics never drive decisions.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner ownership | PASS |
| Conversation First for ordinary questions | PASS |
| Review Before Action (no bypass; create runs no tools) | PASS |
| Not Fully Autonomous | PASS |
| No AgentContextEngine / second Planner / second orchestrator | PASS |
| Goal ≠ CodingPlan ≠ ExecutionPlan ≠ VerificationResult | PASS |
| No mutation / tools on create | PASS |
| Cancel / complete / blocked supported | PASS |
| Ambiguous & one-step do not force goals | PASS |
| C0–C4 closed; no decomposition / execution / autonomy / progress UX | PASS |

**Verdict: PASS**

## Out of scope (later C5)

- Goal-driven execution / autonomy
- Agent state machine
- Progress UX beyond observational diagnostics
- Automatic plan generation from a goal without Review
- Auto-verify / auto-fix loops attached to a goal

Decomposition of an active goal into proposed tasks is **C5.2** —
[goal-decomposition.md](goal-decomposition.md).
