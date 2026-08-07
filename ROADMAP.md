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
| 5 | Project Engine | **Current** (core; deep Git history / artifacts Target — ambient GitSnapshot status is Current via Layer 6 WI) |
| 6 | Workspace & Capability Engine | **Current** (engine + Coding shell + thin UX; rich IDE/canvas Target) |
| 7 | Tool Engine | **Partial** (framework + 12 tools; broader catalog Target) |
| 8 | Provider Ecosystem | **Partial** (Registry + 6 local providers; plugins Target) |
| 9 | Daily Driver | **Target** |

Architectural Integrity (orthogonal to layers): Context sole assemble path, ContextBundle sole request-context contract on `PlannerResponse`, Project ownership, Planner responsibilities, Planner integrity (requests via `handle`), Provider simplification (no ProviderManager), Capability availability (Ready / Experimental / Planned / Unavailable), canonical request pipeline (Intent → Capability → Context Policy → assemble → Action Policy → Permission → Tool), Session ownership (one project open/close lifecycle), Documentation & Diagnostics (Operational / Experimental / Stub / Disabled), Settings Workspace preferences ownership (snapshots/intents only; Model Registry remains catalog SoT) — **Current**. Behavior stage after ContextBundle — **Planned**.

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

Shipped when: boot sequence completes; list/read/search pipelines pass through Planner → Intent → Capability → Context → Action Policy → Permission → Tool.

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

* Deep Git history / project-owned Git Integration productization (working-tree
  status observation is **Current** via [`GitSnapshot`](docs/git-snapshot.md) —
  Workspace Intelligence, not Project Engine)
* Artifact pipelines
* Repository import / conversation convert flows
* Richer live working-tree / IDE file productization beyond the Coding shell

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

**Current:** conversation shell + Coding Workspace shell (five dock pages — Terminal / Problems / Search / Git / Diagnostics — plus an Output placeholder not listed in `pages()`) + `CodingState` + workspace expansion model + capability state / inspector + **WorkspaceSnapshot** (B2.1 / B2.2 / B2.13.2) + **EditorSnapshot** (B2.3 / B2.13.3 editor intelligence) + **ProjectSnapshot** (B2.4 project intelligence) + **GitSnapshot** (B2.5 — **Current**) + **RuntimeSnapshot** (B2.6 — **Current**) + **Context Candidate Graph** (B2.7 / B2.13.1 — **Current**) + **Context Selection** (B2.8) + **Workspace Memory** (B2.9) + **Environmental Resolution** (B2.10) + **Workspace Diagnostics** (B2.11) + **Constitutional Audit** (B2.12) + **docs sync** (B2.13.4).

**Target UX:**

Conversation → chat-only interface

Coding → conversation stays; fuller IDE polish on top of the shipped shell
(Monaco / Terminal / Git panel / Workspace Intelligence are **Current**; deeper
LSP tooling and Git merge/rebase remain Target)

Creation → conversation stays; canvas appears

Research → conversation stays; sources and notes appear

**B2.1:** `WorkspaceSnapshot` is the immutable observation of the live Coding
workspace (project / root / kind / files / selection / cursor / branch /
language / package manager / build system / timestamp). Host observation is
ambient (`ContextMaintenance`); prepare merges the latest completed snapshot.
Context Engine consumes via session inputs. See
[docs/workspace-snapshot.md](docs/workspace-snapshot.md).

**B2.2:** Ambient `ContextMaintenance` refreshes `WorkspaceSnapshot` on editor /
selection / git / diagnostics / terminal / project changes without blocking
conversation. Prepare merges the latest **completed** snapshot only — never
rebuilds a ContextBundle, never reasons, never calls LLMs, never executes tools,
and (Sprint **B2.13.2**) never probes toolchain markers on the conversational
path. Planner remains the sole request owner. See
[docs/context-maintenance.md](docs/context-maintenance.md).

**B2.3:** `EditorSnapshot` is the read-only editor intelligence observation
(active file / open editors / cursor / selection / symbol / enclosing function /
enclosing type / semantic tokens / references / diagnostics / code lens / hover).
Context providers consume it; Planner and Reasoning never call LSP for assemble.
Ambient refresh may use read-only `LspProvider` enrichment. See
[docs/editor-snapshot.md](docs/editor-snapshot.md).

**B2.4:** `ProjectSnapshot` is the read-only project intelligence observation
(metadata / languages / frameworks / package manager / dependency summary /
cargo·npm metadata / repository metadata / workspace layout). Ambient
`ContextMaintenance` owns FS observation; `ProjectProvider` consumes the session
snapshot. Planner never scans projects; no filesystem scanning during requests.
See [docs/project-snapshot.md](docs/project-snapshot.md).

**B2.5:** `GitSnapshot` is the read-only Git intelligence observation (branch /
HEAD / dirty / staged / untracked / conflicts / recent commits). Ambient
`ContextMaintenance` owns read-only `GitProvider` refresh; `GitStatusProvider`
exposes capped summaries. Reasoning never runs git commands. See
[docs/git-snapshot.md](docs/git-snapshot.md).

**B2.6:** `RuntimeSnapshot` is the read-only runtime intelligence observation
(latest cargo check / build / tests, terminal output summary, running processes,
recent failures). Ambient `ContextMaintenance` observes Coding terminal sessions;
TerminalProvider owns PTY updates; `RuntimeProvider` exposes summaries.
Conversation never blocks waiting for runtime; observe never re-runs cargo.
See [docs/runtime-snapshot.md](docs/runtime-snapshot.md).

**B2.7:** Workspace Intelligence exposes `ContextCandidate` nodes instead of
raw bundle sections. Every workspace feed proposes candidates; Context Policy
evaluates relevance, recency, importance, privacy, and budget; only selected
candidates become a `ContextBundle`. Providers never assemble bundles; Planner
ownership unchanged. Migration completed in **B2.13.1** (every provider
implements `propose_candidates`). See
[docs/context-candidates.md](docs/context-candidates.md).

**B2.8:** Context Policy chooses workspace context with deterministic selection
profiles (no AI scoring). Examples: `hello` → Conversation + Memory; `why won't
this compile?` → Conversation + Diagnostics + Current file + Terminal +
Selection; `summarize this project` → Project + Filesystem + Architecture + Git.
All heuristics are documented. See
[docs/context-selection.md](docs/context-selection.md).

**B2.9:** Workspace Memory remembers Coding workspace activity (recent edits,
recently opened files, recent builds, recent failures, current coding
objective). Distinct from Conversation Memory. Context Policy decides when to
include it. See [docs/workspace-memory.md](docs/workspace-memory.md).

**B2.10:** Environmental Resolution — Planner resolves ambiguous workspace
deixis (`rename this`, `fix it`, `why?`, `clean this up`) from Workspace
Intelligence before Reasoning. LLMs never invent workspace references.
See [docs/environmental-resolution.md](docs/environmental-resolution.md).

**B2.11:** Workspace Diagnostics — Developer Diagnostics expose Workspace
Intelligence (snapshot freshness, provider timings, maintenance status,
candidate selection, policy decisions, context budget). Never written to the
conversation transcript. See
[docs/workspace-diagnostics.md](docs/workspace-diagnostics.md).

**B2.12:** Constitutional Audit — Workspace Intelligence (B2.1–B2.11) verified
against VISION / PRINCIPLES / NON_GOALS / ARCHITECTURE / ROADMAP / docs/.
Ownership holds (Planner orchestrates, Workspace observes, providers passive,
ContextEngine sole ContextBundle factory). Residuals noted at audit time were
closed by **B2.13.1**–**B2.13.3** (candidate migration, ambient-only prepare,
Monaco selection). Documentation synchronized in **B2.13.4**.

**B2.13.1:** Complete Context Candidate Migration — every Context Provider
exposes `propose_candidates()`; production assemble no longer relies on
contribution→candidate trait fallback; Context Policy evaluates candidates
uniformly; ContextEngine remains sole bundle factory. See
[docs/context-candidates.md](docs/context-candidates.md).

**B2.13.2:** Remove Synchronous Prepare Probes — conversational
`prepare_context_session` never rebuilds WorkspaceSnapshot or calls
`observe_toolchain`; ambient ContextMaintenance owns observation; prepare
merges the latest completed snapshot (or schedules refresh if missing). See
[docs/workspace-snapshot.md](docs/workspace-snapshot.md) and
[docs/context-maintenance.md](docs/context-maintenance.md).

**B2.13.3:** Monaco Selection Intelligence — Monaco text selection (range +
text) synchronizes into CodingState and the ambient Workspace/EditorSnapshot
pipeline; Environmental Resolution binds `"Explain this."` / `"Rename this."` /
`"Clean this up."` from Workspace Intelligence. Cursor tracking preserved; no
Monaco types in Planner/ContextEngine; no sync LSP. See
[docs/editor-snapshot.md](docs/editor-snapshot.md) and
[docs/environmental-resolution.md](docs/environmental-resolution.md).

**B2.13.4:** Documentation Synchronization — ARCHITECTURE / ROADMAP /
context / workspace-intelligence / providers / experience updated so shipped
Workspace Intelligence (including **Current** GitSnapshot, RuntimeSnapshot, and
Context Candidate Graph) is documented accurately; stale B2 Target residuals
removed. Docs only — no implementation changes. See
[docs/workspace-intelligence.md](docs/workspace-intelligence.md).

⸻

Layer 7 — Tool Engine

Status: **Partial**

Objective

Implement capabilities using interchangeable tools.

Current tools (12; registered at boot)

* `search_files`
* `list_project_tree`
* `search_knowledge`
* `search_project_knowledge`
* `read_file`
* `write_file`
* `manage_path`
* `terminal`
* `git`
* `language_server`
* `query_inventory`
* `scan_filesystem`

Target tools (examples)

Coding → dedicated editor tool (beyond LSP rename / workspace shell)

Images → Local image model, Vision model

Search → Messages, Photos, and other sources

Git → merge / rebase / cherry-pick (status / stage / unstage / discard / commit are Current)

Tools are replaceable.

Capabilities remain constant.

⸻

Layer 8 — Provider Ecosystem

Status: **Partial**

Objective

Allow Jaymi to connect to external systems.

Current providers (6; bound at boot)

* `filesystem`
* `terminal` (PTY)
* `git` (status / stage / unstage / discard / commit)
* `lsp` (Rust Analyzer)
* `embedding.local`
* `ocr.placeholder` (architecture only — OCR capability remains Planned)

Target examples

* Local models / AI providers
* Messages, Calendar, Email, Notes, Browser
* Git merge / rebase / cherry-pick
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
