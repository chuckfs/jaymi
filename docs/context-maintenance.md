# Context Maintenance

**Status: Current Implementation** (Sprint B2.2–B2.6 ambient snapshots;
**B2.13.2** — WorkspaceSnapshot observation entirely background-driven)

Slow host-side context updates and ambient Coding / project / runtime observation
refresh run as **Application-owned background maintenance**. Conversation never
waits for them. The Planner still assembles request context exclusively through:

```text
Application::prepare_context_session
  → ContextEngine::set_session_inputs (latest completed snapshots)
Planner
  → ContextEngine::assemble_with
```

Maintenance **never** builds a parallel `ContextBundle`, **never** reasons,
**never** calls LLMs, **never** executes tools, and **never** bypasses the
Context Engine.

---

## Ownership

| Kind | Refresh owner | When scheduled | Session field | Context provider |
|------|---------------|----------------|---------------|------------------|
| **Git status / GitSnapshot** | Application `ContextMaintenance` (read-only `GitProvider`) | Coding open, Git Refresh, after Planner git mutate (publish + schedule) | `git_status` / `git_snapshot` | `GitStatusProvider` |
| **Workspace inventory** | Application `ContextMaintenance` (filesystem walk) | Coding open, Explorer Refresh | `workspace_inventory` | `WorkspaceInventoryProvider` |
| **Diagnostics** | Application `ContextMaintenance` (`ProblemsRegistry`) | Coding open, Problems Refresh, after Planner activity | `diagnostics` | `DiagnosticsProvider` |
| **File summaries** | Application `ContextMaintenance` (open-file head read) | Coding open, after editor restore | `file_summaries` | `FileSummariesProvider` |
| **Workspace snapshot** | Application `ContextMaintenance` (host observation + `observe_toolchain`) | Coding open; editor / selection / git / diagnostics / terminal / project changes; prepare schedules if missing | `workspace_snapshot` | Context Engine session accessor (not a provider assemble path) |
| **Editor snapshot** | Application `ContextMaintenance` (host observation ± read-only `LspProvider`) | Same Coding observation triggers as WorkspaceSnapshot | `editor_snapshot` | `EditorProvider` / `DiagnosticsProvider` |
| **Project snapshot** | Application `ContextMaintenance` (marker / shallow FS observation) | Coding open; project open / close; coalesced reschedule | `project_snapshot` | `ProjectProvider` |
| **Runtime snapshot** | Application `ContextMaintenance` (Coding terminal + TerminalProvider alive list) | Coding open; terminal complete / close; coalesced reschedule | `runtime_snapshot` | `RuntimeProvider` |

### Must not

| Actor | Must not |
|-------|----------|
| **Context providers** | Shell out to git, walk the workspace, collect Problems, read file bodies, scan project markers, or re-run cargo / tests during assemble |
| **Conversation / `begin_generation` / `prepare_context_session`** | Block on maintenance jobs; rebuild WorkspaceSnapshot; call `observe_toolchain` / marker probes |
| **Maintenance** | Call `ContextEngine::assemble_*`, invent Intent/Capabilities, execute Tools, reason, or call LLMs |
| **Planner** | Own background refresh scheduling, or scan projects for context |
| **Reasoning / PromptBuilder** | Run git commands or re-run cargo / tests |
| **Runtime observation** | Re-run cargo / tests; block conversation waiting for runtime |

Mutating Git / path operations still go **Planner → Tool → Provider**. Maintenance is host-side snapshot refresh only.

---

## Flow

```text
UI / Coding open / editor · selection · git · diagnostics · terminal / project open
        │
        ▼
Application::schedule_context_maintenance(kind)
  (WorkspaceSnapshot → schedule_workspace_snapshot_refresh)
  (EditorSnapshot → schedule_editor_snapshot_refresh)
  (ProjectSnapshot → schedule_project_snapshot_refresh)
  (RuntimeSnapshot → schedule_runtime_snapshot_refresh)
        │  (non-blocking; dedupes in-flight; coalesces observational kinds)
        ▼
background worker → CompletedPayload
  (WorkspaceSnapshot worker runs observe_toolchain)
        │
        ▼
ContextMaintenance store (latest completed)
        │
        ├─► pump_context_maintenance → Coding UI (explorer / git / problems)
        │                              + coalesced observational reschedule
        └─► prepare_context_session → merge into ContextSessionInputs
                    │  (if WorkspaceSnapshot missing → schedule only)
                    ▼
            ContextEngine::assemble_with → providers read session only
```

Conversation requests always consume the **latest completed** WorkspaceSnapshot.
If a job is still in flight (or none has completed yet), prepare uses the
previous completed value or proceeds without one and **schedules** ambient
refresh — never a wait, never a synchronous rebuild / toolchain probe
(Sprint B2.13.2). EditorSnapshot may still bootstrap once from in-memory
CodingState when no completed editor observation exists. When live CodingState
already has open-file / selection chrome (Sprint B2.13.3), prepare **keeps**
those fields and overlays completed language intelligence (symbol / hover /
refs) — stale ambient captures must not regress Monaco selection. `ProjectSnapshot`
is maintenance-only (no request-path FS).

### Performance note

Synchronous marker-file probes (`Cargo.toml`, lockfiles, …) on the conversational
prepare path were removed in B2.13.2 so first-token latency does not include
workspace FS checks. Toolchain freshness is owned by ambient WorkspaceSnapshot
jobs triggered on Coding / project / editor activity.

### WorkspaceSnapshot ambient triggers (B2.2)

| Feed | Application hook |
|------|------------------|
| Editor open / activate / close / edit / save | `open_coding_file*`, `activate_*`, `close_*`, `set_coding_tab_content*`, `save_coding_file`, pane focus/close |
| Selection / cursor | `set_coding_tab_cursor*`, `set_coding_tab_selection*` (B2.13.3) |
| Git | `apply_git_response` publish + pump Git UI apply |
| Diagnostics | `apply_lsp_diagnostics` + pump Problems apply |
| Terminal completes | `apply_terminal_response` |
| Project open / close / Coding open / workspace close | `open_project`, `close_project`, `schedule_coding_open`, `close_ui_workspace` (publishes empty) |
| Prepare with no completed snapshot | `prepare_context_session` → `schedule_workspace_snapshot_refresh` |

Rapid cursor moves coalesce: while a WorkspaceSnapshot / EditorSnapshot job is
inflight, further schedules mark pending; `pump_context_maintenance` reschedules
with fresh CodingState after completion.

### ProjectSnapshot ambient triggers (B2.4)

| Feed | Application hook |
|------|------------------|
| Coding open | `schedule_coding_context_maintenance` |
| Project open | `open_project` → `schedule_project_snapshot_refresh` |
| Project / workspace close | publishes empty `ProjectSnapshot` |
| Coalesced reschedule | `pump_context_maintenance` after inflight ProjectSnapshot |

Cursor thrash does **not** re-walk project markers.

---

## API surface (`Application`)

| Method | Behavior |
|--------|----------|
| `schedule_context_maintenance(kind)` | Start one background job |
| `schedule_workspace_snapshot_refresh()` | Ambient Coding environment observation refresh |
| `schedule_editor_snapshot_refresh()` | Ambient editor intelligence observation refresh |
| `schedule_project_snapshot_refresh()` | Ambient project intelligence observation refresh |
| `schedule_coding_observation_refresh()` | Workspace + editor ambient refresh |
| `schedule_coding_context_maintenance()` | Schedule inventory + git + diagnostics + file summaries + WorkspaceSnapshot + EditorSnapshot + ProjectSnapshot |
| `pump_context_maintenance()` | Non-blocking drain into Coding UI + coalesced snapshot reschedule |
| `refresh_coding_git` / `explorer` / `problems` | **Schedule** (non-blocking) |
| `refresh_coding_*_now` | Synchronous path for first-paint seed / tests |

---

## Related

* [workspace-snapshot.md](workspace-snapshot.md) — observation contract (B2.1) + ambient refresh (B2.2)
* [editor-snapshot.md](editor-snapshot.md) — editor intelligence (B2.3)
* [project-snapshot.md](project-snapshot.md) — project intelligence (B2.4)
* [git-snapshot.md](git-snapshot.md) — Git intelligence (B2.5)
* [context.md](context.md) — Context Engine assemble contract
* [complexity.md](complexity.md) — greeting excludes maintenance-backed providers
* [session-cache.md](session-cache.md) — separate Application cache for cheap immutable snapshots
