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
| **Context Providers** | Propose [`ContextCandidate`](context-candidates.md) nodes via `propose_candidates()` | Decide policy, assemble bundles, execute tools, select capabilities, own observation |
| **Context Policies** | Provider participation + candidate relevance / recency / importance / privacy + deterministic [Context Selection](context-selection.md) profiles | Gather context, mutate providers, execute tools, assemble bundles, AI scoring |
| **Context Engine** | Select candidates under budget; materialize → **sole factory** for `ContextBundle` | Determine Intent, select Capabilities, invent session state, execute tools |
| **Planner** | Orchestrate Intent → Capability → assemble; then branch (tool-backed Action Policy → Permission → Tools, or session/plan/unsupported return); may request empty / reuse engine-minted bundles | Reimplement Context Policy, build parallel context sections, or construct `ContextBundle` directly |
| **Behaviors** | Execute (Planned) | — |
| **Tools** | Perform work (search / read / write / …) | Run inside Context assemble |

Host (`Application`) pushes `ContextSessionInputs` (workspace, editor, diagnostics,
permissions, project-open, search hits, plus **latest completed** ambient
maintenance snapshots: git / inventory / file summaries / WorkspaceSnapshot /
EditorSnapshot / ProjectSnapshot / GitSnapshot / RuntimeSnapshot / Workspace
Memory). Request-selected capabilities arrive only via `AssembleHints`. Slow
refreshes are Application-owned — see [context-maintenance.md](context-maintenance.md)
and [workspace-intelligence.md](workspace-intelligence.md).

**Context Policy** (`jaymi-context`) ≠ **Action Policy** (`jaymi-policies`).

---

## Context Providers

Assemble is **provider-driven**. Each subsystem implements [`ContextProvider`]
and proposes [`ContextCandidate`](context-candidates.md) nodes (Sprint B2.7;
migration completed B2.13.1). Context Policy scores candidates; the engine packs
under budget and materializes only selected candidates into the
[`ContextBundle`]. Providers never assemble bundles.

```text
ProviderRequest { request, session }
        │
        ▼
┌───────────────────┐
│  ContextProvider  │  propose_candidates() → Vec<ContextCandidate>
└───────────────────┘
        │
        ▼
 Context Policy (relevance · recency · importance · privacy)
        │
        ▼
 Context Engine budget-selects + materializes → immutable ContextBundle
```

Rules:

* The engine orchestrates providers **without depending on their internal implementation**
* Each provider exposes a deterministic `relevance(request) -> RelevanceScore` (0..=100)
* The engine **skips** providers below `relevance_threshold` (default 40) before proposing
* Relevance heuristics consider user intent tags, active capabilities, workspace kind, request kind, and Planner **complexity** (via `AssembleHints`) — **no AI scoring**
* When `AssembleHints.complexity` is set, providers marked **Excluded** for that class are skipped before policy evaluation (inspector outcome `SkippedComplexity`); **Required** providers receive a high relevance score; **Optional** providers use normal heuristics — see [complexity.md](complexity.md)
* Each provider exposes `priority` and `estimate_size` for **Context Budgeting**
* Candidates are scored for **relevance / recency / importance / privacy**, then packed under budget — see [context-candidates.md](context-candidates.md)
* Sprint **B2.8** [Context Selection](context-selection.md) chooses which workspace feeds participate (deterministic profiles; no AI)
* Sprint **B2.9** [Workspace Memory](workspace-memory.md) remembers Coding activity (edits / opens / builds / failures / objective); distinct from Conversation Memory; Policy decides inclusion
* Oversized contributions may still be **fitted** after materialize: truncate → summarize → preserve metadata; otherwise skip
* `BudgetReport` / `PolicyReport.candidate_selection` are recorded on the bundle for diagnostics
* Providers may return an empty candidate list when they have nothing to add
* `contribute()` materializes proposed candidates for convenience — production
  assemble always calls `propose_candidates` (Sprint B2.13.1)
* Providers own their subsystem dependencies (Memory / Project / Search / session reads)
* Boot installs the default set via `bind_sources` → `default_providers`; custom sets use `bind_providers` / `register_provider`

### Initial providers

| Provider | Contributes | Declines when |
|----------|-------------|----------------|
| `ConversationProvider` | Conversation summary | No active conversation |
| `ProjectProvider` | Active project / `ProjectContext` (+ ProjectSnapshot summaries) | No open project |
| `WorkspaceProvider` | Active workspace kind + request-selected capabilities (from Planner `AssembleHints`) | Neither is set |
| `EditorProvider` | Current file / selection / open files (+ EditorSnapshot intelligence) | No editor session data |
| `SearchProvider` | Search coordination hint + session hits (never executes search) | No structured search, index summary, or hits |
| `MemoryProvider` | Relevant memories + promotions | — (always contributes memory results) |
| `DiagnosticsProvider` | Session diagnostics | Empty diagnostics |
| `GitStatusProvider` | Session git status / GitSnapshot summaries (**Current** B2.5) | Empty / non-repo without summary |
| `RuntimeProvider` | RuntimeSnapshot summaries (**Current** B2.6) | Empty runtime observation |
| `WorkspaceMemoryProvider` | Coding activity rings (**Current** B2.9) | Empty workspace memory |
| `WorkspaceInventoryProvider` | Session workspace inventory (completed maintenance) | Empty inventory |
| `FileSummariesProvider` | Session file summaries (completed maintenance) | Empty summaries |
| `PermissionProvider` | Session permission grants | Empty permissions |

The engine itself stamps **User Request Metadata** and **Planner Metadata** (assemble generation, folded `ContextSource`s, provider propose / decline notes).

---

## Responsibilities

* Orchestrate registered `ContextProvider`s for each request
* Merge contributions into an immutable `ContextBundle`
* Stamp request / planner metadata
* Expose session inputs the host may push before assemble (`set_session_inputs` / `set_session_workspace`)

Background maintenance of git / inventory / diagnostics / file summaries /
WorkspaceSnapshot / EditorSnapshot / ProjectSnapshot / RuntimeSnapshot /
Workspace Memory is **Application-owned** — see
[context-maintenance.md](context-maintenance.md) and
[workspace-intelligence.md](workspace-intelligence.md).
Providers only read completed session snapshots. Ambient WorkspaceSnapshot
refresh never rebuilds a ContextBundle. Conversational prepare never rebuilds
a WorkspaceSnapshot and never runs `observe_toolchain` (Sprint B2.13.2).

---

## Context Budgeting

Configurable via `ContextBudgetConfig` / `ContextEngine::set_budget_config` (default ~32k characters, 4 chars/token estimate, reserved stamp budget).

Assemble order after relevance filtering (Sprint B2.7 / B2.13.1):

1. Sort providers by `priority` (desc), then relevance
2. Ask each provider for `estimate_size` / candidate estimates
3. `propose_candidates` when the provider participates
4. Context Policy scores candidates; engine budget-selects
5. Materialize selected candidates; `fit_contribution` if a payload exceeds remaining room
6. Record `BudgetReport` / `PolicyReport.candidate_selection` (used chars/tokens, truncated/skipped, summaries)

Fitting prefers dropping bulky payloads (project detail, memory bodies, search previews, selection text) while keeping identity metadata (ids, titles, paths, decisions). Ready for future LLM context windows — no model calls.

---

## ContextBundle caching

Recently assembled bundles are reused when the cache key matches. On a hit the
engine **skips provider propose / materialize work**, restamps planner generation /
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
| Contribution constraints | Applied after materialize, before budget fit |
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
| Git status | Coding / project / git-shaped requests (via Context Selection profiles) |
| Runtime | Compile / debug / terminal-shaped requests (via Context Selection profiles) |
| Workspace Memory | Coding activity when selection profiles include it |
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

| Section | Contents | Status |
|---------|----------|--------|
| Conversation | Active conversation id, title, status, message count | **Current** |
| Active Project | Project identity + optional full `ProjectContext` / ProjectSnapshot | **Current** |
| Active Workspace | UX workspace kind id (`coding`, …) | **Current** |
| Current File | Focused editor path / dirty / language | **Current** |
| Current Selection | Editor selection range + text (Monaco IPC via CodingState) | **Current** (B2.13.3) |
| Open Files | Open editor tabs | **Current** |
| Search Results | Coordination hint + any pre-attached hits (no search executed here) | **Current** |
| Memory Results | Relevant memories + promotion suggestions / ask decision | **Current** |
| Diagnostics | Attached diagnostics for the request | **Current** |
| Git status | Branch / dirty summaries from GitSnapshot | **Current** (B2.5) |
| Runtime intelligence | Terminal / build / test summaries from RuntimeSnapshot | **Current** (B2.6) |
| Workspace Memory | Recent edits / opens / builds / failures / objective | **Current** (B2.9) |
| Workspace inventory | Ambient inventory snapshot | **Current** |
| File summaries | Ambient file head summaries | **Current** |
| Permissions | Attached permission grants / decisions | **Current** |
| Planner Metadata | Assemble generation, contributing `ContextSource`s, Environmental Resolution, notes | **Current** |
| Active Capabilities | Capability ids recorded for the request | **Current** |
| User Request Metadata | Structured flags / content preview from `UserRequest` | **Current** |

**Target** (not yet assembled): Notes / Messages / Browser history feeds.

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
| Per-provider propose | Optional `InspectedProvider.duration_ms` for each provider propose / materialize call |
| Final bundle size | `bundle_size_characters` / `bundle_size_estimated_tokens` |

* Recorded automatically after each successful `ContextEngine::assemble`
* Read via `ContextEngine::inspect_last` / `Application::inspect_context`
* Surfaced in **Developer Diagnostics** (and the headless diagnostics dashboard)
* **Does not affect execution** — never re-assembles, never calls providers for side effects

**Workspace Diagnostics (Sprint B2.11)** aggregates the last inspector report with
Context Maintenance freshness / status into
`DiagnosticsSnapshot.workspace_inspector` for the Developer Diagnostics
**Workspace Intelligence** section (candidates, policy, budget, timings). Never
written to the conversation transcript. See
[workspace-diagnostics.md](workspace-diagnostics.md).

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
   - canonical [`WorkspaceSnapshot`](workspace-snapshot.md) (Sprint B2.1 / B2.2 / B2.13.2) — latest **completed** ambient observation only; prepare never rebuilds or probes toolchain markers (schedules ambient refresh if missing)
   - canonical [`EditorSnapshot`](editor-snapshot.md) (Sprint B2.3 / B2.13.3) — latest **completed** editor intelligence (selection text/range from Monaco via CodingState); providers consume; Planner/Reasoning never call LSP
   - canonical [`ProjectSnapshot`](project-snapshot.md) (Sprint B2.4) — latest **completed** project intelligence; `ProjectProvider` consumes; Planner never scans; no FS on request path
   - canonical [`GitSnapshot`](git-snapshot.md) (Sprint B2.5) — **Current**; latest **completed** Git intelligence; `GitStatusProvider` exposes summaries; Reasoning never runs git
   - canonical [`RuntimeSnapshot`](runtime-snapshot.md) (Sprint B2.6) — **Current**; latest **completed** runtime intelligence; `RuntimeProvider` exposes summaries; TerminalProvider owns updates; conversation never waits; observe never re-runs cargo
   - [`Workspace Memory`](workspace-memory.md) (Sprint B2.9) — Coding activity rings from CodingState
   Closing Coding clears editor / diagnostics / search fields so the bundle does not keep stale UI state.
   Workspace Intelligence enrichments continue to land in the same `prepare_context_session` so conversation automatically receives them.

---

## Session inputs

`ContextSessionInputs` is the host contract for UI/engine state the Context Engine cannot discover itself. Placeholders are not used — unset fields are empty, never invented paths or fake grants.

| Field | Source |
|-------|--------|
| `workspace_kind` | Experience active workspace |
| `current_file` / `current_selection` / `open_files` | Coding `OpenEditors` (selection text from Monaco IPC via CodingState; caret-only when empty) |
| `diagnostics` | Completed maintenance snapshot (else Coding Problems / raw diagnostics) |
| `git_status` | Completed maintenance snapshot |
| `workspace_inventory` | Completed maintenance snapshot |
| `file_summaries` | Completed maintenance snapshot |
| `permissions` | Permission Engine policy matrix summary |
| `active_capabilities` | **Deprecated / empty** — request-selected capability ids come only from Planner `AssembleHints`, never from a Capability Engine catalog |
| `search_hits` | Coding Search panel results |
| `workspace_snapshot` | Latest completed ambient observation ([workspace-snapshot.md](workspace-snapshot.md) / [context-maintenance.md](context-maintenance.md)) — host-refreshed; Context Engine consumes; never builds a ContextBundle |
| `editor_snapshot` | Latest completed editor intelligence ([editor-snapshot.md](editor-snapshot.md)) — consumed by `EditorProvider` / `DiagnosticsProvider`; Planner/Reasoning never call LSP |
| `project_snapshot` | Latest completed project intelligence ([project-snapshot.md](project-snapshot.md)) — ambient-maintained; `ProjectProvider` consumes; Planner never scans; no FS on request path |
| `git_snapshot` | Latest completed Git intelligence ([git-snapshot.md](git-snapshot.md)) — **Current** (B2.5); ambient-maintained; `GitStatusProvider` exposes summaries; Reasoning never runs git |
| `runtime_snapshot` | Latest completed runtime intelligence ([runtime-snapshot.md](runtime-snapshot.md)) — **Current** (B2.6); ambient-maintained; `RuntimeProvider` exposes summaries; TerminalProvider owns updates; conversation never waits |
| `workspace_memory_snapshot` | Coding activity rings ([workspace-memory.md](workspace-memory.md)) — **Current** (B2.9); distinct from Conversation Memory |

Active project and conversation still come from Project / Memory engines via providers — not duplicated into session inputs.

See [context-maintenance.md](context-maintenance.md) for refresh ownership.

* Not a Reasoning Engine
* Not a language model
* Not a prompt builder (`LlmContext` is structured data only)
* Not a replacement for tool-backed search / read / discover execution

Search tools still execute through the Tool Orchestrator. `SearchProvider` only notes coordination hints and copies pre-attached hits.

---

## Status

Implemented as the sole request-context assembler for the Planner. Workspace
Intelligence feeds (B2.1–B2.13.3) and the Context Candidate Graph (B2.7 /
B2.13.1) are **Current**. Provider architecture remains the extension point for
additional context feeds (Notes / Messages / Browser history are **Target**).
Documentation synchronized in Sprint B2.13.4.
