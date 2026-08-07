# Git Snapshot (Sprint B2.5)

**Status: Current** — read-only Git intelligence observation for Coding.

`GitSnapshot` is the immutable representation of working-tree Git state: branch,
HEAD, dirty / staged / untracked / conflict paths, and recent commits.

It does **not** execute tools, reason, apply policy, talk to LLMs, or build a
`ContextBundle`.

## Ownership

| Concern | Owner |
|---------|--------|
| Orchestration (when to assemble) | Planner (via Application `prepare_context_session`) |
| Ambient refresh / git CLI | Application `ContextMaintenance` (`MaintenanceKind::GitStatus`, read-only `GitProvider`) |
| Observation contract | `GitSnapshot` (`jaymi-context`) |
| Consumption | Context providers (`GitStatusProvider`) — session summaries only |
| Mutating Git | Planner → Tool → `GitProvider` |
| Reasoning | Assembled `ContextBundle` / `LlmContext` / Prompt only — **never git** |

```text
GitProvider (read-only status + HEAD + log + conflicts)
        │  maintenance worker only
        ▼
 ambient ContextMaintenance (GitStatus / GitSnapshot job)
        │  completed store
        ▼
 prepare merges latest completed
        │
        ▼
 GitSnapshot ──► ContextSessionInputs
        │
        ▼
 GitStatusProvider ──► GitStatusSection summary
        │
        ▼
 LlmContext / PromptBuilder / Reasoning   ✗ no git commands
```

## Fields

| Field | Meaning |
|-------|---------|
| `is_repository` | Work tree detected |
| `repo_root` | Absolute toplevel |
| `branch` | Current branch |
| `head_sha` / `head_short` | HEAD object name |
| `summary` | Short human label (`clean`, `2 modified`, …) |
| `dirty` | Unstaged / dirty paths (capped) |
| `staged` | Staged paths (capped) |
| `untracked` | Untracked paths (capped) |
| `conflicts` | Merge conflict / unmerged paths (capped) |
| `recent_commits` | Recent commits (capped; newest first) |
| `timestamp` | Capture time (ignored for equality / fingerprints) |

## Rules

* Read-only observation
* Context providers consume `session.git_snapshot` (or derived `git_status`)
* No git commands during Reasoning / PromptBuilder / assemble
* Background updates only (`ContextMaintenance` + post-mutate publish/schedule)
* Distinct from [`ProjectSnapshot`](project-snapshot.md) repository metadata
  (cheap `.git` markers) and from Coding `GitStatusState` (UI dock)

## Capture path

Coding open / Git Refresh / after Planner git mutate →
`MaintenanceKind::GitStatus` worker →
`GitProvider::status` (+ HEAD / log / conflicts) →
`GitSnapshot` + derived `GitStatusSection` →
completed store →
`prepare_context_session` merges via `merge_completed_into_session`.

See [context-maintenance.md](context-maintenance.md).

## Tests

* `jaymi-context` unit tests in `git_snapshot.rs`
* `jaymi-providers` porcelain conflict classification + HEAD/log after commit
* `apps/jaymi` maintenance job publishes + merges `git_snapshot`
* `GitStatusProvider` never shells out
