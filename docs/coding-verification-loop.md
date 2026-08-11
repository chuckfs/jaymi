# Bounded Coding Verification Loop (Sprint C4.6)

## Goal

Connect C3 mutation and C4 verification into one **bounded conversational**
workflow owned by the Planner.

```
Approved Coding Change
→ Apply
→ Verify
→ Result

PASS:
→ Verification Summary
→ Done

FAIL:
→ Diagnostics
→ C2 Investigation
→ C3 Coding Plan
→ Review
→ optional approved correction
→ Apply
→ Verify again
```

Hard maximum: **`MAX_CYCLES = 3`** verification/recovery cycles per user-requested
workflow. This is **not** C5’s general-purpose autonomous Agent Loop.

## Behavior

| Event | Loop phase |
|-------|------------|
| Coding apply with files edited | `AppliedAwaitingVerify` — offer Run Tests (no auto-run) |
| Verification awaiting Review | `AwaitingVerifyReview` |
| Verification passed | `Passed` — done |
| Verification failed (under max) | `FailedAwaitingFix` — `[Review Fix]` into C4.4 |
| Corrective plan proposed | `FixProposed` — patches still need Approve |
| Corrective apply | back to `AppliedAwaitingVerify` |
| Cycle count hits max | `Exhausted` — stop; explain; suppress further fix offers |
| User says “Stop.” / Review Cancel | `Cancelled` |

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Owns `CodingVerificationLoop` session state + orchestration |
| **C3 apply / ExecutionSummary** | Starts or continues the loop after mutation |
| **C4.2 / C4.5** | Verify + summarize; advance cycle outcomes |
| **C4.4** | Corrective plan proposal only |
| **Review Before Action** | Terminal verify + every corrective mutation |
| **Pair Programming** | Conversation First chips; suppressed when exhausted |
| **ReasoningProvider** | Never executes tools / never owns the loop |

## Soft-path note

**C4.6 is not a soft path.** Soft-path order is unchanged:

```
… → test_execution → build_awareness → fix_from_verification → coding_edit → …
```

The loop is Planner session state advanced at apply / verify / fix / cancel hooks.

## Hard lines

1. Reuse C2 / C3 / C4 — no parallel coding-agent architecture.  
2. Every corrective mutation requires explicit Review/Approval.  
3. Never automatically approve a corrective change.  
4. Never silently retry mutation.  
5. Hard `MAX_CYCLES` — stop and explain when reached.  
6. Preserve cycle history for diagnostics / conversation.  
7. Failed verification never triggers an uncontrolled mutation loop.  
8. User can stop/cancel (phrases + Review Cancel).  
9. Each cycle remains observable through existing diagnostics.  
10. Git mutation stays separate (read-only git observation only when already available).

## Constitutional audit

| Rule | Status |
|------|--------|
| Reuse existing C2 / C3 / C4 | PASS |
| No parallel coding agent architecture | PASS |
| Planner owns orchestration | PASS |
| Corrective mutations require Review/Approval | PASS |
| No automatic approval | PASS |
| No silent mutation retry | PASS |
| Hard maximum cycles (`MAX_CYCLES=3`) | PASS |
| Stop and explain at limit | PASS |
| Preserve execution / verification history | PASS |
| No uncontrolled mutation loop on failure | PASS |
| User can stop/cancel | PASS |
| Observable via existing diagnostics | PASS |
| Not C5 general-purpose agent loop | PASS |
| Coding mutation → verification → optional reviewed correction only | PASS |
| Tests (first pass / fail+cancel / max cycles / no auto-approve / no mutation without review) | PASS |

**Verdict: PASS**

### Remaining limitations

- Corrective **apply** still requires the existing generate-patches → Approve path
  after C4.4; the loop tracks that apply when `files_edited` is present.
- Rejecting a corrective Review Card cancels the paused plan; say “Stop.” to
  clear loop state explicitly if needed.
- Standalone “Run the tests.” without a prior coding apply does not start a loop.

## Out of scope

- C5 autonomous Agent Loop / goal-seeking multi-domain agents
- Auto-Approve of verification or corrective mutations
- Silent re-apply / unbounded repair
- A second orchestration kernel beside the Planner
