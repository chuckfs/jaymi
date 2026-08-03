Roadmap

Jaymi is being built as a collection of layers.

Each layer provides new capabilities while remaining stable enough to support everything built on top of it.

The objective is not to ship features as quickly as possible.

The objective is to build a personal AI environment that can grow for years without requiring major architectural rewrites.

How to read this document:

* **Current** — implemented, wired at boot, covered by tests
* **Partial** — foundation present; incomplete relative to the layer objective
* **Target** — planned; keep the objective, do not treat as shipped

⸻

Progress

| Layer | Name | Status |
|-------|------|--------|
| 0 | Foundation | **Current** |
| 1 | Knowledge Engine | **Current** |
| 2 | Understanding Engine | **Current** (OCR stub; lexical embeddings) |
| 3 | Search Engine | **Current** |
| 4 | Memory Engine | **Current** (core; graph/export/aging Target) |
| 5 | Project Engine | **Current** (core; Git/artifacts Target) |
| 6 | Workspace & Capability Engine | **Current** (engine + Coding shell + thin UX; rich IDE/canvas Target) |
| 7 | Tool Engine | **Partial** (framework + 6 tools; most catalog Target) |
| 8 | Provider Ecosystem | **Partial** (Registry + 3 local providers; plugins Target) |
| 9 | Daily Driver | **Target** |

Architectural Integrity (orthogonal to layers): Context sole assemble path, Project ownership, Planner responsibilities, Planner integrity (requests via `handle`), Provider simplification (no ProviderManager), Capability availability (Ready / Experimental / Planned / Unavailable), Pipeline consistency (project knowledge via Cap → Policy → Permission → Tool), Session ownership (one project open/close lifecycle), Documentation & Diagnostics (Operational / Experimental / Stub / Disabled) — **Current**.

⸻

Layer 0 — Foundation

Status: **Current**

Objective

Build the foundation that every other component depends on.

Includes

* Desktop application (egui shell + diagnostics)
* Core architecture
* Planner
* Provider framework (Registry + bound instances)
* Tool framework
* Permission system
* Policy system
* Configuration
* Logging
* SQLite database

Exit Criteria

Jaymi launches and can execute a simple tool through the planner.

Shipped when: boot sequence completes; list/read/search pipelines pass through Planner → Policy → Permission → Tool.

⸻

Layer 1 — Knowledge Engine

Status: **Current**

Objective

Teach Jaymi what exists on the computer.

Responsibilities

* File discovery
* Metadata indexing
* Background indexing / watcher
* Incremental updates
* Local database inventory

Supported Sources (Current)

* Files
* Folders
* Downloads
* Documents

Exit Criteria

Jaymi can answer:

“What exists?”

without opening Finder.

⸻

Layer 2 — Understanding Engine

Status: **Current** (with stubs)

Objective

Teach Jaymi to understand content rather than filenames.

Responsibilities

* PDF / DOCX / Markdown / text / JSON parsing — **Current**
* Image content pipeline — **Current** (OCR engine **Stub**)
* Metadata extraction — **Current**
* Embeddings — **Current** (local lexical hashed embeddings, not a neural model)
* Summaries / deep image understanding — **Target**

Exit Criteria

Jaymi can answer:

“What is this?”

instead of only

“What is this file called?”

⸻

Layer 3 — Search Engine

Status: **Current**

Objective

Build semantic retrieval across every knowledge source.

Features

* Full-text search — **Current**
* Metadata search — **Current**
* Semantic / hybrid ranking — **Current** (over local embeddings)
* Citations — **Current**
* Preview — **Partial**

Exit Criteria

Search is based on meaning rather than filenames.

⸻

Layer 4 — Memory Engine

Status: **Current** (core)

Objective

Allow Jaymi to remember over time.

Memory Types

Conversation Memory — temporary for one conversation — **Current**

Project Memory — long-term memory attached by `project_id` — **Current**

Personal Memory — persistent preferences and important facts — **Current**

Target (not yet productized)

* Full relationship graph
* Merge / split memories
* Automatic aging
* Export of all memories

Exit Criteria

Jaymi remembers intentionally instead of accidentally.

⸻

Layer 5 — Project Engine

Status: **Current** (core)

Objective

Teach Jaymi that work happens inside projects.

A project is not just a folder.

A project includes (Current):

* Files under a root (via Knowledge / Search)
* Conversations
* Documentation / architecture entries in context
* Decisions
* Tasks (as project memory kinds)
* History via memory and conversations

Target:

* Git integration
* Artifact pipelines
* Repository import / conversation convert flows
* Live working-tree / IDE file state

Exit Criteria

Users can simply say:

Continue working on Jaymi.

⸻

Layer 6 — Workspace & Capability Engine

Status: **Current** (engine + thin UX)

Objective

Give Jaymi useful abilities that reshape the experience — not just backend concepts.

Capabilities include (catalog):

* Chat, Coding, Image generation, Search, Vision, OCR, Embeddings, Internet, Automation, File management, Terminal, and related aliases

**Boot registration:** the **full capability catalog** is registered. Availability (Ready / Experimental / Planned / Unavailable) distinguishes conceptual support from what is currently executable. Planned capabilities stay registered.

**Currently executable (inventory-backed):** Search, ReadDocuments, Discover, Index (Ready); Embeddings (Experimental — local lexical). Code / Vision remain Experimental catalog but Unavailable without coding/vision tools. OCR is **Planned** (placeholder provider only; not executable).

Capabilities are abstract.

They describe what Jaymi can do.

Not how it does it.

Capabilities also change the user experience.

Conversation stays permanent. Workspaces expand beside it.

**Current:** conversation shell + Coding Workspace shell (five panels + `CodingState`) + workspace expansion model + capability state / inspector.

**Target UX:**

Conversation → chat-only interface

Coding → conversation stays; full IDE (live editor / terminal / git) on top of the shell

Creation → conversation stays; canvas appears

Research → conversation stays; sources and notes appear

⸻

Layer 7 — Tool Engine

Status: **Partial**

Objective

Implement capabilities using interchangeable tools.

Current tools

* `search_files`
* `search_knowledge`
* `search_project_knowledge`
* `read_file`
* `query_inventory`
* `scan_filesystem`

Target tools (examples)

Coding → Local editor, Language server, Terminal, Git

Images → Local image model, Vision model

Search → Messages, Photos, and other sources

Tools are replaceable.

Capabilities remain constant.

⸻

Layer 8 — Provider Ecosystem

Status: **Partial**

Objective

Allow Jaymi to connect to external systems.

Current providers

* Filesystem
* Local embedding
* OCR placeholder (architecture only)

Target examples

* Local models / AI providers
* Git, Messages, Calendar, Email, Notes, Browser, Terminal
* Installable / enableable provider plugins

Every provider follows the same interface.

There is no ProviderManager — discovery uses ProviderRegistry; execution uses tool-bound instances.

⸻

Layer 9 — Daily Driver

Status: **Target**

Objective

Make Jaymi the first application opened every day.

Users should be able to:

* Chat
* Code
* Search
* Create
* Organize
* Automate

without needing to think about which application to open.

The computer becomes conversational.

⸻

Engineering Principles

Every contribution should improve one or more of the following:

* Simplicity
* Privacy
* Extensibility
* Transparency
* Local-first execution
* Project awareness
* Context awareness
* User ownership

⸻

Non-Goals

Jaymi is not trying to:

* Build a proprietary language model
* Replace every application overnight
* Depend on a single AI provider
* Require cloud services
* Lock users into one ecosystem

⸻

Success

Jaymi succeeds when interacting with a computer feels like talking to someone who understands your work.

The user should no longer think:

“Which application should I open?”

Instead, they simply ask:

“Can you help me do this?”

Jaymi figures out the rest.
