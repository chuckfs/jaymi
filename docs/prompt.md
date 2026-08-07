# Prompt Builder

**Status: Partial** — Sprint B1.2 + B1.8 (model-aware budgeting) + **B1.13.1**
(Prompt → Provider handoff) + **B1.13.2** (ContextBundle section coverage) +
**B1.13.5** (diagnostics inspect delivered prompt).

Prompt construction is a first-class subsystem in `jaymi-reasoning`.

Providers never assemble prompts. The Planner never concatenates prompt strings.
The Reasoning Engine delegates construction to [`PromptBuilder`] and **attaches**
the assembled [`Prompt`] onto [`ReasoningRequest`] before every provider call.

## Pipeline

```text
ContextBundle
  → LlmContext
  → PromptBuilder
  → Prompt
  → ReasoningRequest.prompt
  → ReasoningProvider (transport only)
  → Model
```

Full reasoning path:

```text
LlmContext → PromptBuilder → ReasoningRequest(prompt) → ReasoningProvider
  → StreamingResponse → ReasoningResponse
```

`ReasoningEngine` orchestrates that path: adapts the prompt budget from the
selected model's limits, builds the prompt, attaches it to the request, then
invokes complete / stream.

## Assembled sections

Default emission order (`PromptSectionId::ORDER`):

1. System Instructions
2. Conversation
3. Relevant Memories
4. Active Project
5. Workspace State (workspace kind + open files + git / inventory / summaries fold)
6. Current File
7. Selection
8. Editor Intelligence
9. Project Intelligence
10. Runtime Intelligence
11. Workspace Memory
12. Environmental Resolution (Planner deixis bindings)
13. Coding Understanding (Planner WI scaffold + fill rules — Sprint C1.1 / C1.2 Project Understanding)
14. Coding Review (Planner WI scaffold — Sprint C1.3)
15. Coding Plan (Planner WI scaffold — Sprint C1.4 Generation Planning)
16. Search Results
17. Diagnostics
18. Permissions
19. Capabilities
20. Planner Metadata
21. User Request

Retention priority (higher kept longer under budget pressure): System → User Request
→ Coding Plan → Coding Review → Coding Understanding → Environmental Resolution → Conversation → …

### LlmContext → Prompt mapping (B1.13.2)

| `LlmSectionId` / source | Prompt section | Notes |
|-------------------------|----------------|-------|
| `user_request` | User Request | Plus request `goal` |
| `conversation` | Conversation | Plus `ReasoningRequest.history` turns (B1.13.3 multi-turn) |
| `active_project` | Active Project | |
| `active_workspace` | Workspace State | Folded with open files |
| `open_files` | Workspace State | Folded with workspace kind |
| `current_file` | Current File | |
| `current_selection` | Selection | |
| `editor_intelligence` | Editor Intelligence | |
| `project_intelligence` | Project Intelligence | |
| `runtime_intelligence` | Runtime Intelligence | |
| `workspace_memory` | Workspace Memory | |
| `search_results` | Search Results | |
| `memory_results` | Relevant Memories | |
| `diagnostics` | Diagnostics | |
| `permissions` | Permissions | |
| `active_capabilities` | Capabilities | |
| *(engine)* | System Instructions | Template / override — not from bundle |
| *(providers.environmental)* | Environmental Resolution | Planner bindings (B2.10) |
| *(extensions.coding_understanding)* | Coding Understanding | Planner scaffold (C1.1 / C1.2) |
| *(extensions.coding_review)* | Coding Review | Planner scaffold (C1.3) |
| *(extensions.coding_plan)* | Coding Plan | Planner scaffold (C1.4) |
| *(providers meta)* | Planner Metadata | From `LlmContext.providers` |

Nothing disappears silently. Absent Llm sections are **Excluded** with a note.
Budget pressure uses **Budgeted** (omit) or **Truncated** (shorten). Formatter
drops of present-but-empty payloads are **Filtered**.

## Delivery to providers

Chat-oriented backends receive PromptBuilder output via
`Prompt::to_chat_messages()`:

* Non-`UserRequest` sections → system message (PromptBuilder formatting)
* `UserRequest` body → user message

Providers map those roles onto their wire format only. They must not read
`goal`, `history`, or `LlmContext` to invent parallel prompt content.

## Model-aware budgeting (B1.8)

```text
prompt_tokens = context_window − reserved_completion
```

| Input | Source |
|-------|--------|
| Context window | `ReasoningProvider::model_limits` / `ReasoningModelInfo.context_tokens` |
| Reserved completion | `GenerationParameters.max_output_tokens`, else model `max_output_tokens`, else 1024 |
| Long-context models | Larger `context_tokens` scales the prompt ceiling automatically |

`PromptBudget::from_model_limits` derives the ceiling. When the window is unknown,
PromptBuilder falls back to the default character budget while still recording the
reservation for diagnostics.

Context Engine budgeting still pre-fits the **bundle**; PromptBuilder is the
**second** fit against the model window. Context Policies (`priority`,
`can_truncate`) remain the assemble-time controls.

## Capabilities

| Concern | Support |
|---------|---------|
| Token / character budgeting | `PromptBudget` + fit/omit/truncate |
| Context window + reserved completion | `from_model_limits` / engine auto-adapt |
| Priority sections | `retention_priority` omit-then-truncate |
| Dynamic truncation | Deterministic `apply_budget` |
| Future long-context models | Scales with `context_tokens` |
| Section ordering | Template order or `with_section_order` |
| Provider-independent formatting | `PlainTextFormatter` |
| Provider delivery | `ReasoningRequest.prompt` + `to_chat_messages` |
| Explicit section dispositions | included / excluded / truncated / filtered / budgeted |

## Diagnostics

Every [`Prompt`] carries [`PromptDiagnostics`]. After assemble, PromptBuilder
**seals** diagnostics against `Prompt::to_chat_messages()` (Sprint **B1.13.5**):

* Prompt size (delivered chat message characters)
* Prompt sections (included contributions use delivery framing)
* Truncated sections / excluded sections (disposition lists; unused bodies stay at 0 chars)
* Budget usage (used size matches delivered prompt)
* Conversation turns (prior history folded into Conversation)
* Final token estimate (`final_token_estimate` ≡ delivered token estimate)
* Full `LlmSectionId` coverage (`llm_coverage`)
* Disposition summary (`included=… · excluded=… · truncated=… · filtered=… · budgeted=…`)

Diagnostics never describe unused prompt content as sent: omitted sections remain
listed only for fate transparency with `characters: 0`, and size fields never
count flat `Prompt::text` when delivery framing differs.

Planner / ConversationStream retain diagnostics from the Prompt **attached** at
provider start — not a discarded pre-stream build.

## Engine delegation

```rust
let streaming = reasoning_engine.stream(request)?; // builds + attaches Prompt
let response = streaming.collect()?;
```

## Not in this slice

* PromptBuilder still does not call models
* No UI
* Providers expose limits and generate — they never build prompts
