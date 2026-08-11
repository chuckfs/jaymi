# Verification Diagnostics Feedback (Sprint C4.3)

## Goal

Turn build / test / verification command output into structured,
context-aware evidence instead of dumping raw logs into the conversation.

```
Tool Execution → Verification Result → Planner → Context → Reasoning
→ Conversational Explanation
```

C0–C3 remain closed. C4.1 discovery and C4.2 execution stay as-is.
**Automatic repair is out of scope.**

## Behavior

| Situation | Jaymi says |
|-----------|------------|
| Compiler errors | “Build failed with N errors” + file:line bullets |
| Test failures | “Tests failed” + test names / messages when parseable |
| Warnings only (exit 0) | “Verification succeeded with N warnings” |
| Clean pass | “Verification succeeded” |
| Timeout / tool / environment | Named kind; excerpt when structure is missing |
| Malformed non-zero exit | “Unknown result” — **no invented failures** |

Raw streams stay on the report (`stdout` / bounded `output_excerpt`) for
debugging; conversation prefers structured findings.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Parses output after Terminal Tool finalize; owns `VerificationEvidence` |
| **`verification_diagnostics`** | Kind classification, finding extractors, markdown + BundleDiagnostic mapping |
| **DiagnosticsSection / BundleDiagnostic** | Existing Context diagnostics — `source = "verification"` |
| **ContextEngine** | Sole bundle factory; session diagnostics + RuntimeSnapshot |
| **ReasoningProvider** | May interpret evidence; **never** runs commands |
| **Coding Diagnostics panel** | Observational activity lines only |

## Evidence fields

Where available:

- status / kind · command · exit code · duration
- errors · warnings · affected files · line/range
- test names · failure messages · bounded output excerpt

Kinds: `Success`, `BuildFailure`, `TestFailure`, `Warning`, `ToolFailure`,
`Timeout`, `EnvironmentFailure`, `Unknown`.

## Soft-path note

**C4.5 is not a soft path.** It is a post-process formatter attached when
verification (or a coding apply without verification) completes. Soft-path order
is unchanged:

```
… → multi_file → test_execution (C4.2 + C4.3 enrich)
  → build_awareness → fix_from_verification (C4.4) → coding_edit → …
```

Conversation after a run prefers [`Verification Summary`](verification-summary.md)
over raw evidence dumps; structured findings remain in diagnostics.

## Constitutional audit

| Rule | Status |
|------|--------|
| Preserve raw results in diagnostics / report fields | PASS |
| Structured evidence (status, command, exit, errors, …) | PASS |
| Reuse existing diagnostics (`BundleDiagnostic`) | PASS |
| No second diagnostics system | PASS |
| Feed ContextEngine via session diagnostics + RuntimeSnapshot | PASS |
| Reasoning may interpret; does not execute | PASS |
| Do not invent unsupported failures | PASS |
| Distinguish build / test / warning / tool / timeout / env / unknown | PASS |
| Bound large outputs | PASS |
| Observational — does not decide to mutate code | PASS |
| No automatic repair | PASS |
| Conversation First (structured summary, not raw dump) | PASS |
| Tests (compiler / test / warn / success / malformed / large / multi-file / missing line / tool) | PASS |

**Verdict: PASS**

### Remaining limitations

- Parsers prioritize Cargo / rustc / cargo-test shapes; other toolchains may
  surface as `Unknown` with an excerpt until extractors expand.
- PTY still merges stdout/stderr (C4.2).
- Ambient editor LSP diagnostics are not replaced — verification entries are
  merged alongside and prior `source=verification` rows are refreshed.

## Out of scope

- Automatic fix-from-failure / repair loops (see C4.4 proposal-only bridge)
- A parallel “verification problems” subsystem
- Reasoning-owned shell execution
