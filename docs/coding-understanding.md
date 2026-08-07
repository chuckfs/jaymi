# Coding Understanding (Sprint C1.1)

**Status: Current** — Understand Before Acting.

Before proposing edits, Jaymi returns structured understanding of the current
coding context. Examples:

* `"Explain this function."`
* `"What is this file responsible for?"`
* `"How does this project work?"`
* `"Why is this compiler error happening?"`
* Quick Action **Explain** (`ExplainSelection` / `ExplainFile`)

## Constraints

| Forbidden | Required |
|-----------|----------|
| New context systems | Workspace Intelligence already on `ContextBundle` |
| Provider bypasses | ContextEngine remains sole `ContextBundle` factory |
| Filesystem scans | Observation only from assembled bundle |
| Tool execution | No tools / Execution Plans for understanding turns |
| Edits | No applied mutations |

## Pipeline

```text
UserRequest (Explain* / free-text understanding)
    │
    ▼
Intent → Capability → Complexity → Environmental Resolution
    │
    ▼
Coding Understanding detect          ★ Sprint C1.1
    │  AssembleHints.understanding
    ▼
Context Engine assemble_with
    │  notes: coding_understanding=…
    ▼
scaffold_from_bundle (WI only)
    │
    ├─ no Reasoning backend → structured markdown scaffold
    │
    └─ Reasoning path
         LlmContext.extensions["coding_understanding"]
              │
              ▼
         Prompt section: Coding Understanding
              │
              ▼
         Model elaborates the six sections (still no tools / edits)
```

## Structured response

Every understanding turn surfaces:

1. **Purpose**
2. **Responsibilities**
3. **Key Symbols**
4. **Relationships**
5. **Potential Issues**
6. **Suggested Next Actions**

`PlannerResponse.coding_understanding` carries the WI scaffold. Conversation
`content` is either the scaffold markdown (soft path) or the model elaboration
instructed to keep the same headings.

## Ownership

| Concern | Owner |
|---------|--------|
| Detect understanding mode | **Planner** (`coding_understanding::detect_understanding_request`) |
| WI scaffold from bundle | **Planner** (`scaffold_from_bundle`) — observation only |
| Assemble / stamp notes | Context Engine (`AssembleHints.understanding`) |
| Prompt section | PromptBuilder (`CodingUnderstanding`) |
| Elaborate sections | Reasoning Engine (optional) |
| Tools / edits / FS scans | **Forbidden** on this path |

Planner ownership is unchanged: Intent and Capability selection are not altered
by understanding mode. Understanding only annotates hints and instructs Reasoning.

## Focus

| Focus | Triggers (examples) |
|-------|---------------------|
| Selection | `ExplainSelection`, “explain this function” |
| File | `ExplainFile`, “what is this file responsible for?” |
| Project | “how does this project work?” / C1.2 angles (overview, architecture, modules, feature placement) |
| Diagnostic | “why is this compiler error happening?” |

Project focus is deepened by Sprint **C1.2** — see
[project-understanding.md](project-understanding.md) for Project Understanding
headings and constitutional audit.

## Related

- [project-understanding.md](project-understanding.md) — whole-project orientation (C1.2)
- [coding-actions.md](coding-actions.md) — Explain Quick Action → typed intents
- [environmental-resolution.md](environmental-resolution.md) — deixis binding
- [workspace-intelligence.md](workspace-intelligence.md) — WI sources
- [planner.md](planner.md) — orchestration
- [prompt.md](prompt.md) — Coding Understanding prompt section

## Constitutional audit (C1.1)

### Ownership

```text
UserRequest / CodingAction::Explain*
        │
        ▼
 Planner detect_understanding_request     ★ Planner owns mode detection
        │  AssembleHints.understanding
        ▼
 ContextEngine assemble_with              ★ sole ContextBundle factory
        │  notes stamp only
        ▼
 Planner scaffold_from_bundle             ★ observation from bundle only
        │
        ├─ soft: markdown scaffold (no Reasoning)
        │
        └─ LlmContext.extension → PromptBuilder → Reasoning
             (elaborates; never tools / edits / FS)
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Mode detect / scaffold / instruct | Planner | Pass |
| ContextBundle factory | ContextEngine | Pass — no parallel context system |
| WI observation | Ambient ContextMaintenance → session | Pass — reused as-is |
| Provider contribute | Context Providers via assemble | Pass — no bypass |
| Tools / Execution Plans | Not invoked for understanding | Pass |
| Edits / filesystem scans | Not performed | Pass |
| Intent / Capability ownership | Unchanged Decision Engine | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Conversational understanding before acting; computer bridges context gap | Pass — structured understanding from live WI |
| PRINCIPLES | Offline-first, privacy, transparency, local orchestration | Pass — no cloud requirement; scaffold works without LLM; sections visible |
| NON_GOALS | Not an LLM product; not OS; no silent tool sprawl | Pass — uses existing Reasoning provider path; no new engines |
| ARCHITECTURE | Planner orchestrates; ContextEngine assembles; tools behind plans | Pass |
| ROADMAP | Chapter IV / C1.1 Understand Before Acting | Pass — shipped as Current |

### Residuals

* Model elaboration quality depends on the wired Reasoning provider; without one, the WI scaffold markdown is the honest response.
* Understanding does not yet auto-gate Edit/Refactor proposals (future Pair Programming sprints may require understanding before mutation plans).
* Free-text heuristics are deterministic first-match; ambiguous questions may fall through to ordinary conversation.