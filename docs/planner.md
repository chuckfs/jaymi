Planner

**Status: Current Implementation** (orchestration kernel) · **Partial:** Conversation state machine + streaming Reasoning (Sprint B1.1–B1.7); tool calling through the model still later

The Planner is the orchestration kernel of Jaymi.

The Planner is responsible for coordinating every interaction within Jaymi.

It understands user goals, gathers context, delegates work, enforces permissions, and manages execution.

The Planner does not perform the work itself.

Instead, it coordinates the systems that do.

This separation allows Jaymi to evolve independently of any individual AI model, provider, or tool.

⸻

Core Philosophy

Jaymi is not built around a language model.

It is built around decision making.

**Target:** Language models help Jaymi reason.

**Current:** The Decision Engine routes intents deterministically. Unknown / conversational requests go Planner → ContextBundle → Reasoning Engine → response. The Reasoning Engine orchestrates PromptBuilder → provider selection → stream/complete with timeouts, cancellation, metrics, and retry. Ollama may be wired at boot. Tool-backed, PlanWork, Review, and execution paths do not call Reasoning. See [docs/reasoning.md](reasoning.md).

Canonical identity is `jaymi_core::IntentId`. The Planner's payload-bearing `Intent` maps to that id via `Intent::id()`. Context, Capabilities, Behaviors, and Policies must reference `IntentId` — Context never re-classifies free-text intent independently.

The Planner decides what happens next.

This distinction allows intelligence to improve over time without changing the architecture.

⸻

Responsibilities

The Planner is responsible for:

* Understanding the user’s goal
* Determining whether additional reasoning is required
* Building execution plans (every meaningful action)
* Loading context through the Context Engine (`assemble`)
* Selecting capabilities
* Choosing tools
* Gating plans through review when required
* Coordinating execution
* Producing execution summaries
* Managing permissions
* Recovering from failures
* Producing the final response

The Planner never directly edits files, generates images, executes terminal commands, or searches documents.

Those responsibilities belong to specialized tools.

The Planner does not assemble memory, project, or search context itself.

That responsibility belongs to the Context Engine. **ContextEngine is the sole
factory for `ContextBundle`** — the Planner requests assemble / empty / reuse
bundles; it never constructs them.

The Planner does not own long-lived Memory or Project CRUD APIs.

* Project create / delete / list / lookup belong to the Project Engine
* Memory store / retrieve / promote / conversations / personal preferences belong to the Memory Engine
* Application (or tools) call those engines directly for administrative operations

Project session open/close has exactly one lifecycle: Application delegates →
Planner orchestrates → Project Engine owns open state (Memory mirrors the id
for context assembly). Continue / Open / Close intents and Application
`open_project` / `close_project` / `set_active_project` all enter
`Planner::handle` — there is no Application→Engine session bypass.

⸻

Request Pipeline

Every tool-backed request follows the same kernel path (**Current**):

```text
User Request
↓
Planner
↓
Intent Resolution
↓
Capability Resolution
↓
Context Policy Engine
↓
Context Providers
↓
Context Engine
↓
ContextBundle
↓
Behavior                          # Planned — not implemented
↓
Action Policies
↓
Permissions
↓
Tool Orchestrator
↓
Providers
↓
Planner Response
```

Context Policies decide which context providers may contribute. Action Policies decide whether a tool/provider candidate may execute. They are different engines.

⸻

Architecture

User
   │
   ▼
Conversation
   │
   ▼
Planner
   │
   ├───────────────┐
   │               │
Decision Engine    Reasoning Engine (conversational / unknown after Context)
   │               │
   └──────┬────────┘
          │
   Intent → Capability
          │
   Context Policy → Providers → Context Engine → ContextBundle
          │
   Action Policy → Permission
          │
    Tool Orchestrator
          │
 ProviderRegistry + bound providers
          │
 Local Resources / AI Models / Internet

The Planner remains deterministic.

Reasoning is delegated.

Execution is delegated.

Provider discovery uses the ProviderRegistry. Tools hold concrete provider
instances bound at boot — there is no separate ProviderManager.

⸻

Decision Engine

The Decision Engine contains deterministic application logic for **Intent** and
**Capability** mapping.

It answers questions such as:

* What Intent does this request map to?
* Which capabilities are required for that Intent?
* Does this Intent need the Reasoning Engine? (empty capabilities → conversational)

These decisions should never depend on a language model.

Internet access, permission grants, and memory relevance are **not** Decision
Engine concerns — they belong to Action Policy, Permission Engine, and
Context Policy / MemoryProvider during assemble.

⸻

Reasoning Engine

**Status: Partial** (Sprint B1.1–B1.7: contracts, Prompt Builder, Ollama, engine orchestration, conversational streaming, conversation state machine)

The Reasoning Engine exists to solve problems that require language understanding.

Examples include:

* Conversational / unknown requests (**Current** — after Context assemble)
* Understanding ambiguous requests
* Summarizing information
* Planning complex workflows
* Explaining results
* Generating natural language
* Choosing between multiple reasonable interpretations

The Reasoning Engine may use any compatible language model.

It is replaceable.

**Current:** Unknown / conversational intents stream through `ConversationStream` after `ContextBundle` assemble. Prior Experience turns are passed as `ReasoningRequest.history` (Sprint B1.13.3) so PromptBuilder can format multi-turn context. The Planner owns a conversation runtime state machine (`ConversationState`): Idle → Preparing Context → Reasoning / Streaming / Waiting For Review / Executing → Completed / Cancelled / Failed. Experience / UI mirror that state; they never invent transitions (enforced Sprint **B1.13.7**). Pumpable (`start`/`pump`/`complete`) and blocking (`run_with_observer`) delivery share assemble + terminal mapping; intentional host differences are documented in Sprint **B1.13.8**. See [docs/reasoning.md](reasoning.md) and [docs/experience.md](experience.md).

⸻

Conversation State Machine

**Status: Current** (Sprint B1.7)

```text
Idle
  → Preparing Context
  → Reasoning | Streaming | Waiting For Review | Executing
  → Completed | Cancelled | Failed
```

| State | When |
|-------|------|
| Idle | No active request |
| Preparing Context | Intent resolved; Context assemble in progress |
| Reasoning | Generation started; waiting for first visible token |
| Streaming | Tokens arriving into the conversation |
| Waiting For Review | Execution Plan paused on a Review Card |
| Executing | Approved plan running tools |
| Completed / Cancelled / Failed | Terminal for the turn |

`StreamingLifecycle` remains the generation sub-machine under Reasoning / Streaming. `ExecutionStatus` remains plan-scoped. `PlannerResponse.conversation_state` is authoritative.

⸻

Request Lifecycle

Every user-facing request enters `Planner::handle` (**Current**). After Intent
and Capability resolution, every path assembles a `ContextBundle`. Stages after
assemble **branch** by Intent class:

| Variant | Context assemble | Behavior | Execution Plan → Review → Tool → Summary |
|---------|------------------|----------|------------------------------------------|
| Tool-backed (search / read / list / …) | Yes | Planned (skipped) | Yes |
| PlanWork | Yes | Planned (skipped) | No — capability plan only |
| Session open / close / continue | Yes (after Project Engine mutate) | Planned (skipped) | No |
| Conversational / unknown | Yes | Planned (skipped) → ConversationStream → Response | No |

Canonical stage vocabulary (`RequestStage`):

```text
Receive Request
↓
Determine Intent
↓
Resolve Capability
↓
Evaluate Context Policy
↓
Collect From Providers
↓
Assemble ContextBundle
↓
Run Behavior                          # Planned — not implemented
                                      # conversational / unknown → Reasoning Engine here
↓
Create Execution Plan                 # tool-backed only
↓
Review Execution Plan                 # when review required
↓
Evaluate Action Policy                # during plan gating
↓
Check Permissions                     # during plan gating
↓
Execute Tool                          # Approved plans only
↓
Invoke Providers                      # inside tool execution (bound providers)
↓
Summarize Execution                   # tool-backed only
↓
Respond                               # ContextBundle always attached
```

### Execution Summary

**Status: Current** — structured summaries for completed / partial / failed / cancelled plans

Every completed (or gated terminal) plan produces an [`ExecutionSummary`] with:

* Goal
* Actions performed
* Resources changed
* Files edited
* Duration
* Warnings
* Errors
* Next suggested actions

The Planner builds summaries from the plan plus tool [`ToolExecutionMetadata`].
Summaries appear naturally in the conversation transcript and are stored through
the Memory Engine (`kind = execution_summary`) for later retrieval.

### Approval History

**Status: Current** — searchable record of Review Card decisions

Every Approve / Modify / Cancel is recorded with:

* Execution Plan ID
* Timestamp
* User decision
* Reason (optional — Modify note / cancel explanation)
* Modified plan (child id after Modify)
* Execution result (when Approve runs tools, or cancel outcome)

History is kept in-session on the Planner and persisted to Memory
(`kind = approval_history`) for transparency, future retrieval, Planner
reasoning, and Coding Diagnostics. Search via
`Application::search_approval_history` / `Planner::search_approval_history`.

Sensitive fields (reasons, goals, resource paths, edited files) are **redacted**
when access is Restricted — Context / external exports must not expose them
outside the user's permission boundary. Local UI uses Full access.

Coding / Developer **Diagnostics** surfaces the live pause store, review state,
risk, plan permissions, pending/completed approvals, execution summaries, and
approval history, including explicit pause/resume explanations for developers.

Do not treat older “Retrieve Memory → Select Capabilities” diagrams as Current — Memory/Project/Search contribute only through Context Providers during assemble.

`InvokeProviders` is not a second Planner hop: tools call their bound providers while `ExecuteTool` runs.

No user-facing retrieval bypasses `Planner::handle`. Pipeline **variants** after assemble are intentional — not Application→Engine shortcuts.

User-facing retrieval always enters `Planner::handle`, including:

* Inventory search (Search Engine via tools)
* Project knowledge search (Project Engine, mediated by the Planner)
* List / read / discover / index
* Continue / close project

Administrative Memory and Project CRUD may resolve owning engines directly (see Architectural Integrity Slice 3). That is not a request bypass.

⸻

Intent

Every interaction begins with a goal.

### Current intents (examples)

Continue working on Jaymi.
↓
Resume Project

list / read / search / discover / index
↓
Tool-backed paths through Action Policy → Permission → Tool

search project knowledge (structured → Intent → Capability → Context → Action Policy → Permission → `search_project_knowledge` tool)
↓
Project Engine (mediated by Planner)

capability planning (“help me code …”)
↓
Execution plan without tools

### Target intent examples

Generate a logo.
↓
Create Image

Find Heather's Canva login.
↓
Search Personal Knowledge

The Planner cares about goals, not wording.

⸻

Context

Context is assembled intentionally through the Context Engine (`assemble`).

**Current:** Only conversational / unknown requests invoke the Reasoning Engine (after Context assemble). Tool, PlanWork, Review, and execution paths do not.

Possible sources include:

### Current

* Active project
* Previous conversation / conversation-scoped memory
* Project memory
* Personal memory
* Search coordination hints
* Active workspace session state

### Target

* Live search result dumps as context (tools still execute search)
* Git status, terminal output, notes, messages, browser history as first-class feeds

Only relevant information should be retrieved.

⸻

Planning

The Planner converts user intent into executable tasks.

### Target planning example

User
↓
Find the ChatGPT images I downloaded yesterday.
↓
Execution Plan
Search Downloads
↓
Filter Yesterday
↓
Identify Images
↓
Analyze Metadata
↓
Generate Results

### Current planning

Tool-backed intents create a Planner-owned **Execution Plan** after Context
assemble. Routing from Intent to Tool uses a compile-time **`ToolRouteTable`**
(`IntentToolHandler` registration) — not a growing `handle` match arm per tool.
The plan is immutable in content and progresses through Draft →
Ready → (AwaitingReview) → Approved → Executing → Completed / Failed /
Cancelled. Tools run only from an Approved plan; providers never see plans.

When review is required, the Planner **pauses**: the Execution Plan enters
AwaitingReview, the exact tool input is retained, and the conversation stays
active. There is exactly one approval implementation
(`Application::submit_review` → `Planner::resolve_review`). Entry points
(Conversation, Coding Workspace, Git, Terminal, Explorer, future Image /
Research) may differ in Review UI, but approval semantics never do: every path
emits `ReviewIntent` and tools run only from an Approved plan.

Conversation Review Cards are conversational (opening + Plan bullets + approval
notice + Approve / Cancel / Modify with example phrases) and emit intents only —
they never execute. Coding gestures (Save, Delete, Run, Git Commit, LSP rename)
may auto-submit `ReviewIntent::Approve` after an explicit user action; the
Planner still receives Approve and resumes the same paused plan.

Filesystem deletes record a **Deletion Method** on the Execution Plan (`trash`
or `permanent`). Review Cards explain Trash recovery vs permanent delete.
Execution Summaries record files moved, permanent deletes, and recovery
availability. Diagnostics show **Deletion strategy**.

Mutating tools attach a structured **Preview** (unified diff, rename/move
paths, git impact, LSP edits). The Planner stores it on the Execution Plan;
Review Cards embed it and truncate large bodies with expand.

- **Approve** resumes the same plan (no replan) and executes tools.
- **Modify** regenerates only affected steps into a **child** plan (new id,
  lineage + revision history). The parent is cancelled; the child is re-paused
  for approval. ContextBundle is **not** rebuilt unless the modification
  requires it (Planner requests `ContextEngine::empty_bundle()` instead of
  constructing a bundle). Review Cards show the revision diff.
- **Cancel** cancels the pause without executing.
- A **new user request** (any `handle`) also invalidates paused plans.
- **Timeout** and **duplicate approval** fail deterministically without double-executing.

Capability **PlanWork** intents produce a separate **Capability Plan**
(composition of capabilities, tools, providers, and permissions) without
running tools. Planned steps may appear so the plan stays honest, but only
executable-tier steps make a capability plan ready.

Execution plans should remain simple, deterministic, and explainable.

⸻

Capabilities

Capabilities describe what Jaymi can accomplish — the **conceptual catalog** stays registered even when fulfillment is not ready yet.

Availability distinguishes conceptual support from executable support. See [capabilities.md](capabilities.md).

Examples include:

* Chat (Planned)
* Search (Ready)
* Code (Experimental catalog; Experimental effective at boot — terminal / git / language_server fulfill inventory)
* Vision / Embeddings (Experimental catalog; Vision Unavailable without vision tools; Embeddings Experimental via `embedding.local`)
* OCR (Planned — placeholder provider; not executable)
* Generate Images / Browse Internet / Automate Tasks / Internet / Automation (Planned)
* File Management / Execute Terminal Commands (Ready)
* Read Documents / Discover (Ready)
* Index (Ready catalog; Unavailable on discover until a provider ads Index — `scan_filesystem` still registers as the Index tool)

Capabilities are stable.

They should not depend on specific tools.

⸻

Tool Selection

Every capability may be fulfilled by one or more tools.

Example:

Search

↓

Search Engine (`search_knowledge`) — single retrieval index for Quick Open, Find in Files, Search Files, and Search Knowledge

↓

Filesystem / list tools (directory browse, project tree)

↓

Messages / Documents / Photos tools (as available)

UI never queries the Search Engine directly — Application → Planner → Capability::Search.

Tools may change over time.

Capabilities should not.

⸻

Execution

Execution is managed step by step.

The Planner tracks:

* Progress
* Dependencies
* Success
* Failure
* Recovery

If execution fails, the Planner may:

* Retry
* Select another tool
* Request clarification
* Continue with partial results
* Explain the failure

Execution should always remain predictable.

⸻

Permissions

The Planner determines whether approval is required before execution.

Typical approval points include:

* Editing files
* Running terminal commands
* Sending messages
* Installing software
* Modifying repositories
* Deleting information

Approval requests should explain:

* What will happen
* Why it is needed
* Expected outcome

The user always makes the final decision.

⸻

Memory Integration

**Current:** Context Engine retrieves relevant memories and promotion suggestions for every `handle` into the `ContextBundle`. Read them via `PlannerResponse.memory()` / `promotion_suggestions()` / `promotion_ask()` — never from parallel response fields. Suggestions are never auto-applied.

**Target:** Richer intent-driven “should this be remembered?” flows and conversational promotion UX.

The Planner (via Context) determines:

* Which memories are relevant
* Whether the user should be asked about promotions
* Whether information belongs to conversation, project, or personal memory (when storing intentionally)

Memory should always be intentional.

⸻

Explainability

Every decision made by the Planner should be explainable.

Jaymi should always be able to answer questions such as:

* Why did you search there?
* Why did you use this tool?
* Why did you ask for permission?
* Why did you retrieve this memory?
* Why was internet access required?

Transparency is required for trust.

⸻

Design Principles

The Planner should always:

* Prefer deterministic logic over AI when possible.
* Retrieve before reasoning.
* Build context before thinking.
* Prefer local execution.
* Ask before meaningful actions.
* Keep execution understandable.
* Separate orchestration from intelligence.

⸻

Long-Term Vision

The Planner is designed to remain stable for the lifetime of the project.

Language models will improve.

Providers will evolve.

Tools will change.

Capabilities will grow.

The Planner should not.

It remains the orchestration kernel that connects every part of Jaymi into a single intelligent system.
