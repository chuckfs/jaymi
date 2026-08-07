# Workspace Snapshot (Sprint B2.1 / B2.2 / B2.13.2)

**Status: Current** — canonical observational Coding environment contract with
**entirely background-driven** ambient refresh.

`WorkspaceSnapshot` is the single immutable representation of the live Coding
workspace. It answers: *what does the environment look like right now?*

It does **not** answer how to change it, whether an action is allowed, or what
Jaymi should say next.

## Ownership

| Concern | Owner |
|---------|--------|
| Orchestration (when to assemble) | Planner (via Application `prepare_context_session`) |
| Ambient refresh scheduling + observation | Application `ContextMaintenance` (never Planner) |
| Observation contract | `WorkspaceSnapshot` (`jaymi-context`) |
| Consumption | Context Engine (`ContextSessionInputs` / `ContextEngine::workspace_snapshot`) |
| Live mutable UX state | `CodingState` (capability-engine) |
| Project identity | Project Engine |
| Git / inventory contributions | Providers + Application maintenance |
| Tool execute / Action Policy / Permission | Unchanged — snapshot never enters those paths |

```text
CodingState + ProjectEngine + Git maintenance + marker files
        │  (host observation only — ambient worker)
        ▼
 ambient ContextMaintenance (WorkspaceSnapshot job)
   including observe_toolchain marker probes
        │  completed store
        ▼
 prepare_context_session merges latest completed
   (never rebuilds · never probes FS)
        │
        ▼
 WorkspaceSnapshot  ──► ContextSessionInputs
        │
        ▼
 ContextEngine (consumes; still sole ContextBundle factory)
```

## Fields

| Field | Meaning |
|-------|---------|
| `active_project` | Project id / name / root from Project Engine |
| `workspace_root` | Canonical root (project root, else explorer mirror) |
| `workspace_kind` | UX kind id (`coding`, …) |
| `current_file` | Active buffer path / dirty / language |
| `open_files` | Open tabs |
| `active_selection` | Selection range + text when Monaco reports a span; caret-as-zero-width otherwise |
| `cursor` | Explicit caret position |
| `active_branch` | Git branch when known |
| `language` | Denormalized from `current_file.language` |
| `package_manager` | Marker-file observation (`Cargo.toml`, lockfiles, …) |
| `build_system` | Marker-file observation (`Cargo.toml`, `CMakeLists.txt`, …) |
| `timestamp` | Capture time (ignored for equality / cache fingerprints) |

## Rules

* Observational only — no tools, no reasoning, no policy, no LLMs
* Never constructs a `ContextBundle`
* No UI logic inside the type
* No provider bypasses — providers contribute data; host observes; Context Engine assembles
* Distinct from capability-engine `EditorWorkspaceSnapshot` (chrome persistence only)
* Conversation always consumes the **latest completed** ambient snapshot (never waits)
* **Sprint B2.13.2:** conversational prepare never rebuilds a WorkspaceSnapshot
  and never calls `observe_toolchain` / marker probes

## Capture path

### Ambient only (Sprint B2.2 / B2.13.2)

Editor / selection / git / diagnostics / terminal / project changes →
`Application::schedule_workspace_snapshot_refresh` →
background worker (`capture_workspace_snapshot_from_coding`, including
`observe_toolchain`) →
`CompletedMaintenanceSnapshots.workspace_snapshot` →
`prepare_context_session` merges via `merge_completed_into_session`.

If no completed snapshot exists yet, prepare **schedules** ambient refresh
(non-blocking) and proceeds without a WorkspaceSnapshot until the job
completes. There is no synchronous prepare bootstrap.

Toolchain detection (`observe_toolchain`) runs **only** inside the ambient
worker (filesystem presence checks).

See [context-maintenance.md](context-maintenance.md).

## Tests

* `jaymi-context` unit tests in `workspace_snapshot.rs`
* `apps/jaymi` `context_session` tests: prepare builder does not attach /
  probe WorkspaceSnapshot; ambient capture still observes toolchain
* `apps/jaymi` `context_maintenance` unit + integration tests for ambient merge
