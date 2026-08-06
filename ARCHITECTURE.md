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

The same kernel grows to coordinate Messages, Git, Terminal, Photos, Browser, Calendar, Notes, cloud AI, and richer workspace UIs — without changing how users talk to Jaymi.

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
paths **branch** (tool-backed vs session vs PlanWork vs unsupported) — see
[docs/planner.md](docs/planner.md#request-lifecycle). That is not an
Application→Engine bypass.

Project knowledge search is a Planner-mediated request (`UserRequest::search_project_knowledge` → `handle` → Intent → Capability → Context assemble → Action Policy → Permission → `search_project_knowledge` tool → Project Engine).

Project session open/close has one lifecycle: Application delegates → Planner orchestrates → Project Engine owns open state (Memory mirrors the id). There is no Application→Engine session bypass.

**Partial / Stub:** Reasoning Engine (language-model backends) is not implemented (`is_implemented() == false`).

⸻

Context Engine

Status: **Current**

The Context Engine **assembles** only the context required for the current request.

The Planner calls `ContextEngine::assemble_with` **after** Intent and Capability resolution, passing `AssembleHints` (`IntentId` + capability ids). Context Policy and providers derive relevance from that Intent only — they do not re-classify free-text intent.

Ownership:

* **Providers** contribute their own sections (Memory / Project / Search coordination / session-backed editor data)
* **Context Policies** decide participation only
* **Context Engine** orchestrates providers under policy + relevance + budget — it does not determine Intent, select Capabilities, invent session state, or execute tools
* **Host** pushes `ContextSessionInputs` before assemble
* **Tools** perform search / filesystem work after Action Policy + Permission

It returns a unified `ContextBundle`. The Planner does not assemble these pieces itself.

Recently assembled bundles are cached by project, workspace, conversation, active file, and request type (plus a request fingerprint). The cache is invalidated when files, project, workspace, conversation, or the search index change — performance only; correctness is unchanged. See `docs/context.md`.

Context History retains recent bundles with timestamp, request, providers used, bundle size, and execution duration for debugging and future reasoning transparency.

The LLM-facing Context API (`ContextEngine::to_llm_context` → `LlmContext`) converts a bundle into a stable, deterministically serializable structure for future model consumers — no model calls, no prompts. See `docs/context.md`.

Context Policies (`ContextPolicyEngine`) decide which providers may participate before assemble — relevant, minimal, privacy-aware, deterministic, and explainable. Independent of the **Action Policy Engine** (`jaymi-policies`, lifecycle name `policy_engine`) and of any LLM.

### Context Providers

Assemble is provider-driven. Subsystems implement `ContextProvider` with deterministic `relevance`, `priority`, and `estimate_size`. The engine skips low-relevance providers, allocates a configurable character/token budget to higher-priority providers first, and fits oversized contributions (truncate / summarize / preserve metadata). No AI scoring. The engine orchestrates providers without depending on their internals.

Initial providers: Conversation, Project, Workspace, Editor, Search, Memory, Diagnostics, Permission.

### Current ContextBundle (immutable snapshot)

First-class sections: Conversation, Active Project, Active Workspace, Current File, Current Selection, Open Files, Search Results, Memory Results, Diagnostics, Permissions, Planner Metadata, Active Capabilities, User Request Metadata.

The bundle never searches or reasons — Planner execution, Behaviors (**Planned**), and future LLM providers consume it as a frozen snapshot (`PlannerResponse.context()` / `context_bundle`). Parallel `memory_context` / `project_context` / `search_context` response fields are removed; use bundle accessors.

### Target context sources (not yet assembled)

* Live Git status
* Terminal output
* Notes / Messages / Browser history as first-class context feeds

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

Conversation remains permanent. Workspace kinds can expand beside it (conversation shell + **Coding Workspace shell** with five dock pages — Terminal / Problems / Search / Git / Diagnostics — plus Output placeholder, expansion chrome, and capability state / inspector).

See [docs/capabilities.md](docs/capabilities.md).

### Target capability catalog & UX

* Promote Planned capabilities as tools/providers land
* Coding → live editor / terminal / git on top of the existing shell
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

Examples: OCR Provider and Reasoning are **Stub**; Policies and local lexical Embeddings are **Experimental**; Index / Watcher report **Disabled** when indexing is turned off. Subsystems must not overstate readiness.

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

* Git history / live Git status
* Artifact pipelines
* Full IDE working-file state

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

Status: **Target** (interchangeability designed; Reasoning stub)

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
