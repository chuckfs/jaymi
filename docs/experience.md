# Experience

**Status: Current Implementation** (conversation shell + Coding Workspace shell + expansion model) · **Target:** full IDE / canvas / research surfaces

## Conversation First

Jaymi is conversation-first.

The conversation is the primary interface.

Users should never feel like they are switching applications.

Instead, the conversation expands into specialized workspaces as tasks become more complex.

---

## Workspaces

Jaymi defines primary workspace kinds. **Current:** conversation shell plus expansion chrome and capability/workspace state. **Target:** rich panels described below.

### Conversation

**Current:** default experience — general conversation, planning, memory, diagnostics.

### Coding Workspace

**Current:** shell expands from the right with five panels bound to temporary `CodingState`:

- Project Explorer (live project tree via Planner → `list_project_tree` → Filesystem Provider)
- Editor (Monaco via wry WebView overlay; syntax highlighting, line numbers, optional minimap, search, multi-cursor, undo/redo; buffers in `CodingState` via Planner → `read_file` / `write_file`; Save + ⌘S)
- Language Server (Rust Analyzer via Planner → `language_server` → LSP Provider: diagnostics, hover, autocomplete, go to definition, rename, find references)
- Terminal (persistent PTY via Planner → `terminal` → Terminal Provider; scrolling + history)
- Git (live status / stage / unstage / discard / commit via Planner → `git` → Git Provider)
- Diagnostics (read-only operational panel: active project, workspace state, planner activity, tool execution, provider status, indexing, memory context, permissions, current capability, timing metrics; plus LSP problems)

**Activation (UI):** conversation header **⋯** menu → **Start Coding Project**. That reuses the existing Coding shell and `CodingState` without creating a second conversation. Closing the workspace returns to the same chat.

The conversation remains visible and persistent. Monaco and the Language Server (Rust Analyzer) are embedded in Coding Workspace (buffers and diagnostics survive UI remounts via `CodingState`). Broader LSP tooling remains **Target** for Layer 7 polish. Terminal PTY and Git panel are available in Coding Workspace.

The conversation becomes project-aware but never resets.

### Creation Workspace

**Target:** conversation stays; canvas / image / asset tools appear.

### Research Workspace

**Target:** conversation stays; sources and notes appear.

---

## Closing a Workspace

**Current:** closing an expanded workspace keeps the conversation and session state consistent with capability/workspace rules.

The conversation is permanent.

Workspaces expand and collapse around it.

---

## Relationship to Capabilities

**Current:** Capability Engine plans and inspects capabilities with **availability** (Ready / Experimental / Planned / Unavailable). Workspace kinds map to capability expansions; Inspector shows availability and active workspace. See [capabilities.md](capabilities.md).

**Target:** selecting Coding / Creation / Research fully materializes the specialized surfaces described above.
