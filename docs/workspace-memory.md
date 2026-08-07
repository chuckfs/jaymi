# Workspace Memory (Sprint B2.9)

**Status: Current** — session-scoped Coding workspace activity memory.

`WorkspaceMemorySnapshot` remembers workspace activity:

* Recent edits
* Recently opened files
* Recent builds
* Recent failures
* Current coding objective

It is **distinct from Conversation Memory** (Memory Engine retrieve / promote /
Working → Conversation → Project → Personal). Context Policy decides when this
feed participates in a `ContextBundle`.

## Ownership

| Concern | Owner |
|---------|--------|
| Live activity rings | `CodingState.workspace_activity` |
| Recently opened MRU | `OpenEditors.recently_opened` |
| Build / failure recording | Application (terminal apply) + CodingState |
| Coding objective | Application soft-update on coding-shaped prompts; CodingState stores |
| Observation contract | `WorkspaceMemorySnapshot` (`jaymi-context`) |
| Contribution | `WorkspaceMemoryProvider` (session only) |
| Inclusion | Context Policy + Context Selection profiles |
| Bundle factory | Context Engine only |

```text
CodingState (edits / objective / builds)
        │  + editors.recently_opened
        ▼
 prepare captures WorkspaceMemorySnapshot
        │  (observe_workspace_memory — no Memory Engine writes)
        ▼
 ContextSessionInputs.workspace_memory_snapshot
        │
        ▼
 WorkspaceMemoryProvider → WorkspaceMemory section / candidate
        │
        ▼
 Context Policy (selection allowlist) → ContextBundle
```

## Distinct from Conversation Memory

| | Workspace Memory | Conversation Memory |
|--|------------------|---------------------|
| Store | Coding session rings | Memory Engine |
| Lifetime | Coding / workspace session | Conversation / Project / Personal scopes |
| Writes | Observational host updates | retrieve / promote APIs |
| Provider | `workspace_memory` | `memory` |
| Greeting | Omitted | Included (Required) |

## Policy inclusion

Allowed (examples): `debug_compile`, `coding_general`, `terminal`, `file_edit`,
`project_session`, `project_overview`.

Omitted: `greeting`, `small_talk` (strict). Complexity tier **Excluded** for
greeting; **Required** for `coding_question`.

## Rules

* Never writes Conversation / Project / Personal memories
* Never builds a `ContextBundle`
* Conversation never blocks waiting for activity refresh
* Cleared on project / coding workspace close

## Related

* [runtime-snapshot.md](runtime-snapshot.md) — live terminal / cargo outcomes
* [context-selection.md](context-selection.md) — when this feed is selected
* [memory.md](memory.md) — Conversation / Project / Personal Memory Engine
* [context.md](context.md) — Context Engine contract
