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

- Project Explorer (stub tree / selection)
- Editor (open-file list)
- Terminal (session placeholders)
- Git (status placeholder)
- Diagnostics (workspace diagnostic list)

**Activation (UI):** conversation header **⋯** menu → **Start Coding Project**. That reuses the existing Coding shell and `CodingState` without creating a second conversation. Closing the workspace returns to the same chat.

The conversation remains visible and persistent. Real editor / LSP / PTY / Git tools are **Target** (Layer 7); Code capability stays Unavailable until those tools exist.

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
