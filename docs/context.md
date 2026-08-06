# Context Engine

**Status: Current Implementation**

The Context Engine is the sole request-context assembler for the Planner. This document describes shipped behavior.

**ContextEngine is the sole factory for `ContextBundle`.** Bundles are minted only through:

```text
ContextEngine::assemble_with(request, hints) -> ContextBundle   # request path
ContextEngine::assemble(request) -> ContextBundle               # test / admin helper
ContextEngine::empty_bundle() -> ContextBundle                  # empty placeholder (no providers)
ContextEngine::reuse_bundle(&prior) -> ContextBundle            # attach prior engine-minted snapshot
```

The Planner may request context, request an empty bundle, or reuse a previously assembled bundle. It must not construct `ContextBundle` itself (`ContextBundle::default()` / `ContextBundleBuilder` are not Planner APIs).

The Context Engine assembles only the knowledge required for the current request.

The Planner calls after Intent and Capability resolution:

```text
ContextEngine::assemble_with(request, hints) -> ContextBundle
```

`hints` carries the canonical [`IntentId`](../crates/jaymi-core/src/intent.rs) and selected capability ids into Context Policy / relevance. Context never invents a second intent taxonomy. The Planner does not coordinate Memory, Project, Search, or session workspace state itself.

---

## Ownership (one purpose each)

| Subsystem | Owns | Must not |
|-----------|------|----------|
| **Context Providers** | Contribute their own sections | Decide policy, assemble other providers, execute tools, select capabilities |
| **Context Policies** | Participate / priority / constraints / sensitivity | Gather context, mutate providers, execute tools |
| **Context Engine** | Assemble under policy + relevance + budget; **sole factory** for `ContextBundle` | Determine Intent, select Capabilities, invent session state, execute tools |
| **Planner** | Orchestrate Intent → Capability → assemble; then branch (tool-backed Action Policy → Permission → Tools, or session/plan/unsupported return); may request empty / reuse engine-minted bundles | Reimplement Context Policy, build parallel context sections, or construct `ContextBundle` directly |
| **Behaviors** | Execute (Planned) | — |
| **Tools** | Perform work (search / read / write / …) | Run inside Context assemble |

Host (`Application`) pushes `ContextSessionInputs` (workspace, editor, diagnostics, permissions, project-open, search hits, plus **latest completed** background maintenance snapshots: git / inventory / file summaries). Request-selected capabilities arrive only via `AssembleHints`. Slow refreshes are Application-owned — see [context-maintenance.md](context-maintenance.md).

**Context Policy** (`jaymi-context`) ≠ **Action Policy** (`jaymi-policies`).

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
* Relevance heuristics consider user intent tags, active capabilities, workspace kind, request kind, and Planner **complexity** (via `AssembleHints`) — **no AI scoring**
* When `AssembleHints.complexity` is set, providers marked **Excluded** for that class are skipped before policy evaluation (inspector outcome `SkippedComplexity`); **Required** providers receive a high relevance score; **Optional** providers use normal heuristics — see [complexity.md](complexity.md)
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
| `WorkspaceProvider` | Active workspace kind + request-selected capabilities (from Planner `AssembleHints`) | Neither is set |
| `EditorProvider` | Current file / selection / open files | No editor session data |
| `SearchProvider` | Search coordination hint + session hits (never executes search) | No structured search, index summary, or hits |
| `MemoryProvider` | Relevant memories + promotions | — (always contributes memory results) |
| `DiagnosticsProvider` | Session diagnostics | Empty diagnostics |
| `GitStatusProvider` | Session git status (completed maintenance) | Empty / non-repo without summary |
| `WorkspaceInventoryProvider` | Session workspace inventory (completed maintenance) | Empty inventory |
| `FileSummariesProvider` | Session file summaries (completed maintenance) | Empty summaries |
| `PermissionProvider` | Session permission grants | Empty permissions |

The engine itself stamps **User Request Metadata** and **Planner Metadata** (assemble generation, folded `ContextSource`s, provider contribute/decline notes).

---

## Responsibilities

* Orchestrate registered `ContextProvider`s for each request
* Merge contributions into an immutable `ContextBundle`
* Stamp request / planner metadata
* Expose session inputs the host may push before assemble (`set_session_inputs` / `set_session_workspace`)

Background maintenance of git / inventory / diagnostics / file summaries is **Application-owned** — see [context-maintenance.md](context-maintenance.md). Providers only read completed session snapshots.

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

Recently assembled bundles are reused when the cache key matches. On a hit the
engine **skips all provider `contribute` work**, restamps planner generation /
request metadata, and records `cache_hit=true` on the Context Inspector.

Reuse lives **entirely inside ContextEngine**. Planner never builds keys or
reads the LRU — it only asks for a fresh assemble via
`ContextEngine::request_fresh_context(reason)` when it knows context must change.

### Key

| Dimension | Source |
|-----------|--------|
| Project | Open project id (`ProjectEngine`) |
| Workspace | Session UX workspace kind |
| Conversation | Active conversation id (`MemoryEngine`) |
| Conversation revision | `(updated_at, message_count)` — detects unchanged conversational state without loading the transcript |
| Session fingerprint | Diagnostics, editor, permissions, search hits, … |
| Active file | Session current file path |
| Request type | Derived request kind (`chat`, `file_read`, `search`, …) |
| Request fingerprint | Content + structured request fields (memory / write / search / …) |
| Epoch | Bumped on every invalidation |
| Threshold / budget | Relevance threshold + max character budget |
| Hints / policies | AssembleHints + active Context Policy fingerprints |

### Invalidation / fresh context

`ContextEngine::request_fresh_context(reason)` clears entries and bumps the epoch when:

| Reason | Typical trigger |
|--------|-----------------|
| `conversation_changed` | Active conversation switch (`Application::set_active_conversation`) |
| `workspace_changed` | UX workspace kind change |
| `project_changed` | Project open / close (Planner) |
| `diagnostics_changed` | Session diagnostics snapshot change |
| `files_changed` / `search_index_updated` | Planner tool success (`PreparedToolCall.fresh_context`) |
| `editor_changed` / `permissions_changed` / … | Other session-input deltas |
| Planner-requested | Any explicit `request_fresh_context` call |

Identical session rewrites (same workspace / same diagnostics) do **not** invalidate.

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

Architectural path into Reasoning (Sprint B1.1–B1.13.5):

```text
ContextBundle → LlmContext → PromptBuilder → ReasoningRequest.prompt
  → ReasoningProvider → ConversationStream → ReasoningResponse
```

Orchestrated by `ReasoningEngine` / `ConversationStream`. PromptBuilder adapts to
`context_window − reserved_completion` and is the sole source of generation text.
Every `LlmSectionId` is traced into a prompt disposition (included / excluded /
truncated / filtered / budgeted) — see [docs/prompt.md](prompt.md). Providers
perform transport only. Conversational / unknown Planner requests stream after
assemble. See [docs/reasoning.md](reasoning.md).

`LlmContext` remains structured data only — it does **not** build prompts.
Prompt assembly is owned by `PromptBuilder`; the engine attaches the `Prompt`
onto the request before every provider call.

---

## Context Policies

The Context Policy Engine filters and prioritizes Context Providers **before** the ContextBundle is assembled. Policies never gather context — they only decide what may participate.

**Current** request pipeline (see also `ARCHITECTURE.md` / `docs/planner.md`):

```text
User Request → Planner → Intent → Capability
        │
        ▼
Context Policy Engine  →  Select / constrain providers
        │
        ▼
Context Providers → Context Engine assemble → ContextBundle
        │
        ▼
Behavior (Planned) → Action Policies → Permissions → Tools → Response
```

Planner passes `AssembleHints` (`IntentId` + capability ids + optional Planner
`complexity` class) into `assemble_with`. Context derives Intent facets from
that Intent only — it never runs a parallel free-text intent or complexity
classifier. Complexity bias is applied from the Planner-supplied label
([docs/complexity.md](complexity.md)).

### Decision fields

Each policy answers: participate?, why?, priority?, can_truncate?, requires_user_approval?, exclude_sensitive?, contribution constraints.

These fields are **enforced during assemble**:

| Field | Enforcement |
|-------|-------------|
| `participate` | Provider omitted (`SkippedPolicy`) |
| `requires_user_approval` | Omitted until id is in `session.approved_context_providers` (`SkippedApproval`) |
| `exclude_sensitive` | Expands to redaction constraints (memory bodies, search previews, selection text) |
| Contribution constraints | Applied after `contribute`, before budget fit |
| `can_truncate` | When false, oversized contributions are skipped (`policy_forbids_truncation`) instead of fitted |
| Sensitivity | Providers above `max_sensitivity` denied unless required; `Sensitive` also requires approval |

### Initial rules (`jaymi_default_context`)

| Provider | Rule |
|----------|------|
| Conversation | Always include (recent interaction) |
| Project | Only when a project is open |
| Workspace | Active workspace only |
| Editor | Current file + selection; open editors excluded; **selection text requires approval** |
| Search | Only when retrieval / files / symbols are required |
| Memory | Matching memories only; **Private bodies redacted** (`exclude_sensitive`) |
| Diagnostics | Coding capability or debug intent |
| Permission | Always include **summary only** (no explanation/resource detail) |

### Sensitivity

Providers declare `Sensitivity` (`public` < `workspace` < `project` < `private` < `sensitive`). Policies block oversensitive contributions unless required. `Sensitive` contributions additionally require user approval.

### Explainability

Every bundle records a `PolicyReport`: active policies, per-provider Included/Excluded/Pending reasons, approval status, enforced constraints, truncation reasons, sizes before/after filtering, and assembled size. Surfaced in the Context Inspector and Developer Diagnostics.

### Extensibility

`ContextPolicy` trait supports future local-only, cloud restrictions, trust levels, enterprise, project-scoped privacy, and user-defined rules — not implemented yet.

---

## ContextBundle

`ContextBundle` is the **sole authoritative request-context contract** in Jaymi (**Current**).

It is the first-class, immutable snapshot assembled for a single request. It does **not** search or reason — it is purely data assembled from providers. Once built, fields are private and only accessors are exposed.

**Factory:** every production `ContextBundle` is created by `ContextEngine` (`assemble_with` / `assemble` / `empty_bundle` / `reuse_bundle`). See ownership table above.

Consumers (**Current** / **Planned**):

| Consumer | Contract |
|----------|----------|
| Planner execution | `PlannerResponse.context()` / `.context_bundle` |
| Behaviors | **Planned** — consume `ContextBundle` only |
| LLM / Reasoning | **Partial** — conversational path + model-aware prompt budgeting ([docs/prompt.md](prompt.md)) |

`PlannerResponse` no longer carries parallel `memory_context`, `project_context`, or `search_context` fields. Use:

* `response.memory()` → bundle memory
* `response.project()` → bundle active project detail
* `response.promotion_suggestions()` / `response.promotion_ask()` → bundle memory promotions

Administrative Memory/Project CRUD APIs on Application remain for store management — they are not request-context substitutes.

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

Developer-facing diagnostics for the most recent `ContextBundle` assemble. Every
Context decision on that assemble is transparent:

| Field | Meaning |
|-------|---------|
| Provider order | Evaluation order (`Eval`) for every provider; budget allocation order (`Alloc`) for ranked providers; `contributor_order` lists accepted providers in allocation order |
| Relevance score | Per-provider score vs assemble threshold |
| Policy decisions | Full `PolicyReport` (included / excluded / pending, constraints, reasons) |
| Sensitivity | Per-provider sensitivity at decision time |
| Approval requirements | `requires_user_approval` + `approval_status` (`not_required` / `approved` / `pending` / `n/a`) |
| Budget allocation | Used / max characters & tokens, truncated and budget-skipped providers |
| Truncation | Per-provider truncate / summarize / budget-skip labels |
| Cache hit/miss | `cache_status()` (`hit` / `miss`) |
| Assembly duration | Wall-clock `duration_ms` |
| Per-provider contribute | Optional `InspectedProvider.duration_ms` for each `contribute` call |
| Final bundle size | `bundle_size_characters` / `bundle_size_estimated_tokens` |

* Recorded automatically after each successful `ContextEngine::assemble`
* Read via `ContextEngine::inspect_last` / `Application::inspect_context`
* Surfaced in **Developer Diagnostics** (and the headless diagnostics dashboard)
* **Does not affect execution** — never re-assembles, never calls providers for side effects

---

## Validation Suite

The Context system is covered by a dedicated validation suite (A10.9):

* **Crate:** `cargo test -p jaymi-context --test validation_suite`
  — deterministic assembly & ordering, inclusion / exclusion, budget,
  sensitivity, approval, cache invalidation, bundle immutability,
  provider independence, Context Policy determinism
* **App / Planner:** `cargo test -p jaymi --test context_validation`
  — every `handle` assembles once, attaches `ContextBundle`, inspector
  explainability, include/exclude through Planner, diagnostics

Together with the focused Context unit and integration tests
(`context_engine`, `context_contract`, `context_policies`,
`context_inspector`, `context_session_wiring`, `context_history`),
the Context Engine is one of the best-tested systems in Jaymi.

---

## Boot

1. Context Engine initializes after the Memory Engine (lifecycle dependency).
   Project Engine and Search Engine are **bound later** via `bind_sources`
   (not lifecycle `DEPENDENCIES` — boot initializes Context before those engines).
2. After Project Engine and Search Engine are ready, Application binds sources:
   - Memory Engine
   - Project Engine
   - Search Engine  
   which installs the default `ContextProvider` set.
3. Planner receives `Arc<ContextEngine>` and calls `assemble_with` after Intent and Capability resolution on every `handle`.
4. Application pushes a full [`ContextSessionInputs`] snapshot via `prepare_context_session` before **every** Planner assemble path — tool-backed `handle`, conversational `begin_generation` / `handle_streaming_with_workspace`, and after workspace expand/close. There is no alternate preparation path for conversation:
   - active workspace kind
   - current file / cursor selection / open files (from CodingState when Coding is open)
   - diagnostics (Problems panel preferred over raw LSP diagnostics)
   - permission policy summary (synthesized from Permission Engine)
   - search panel hits (when present)
   - `active_capabilities` left empty (request-selected ids arrive only via Planner `AssembleHints`)
   Closing Coding clears editor / diagnostics / search fields so the bundle does not keep stale UI state.
   Future Workspace Intelligence enrichments land in the same `prepare_context_session` so conversation automatically receives them.

---

## Session inputs

`ContextSessionInputs` is the host contract for UI/engine state the Context Engine cannot discover itself. Placeholders are not used — unset fields are empty, never invented paths or fake grants.

| Field | Source |
|-------|--------|
| `workspace_kind` | Experience active workspace |
| `current_file` / `current_selection` / `open_files` | Coding `OpenEditors` (selection = caret until Monaco selection IPC) |
| `diagnostics` | Completed maintenance snapshot (else Coding Problems / raw diagnostics) |
| `git_status` | Completed maintenance snapshot |
| `workspace_inventory` | Completed maintenance snapshot |
| `file_summaries` | Completed maintenance snapshot |
| `permissions` | Permission Engine policy matrix summary |
| `active_capabilities` | **Deprecated / empty** — request-selected capability ids come only from Planner `AssembleHints`, never from a Capability Engine catalog |
| `search_hits` | Coding Search panel results |

Active project and conversation still come from Project / Memory engines via providers — not duplicated into session inputs.

See [context-maintenance.md](context-maintenance.md) for refresh ownership.

* Not a Reasoning Engine
* Not a language model
* Not a prompt builder (`LlmContext` is structured data only)
* Not a replacement for tool-backed search / read / discover execution

Search tools still execute through the Tool Orchestrator. `SearchProvider` only notes coordination hints and copies pre-attached hits.

---

## Status

Implemented as the sole request-context assembler for the Planner. Provider architecture is the extension point for additional context feeds.
