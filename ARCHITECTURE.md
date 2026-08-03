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
                     ▼
              Context Engine
           (sole assemble path)
                     │
     ┌───────────────┼────────────────┐
     │               │                │
 Memory Engine  Project Engine  Search Engine
     │               │                │
     └───────────────┼────────────────┘
                     │
      Capability / Policy / Permission
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

Instead, the Planner orchestrates:

Request → Context → Capability → Policy → Permission → Execution Plan → Tool Engine → Response

It decides:

* What is the user trying to accomplish?
* What information is required?
* Which memories are relevant?
* Which capabilities are needed?
* Which tools should be used?
* Does this require user approval?
* What should happen next?

Nothing bypasses it for request handling.

Project knowledge search is a Planner-mediated request (`UserRequest::search_project_knowledge` → `handle`).

**Partial / Stub:** Reasoning Engine (language-model backends) is not implemented (`is_implemented() == false`).

⸻

Context Engine

Status: **Current**

The Context Engine determines what Jaymi should know before responding.

Rather than loading everything into a model, it builds only the context required for the current request.

The Planner calls exactly one method: `ContextEngine::assemble`.

The Context Engine coordinates:

* Memory Engine (relevant memories and promotion suggestions)
* Project Engine (open project workspace)
* Search Engine (coordination hints when appropriate — tools still execute search)
* Active workspace / session state

It returns a unified `ContextBundle`. The Planner does not assemble these pieces itself.

### Current ContextBundle sources

* Active / open project workspace
* Retrieved memories + promotion suggestions
* Active conversation scoping (via Memory)
* Active UX workspace / session state
* Lightweight search coordination hints

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

Status: **Current** (engine + thin UX)

Capabilities define what Jaymi knows how to do.

### Current

Boot registers the **full capability catalog**. Availability (Ready / Experimental / Planned / Unavailable) distinguishes conceptual support from what is currently executable. Planned capabilities stay registered.

Conversation remains permanent. Workspace kinds can expand beside it (conversation shell + expansion chrome + capability state / inspector).

See [docs/capabilities.md](docs/capabilities.md).

### Target capability catalog & UX

* Promote Planned capabilities as tools/providers land
* Coding → full IDE slides in
* Creation → canvas appears
* Research → sources and notes appear

Capabilities describe behavior.

They do not describe implementation.

⸻

Tool Orchestrator

Status: **Partial**

Tools are concrete implementations of capabilities.

### Current tools

* `search_files`
* `search_knowledge`
* `read_file`
* `query_inventory`
* `scan_filesystem`

The planner selects tools automatically for supported intents.

### Target tools

Messages, Photos, Editor, LSP, Git, Terminal, Image Model, Vision Model, and many more — interchangeable under stable capabilities.

⸻

Provider System

Status: **Partial**

Providers connect Jaymi to resources.

### Current

* ProviderRegistry — discovery and diagnostics (identity metadata)
* Concrete provider instances bound into tools at boot: Filesystem, Local Embedding, OCR Placeholder
* Capability Engine soft-matching for plans (`providers_for`)

There is no ProviderManager. Execution goes through tools.

### Target

Git, Messages, Mail, Calendar, Browser, Photos, Notes, local/cloud AI models, installable provider plugins.

Providers expose consistent interfaces.

The rest of the system never depends on provider-specific behavior.

Providers can be added, removed, or replaced without affecting the planner.

⸻

Permission Engine

Status: **Current** (rule engine)

Jaymi should never surprise the user.

### Current

Permission categories and scopes; default rules (e.g. filesystem read allowed; many write/network/terminal actions denied or require approval); consulted before tool execution.

### Target

Conversational approval UX, plain-language previews, permission history, revocation UI, reversible defaults (Trash vs delete).

The user remains in control.

⸻

Policy Engine

Status: **Partial**

Policies constrain tool candidates before permissions.

### Current enforced

* Offline First
* Privacy Maximum

### Declared / Target enforcement

Highest Quality, Fastest, Battery Saver, Developer / Creative / Research modes, rich multi-scope resolution, user-custom policies.

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
