# Agent Context Maintenance (Sprint C5.5)

## Goal

Allow a multi-step Coding Goal to keep **coherent context** for model requests
without stuffing the entire execution history into every turn.

Reuse the existing **ContextEngine** and **Workspace Intelligence**.

**Do not create `AgentContextEngine`.**

```text
CodingGoal (Planner-owned)
  → MaintainedAgentContext (compact view)
  → ContextEngine::assemble_with (sole ContextBundle factory)
  → LlmContext::from_bundle + agent_goal_context extension
```

## Maintained state (where relevant)

| Field | Source |
|-------|--------|
| Goal / objective / status | `CodingGoal` |
| Task list / current / completed | Goal tasks (completed **summarized**) |
| Coding plan / approved ops / execution / verification refs | Workflow history + lifecycle (refs only) |
| Failures | verify_fail / blockers (kept for Correcting) |
| Decisions | Constraints, acceptance, decision-like notes |
| User constraints | **Never dropped** by compaction |
| Relevant files / symbols | Goal + current task; prefer Policy-selected bundle paths |
| Workspace state | Lightweight labels from assembled bundle (not snapshot copies) |

## Soft-path order

```
… → multi_step_execution (C5.4)
  → agent_context_maintenance (C5.5; also early-gated)
  → approval_autonomy_policy (C5.6; also early-gated)
  → agent_progress_ux (C5.7; also early-gated)
  → agent_pause_resume_cancel (C5.8; also early-gated)
  → agent_failure_recovery (C5.9; also early-gated)
  → coding_goal_completion (C5.10; also early-gated)
  → agent_session_continuity (C5.11; also early-gated)
  → coding_edit → …
```

Phrases: “Maintain agent context.” / “Show agent context.” /
“Compact goal context.” / “Agent context status.”

## Bounds

| Bound | Value |
|-------|-------|
| Goal context char budget | `MAX_GOAL_CONTEXT_CHARS` = 4_000 |
| Completed task summaries | `MAX_COMPLETED_TASK_SUMMARIES` = 8 |
| Decision entries | `MAX_DECISION_ENTRIES` = 8 |
| Failure entries | `MAX_FAILURE_ENTRIES` = 6 |

Full ContextEngine budget (`DEFAULT_MAX_CHARACTERS` = 32_000) still applies to
the assembled bundle via existing ContextPolicy selection.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns `MaintainedAgentContext` on the Coding Goal; soft path + LLM extension |
| **ContextEngine** | Sole `ContextBundle` factory |
| **ContextPolicy** | Selects evidence into the bundle; maintenance prefers those paths |
| **Workspace Intelligence** | Ambient snapshots via session — never duplicated here |
| **Memory Engine** | Not written by this sprint |

## Guards

1. Goal state remains Planner-owned.
2. No second ContextBundle assembler / no AgentContextEngine.
3. No duplicate WorkspaceSnapshot / ProjectSnapshot / GitSnapshot payloads.
4. Full workflow / transcript history is **not** injected.
5. Compaction retains constraints and recoverable decisions.
6. Failed verification evidence stays available when Correcting.
7. Current task is prioritized over completed summaries.
8. No project file mutation; no autonomous Memory writes.
9. Ordinary C2/C3/C4 conversations unchanged when no active Coding Goal.

## Constitutional audit

| Rule | Status |
|------|--------|
| Goal state Planner-owned | PASS |
| ContextEngine sole ContextBundle factory | PASS |
| Workspace Intelligence for observations | PASS |
| No duplicate snapshot types | PASS |
| Use ContextPolicy-selected evidence | PASS |
| No full historical transcript injection | PASS |
| Completed history summarized/compacted | PASS |
| Current task prioritized | PASS |
| Failed verification retained for correction | PASS |
| User constraints retained | PASS |
| Decisions recoverable | PASS |
| No project file modification | PASS |
| No autonomous Memory writes | PASS |
| C2/C3/C4 ordinary path preserved | PASS |
| No AgentContextEngine | PASS |

**Verdict: PASS**
