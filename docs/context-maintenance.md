# Context Maintenance

**Status: Current Implementation**

Slow host-side context updates run as **Application-owned background maintenance**. Conversation never waits for them. The Planner still assembles request context exclusively through:

```text
Application::prepare_context_session
  → ContextEngine::set_session_inputs (latest completed snapshots)
Planner
  → ContextEngine::assemble_with
```

Maintenance **never** builds a parallel `ContextBundle` and **never** bypasses the Context Engine.

---

## Ownership

| Kind | Refresh owner | When scheduled | Session field | Context provider |
|------|---------------|----------------|---------------|------------------|
| **Git status** | Application `ContextMaintenance` (read-only `GitProvider`) | Coding open, Git Refresh, after Planner git mutate (publish) | `git_status` | `GitStatusProvider` |
| **Workspace inventory** | Application `ContextMaintenance` (filesystem walk) | Coding open, Explorer Refresh | `workspace_inventory` | `WorkspaceInventoryProvider` |
| **Diagnostics** | Application `ContextMaintenance` (`ProblemsRegistry`) | Coding open, Problems Refresh, after Planner activity | `diagnostics` | `DiagnosticsProvider` |
| **File summaries** | Application `ContextMaintenance` (open-file head read) | Coding open, after editor restore | `file_summaries` | `FileSummariesProvider` |

### Must not

| Actor | Must not |
|-------|----------|
| **Context providers** | Shell out to git, walk the workspace, collect Problems, or read file bodies during assemble |
| **Conversation / `begin_generation`** | Block on maintenance jobs |
| **Maintenance** | Call `ContextEngine::assemble_*`, invent Intent/Capabilities, or execute Tools |
| **Planner** | Own background refresh scheduling |

Mutating Git / path operations still go **Planner → Tool → Provider**. Maintenance is host-side snapshot refresh only.

---

## Flow

```text
UI / Coding open / Planner activity
        │
        ▼
Application::schedule_context_maintenance(kind)
        │  (non-blocking; dedupes in-flight)
        ▼
background worker → CompletedPayload
        │
        ▼
ContextMaintenance store (latest completed)
        │
        ├─► pump_context_maintenance → Coding UI (explorer / git / problems)
        └─► prepare_context_session → merge into ContextSessionInputs
                    │
                    ▼
            ContextEngine::assemble_with → providers read session only
```

Conversation requests always consume the **latest completed** snapshot. If a job is still in flight, the previous completed values (or empty defaults) are used.

---

## API surface (`Application`)

| Method | Behavior |
|--------|----------|
| `schedule_context_maintenance(kind)` | Start one background job |
| `schedule_coding_context_maintenance()` | Schedule inventory + git + diagnostics + file summaries |
| `pump_context_maintenance()` | Non-blocking drain into Coding UI (UI frame + prepare) |
| `refresh_coding_git` / `explorer` / `problems` | **Schedule** (non-blocking) |
| `refresh_coding_*_now` | Synchronous path for first-paint seed / tests |

---

## Related

* [context.md](context.md) — Context Engine assemble contract
* [complexity.md](complexity.md) — greeting excludes maintenance-backed providers
* [session-cache.md](session-cache.md) — separate Application cache for cheap immutable snapshots
