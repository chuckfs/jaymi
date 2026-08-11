# Agent State Machine (Sprint C5.3)

## Goal

Introduce a Planner-owned lifecycle for an active Coding Goal — the first
explicit agent lifecycle, **bounded and non-autonomous**.

C5.1 provides the Coding Goal. C5.2 provides proposed tasks.
C0–C4 remain closed. This sprint does **not** implement multi-task autonomous
orchestration.

## States

| State | Meaning |
|-------|---------|
| Idle | Goal exists; work not started |
| Understanding | Framing the objective |
| Investigating | C2-style investigation (state only here) |
| Planning | C3 plan authoring (state only) |
| AwaitingApproval | Review Before Action — **mutation execution stopped** |
| Executing | Only already-approved operations |
| Verifying | C4 verification — **no silent mutation** |
| Correcting | Prepare a **new reviewed C3 plan** — not auto-mutation |
| Blocked | External blocker (≠ Failed) |
| Paused | User pause (≠ Cancelled) |
| Cancelled | Explicit cancel |
| Completed | Requires completion evidence |
| Failed | Execution/hard failure (≠ Blocked) |

## Distinctions

| Layer | Type |
|-------|------|
| Coarse goal bookkeeping | `CodingGoalStatus` (C5.1) |
| Per-task status | `GoalTaskStatus` (C5.2) |
| Authoritative agent lifecycle | `AgentLifecycleState` (C5.3) |

## Behavior

| User / Planner event | Effect |
|----------------------|--------|
| “Begin understanding.” | Idle → Understanding |
| “Begin investigation.” | → Investigating |
| “Pause this goal.” / “Resume this goal.” | Pause ↔ prior state |
| “Cancel this goal.” | → Cancelled (explicit) |
| SubmitForApproval / RecordApproval | AwaitingApproval; Executing gated on approval |
| VerificationFailed / BeginCorrection | → Correcting (`correction_requires_new_c3_plan`) |
| Complete without evidence | **Rejected** |
| LLM / arbitrary UI state write | **Not permitted** — Planner owns transitions |

Lifecycle phrases are gated **early** in `Planner::handle` so they are not stolen
by OpenProject / session intents (e.g. “Resume this goal.”).

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Sole transition authority (`apply_event` / `transition_goal`) |
| **Experience/UI** | May **mirror** state via diagnostics — must not invent transitions |
| **Reasoning / LLM** | Must not directly change agent state |
| **C3 / C4** | Keep mutation / verification execution ownership |

## Guards

1. Invalid transitions rejected (`TransitionError`).
2. `MAX_AGENT_TRANSITIONS` (64) — no unbounded loops.
3. `allows_mutation_execution()` only in **Executing**.
4. Executing requires recorded approval for the plan id.
5. No Failed/Correcting/Verifying → Executing shortcut (no auto failure→mutation).
6. Completed requires sufficient `CompletionEvidence`.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner-owned transitions | PASS |
| UI mirrors only | PASS |
| Explicit events/causes | PASS |
| Invalid transitions rejected | PASS |
| One authoritative agent state per goal | PASS |
| Task vs goal agent state distinguishable | PASS |
| LLM cannot change state | PASS |
| UI cannot arbitrarily mutate state | PASS |
| Cancel explicit | PASS |
| Pause ≠ Cancel | PASS |
| Failed ≠ Blocked | PASS |
| Completed requires evidence | PASS |
| AwaitingApproval stops mutation | PASS |
| Executing = approved ops only | PASS |
| Verifying no silent mutation | PASS |
| Correcting → new reviewed C3 plan intent | PASS |
| No auto failure→mutation | PASS |
| Bounded transitions | PASS |
| C3/C4 ownership preserved | PASS |
| No multi-task autonomy | PASS |

**Verdict: PASS**

## Out of scope (later C5)

- Unrestricted multi-task autonomy / auto-chaining all tasks in one turn
- Automatic advancement through the full lifecycle without user/Planner events
- AgentContextEngine / second Planner
- Long-running session persistence, autonomy modes, advanced progress UX
  (see C5.4 for bounded multi-step orchestration)
