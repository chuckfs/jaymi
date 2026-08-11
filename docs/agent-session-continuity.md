# Long-Running Coding Sessions (Sprint C5.11)

## Goal

Make Coding Goals resilient across long sessions containing many tasks, tool
calls, context changes, interruptions, and verification cycles — **without** an
independent agent database.

```text
CodingGoalStore (Planner Mutex — process session)
  → bound histories / prune terminal goals
  → C5.5 compact (constraints preserved)
  → C5.8 fingerprint revalidation after pauses
  → C5.6 invalidate pending approvals on plan/project drift
  → serde snapshot hitch (optional; no autonomous Memory writes)
```

## Soft-path order

```
… → agent_failure_recovery (C5.9; also early-gated)
  → coding_goal_completion (C5.10; also early-gated)
  → agent_session_continuity (C5.11; also early-gated)
  → agentic_coding_experience (C5.12; status early-gated; begin after assemble)
  → coding_goal (C5.1) → …
```

Phrases: “Session continuity status.” / “Prepare long-running session.” /
“Compact session context.” / “Revalidate after pause.”

C5.8–C5.10 Pause/Resume/Cancel/Recover/Complete win over continuity phrases.
C5.12 Begin yields those control phrases and waits for assembled Context on create.

## What a goal must survive

| Stress | Continuity response |
|--------|---------------------|
| Many conversational turns | Store kept on Planner Mutex across turns |
| Context compaction | C5.5 compact; **constraints retained** |
| Pause / resume | C5.8 stop-new-ops; continuity revalidate before blind resume |
| User interruption | Pause remains paused — no background execution |
| Task / verification failure | Failed + completed task state preserved |
| Workspace / stale project | Fingerprint drift → fresh approval required |
| Multiple approval cycles | Bound approvals invalidated when plan/project changes |

## Persistence stance

| Mechanism | Used? |
|-----------|-------|
| Planner `CodingGoalStore` (session Mutex) | **Yes** — primary |
| Serde JSON snapshot of the store | **Yes** — portable hitch for existing session/config later |
| Independent agent / goal database | **No** |
| Autonomous Memory Engine writes | **No** |
| SessionCache / Workspace Memory as authority | **No** |

Live process store does **not** survive OS-level app restart today.
`survives_process_restart=false` is reported honestly; serde round-trip proves
the portable form for hitching onto existing persistence when product requires it.

## Bounds

| Bound | Value |
|-------|-------|
| Terminal goals / conversation | `MAX_TERMINAL_GOALS_PER_CONVERSATION` = 8 |
| Goal notes | `MAX_GOAL_NOTES` = 48 |
| Continuity history | `MAX_CONTINUITY_HISTORY` = 24 |
| Workflow records retained | `MAX_WORKFLOW_RETAINED` = 64 |
| Long task list soft signal | `LONG_TASK_LIST_SOFT_CAP` = 32 |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns store + continuity apply |
| **C5.5** | Compaction (constraints / decisions) |
| **C5.6** | Approval invalidate on plan change |
| **C5.8** | Pause stop-new-ops + fingerprints |
| **Experience / UI** | Mirrors `session_continuity` diagnostics |

## Guards

1. Persist only via existing session-appropriate mechanisms (Mutex + serde hitch).
2. No independent agent database.
3. Preserve goal / tasks / status / criteria / completed work / failures /
   decisions / current task / verification state.
4. Revalidate project state after long pauses.
5. Do not blindly resume stale plans.
6. Invalidate pending approvals when underlying plans change.
7. Context compaction preserves user constraints.
8. Bound history growth.
9. No background autonomous work.
10. Paused goals do not continue executing while the user is away.

## Constitutional audit

| Rule | Status |
|------|--------|
| Existing persistence only (no agent DB) | PASS |
| Preserve goal / tasks / criteria / failures / verification | PASS |
| Revalidate after long pauses | PASS |
| No blind stale resume | PASS |
| Invalidate approvals on plan change | PASS |
| Compaction preserves constraints | PASS |
| Bounded history / store prune | PASS |
| No background autonomous work | PASS |
| Paused does not keep executing | PASS |
| Honest restart story (`survives_process_restart=false`) | PASS |
| Serde-portable snapshot available | PASS |
| Planner-owned | PASS |

**Verdict: PASS**
