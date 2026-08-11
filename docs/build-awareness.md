# Build Awareness (Sprint C4.1)

## Goal

Teach Jaymi to understand how the current project is built and which existing
verification commands are appropriate after coding changes — **without running
them**.

C0–C3 remain closed. This sprint does not reopen or redesign them.
C4.2 (command execution) is explicitly out of scope.

## Behavior

| User says | Jaymi does |
|-----------|------------|
| “Does this build?” | Discovers grounded check/build candidates from WI |
| “Check whether this compiles.” | Prefers compile-oriented candidates (`cargo check`, `npm run typecheck`, …) |
| “Verify the changes.” | Lists verification-oriented candidates |
| “What should I run to check this?” | Recommends project-defined commands only |
| “Is this project currently buildable?” | Explains discovery; notes that execution is deferred; reuses ambient diagnostics |

When evidence is missing, ambiguous, unsupported, or there is no active project,
Jaymi says so — **never invents** a command.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Sole verification intent + soft path (`complete_build_awareness`) |
| **ProjectSnapshot / WI** | Observed build system, package manager, Cargo/npm metadata, scripts |
| **Diagnostics section** | Ambient Problems reused (not a fresh build) |
| **ContextEngine** | Sole ContextBundle factory |
| **Tool Engine / Terminal** | Not invoked in C4.1 |
| **ReasoningProvider** | Not used to invent or execute shell commands |

## Soft-path order

After C1.5 / C1.7; before coding_edit / review / plan / understanding:

```
… → code_generation → multi_file → test_execution (C4.2) → build_awareness (C4.1)
  → fix_from_verification (C4.4) → coding_goal (C5.1) → coding_edit → …
```

## VerificationTarget

Where possible, each candidate carries:

- project
- build system
- candidate verification command
- purpose
- working directory
- relevant configuration
- expected output type
- confidence / evidence

Supported discovery (from existing observation, not a second scanner):

| Evidence | Candidates |
|----------|------------|
| Cargo / `Cargo.toml` | `cargo check`, `cargo build`, (`cargo test` for verify/recommend) |
| `package.json` scripts | `npm`/`pnpm`/`yarn run <script>` only for observed keys (`check`, `test`, `build`, `lint`, …) |
| `go.mod` | `go build ./…`, `go test ./…` |
| Makefile | default `make` only (low confidence; targets not parsed) |
| CMake only | no invented command |

## Diagnostics

`LastPlannerActivity` + Coding Diagnostics expose `build_awareness_*` from
`to_diagnostics_summary()` — observational only. Conversation gets markdown
discovery, not raw internals.

## Constitutional lines

Planner Over Models · Conversation First · Intelligence Through Context ·
Projects First-Class · Review Before Action · Not Fully Autonomous · Offline First

## Constitutional audit

| Rule | Status |
|------|--------|
| Planner owns verification intent / orchestration | PASS |
| Reuse WI / ProjectSnapshot (no second discovery) | PASS |
| Prefer project-defined scripts / config / conventions | PASS |
| No command invention without evidence | PASS |
| Read-only (no terminal / mutation) | PASS |
| No Reasoning shell execution | PASS |
| No Planner → Tool → Provider bypass | PASS |
| Reuse existing diagnostics | PASS |
| Soft-path tests (Rust, scripts, multi, missing, ambiguous, none, unsupported, no invention, no mutation, no bypass) | PASS |

**Verdict: PASS**

### Remaining limitations

- Make / CMake targets and custom CI wrappers are not fully parsed.
- Buildability cannot be confirmed until C4.2 executes a check.
- Script preference is heuristic over observed script keys.
- Monorepo package-local scripts beyond the root ProjectSnapshot are not expanded.

## Out of scope (C4.3+)

- Automatic command execution → **done in C4.2** ([test-execution.md](test-execution.md))
- Streaming build output into conversation
- Auto-fix from compiler errors
- Rich failure interpretation (C4.3)
