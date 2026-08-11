# Verification Summary (Sprint C4.5)

## Goal

Give Jaymi a concise, useful conversational summary after coding verification.

The summary answers:

- What was changed?
- What was verified?
- What passed?
- What failed?
- What remains?
- What should happen next?

Example shape:

```
## Implemented

6 files changed.

## Verification

✓ cargo check — passed
✓ cargo test — 43 tests passed
✗ 1 integration test failed

## Failure

`tests/settings.rs:84`

The test still expects the previous provider-selection behavior.

## Recommended next step

Fix the failing tests.

[Review Fix]
```

C0–C4.4 remain closed. **Recommended actions are never auto-executed.**

## Behavior

| Situation | Summary |
|-----------|---------|
| Tests pass | `Passed` + command marks; optional Implemented from prior apply |
| Build / test failure | `Failed` + file:line highlights + `[Review Fix]` into C4.4 |
| Awaiting review | `NotRun` — never claims tests passed |
| Cancelled | `Cancelled` — nothing ran |
| Environment / tool / timeout | Failed with remains; **no** code-fix chip |
| Coding apply without verification | `NotRun` + “Run the tests.” |

Detailed C4.3 evidence stays in diagnostics / activity unless the user asks.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Composes `VerificationSummary` after finalize / resume / cancel |
| **C3 ExecutionSummary** | “What changed” (`files_edited`) |
| **C4 VerificationEvidence / TestExecutionReport** | What was verified / passed / failed |
| **Pair Programming** | Surfaces `[Review Fix]` / `Run Tests` chips (Conversation First) |
| **C4.4** | Owns corrective planning when the user follows the recommendation |
| **ReasoningProvider** | Not required; never executes tools from summary chips |

## Soft-path note

**C4.5 is not a soft path.** It is a post-process formatter attached when
verification (or a coding apply without verification) completes. Soft-path order
is unchanged:

```
… → test_execution → build_awareness → fix_from_verification → coding_edit → …
```

## Status vocabulary

`NotRun` · `Passed` · `Failed` · `Blocked` · `Cancelled` · `Unknown`

Never invent a run. Never say “tests passed” when tests were not run.

## Constitutional audit

| Rule | Status |
|------|--------|
| Use actual ExecutionSummary + VerificationEvidence | PASS |
| Never claim verification that did not occur | PASS |
| Never say tests passed when not run | PASS |
| Distinguish not run / passed / failed / blocked / cancelled / unknown | PASS |
| Link failures to files/lines when available | PASS |
| Recommended next actions grounded in evidence | PASS |
| Recommended fix enters C3/C4.4 review pipeline (chip → conversation) | PASS |
| Do not auto-execute recommended actions | PASS |
| Keep detailed diagnostics out of normal conversation | PASS |
| Preserve coding conversation UX (pair chips / Review Before Action) | PASS |
| Tests (pass / partial / build / test / not run / cancel / env / multi-cmd / recommend) | PASS |

**Verdict: PASS**

### Remaining limitations

- “Implemented” depends on a remembered / current ExecutionSummary with
  `files_edited`; pure test turns may omit that section.
- Multiple commands appear only when the caller supplies additional verified
  lines (or prior remembered checks) — a single test run still yields one command.
- Pass counts are parsed only from common cargo-style result lines.

## Out of scope

- Autonomous repair / auto-Approve from the summary chip
- Dumping full compiler logs into conversation
- A second summary system beside Conversation + Diagnostics

See also: bounded Apply→Verify→Fix orchestration in
[`coding-verification-loop.md`](coding-verification-loop.md) (C4.6).
