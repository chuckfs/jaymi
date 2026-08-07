Architecture

Jaymi is built around a single idea: every interaction begins with understanding intent.

The user never interacts directly with AI models, providers, tools, or services.

The user talks to Jaymi.

Jaymi decides everything else.

This document defines the architectural principles that guide the project.

How to read this document:

* **Current Implementation** — shipped in crates, wired at boot, covered by tests
* **Partial / Stub** — present but incomplete
* **Target Architecture** — intentional future; kept for direction

Layer progress is tracked in `ROADMAP.md`.

⸻

The Core Philosophy

Jaymi is not an AI model.

Jaymi is not a chatbot.

Jaymi is not an IDE.

Jaymi is not an automation platform.

Jaymi is an intelligent environment that coordinates all of these systems through a single conversational interface.

The architecture is designed so that any individual component can be replaced without changing how users interact with Jaymi.

⸻

System Overview

### Current Implementation (kernel)

```text
                   User
                     │
                     ▼
         Conversation + Diagnostics UI
                     │
                     ▼
             Planner (Kernel)
                     │
          Intent → Capability
                     │
         Context Policy Engine
                     │
          Context Providers
                     │
              Context Engine
           (sole assemble path)
                     │
              ContextBundle
                     │
      Action Policy / Permission
                     │
             Tool Orchestrator
                     │
   ProviderRegistry + tool-bound providers
                     │
        Filesystem • Local embeddings • …
```

Every **user request** follows this architecture through `Planner::handle`.

There are no shortcuts for request handling.

Administrative Memory/Project CRUD may resolve owning engines directly (see Planner section).

**Planned (not Current):** a Behavior stage after `ContextBundle` and before Action Policies.

### Target Architecture (periphery)

The same kernel grows to coordinate Messages, Photos, Browser, Calendar, Notes,
cloud AI, and richer Creation / Research workspace UIs — without changing how
users talk to Jaymi.

Coding-side Git status observation, Terminal PTY sessions, and Monaco editing
are **Current** (Workspace Intelligence + Coding shell). Deeper Git mutations
(merge / rebase / cherry-pick), Notes / Messages / Browser **context feeds**,
and full IDE polish remain Target.

⸻

Planner

Status: **Current** (orchestration kernel)

The Planner is the heart of Jaymi.

Every user request passes through it.

The planner does not generate code.

It does not search files.

It does not execute terminal commands.

It does not own long-lived Memory or Project CRUD APIs.

Those belong to the Memory Engine and Project Engine. Application calls those engines directly for administrative work.

Instead, the Planner orchestrates the **canonical request pipeline** (**Current**):

```text
User Request
  → Planner
  → Intent Resolution
  → Capability Resolution
  → Context Policy Engine
  → Context Providers
  → Context Engine
  → ContextBundle
  → Behavior                          # Planned — not implemented
  → Action Policies
  → Permissions
  → Tool Orchestrator
  → Providers
  → Planner Response
```

Context Policies (which providers may contribute) are distinct from Action Policies (whether a tool/provider candidate may run).

It decides:

* What is the user trying to accomplish? (Intent)
* Which capabilities are needed?
* Which tools should be used? (via Capability → Tool Orchestrator on tool-backed paths)
* What should happen next?

Context Policy + providers decide which memories / project / search sections
are relevant during assemble. Action Policy + Permission decide whether a
tool candidate may run and whether user approval is required.

Every user-facing request enters `Planner::handle`. After Context assemble,
paths **branch** (tool-backed vs session vs PlanWork vs conversational) — see
[docs/planner.md](docs/planner.md#request-lifecycle). That is not an
Application→Engine bypass.

Project knowledge search is a Planner-mediated request (`UserRequest::search_project_knowledge` → `handle` → Intent → Capability → Context assemble → Action Policy → Permission → `search_project_knowledge` tool → Project Engine).

Project session open/close has one lifecycle: Application delegates → Planner orchestrates → Project Engine owns open state (Memory mirrors the id). There is no Application→Engine session bypass.

**Partial:** Reasoning Engine + conversation state machine; conversational / unknown requests share `prepare_context_session` with tool-backed paths, then Context assemble → PromptBuilder → provider; diagnostics inspect delivered prompt; Model Registry populates `ReasoningRequest.model`; Experience/UI mirror Planner `ConversationState` only; pumpable and blocking delivery share assemble/terminal mapping with intentional host differences; Settings Workspace persists reasoning preferences via Application (Sprint B1.1–B1.13.8 + Settings). See [docs/reasoning.md](docs/reasoning.md), [docs/settings.md](docs/settings.md), and [docs/planner.md](docs/planner.md).

⸻

Context Engine

Status: **Current**

The Context Engine **assembles** only the context required for the current request.

The Planner calls `ContextEngine::assemble_with` **after** Intent and Capability
resolution (and conversational Complexity Assessment), passing `AssembleHints`
(`IntentId` + capability ids + optional complexity class). Context Policy and
providers derive Intent relevance from that Intent only — they do not
re-classify free-text intent. Complexity is Planner-authored and biases
provider scores only ([docs/complexity.md](docs/complexity.md)).

Ownership:

* **Providers** propose [`ContextCandidate`](docs/context-candidates.md) nodes from their own feeds (Memory / Project / Search / session-backed editor data) — they never assemble bundles
* **Context Policies** decide provider participation and score candidates (relevance / recency / importance / privacy)
* **Context Engine** selects under budget, materializes sections, and is the sole factory for `ContextBundle` — it does not determine Intent, select Capabilities, invent session state, or execute tools
* Host (`Application`) pushes `ContextSessionInputs` before assemble, including canonical [`WorkspaceSnapshot`](docs/workspace-snapshot.md) (B2.1 / B2.2), [`EditorSnapshot`](docs/editor-snapshot.md) (B2.3), [`ProjectSnapshot`](docs/project-snapshot.md) (B2.4), [`GitSnapshot`](docs/git-snapshot.md) (B2.5), and [`RuntimeSnapshot`](docs/runtime-snapshot.md) (B2.6)
* **Tools** perform search / filesystem work after Action Policy + Permission

It returns a unified `ContextBundle`. The Planner does not assemble these pieces itself.

**ContextEngine is the sole factory for `ContextBundle`.** Production / Planner
code obtains bundles only via `assemble_with` / `assemble`, `empty_bundle` (when
review flows intentionally skip reassemble), or `reuse_bundle` (attach a prior
engine-minted snapshot). The Planner never constructs `ContextBundle` directly.

**WorkspaceSnapshot (Sprint B2.1 / B2.2 / B2.13.2)** is the single immutable
observation of the live Coding environment (project, root, kind, files,
selection, cursor, branch, language, package manager, build system, timestamp).
It is observational only — no tools, no reasoning, no policy, and it never
creates a `ContextBundle`. The Application refreshes it **entirely via ambient
`ContextMaintenance`** (including `observe_toolchain` marker probes); prepare
merges the latest **completed** snapshot and never rebuilds or probes the
filesystem on the conversational path. Context Engine consumes it via session
inputs (`ContextEngine::workspace_snapshot`). See
[docs/workspace-snapshot.md](docs/workspace-snapshot.md).

**EditorSnapshot (Sprint B2.3 / B2.13.3)** is the read-only editor intelligence
observation (active file, open editors, cursor, selection range + text, symbol,
enclosing function/type, semantic tokens, references, diagnostics, code lens,
hover). Context providers consume `ContextSessionInputs.editor_snapshot`; Planner
and Reasoning never call LSP to obtain it. Monaco selection IPC updates
CodingState only; ambient maintenance publishes snapshots. Ambient Application
maintenance may enrich hover/references via read-only `LspProvider`. See
[docs/editor-snapshot.md](docs/editor-snapshot.md).

**ProjectSnapshot (Sprint B2.4)** is the read-only project intelligence observation
(metadata, languages, frameworks, package manager, dependency summary, cargo/npm
metadata, repository metadata, workspace layout). Ambient Application maintenance
owns marker / shallow FS observation; `ProjectProvider` consumes the completed
session snapshot. Planner never scans projects; providers never filesystem-scan
during requests. See [docs/project-snapshot.md](docs/project-snapshot.md).

**GitSnapshot (Sprint B2.5)** is the read-only Git intelligence observation
(branch, HEAD, dirty / staged / untracked / conflict paths, recent commits).
Ambient Application maintenance owns read-only `GitProvider` refresh;
`GitStatusProvider` exposes capped summaries. Reasoning never runs git commands.
See [docs/git-snapshot.md](docs/git-snapshot.md).

**RuntimeSnapshot (Sprint B2.6)** is the read-only runtime intelligence
observation (latest cargo check / build / tests, terminal output summary,
running processes, recent failures). Ambient Application maintenance observes
Coding terminal sessions; TerminalProvider owns PTY updates; `RuntimeProvider`
exposes capped summaries. Conversation never blocks waiting for runtime; observe
never re-runs cargo. See [docs/runtime-snapshot.md](docs/runtime-snapshot.md).

**Context Candidate Graph (Sprint B2.7 / B2.13.1)** is the assemble unit between
Workspace Intelligence feeds and the `ContextBundle`. Every Context Provider
exposes candidates through `propose_candidates()`; Context Policy evaluates
relevance, recency, importance, privacy, and budget uniformly; only selected
candidates are materialized into bundle sections. Providers never assemble
bundles; Planner ownership is unchanged. See
[docs/context-candidates.md](docs/context-candidates.md).

**Context Selection (Sprint B2.8)** teaches Context Policy to choose workspace
feeds with deterministic profiles (no AI). Example mappings: greeting →
Conversation + Memory; compile/debug → Conversation + Diagnostics + Current
file + Terminal + Selection; project summary → Project + Filesystem +
Architecture + Git. Heuristics are fully documented in
[docs/context-selection.md](docs/context-selection.md).

**Workspace Memory (Sprint B2.9)** remembers Coding workspace activity (recent
edits, recently opened files, recent builds, recent failures, coding objective).
It is distinct from Conversation Memory; Context Policy decides inclusion. See
[docs/workspace-memory.md](docs/workspace-memory.md).

**Environmental Resolution (Sprint B2.10)** lets the Planner bind ambiguous
workspace deixis (`this` / `it` / `why?` / …) from Workspace Intelligence
before Reasoning. LLMs never invent workspace references. See
[docs/environmental-resolution.md](docs/environmental-resolution.md).

**Workspace Diagnostics (Sprint B2.11)** expose Workspace Intelligence in
Developer Diagnostics only: snapshot freshness, provider timings, maintenance
status, candidate selection, policy decisions, and context budget. Never
written to the conversation transcript. See
[docs/workspace-diagnostics.md](docs/workspace-diagnostics.md).

**Constitutional Audit (Sprint B2.12)** verified Workspace Intelligence against
VISION / PRINCIPLES / NON_GOALS / ARCHITECTURE / ROADMAP / docs/. Ownership
holds. Residuals called out at audit time were closed in follow-on sprints:
**B2.13.1** (candidate migration), **B2.13.2** (ambient-only WorkspaceSnapshot
prepare), **B2.13.3** (Monaco selection → snapshots → Environmental Resolution).
**B2.13.4** synchronized documentation with the shipped Workspace Intelligence
surface (docs only).

Recently assembled bundles are cached by project, workspace, conversation, active file, and request type (plus a request fingerprint). The cache is invalidated when files, project, workspace, conversation, or the search index change — performance only; correctness is unchanged. See `docs/context.md`.

Slow host-side context refreshes (git status / GitSnapshot, workspace inventory, diagnostics, file summaries, WorkspaceSnapshot, EditorSnapshot, ProjectSnapshot, RuntimeSnapshot) run as Application background maintenance and never block conversation; assemble still goes only through ContextEngine. See `docs/context-maintenance.md`.

Context History retains recent bundles with timestamp, request, providers used, bundle size, and execution duration for debugging and future reasoning transparency.

The LLM-facing Context API (`ContextEngine::to_llm_context` → `LlmContext`) converts a bundle into a stable, deterministically serializable structure for future model consumers — no model calls, no prompts. See `docs/context.md`.

Context Policies (`ContextPolicyEngine`) decide which providers may participate before assemble — relevant, minimal, privacy-aware, deterministic, and explainable. Independent of the **Action Policy Engine** (`jaymi-policies`, lifecycle name `policy_engine`) and of any LLM.

### Context Providers

Assemble is provider-driven. Subsystems implement `ContextProvider` with
deterministic `relevance`, `priority`, `estimate_size`, and
`propose_candidates`. The engine skips low-relevance providers, runs Context
Policy over candidates, packs under budget, and materializes selected nodes.
No AI scoring. Providers never assemble bundles. The engine orchestrates
providers without depending on their internals.

Initial providers: Conversation, Project, Workspace, Editor, Search, Memory,
Diagnostics, Git, Runtime, Workspace Memory, Workspace Inventory, File
Summaries, Permission.

### Current ContextBundle (immutable snapshot)

First-class sections: Conversation, Active Project, Active Workspace, Current File, Current Selection, Open Files, Search Results, Memory Results, Diagnostics, Permissions, Planner Metadata, Active Capabilities, User Request Metadata — plus Git status, runtime intelligence, workspace memory, inventory, and file summaries when selected.

The bundle never searches or reasons — Planner execution, Behaviors (**Planned**), and future LLM providers consume it as a frozen snapshot (`PlannerResponse.context()` / `context_bundle`). Parallel `memory_context` / `project_context` / `search_context` response fields are removed; use bundle accessors.

### Target context sources (not yet assembled)

* Notes / Messages / Browser history as first-class context feeds

### Current Workspace Intelligence context sources (assembled)

| Source | Status | Contract |
|--------|--------|----------|
| Conversation / Memory / Project / Search / Editor / Diagnostics / Permissions | **Current** | Context Providers |
| WorkspaceSnapshot | **Current** (B2.1 / B2.2 / B2.13.2) | Ambient observation |
| EditorSnapshot (incl. Monaco selection) | **Current** (B2.3 / B2.13.3) | Ambient + CodingState |
| ProjectSnapshot | **Current** (B2.4) | Ambient observation |
| GitSnapshot / git status summaries | **Current** (B2.5) | Ambient + `GitStatusProvider` |
| RuntimeSnapshot / terminal summaries | **Current** (B2.6) | Ambient + `RuntimeProvider` |
| Context Candidate Graph | **Current** (B2.7 / B2.13.1) | `propose_candidates` → Policy → materialize |
| Context Selection profiles | **Current** (B2.8) | Deterministic feed choice |
| Workspace Memory | **Current** (B2.9) | Coding activity rings |
| Environmental Resolution | **Current** (B2.10) | Planner deixis binding |
| Workspace Diagnostics | **Current** (B2.11) | Developer Diagnostics only |

Context is assembled dynamically.

Context is never assumed.

⸻

Memory Engine

Status: **Current** (core)

Memory allows Jaymi to improve over time while remaining transparent and user-controlled.

Jaymi maintains three independent memory systems.

Conversation Memory — temporary; exists within the current conversation; discarded unless promoted.

Project Memory — attached by `project_id`. The Project Engine owns project identity. Memory, Search, and Knowledge reference projects only by that id.

Personal Memory — long-term preferences and important facts; always editable.

Request-time retrieval for Planner responses goes through the Context Engine → Memory Engine.

### Target

* Full relationship graph
* Merge / split / export
* Automatic aging policies

⸻

Workspace & Capability Engine

Status: **Current** (engine + Coding Workspace shell + thin UX)

Capabilities define what Jaymi knows how to do.

### Current

Boot registers the **full capability catalog**. Availability (Ready / Experimental / Planned / Unavailable) distinguishes conceptual support from what is currently executable. Planned capabilities stay registered.

**Current:** conversation shell + Coding Workspace shell (five dock pages — Terminal / Problems / Search / Git / Diagnostics — plus Output placeholder, expansion chrome, and capability state / inspector) + canonical [`WorkspaceSnapshot`](docs/workspace-snapshot.md) (B2.1 / B2.2 / B2.13.2) + [`EditorSnapshot`](docs/editor-snapshot.md) (B2.3 / B2.13.3) + [`ProjectSnapshot`](docs/project-snapshot.md) (B2.4) + [`GitSnapshot`](docs/git-snapshot.md) (B2.5 — **Current**) + [`RuntimeSnapshot`](docs/runtime-snapshot.md) (B2.6 — **Current**) + [`Context Candidate Graph`](docs/context-candidates.md) (B2.7 / B2.13.1 — **Current**) + [`Context Selection`](docs/context-selection.md) (B2.8) + [`Workspace Memory`](docs/workspace-memory.md) (B2.9) + [`Environmental Resolution`](docs/environmental-resolution.md) (B2.10) + [`Workspace Diagnostics`](docs/workspace-diagnostics.md) (B2.11) + Constitutional Audit (B2.12) + docs sync (B2.13.4).

See [docs/capabilities.md](docs/capabilities.md).

### Target capability catalog & UX

* Promote Planned capabilities as tools/providers land
* Coding → fuller IDE polish on top of the existing shell (Monaco / Terminal /
  Git panel / Workspace Intelligence are **Current**; merge/rebase and broader
  LSP tooling remain Target)
* Creation → canvas appears
* Research → sources and notes appear

Capabilities describe behavior.

They do not describe implementation.

⸻

Tool Orchestrator

Status: **Partial**

Tools are concrete implementations of capabilities.

### Current tools (12)

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

The planner selects tools automatically for supported intents. Mutating tools pause for Review before execution (`Application::submit_review`).

### Target tools

Messages, Photos, dedicated Editor tool, Image Model, Vision Model, Git merge/rebase/cherry-pick, and many more — interchangeable under stable capabilities.

⸻

Provider System

Status: **Partial**

Providers connect Jaymi to resources.

### Current

* ProviderRegistry — discovery and diagnostics (identity metadata)
* Concrete provider instances bound into tools at boot: Filesystem, Terminal (PTY), Git, Language Server (Rust Analyzer), Local Embedding, OCR Placeholder
* Capability Engine soft-matching for plans (`providers_for`)

There is no ProviderManager. Execution goes through tools.

### Target

Messages, Mail, Calendar, Browser, Photos, Notes, local/cloud AI models, Git merge/rebase/cherry-pick, installable provider plugins.

Providers expose consistent interfaces.

The rest of the system never depends on provider-specific behavior.

Providers can be added, removed, or replaced without affecting the planner.

⸻

Permission Engine

Status: **Current** (rule engine) · Review Before Action **Current** · durable grants / revoke **Target**

Jaymi should never surprise the user.

### Current

Permission categories and scope enums; default decisions consulted before tool execution:

| Category · Action | Default |
|-------------------|---------|
| Filesystem · Read | Allowed |
| Filesystem · Write | RequiresApproval |
| Filesystem · Delete | RequiresApproval |
| Terminal · Execute | RequiresApproval |
| Internet / Communication / System / AI Providers | Denied |

Planner combines Permission + Action Policy + ToolRisk (Denied > RequiresApproval > Allowed). Review Before Action uses one lifecycle — ExecutionPlan → Review → `ReviewIntent` → Planner → Approved → Execution (`Application::submit_review`). Conversation Review Cards and Coding gestures (Save / Delete / Run / Git / LSP rename) share that path; tools never execute outside an Approved plan. Action previews and Trash-default deletes are Current.

### Target

Durable permission grants by scope, permission history, revocation UI. Scope values (`Once` / `Conversation` / `Project` / `Global`) exist on requests today but are not yet enforced by a grant store.

The user remains in control.

⸻

Action Policy Engine

Status: **Partial** (`jaymi-policies`, lifecycle name `policy_engine`)

Action Policies constrain **tool/provider candidates** after Capability resolution and Context assemble — before Permission checks. Distinct from Context Policies (`jaymi-context`).

### Current enforced (boot-active)

* Offline First

### Declared enforcement (constraint logic exists; not boot-active)

* Privacy Maximum

### Declared / Target enforcement

Highest Quality, Fastest, Battery Saver, Developer / Creative / Research modes, rich multi-scope resolution, user-custom policies.

⸻

Diagnostics

Status: **Current**

The developer dashboard reports subsystem readiness with four honest labels:

| Status | Meaning |
| --- | --- |
| **Operational** | Ready for its declared role |
| **Experimental** | Present and usable, with known limitations |
| **Stub** | Lifecycle-wired placeholder / architecture only |
| **Disabled** | Intentionally off, not wired, or failing |

Examples: OCR Provider is **Stub**; Reasoning is **Partial** (Ollama + conversational Planner path — see [docs/ollama.md](docs/ollama.md) / [docs/reasoning.md](docs/reasoning.md)); Policies and local lexical Embeddings are **Experimental**; Index / Watcher report **Disabled** when indexing is turned off. Subsystems must not overstate readiness.

⸻

Knowledge Pipeline

Status: **Current** through Store/Retrieve; Reason is stub

Every piece of information follows the same lifecycle.

Discover → Read → Understand → Index → Store → Retrieve → Reason → Respond

**Current:** Discover through Retrieve are implemented locally.

**Stub / Target:** Reason (language-model backends).

Jaymi reasons over understanding—not raw files — once Reasoning is wired.

⸻

Projects

Status: **Current** (core)

Projects are first-class citizens.

A project is more than a folder.

A project is a living workspace.

### Current

* Source code / documents under a root (via Knowledge)
* Conversations, architecture/decision entries, tasks (memory kinds), project memory
* Assembled `ProjectContext`
* “Continue working on …” restores workspace context

### Target

* Deep Git history / project-owned Git Integration productization (ambient
  working-tree **GitSnapshot** status is **Current** via Workspace Intelligence —
  see [docs/git-snapshot.md](docs/git-snapshot.md))
* Artifact pipelines
* Full IDE working-file productization beyond the Coding shell

When a project is opened, Jaymi restores its engine-backed context automatically.

The user continues working rather than starting over.

⸻

Offline-First

Status: **Current** foundation

Jaymi performs as much work locally as possible.

### Current

* Search, Memory, Document parsing, Indexing, Context construction, Project awareness
* Local lexical embeddings
* Offline-first / privacy policies on tool candidates

### Partial / Target

* OCR (architecture stub)
* Local neural models for reasoning / generation
* Optional cloud AI providers

Internet access is treated as another capability.

The system functions without cloud services.

⸻

AI Models

Status: **Partial** — Ollama backend + Reasoning Engine + conversation state machine + Model Registry + Conversational Reasoning diagnostics + Prompt → Provider handoff + Context section coverage + multi-turn history + shared conversational context prep + delivered-prompt diagnostics + registry→request model loop + Planner-owned runtime mirrored by UI + dual delivery clarity + Settings Workspace Reasoning preferences (`ConversationState`, Sprint B1.1–B1.13.8 + Settings)

Models are interchangeable.

Jaymi never depends on a specific model.

Instead, the planner requests capabilities.

Providers determine how those capabilities are fulfilled.

As models improve, Jaymi improves without architectural changes.

⸻

Extensibility

Status: **Current** modular kernel; **Target** plugin ecosystem

Every major subsystem is replaceable.

### Current

New tools and providers can be registered in-process at boot; Capability Engine discovers them for plans.

### Target

Developers extend Jaymi by adding installable:

* Providers
* Tools
* Capabilities
* Importers
* Memory strategies
* Search engines
* AI models

The planner should not require modification to support new functionality.

⸻

Guiding Principles

Every architectural decision should reinforce these principles.

* The planner is the center of the system.
* The user owns their data.
* Local execution comes first.
* Every action should be explainable.
* Every important action should be reviewable.
* Components should be modular.
* Providers should be interchangeable.
* Intelligence comes from context, not model size.
* The conversation is the primary interface.

⸻

The Long-Term Vision

Jaymi is not built around a language model.

It is built around a planner capable of understanding people, coordinating tools, managing context, and orchestrating work.

Models will improve.

Tools will evolve.

Providers will change.

The planner remains.

Everything else is replaceable.

The architecture should ensure that, no matter how the technology changes, interacting with Jaymi always feels the same:

You simply talk to your computer.

It understands what you mean.

Then it figures out the rest.
