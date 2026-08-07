# Runtime Snapshot (Sprint B2.6)

**Status: Current** — read-only runtime intelligence observation.

`RuntimeSnapshot` is the immutable representation of live Coding runtime state:
latest cargo check / build / tests, a terminal output summary, running sessions,
and recent failures.

It does **not** execute tools, re-run cargo or tests, reason, apply policy, talk
to LLMs, or build a `ContextBundle`.

## Ownership

| Concern | Owner |
|---------|--------|
| Terminal execution (PTY) | TerminalProvider via Planner → Tool |
| Ambient refresh | Application `ContextMaintenance` (`MaintenanceKind::RuntimeSnapshot`) |
| Observation contract | `RuntimeSnapshot` (`jaymi-context`) |
| Consumption | Context providers (`RuntimeProvider`) — session only |
| Request / Conversation path | **Must not** block waiting for runtime |

```text
TerminalProvider (PTY execute via Planner→Tool)
        │  CodingState.terminal_sessions (+ alive from list_sessions)
        ▼
 ambient ContextMaintenance (RuntimeSnapshot job)
        │  observe_runtime_intelligence (heuristics only; no cargo re-run)
        │  completed store
        ▼
 prepare merges latest completed
        │  (never waits; never re-runs cargo on request)
        ▼
 RuntimeSnapshot ──► ContextSessionInputs
        │
        ▼
 RuntimeProvider ──► RuntimeIntelligence section
        │  (relevance + budget select what enters the bundle)
        ▼
 LlmContext / PromptBuilder / Reasoning
```

## Fields

| Field | Meaning |
|-------|---------|
| `latest_cargo_check` | Most recent observed `cargo check` / typecheck-style outcome |
| `latest_build` | Most recent observed build / compile outcome |
| `latest_tests` | Most recent observed test-suite outcome |
| `terminal_summary` | Active session id, counts, last command, capped output tail |
| `running_processes` | Alive / active terminal session refs (capped) |
| `recent_failures` | Recent failed command outcomes (newest first, capped) |
| `timestamp` | Capture time (ignored for equality / fingerprints) |

## Rules

* Observational only — never re-runs cargo / tests during observation or assemble
* TerminalProvider owns live updates; Application clones facts for ambient jobs
* Context providers consume `session.runtime_snapshot` summaries only
* Conversation / `begin_generation` never blocks waiting for runtime
* Distinct from Problems / Diagnostics (LSP) and from [`GitSnapshot`](git-snapshot.md)

## Capture path

Coding open / terminal complete / coalesced reschedule →
`Application::schedule_runtime_snapshot_refresh` →
`MaintenanceKind::RuntimeSnapshot` worker →
`observe_runtime_intelligence` →
completed store →
`prepare_context_session` merges via `merge_completed_into_session`.

Workspace / project close publishes an empty `RuntimeSnapshot` so conversation
does not keep a stale observation.
