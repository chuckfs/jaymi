# Editor Snapshot (Sprint B2.3)

**Status: Current** — read-only editor intelligence observation for Coding.

`EditorSnapshot` is the immutable representation of language-aware editor state
for the active Coding buffer. It answers: *what does the editor + language
server observation look like right now?*

It does **not** execute tools, reason, apply policy, talk to LLMs, or build a
`ContextBundle`.

## Ownership

| Concern | Owner |
|---------|--------|
| Orchestration (when to assemble) | Planner (via Application `prepare_context_session`) |
| Ambient refresh | Application `ContextMaintenance` (`MaintenanceKind::EditorSnapshot`) |
| Observation contract | `EditorSnapshot` (`jaymi-context`) |
| Consumption | Context providers (`EditorProvider`, `DiagnosticsProvider`) |
| Interactive LSP (rename / goto / UI hover) | Application `coding_lsp_*` → Planner → `language_server` tool |
| Reasoning | Assembled `ContextBundle` / `LlmContext` / Prompt only — **never LSP** |

```text
CodingState (+ optional read-only LspProvider enrichment)
        │  host observation only
        ▼
 ambient ContextMaintenance (EditorSnapshot job)
        │  completed store
        ▼
 prepare merges latest completed
        │
        ▼
 EditorSnapshot ──► ContextSessionInputs
        │
        ▼
 EditorProvider / DiagnosticsProvider ──► ContextBundle
        │
        ▼
 LlmContext / PromptBuilder / Reasoning   ✗ no LSP
```

## Fields

| Field | Meaning |
|-------|---------|
| `active_file` | Focused buffer path / dirty / language |
| `open_editors` | Open tabs |
| `cursor` | Explicit caret |
| `selection` | Selection range + selected text when Monaco reports a span; caret-as-zero-width with `text: None` when empty |
| `symbol` | Symbol at cursor when known |
| `enclosing_function` | Enclosing function / method when known |
| `enclosing_type` | Enclosing type when known |
| `semantic_tokens` | Semantic token spans (capped; empty until host fills) |
| `references` | Reference locations (capped; ambient LspProvider when available) |
| `diagnostics` | Editor diagnostics (from Coding Problems / LSP working set) |
| `code_lens` | Code lenses (empty until host fills) |
| `hover` | Hover at cursor (ambient LspProvider when available) |
| `timestamp` | Capture time (ignored for equality / fingerprints) |

## Rules

* Read-only observation
* Context providers consume `session.editor_snapshot`
* Planner never reads LSP to build request context
* Reasoning never talks to LSP
* Ambient enrichment may use **read-only** `LspProvider::execute` (same pattern as
  Git maintenance) — never the `language_server` tool / Planner path
* Distinct from [`WorkspaceSnapshot`](workspace-snapshot.md) and chrome
  `EditorWorkspaceSnapshot`

## Capture path

Editor / selection / diagnostics / terminal / project / Coding open →
`Application::schedule_coding_observation_refresh` →
`MaintenanceKind::EditorSnapshot` worker →
`capture_editor_snapshot_from_coding` (+ optional hover/references) →
completed store →
`prepare_context_session` merges via `merge_completed_into_session`.

### Monaco selection (Sprint B2.13.3)

```text
Monaco onDidChangeCursorSelection
        │  IPC Selection { path, range, text }
        ▼
 CodingState.EditorViewState.selection   (workspace observation)
        │  schedule ambient Editor/Workspace refresh
        ▼
 fill_editor_sections → current_selection
        │
        ▼
 EditorSnapshot.selection / WorkspaceSnapshot.active_selection
        │
        ▼
 Environmental Resolution / EditorProvider
```

Monaco types never reach Planner or ContextEngine. Cursor IPC remains
independent (caret tracking preserved). No synchronous LSP on this path.

See [context-maintenance.md](context-maintenance.md).

## Tests

* `jaymi-context` unit tests in `editor_snapshot.rs`
* `apps/jaymi` `context_session` asserts editor snapshot attachment
* Provider contribute paths prefer `editor_snapshot` when present
