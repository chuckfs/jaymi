# Agentic Coding Experience (Sprint C5.12)

## Goal

Integrate C5.1–C5.11 into one coherent conversational coding experience.

```text
User: "Build me a Pong game."
  → Understand → Coding Goal → Decompose → Investigate → Plan
  → Request approval → Implement → Verify → Diagnose
  → Propose reviewed corrections → Continue → Report completion
```

The user should not manually orchestrate every coding stage. Progress stays
visible but concise. Conversation remains the primary interface. Planner remains
the sole orchestrator.

## Soft-path order

```
… → agent_pause_resume_cancel (C5.8)
  → agent_failure_recovery (C5.9)
  → coding_goal_completion (C5.10)
  → agent_session_continuity (C5.11)
  → agentic_coding_experience (C5.12)
       · Status phrases: early-gated (empty bundle OK)
       · Begin (multi-step create): after assemble (ContextBundle)
  → coding_goal (C5.1; create usually already handled by C5.12)
  → …
```

Phrases:

* Begin — multi-step / greenfield creates reused from C5.1
  (`Build me a Pong game.`, `Build a login system.`, …)
* Status — `Show agentic coding experience.` /
  `How is this coding goal going end-to-end?`

Control / recover / complete / continuity / progress / decompose / start phrases
**yield** to C5.2–C5.11 (C5.12 does not steal them).

Status falls back to the **latest conversation goal** when the active pointer was
cleared on cancel/complete, so end-state narration still works.

## What this sprint is (and is not)

| Is | Is not |
|----|--------|
| Compose narration + goal create over C5.1–C5.11 | A parallel coding agent |
| Conversation First path for complex objectives | Hidden autonomous filesystem access |
| Concise UX: plan / now / approval / changed / worked / wrong / remains | Chain-of-thought dump |
| Reuse C2/C3/C4 engines | Direct model→tool execution |
| Evidence-based success via C4 + C5.10 | Silent continue after cancel |
| New review for corrective mutations (C4.4 / C5.6 / C5.9) | Silent destructive retries |
| | Web research / images / messaging / computer control |

Begin **creates** a Coding Goal and narrates the path — it does **not** auto
Start → Apply → Verify. Those remain Conversation First turns (and retain
Review Before Action for consequential mutation).

## UX acceptance fields

| Field | Source |
|-------|--------|
| What Jaymi plans to do | Tasks / objective (`what_planned`) |
| What Jaymi is currently doing | Progress phase (`what_doing`) |
| When approval is needed | Lifecycle / policy (`approval_needed`) |
| What changed | Completed work (`what_changed`) |
| Whether it worked | Verification + completion (`whether_worked`) |
| What went wrong | Failures / blockers (`what_went_wrong`) |
| What remains | Remaining work / open tasks (`what_remains`) |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Soft-path + store; sole orchestrator |
| **C5.1–C5.11** | Goal / tasks / lifecycle / progress / control / recovery / completion / continuity |
| **C2 / C3 / C4** | Investigate / plan+apply / verify — unchanged |
| **Experience / UI** | Mirrors `agentic_coding_experience` + progress diagnostics |

## Guards

1. Reuse C2/C3/C4 — no parallel coding agent architecture.
2. Planner remains the central orchestrator.
3. Conversation remains the primary interface.
4. Progress visible but concise (no CoT).
5. User can pause / stop / cancel (C5.8).
6. Every consequential mutation retains existing authorization.
7. Every corrective mutation requires a **new** review.
8. Verification evidence determines success.
9. Goal completion is evidence-based (C5.10).
10. Agent cannot silently continue after cancellation.
11. Agent cannot silently retry destructive actions.
12. No chain-of-thought exposed.
13. No hidden autonomous filesystem access.
14. No direct model-to-tool execution.
15. No unrelated capabilities (web / images / messaging / computer control).

## Product audit

| Criterion | Status |
|-----------|--------|
| Inexperienced user can give a complex objective without staging each step | PASS — Begin creates goal + narrates next Conversation First moves |
| Understands plan / now / approval / changed / worked / wrong / remains | PASS — At-a-glance fields + stage path |
| Can pause / stop / cancel | PASS — yields to C5.8 |
| Sees verification and correction without inventing success | PASS — C4 + C5.9/C5.10 composed |
| No redesign of C2/C3/C4 | PASS |

**Product verdict: PASS**

## Constitutional audit

| Rule | Status |
|------|--------|
| Reuse C2/C3/C4 | PASS |
| No parallel coding agent | PASS |
| Planner-owned orchestration | PASS |
| Conversation First | PASS |
| Concise progress, no CoT | PASS |
| Pause / stop / cancel | PASS |
| Authorization retained | PASS |
| New review for corrections | PASS |
| Verification evidence for success | PASS |
| Evidence-based completion | PASS |
| No silent continue after cancel | PASS |
| No silent destructive retry | PASS |
| No hidden FS / model→tool | PASS |
| No unrelated capabilities | PASS |
| Begin does not auto-mutate | PASS |

**Constitutional verdict: PASS**
