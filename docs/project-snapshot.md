# Project Snapshot (Sprint B2.4)

**Status: Current** — read-only project intelligence observation.

`ProjectSnapshot` is the immutable representation of a software project as an
artifact: metadata, languages, frameworks, package manager / build system,
dependency-graph summary, cargo/npm metadata, repository metadata, and
workspace layout.

It does **not** execute tools, reason, apply policy, talk to LLMs, or build a
`ContextBundle`.

## Ownership

| Concern | Owner |
|---------|--------|
| Orchestration (when to assemble) | Planner (via Application `prepare_context_session`) |
| Ambient refresh / FS observation | Application `ContextMaintenance` (`MaintenanceKind::ProjectSnapshot`) |
| Observation contract | `ProjectSnapshot` (`jaymi-context`) |
| Consumption | Context providers (`ProjectProvider`) — session only |
| Project identity | Project Engine (`get` / open id) |
| Request path | Providers **must not** scan the filesystem |
| Planner | **Never** scans projects for context |

```text
Project Engine identity + project root
        │  host facts only (no FS)
        ▼
 ambient ContextMaintenance (ProjectSnapshot job)
        │  observe_project_intelligence (marker / shallow parse)
        │  completed store
        ▼
 prepare merges latest completed
        │  (never waits; never FS-walks on request)
        ▼
 ProjectSnapshot ──► ContextSessionInputs
        │
        ▼
 ProjectProvider ──► ActiveProject + ProjectIntelligence sections
        │  (relevance + budget select what enters the bundle)
        ▼
 LlmContext / PromptBuilder / Reasoning
```

## Fields

| Field | Meaning |
|-------|---------|
| `metadata` | Project id / name / description / root / type |
| `languages` | Detected languages (capped) |
| `frameworks` | Detected frameworks (capped) |
| `package_manager` | Observed package manager kind |
| `build_system` | Observed build system kind |
| `dependency_summary` | Top-level deps, counts, workspace members |
| `cargo` | Shallow Cargo.toml metadata when present |
| `npm` | Shallow package.json metadata when present |
| `repository` | `.git` presence + head branch when cheap |
| `workspace_layout` | Shape label + top-level dirs |
| `timestamp` | Capture time (ignored for equality / fingerprints) |

## Rules

* Read-only observation
* Context providers consume `session.project_snapshot`
* Planner never scans projects
* No filesystem scanning during ordinary request assemble for intelligence
* Ambient observation may read marker files and shallow-parse manifests
* `ProjectContext` detail is attached only for project-session intents
  (open / continue) so the ContextBundle open API stays honest — not for chat
* Distinct from [`WorkspaceSnapshot`](workspace-snapshot.md) (live Coding chrome)
  and [`EditorSnapshot`](editor-snapshot.md) (buffer / LSP)

## Capture path

Project open / Coding open / coalesced reschedule →
`Application::schedule_project_snapshot_refresh` →
`MaintenanceKind::ProjectSnapshot` worker →
`observe_project_intelligence` →
completed store →
`prepare_context_session` merges via `merge_completed_into_session`.

Cursor / editor thrash does **not** re-walk project markers; that stays on
workspace + editor observation refresh.

See [context-maintenance.md](context-maintenance.md).

## Tests

* `jaymi-context` unit tests in `project_snapshot.rs`
* `apps/jaymi` maintenance job publishes + merges `project_snapshot`
* `ProjectProvider` prefers snapshot; `detail: None` on the request path
