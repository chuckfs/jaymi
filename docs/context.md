# Context Engine

**Status: Current Implementation**

The Context Engine is the sole request-context assembler for the Planner. This document describes shipped behavior.

The Context Engine assembles only the knowledge required for the current request.

The Planner calls a single method:

```text
ContextEngine::assemble(request) -> ContextBundle
```

The Planner does not coordinate Memory, Project, Search, or session workspace state itself.

---

## Context Providers

Assemble is **provider-driven**. Each subsystem implements [`ContextProvider`] and may contribute data to the [`ContextBundle`].

```text
ProviderRequest { request, session }
        │
        ▼
┌───────────────────┐
│  ContextProvider  │  contribute() → Option<ContextContribution>
└───────────────────┘
        │
        ▼
 Context Engine merges contributions → immutable ContextBundle
```

Rules:

* The engine orchestrates providers **without depending on their internal implementation**
* Each provider exposes a deterministic `relevance(request) -> RelevanceScore` (0..=100)
* The engine **skips** providers below `relevance_threshold` (default 40) before calling `contribute`
* Relevance heuristics consider user intent tags, active capabilities, workspace kind, and request kind — **no AI scoring**
* Each provider exposes `priority` and `estimate_size` for **Context Budgeting**
* The engine allocates a configurable character/token budget to **higher-priority providers first**
* Oversized contributions are **fitted** provider-agnostically: truncate → summarize → preserve metadata; otherwise skip
* `BudgetReport` is recorded on the bundle for diagnostics and future LLM windowing
* Providers may still return `Ok(None)` from `contribute` when they have nothing to add
* Providers own their subsystem dependencies (Memory / Project / Search / session reads)
* Boot installs the default set via `bind_sources` → `default_providers`; custom sets use `bind_providers` / `register_provider`

### Initial providers

| Provider | Contributes | Declines when |
|----------|-------------|----------------|
| `ConversationProvider` | Conversation summary | No active conversation |
| `ProjectProvider` | Active project / `ProjectContext` | No open project |
| `WorkspaceProvider` | Active workspace kind + session capabilities | Neither is set |
| `EditorProvider` | Current file / selection / open files | No editor session data |
| `SearchProvider` | Search coordination hint + session hits (never executes search) | No structured search, index summary, or hits |
| `MemoryProvider` | Relevant memories + promotions | — (always contributes memory results) |
| `DiagnosticsProvider` | Session diagnostics | Empty diagnostics |
| `PermissionProvider` | Session permission grants | Empty permissions |

The engine itself stamps **User Request Metadata** and **Planner Metadata** (assemble generation, folded `ContextSource`s, provider contribute/decline notes).

---

## Responsibilities

* Orchestrate registered `ContextProvider`s for each request
* Merge contributions into an immutable `ContextBundle`
* Stamp request / planner metadata
* Expose session inputs the host may push before assemble (`set_session_inputs` / `set_session_workspace`)

---

## Context Budgeting

Configurable via `ContextBudgetConfig` / `ContextEngine::set_budget_config` (default ~32k characters, 4 chars/token estimate, reserved stamp budget).

Assemble order after relevance filtering:

1. Sort providers by `priority` (desc), then relevance
2. Ask each provider for `estimate_size`
3. `contribute` when budget remains
4. `fit_contribution` if the payload exceeds remaining room
5. Record `BudgetReport` (used chars/tokens, truncated/skipped providers, summaries)

Fitting prefers dropping bulky payloads (project detail, memory bodies, search previews) while keeping identity metadata (ids, titles, paths, decisions). Ready for future LLM context windows — no model calls.

---

## ContextBundle caching

Recently assembled bundles are reused when the cache key matches. Correctness is preserved: hits never skip invalidation or fingerprint checks.

### Key

| Dimension | Source |
|-----------|--------|
| Project | Open project id (`ProjectEngine`) |
| Workspace | Session UX workspace kind |
| Conversation | Active conversation id (`MemoryEngine`) |
| Active file | Session current file path |
| Request type | Derived request kind (`chat`, `file_read`, `search`, …) |
| Request fingerprint | Content + structured request fields (memory / write / search / …) |
| Epoch | Bumped on every invalidation |
| Threshold / budget | Relevance threshold + max character budget |

### Invalidation

`ContextEngine::invalidate_cache(reason)` clears entries and bumps the epoch when:

* Files change (Planner write / manage_path)
* Project changes (open / close)
* Workspace changes (`set_session_workspace` / `set_session_inputs` when values differ)
* Conversation changes (`Application::set_active_conversation`)
* Search index / inventory updates (Discovery scan hooks, including filesystem watcher flushes; Planner `index` intents)

Cache hits still increment `assemble_count`, restamp planner generation / request metadata, and record a Context Inspector report with `cache_hit=true`.

---

## Context History

Retains the most recent assembled `ContextBundle`s for debugging and future reasoning transparency.

Each entry records:

| Field | Meaning |
|-------|---------|
| `timestamp_unix_ms` | When the assemble finished |
| `request` | Request content preview |
| `providers_used` | Provider ids that contributed |
| `bundle_size_characters` / tokens | Assembled size (budget accounting) |
| `duration_ms` | Wall-clock assemble duration |
| `bundle` | Immutable snapshot retained for inspection |

Also notes assemble generation and whether the entry was a cache hit. Default capacity is 32 (LRU ring). Read via `ContextEngine::history` / `Application::context_history`. Surfaced in Developer Diagnostics. Recording history never affects Planner execution.

---

## LLM-facing Context API

Converts an assembled `ContextBundle` into a structured representation for future language-model consumers.

```text
ContextBundle
      │
      ▼
ContextEngine::to_llm_context(bundle) → LlmContext
      │
      ▼
LlmContext::to_json()  // deterministic serialization
```

Rules:

* **Does not call models**
* **Does not build prompts**
* Stable section order via `LlmSectionId::ORDER`
* Deterministic JSON (`serde` field order + `BTreeMap` extensions)
* Provider metadata (`sources`, assemble notes, budget) travels with the payload
* `extensions` map reserved for future additive fields without breaking section order
* Schema versioned (`LLM_CONTEXT_SCHEMA_VERSION`)

LLMs should eventually consume `LlmContext` instead of querying Memory, Project, Search, or other subsystems directly.

---

## Context Policies

The Context Policy Engine filters and prioritizes Context Providers **before** the ContextBundle is assembled. Policies never gather context — they only decide what may participate.

```text
User Request → Planner → Intent
        │
        ▼
Context Policy Engine  →  Select / constrain providers
        │
        ▼
Context Engine assemble → ContextBundle → Behavior / Future LLM
```

### Decision fields

Each policy answers: participate?, why?, priority?, can_truncate?, requires_user_approval?, exclude_sensitive?, contribution constraints.

### Initial rules (`jaymi_default_context`)

| Provider | Rule |
|----------|------|
| Conversation | Always include (recent interaction) |
| Project | Only when a project is open |
| Workspace | Active workspace only |
| Editor | Current file + selection; open editors excluded by default |
| Search | Only when retrieval / files / symbols are required |
| Memory | Only when the request benefits from matching memories |
| Diagnostics | Coding capability or debug intent |
| Permission | Always include summary (no internal implementation details) |

### Sensitivity

Providers declare `Sensitivity` (`public` < `workspace` < `project` < `private` < `sensitive`). Policies block oversensitive contributions unless required.

### Explainability

Every bundle records a `PolicyReport`: active policies, per-provider Included/Excluded reasons, sizes before/after filtering, and assembled size. Surfaced in the Context Inspector and Developer Diagnostics.

### Extensibility

`ContextPolicy` trait supports future local-only, cloud restrictions, trust levels, enterprise, project-scoped privacy, and user-defined rules — not implemented yet.

---

## ContextBundle

`ContextBundle` is the first-class, immutable snapshot assembled for a single request. It does **not** search or reason — it is purely data assembled from providers. Once built, fields are private and only accessors are exposed.

It is the standard object passed into Planner execution, Behaviors, and future LLM providers (`PlannerResponse.context_bundle`).

### Sections

| Section | Contents |
|---------|----------|
| Conversation | Active conversation id, title, status, message count |
| Active Project | Project identity + optional full `ProjectContext` |
| Active Workspace | UX workspace kind id (`coding`, …) |
| Current File | Focused editor path / dirty / language |
| Current Selection | Editor selection range + optional text |
| Open Files | Open editor tabs |
| Search Results | Coordination hint + any pre-attached hits (no search executed here) |
| Memory Results | Relevant memories + promotion suggestions / ask decision |
| Diagnostics | Attached diagnostics for the request |
| Permissions | Attached permission grants / decisions |
| Planner Metadata | Assemble generation, contributing `ContextSource`s, notes |
| Active Capabilities | Capability ids recorded for the request |
| User Request Metadata | Structured flags / content preview from `UserRequest` |

---

## Context Inspector

Developer-facing diagnostics for the most recent `ContextBundle` assemble.

* Recorded automatically after each successful `ContextEngine::assemble`
* Read via `ContextEngine::inspect_last` / `Application::inspect_context`
* Surfaced in **Developer Diagnostics** (and the headless diagnostics dashboard)
* Shows: contributing providers, contribution sizes, relevance scores, priorities, budget allocation, omitted providers, truncation / summary decisions, bundle section presence, and whether the assemble was a **cache hit**
* **Does not affect execution** — never re-assembles, never calls providers for side effects

---

## Boot

1. Context Engine initializes after the Memory Engine (lifecycle dependency).
2. After Project Engine and Search Engine are ready, Application binds sources:
   - Memory Engine
   - Project Engine
   - Search Engine  
   which installs the default `ContextProvider` set.
3. Planner receives `Arc<ContextEngine>` and calls `assemble` at the start of every `handle`.
4. Application syncs experience workspace into the Context Engine via `prepare_context_session` before `handle`.

---

## What this is not

* Not a Reasoning Engine
* Not a language model
* Not a prompt builder (`LlmContext` is structured data only)
* Not a replacement for tool-backed search / read / discover execution

Search tools still execute through the Tool Orchestrator. `SearchProvider` only notes coordination hints and copies pre-attached hits.

---

## Status

Implemented as the sole request-context assembler for the Planner. Provider architecture is the extension point for additional context feeds.
