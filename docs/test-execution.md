# Test Execution (Sprint C4.2)

## Goal

Allow Jaymi to execute appropriate project tests through the existing
Planner → Tool → Provider architecture.

C4.1 Build Awareness remains the discovery layer. C0–C3 stay closed.
Structured interpretation of results is Sprint C4.3
([docs/verification-diagnostics.md](verification-diagnostics.md)).

## Behavior

| User says | Jaymi does |
|-----------|------------|
| “Run the tests.” | Discovers a grounded test command (C4.1), pauses for Review, then runs via Terminal Tool |
| “Test what we just changed.” | Prefers a targeted filter when file/git evidence supports it; else full suite |
| “Verify the tests pass.” | Same orchestration; reports pass/fail without mutating code |
| “Run the relevant tests.” | Targeted when possible; otherwise full project tests |

Missing evidence → honest “no test command” (no invention).
Failures → reported only; **no** automatic edits or silent retries.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Test intent, target selection, ExecutionPlan, review resume, `TestExecutionReport` |
| **C4.1 Build Awareness** | `VerificationTarget` discovery (test-purpose candidates) |
| **Terminal Tool / Provider** | Sole command execution path |
| **Permission + ToolRisk** | Terminal·Execute + Destructive → Review Before Action |
| **RuntimeSnapshot** | Latest test outcome for Context / Reasoning on later turns |
| **ReasoningProvider** | Never executes shell |

## Soft-path order

```
… → multi_file → test_execution (C4.2) → build_awareness (C4.1)
  → fix_from_verification (C4.4) → coding_edit → …
```

## Captured fields

`TestExecutionReport` carries (when available):

- command · working directory · exit status · stdout · stderr · duration · affected project
- scope (targeted / full_suite) · truncation flag · status

PTY streams are combined today; `stderr` is reserved for a future split.
Large output is head+tail truncated while preserving error tails.

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner owns test intent / orchestration | PASS |
| Reuse C4.1 VerificationTarget | PASS |
| Planner → Tool → Provider only | PASS |
| No Reasoning/LLM shell execution | PASS |
| Permission / risk / review gates | PASS |
| Evidence-grounded commands; no invention | PASS |
| Targeted when evidence allows; else full suite | PASS |
| Capture command/cwd/exit/output/duration/project | PASS |
| No automatic mutation from failures | PASS |
| No silent retries | PASS |
| Conversation First | PASS |
| Results available to Context (RuntimeSnapshot) | PASS |
| Bounded large output | PASS |
| Tests (pass/fail/target/full/missing/timeout/large/permission/no bypass/no mutation) | PASS |

**Verdict: PASS**

### Remaining limitations

- Stdout/stderr not split (single PTY stream).
- Targeting heuristics are conservative (Cargo/Go/JS path filters only).
- Host must Approve the Review Card (or Coding gesture auto-Approve) before run.

## Out of scope

- Autonomous repair loops (C4.4 prepares a reviewed Coding Plan only)
- Rich failure classification beyond C4.3
- Parallel test orchestration / coverage dashboards
