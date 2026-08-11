# Fix From Verification Failure (Sprint C4.4)

## Goal

Allow Jaymi to understand a verification failure and prepare a corrective
Coding Plan using the existing C2 + C3 architecture — **without** autonomous
repair.

```
C3 Mutation → C4 Verification → C4 Failure → C2 Investigation → C3 Coding Plan
→ Review Before Action → Approval → Existing C3 execution
```

Not:

```
Verification Failure → LLM → filesystem
```

C0–C3 remain closed. C4.1–C4.3 stay as discovery / execution / diagnostics.
**Automatic repeated repair loops are out of scope.**

## Behavior

| User says | Jaymi does |
|-----------|------------|
| “Fix the build failure.” | Loads `VerificationEvidence`, investigates via C2, authors a C3 Coding Plan |
| “Fix the failing tests.” | Same, preferring test-failure grounding |
| “Fix it.” (after a failed run) | Deictic fix only when failure evidence is present |
| Environment / tool / timeout failure | Explains it is not a code mutation; no invented edits |
| Malformed / unknown failure | States what is unknown; does not invent files |

When evidence is insufficient, Jaymi says so. Corrective plans are **proposals** —
mutations still require the existing Review Before Action path (generate patches →
Approve). Nothing is auto-applied or silently retried.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Soft path `fix_from_verification`; owns diagnosis + orchestration |
| **C4.3 VerificationEvidence** | Structured failure input (remembered + session) |
| **C2 / Change Investigation** | Read-only investigation of failing files / symbols / deps |
| **C3 Coding Plan** | Same `CodingPlan` type (`plan_from_change_investigation`) |
| **Review Before Action** | Required later for any mutation ExecutionPlan |
| **ReasoningProvider** | May elaborate later; never executes tools |
| **Terminal / write tools** | Not invoked in this turn |

## Soft-path order

```
… → multi_file → test_execution → build_awareness
  → fix_from_verification (C4.4) → coding_goal (C5.1) → coding_edit → …
```

## Actionability

| Kind | Plan behavior |
|------|----------------|
| Build / test failure with findings | Actionable → full C2 investigation + Coding Plan |
| Unknown / empty findings | Insufficient evidence → clarification; no invented targets |
| Environment / tool / timeout | Not code-related → diagnosis only |
| No recent failure | Ask to run verification first |

## Hard lines

1. Never automatically modify files.  
2. Never automatically approve the corrective plan.  
3. Never silently retry mutations or re-run verification from this path.  
4. Never allow Reasoning to execute tools.  
5. Reuse existing C3 mutation architecture for later apply.  
6. Do not invent failures or file targets unsupported by evidence.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner receives structured VerificationResult / Evidence | PASS |
| Determine actionability from evidence | PASS |
| Use C2 codebase reading via Change Investigation | PASS |
| Identify failing files / symbols / deps / likely cause / evidence | PASS |
| Honest unknowns when evidence insufficient | PASS |
| Generate C3 Coding Plan for proposed correction | PASS |
| Corrective mutations go through Review Before Action | PASS |
| No automatic file modification | PASS |
| No automatic approval | PASS |
| No silent mutation / verification retries | PASS |
| Reasoning does not execute tools | PASS |
| Reuse C3 architecture (no parallel repair system) | PASS |
| No automatic repeated repair loops | PASS |
| Tests (compiler / test / multi / unrelated / insufficient / changed-code note / env / multi-file / no mutation / review) | PASS |

**Verdict: PASS**

### Remaining limitations

- Deictic “fix this” without failure language still uses generic Coding Edit (C3.1)
  unless recent verification evidence is present.
- Full C2 gather depends on search/read tooling and project files on disk.
- Applying the corrective plan still requires a later generate/patch + Approve turn.

## Out of scope

- Autonomous repair loops / auto-apply on failure
- Auto-Approve of corrective ExecutionPlans
- Silent re-run of failing verification from the fix path
- A second “repair engine” beside C2/C3
