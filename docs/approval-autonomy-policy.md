# Approval and Autonomy Policy (Sprint C5.6)

## Goal

Define exactly what an Agentic Coding Goal may do **automatically** and what
requires **explicit user authorization**.

This sprint establishes **policy boundaries**.

**Do not** implement user-configurable autonomy modes.  
**Do not** create an `AgentApproval` system.

```text
READ / OBSERVE     → may proceed without mutation approval (when Permission allows)
MUTATION / SIDE EFFECT → Permission Engine + ToolRisk + Review Before Action
```

## Default policy

### READ / OBSERVE

May proceed without mutation approval:

- search, read files, symbol navigation
- inspect diagnostics / project state / Git status
- investigate, reason, formulate plans
- interpret verification output

### MUTATION / SIDE EFFECT

Requires existing approval / policy mechanisms:

- write / modify / delete / rename / move files
- destructive terminal operations
- Git mutations
- external side effects

Classification reuses **ToolRisk** + **PermissionCategory/Action** — never LLM judgment.

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

Phrases: “What can this goal do automatically?” / “What requires approval?” /
“Autonomy policy status.”

## Binding rules

| Rule | Behavior |
|------|----------|
| Attribution | Approval binds to `plan_id` + `task_id` (+ `goal_id`) |
| Cross-task | Advancing tasks invalidates prior task approvals |
| Cancel | Invalidates pending / recorded bindings |
| Plan change | New `SubmitForApproval` invalidates prior binding |
| Corrective | Failure / Correcting invalidates — new C3 plan needs new Review |
| Stale | `assert_execution_authorized` rejects mismatched / invalidated bindings |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns `GoalAutonomyState` on the Coding Goal; soft-path explanation |
| **Permission Engine** | Authorization decisions (reused) |
| **ToolRisk** | Review derivation (reused) |
| **Review Before Action** | User Approve / Modify / Cancel (reused) |
| **Agent lifecycle (C5.3)** | AwaitingApproval / Executing gates (reused) |
| **LLM** | Must not self-authorize |

## Guards

1. No AgentApproval system.
2. No configurable autonomy modes.
3. LLM cannot determine authorization.
4. Agent state cannot bypass Permission / ToolRisk / Review.
5. Read-only lifecycle states may advance when policy allows.
6. Consequential mutation pauses for Review.
7. Approvals attributable to current plan/task.
8. Approvals do not leak across unrelated tasks.
9. Cancellation / plan change / corrective path invalidate as appropriate.

## Constitutional audit

| Rule | Status |
|------|--------|
| Reuse Permission Engine | PASS |
| Reuse ToolRisk | PASS |
| Reuse Review Before Action | PASS |
| No AgentApproval system | PASS |
| LLM cannot self-authorize | PASS |
| Agent state cannot bypass gates | PASS |
| Read-only auto transitions where allowed | PASS |
| Mutation pauses for authorization | PASS |
| Approval attributable to plan/task | PASS |
| No cross-task approval leak | PASS |
| Cancel invalidates pending approvals | PASS |
| Plan change requires new approval | PASS |
| Failed execution ≠ reuse old approval | PASS |
| No autonomy modes | PASS |

**Verdict: PASS**
