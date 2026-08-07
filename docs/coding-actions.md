# Coding Actions

**Status: Current** (Sprint C0.1)

Coding Actions turn the Coding Quick Action Bar into first-class Planner entry
points. The UI emits typed [`CodingAction`](../crates/jaymi-core/src/coding_action.rs)
values only. The Planner owns routing. Workspace Intelligence supplies context.
Mutations still require Execution Plans and review.

## Ownership

```text
Quick Action Bar click
        │  CodingAction only
        ▼
 Application::begin_coding_action / begin_explain_coding_action
        │  WI bind (selection / file / run hint) → UserRequest
        ▼
 Planner Decision Engine
        │
   ┌────┴────────────────────────┐
   ▼                             ▼
 Conversational Reasoning     SearchKnowledge / RunTerminal
 (Explain / Edit / Refactor / (reviewed Execution Plan for Run)
  OpenCodingActions menu)
```

| Concern | Owner |
|---------|--------|
| Button click → typed action | UI (`QuickAction` → `QuickActionEffect`) |
| Selection vs file for Explain | Application (CodingState / WI) |
| Intent routing | Planner Decision Engine |
| Context | Workspace Intelligence → ContextEngine |
| Mutations | Execution Plan + Review Card |

## Intents

| CodingAction | Behavior |
|--------------|----------|
| `ExplainSelection` | Coding Understanding of the current selection (C1.1) |
| `ExplainFile` | Coding Understanding of the active file (C1.1) |
| `EditSelection` | Conversational turn that asks what change is desired |
| `RefactorSelection` | Conversational refactoring **proposal** (no edits yet) |
| `SearchWorkspace` | Semantic search when a query/selection exists; otherwise an honest ask |
| `RunProject` | Reviewed terminal run (`cargo test` / `npm test` / …) when known; otherwise an honest ask |
| `OpenCodingActions` | Deterministic Planner menu (More) |

`CodingAction` is request metadata on `UserRequest`, not a parallel `IntentId`
taxonomy. Decision Engine maps actions onto `Unknown` (conversational),
`SearchKnowledge`, or `RunTerminal`.

## Conversation First

Every toolbar action appears as a normal user turn in the conversation
(`record_user_message` + generation pipeline). There is no separate workflow
chrome and no direct editor / terminal / provider call from the bar.

## Honesty

Unsupported or incomplete states never fail silently:

- Search with no query → ask for a query
- Run with no project/command → ask for a command
- More → always lists available Coding Actions

## Related

- [coding-understanding.md](coding-understanding.md) — structured Explain (C1.1)
- [experience.md](experience.md) — Quick Action Bar UX
- [planner.md](planner.md) — Intent → Execution Plan
- [workspace-intelligence.md](workspace-intelligence.md) — selection / file context
- [environmental-resolution.md](environmental-resolution.md) — deixis binding
