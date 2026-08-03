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

**Current:** chat-forward side expansion (capped width beside conversation) with a VS Code-inspired shell bound to temporary `CodingState`:

- **Editor** fills the code space (Monaco via wry WebView overlay when ready; egui buffer always available; Save + ⌘S)
- **Explorer** interactive tree on the **right** of the editor (Planner → `list_project_tree`)
- **Bottom tabs** toggle Terminal / Git / Problems without leaving the editor
- Language Server (Rust Analyzer via Planner → `language_server`)
- Terminal (PTY via Planner → `terminal`)
- Git (Planner → `git`)
- Diagnostics / Problems (read-only operational + LSP problems)

**Activation (UI):** conversation header **⋯** menu → **Open Project…** (folder picker; creates or reuses a project for that root, then opens Coding) or **Recent Projects**, or **Start Coding Project** (opens the Coding shell for the already-active project). That reuses the existing Coding shell and `CodingState` without creating a second conversation. Closing the workspace returns to the same chat. The Project Explorer empty state also offers **Open Project…**.

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
