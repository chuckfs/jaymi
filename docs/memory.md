Memory

**Status: Current Implementation** (core) · **Target:** graph, merge/split, aging, export

Memory allows Jaymi to learn over time while remaining transparent, editable, and entirely under the user’s control.

Unlike conversation history, memory is intentional.

Jaymi does not automatically remember everything.

Instead, memory is promoted, organized, retrieved, and managed according to well-defined rules.

Memory exists to improve future interactions—not to permanently store every interaction.

⸻

Philosophy

Memory should behave more like a notebook than a surveillance system.

Jaymi remembers what is useful.

The user decides what is permanent.

Every memory should have a purpose.

⸻

Memory Architecture

**Current request path** for Planner responses (Memory contributes via Context Provider):

```text
User → Conversation → Planner → Intent → Capability
  → Context Policy → MemoryProvider (among others) → ContextBundle
  → Action Policy → Permission → Tools → Response
```

Administrative store / promote / personal CRUD may call the Memory Engine directly (not a request bypass).

```text
User
   │
   ▼
Conversation
   │
   ▼
Planner (Intent → Capability)
   │
   ▼
Context Engine  (assemble_with → MemoryProvider)
   │
   ▼
Memory Engine
   │
   ├──────────────┐
   │              │
Retrieve      Promote
   │              │
   ▼              ▼
Memory Store  Memory Store
```

The Planner triggers Context assemble for every request. Context Policy and
MemoryProvider decide whether memory sections are included; the Memory Engine
owns how memories are stored and retrieved.

⸻

Memory Types

Jaymi has three independent memory systems.

**Related (not a Memory Engine scope):** [Workspace Memory](workspace-memory.md)
(Sprint B2.9) remembers Coding workspace activity (edits / opens / builds /
failures / objective). It is owned by CodingState, selected by Context Policy,
and never writes Conversation / Project / Personal memories.

⸻

Conversation Memory

Conversation Memory exists only for the lifetime of a conversation.

Purpose:

* Maintain context
* Avoid repetition
* Support natural conversation

Conversation Memory is temporary.

When the conversation ends, it is discarded unless promoted.

Examples:

* Current discussion
* Temporary variables
* Recent reasoning
* Short-lived plans

⸻

Project Memory

Project Memory belongs to a project.

Every project maintains its own memory.

Memory never owns project identity. The Project Engine creates and looks up projects; Memory stores and retrieves records keyed only by `project_id`.

Project Memory may include:

* Architecture decisions
* Design discussions
* Coding conventions
* TODOs
* Technical debt
* Documentation
* Important conversations

Project Memory travels with the project.

Opening a project restores its memory automatically.

⸻

Personal Memory

Personal Memory represents long-term knowledge about the user.

Examples include:

* Preferences
* Writing style
* Frequently used workflows
* Favorite tools
* Persistent settings

Personal Memory should be intentionally created.

It should never grow without limit.

⸻

Memory Lifecycle

Every memory follows the same lifecycle.

Observe
↓
Evaluate
↓
Promote
↓
Store
↓
Index
↓
Retrieve
↓
Update
↓
Archive / Delete

Memory is never created accidentally.

Structured `kind` values used by Application today include
`execution_summary` (plan outcomes) and `approval_history` (Review Card
decisions). Approval History is searchable via Memory query + Planner
session store; sensitive fields stay Private and must be redacted for
Restricted / Context exports.

⸻

Memory Promotion

Most information should never become memory.

The Planner evaluates whether information should be promoted.

Possible triggers include:

* User explicitly asks
* Repeated behavior
* Important project decision
* Significant preference
* Long-term workflow

Promotion should remain conservative.

⸻

Memory Retrieval

When responding to a request, the Planner asks:

“What memories are relevant?”

The Memory Engine retrieves only relevant memories.

Retrieval may consider:

* Semantic similarity
* Current project
* Conversation context
* User intent
* Recency
* Importance

More memories do not necessarily produce better responses.

⸻

Memory Structure

Every memory should contain structured metadata.

Examples include:

Identity

* Memory ID
* Memory Type
* Creation Date
* Last Updated

Content

* Summary
* Detailed Content
* Embedding

Relationships

* Associated Project
* Associated Conversation
* Related Memories

Metadata

* Confidence
* Importance
* Tags
* Source

This structure enables efficient retrieval and explainability.

⸻

Memory Relationships

**Status: Target** (scaffolding exists; not a productized graph)

Memories should form a graph rather than isolated records.

Example:

Rust
↓
Jaymi Project
↓
Planner
↓
Provider System

Relationships improve retrieval and navigation.

⸻

Memory Editing

Users should always be able to:

* View
* Edit
* Merge
* Split
* Delete
* Archive

Jaymi should never create memories that cannot be modified.

⸻

Memory Aging

**Status: Target**

Not every memory remains useful forever.

Jaymi should support:

* Archiving
* Manual deletion
* Automatic expiration (optional)
* Importance scoring

The user always remains in control.

⸻

Explainability

Jaymi should always be able to answer:

* Why was this memory retrieved?
* Why was it created?
* Where did it come from?
* Why is it considered important?

Memory should never become a black box.

⸻

Privacy

All memories belong to the user.

By default:

* Stored locally
* Searchable locally
* Editable locally
* Exportable
* Deletable

Cloud synchronization, if implemented in the future, should remain optional.

⸻

Search

Memory should be searchable through natural language.

Examples:

What projects have I worked on involving OCR?

What decisions did I make about the Planner?

What do I usually prefer for image generation?

The user should never need to know where the memory is stored.

⸻

Memory Is Not History

History records everything.

Memory records what matters.

This distinction is fundamental to Jaymi.

Conversation history may contain thousands of messages.

Memory should remain concise, meaningful, and useful.

⸻

Design Principles

The Memory Engine should always:

* Prefer quality over quantity.
* Retrieve before reasoning.
* Store structured knowledge.
* Remain fully editable.
* Preserve user ownership.
* Explain retrieval decisions.
* Keep memory intentional.

⸻

Long-Term Vision

Memory is what allows Jaymi to become increasingly helpful over time without becoming intrusive.

The goal is not perfect recall.

The goal is meaningful understanding.

Jaymi should remember what matters, forget what does not, and always allow the user to remain in control.
