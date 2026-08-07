# Coding Review (Sprint C1.3)

**Status: Current** — Structured code review without edits or execution.

Examples:

* `"Review this file."`
* `"Review this function."`
* `"Review my changes."`

## Constraints

| Forbidden | Required |
|-----------|----------|
| File modifications | Observation only |
| Tool / command execution | Conversational review path |
| Execution Plans | Review responses only |
| New context systems | Assembled ContextBundle / WI |
| Provider bypasses / FS scans | ContextEngine sole factory |

## Structured response

```text
## Coding Review
### Strengths
### Weaknesses
### Potential Bugs
### Complexity
### Performance
### Maintainability
### Architecture
```

`PlannerResponse.coding_review` carries the WI scaffold. Soft path (no Reasoning
backend) returns the structured markdown directly.

## Focus

| Focus | Triggers (examples) |
|-------|---------------------|
| File | “Review this file.” |
| Function | “Review this function.” / selection |
| Changes | “Review my changes.” (GitSnapshot-derived dirty / staged paths) |

## Pipeline

```text
UserRequest ("review …")
    │
    ▼
Intent → Capability → Complexity → Environmental Resolution
    │
    ▼
Coding Review detect                 ★ Sprint C1.3 (before Understanding)
    │  AssembleHints.review
    ▼
ContextEngine assemble_with
    │  notes: coding_review=…
    ▼
scaffold_from_bundle (WI only)
    │
    ├─ soft markdown (## Coding Review)
    └─ LlmContext.extensions["coding_review"]
         → Prompt section: Coding Review
         → Reasoning elaborates (still no tools / edits / plans)
```

## Ownership

| Concern | Owner |
|---------|--------|
| Detect review mode / focus | **Planner** (`coding_review::detect_review_request`) |
| WI scaffold | **Planner** (`scaffold_from_bundle`) |
| Assemble / stamp notes | Context Engine |
| Prompt section | PromptBuilder (`CodingReview`) |
| Elaborate | Reasoning (optional) |
| Tools / edits / Execution Plans | **Forbidden** |

Planner Intent / Capability ownership is unchanged. Review only annotates
`AssembleHints` and instructs Reasoning.

## Related

- [coding-understanding.md](coding-understanding.md) — C1.1 understand before acting
- [project-understanding.md](project-understanding.md) — C1.2 project orientation
- [coding-plan.md](coding-plan.md) — C1.4 generation planning
- [workspace-intelligence.md](workspace-intelligence.md) — WI sources
- [planner.md](planner.md) — orchestration
- [prompt.md](prompt.md) — Coding Review prompt section

## Constitutional audit (C1.3)

### Ownership

```text
"Review this file / function / my changes"
        │
        ▼
 Planner detect_review_request            ★ owns mode (Intent unchanged)
        │  AssembleHints.review
        ▼
 ContextEngine assemble_with              ★ sole ContextBundle factory
        │
        ▼
 scaffold from file / selection / git /
 diagnostics / project / memory / runtime ★ no FS / tools / plans / edits
        │
        └─ ## Coding Review (seven sections)
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Mode detect / scaffold / instruct | Planner | Pass |
| ContextBundle factory | ContextEngine | Pass |
| WI observation | Assembled sections only | Pass |
| Tools / Execution Plans | Not invoked | Pass |
| Edits / execution | None | Pass |
| Intent / Capability | Unchanged | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Help users understand their work | Pass — review before mutation |
| PRINCIPLES | Local, transparent, offline soft path | Pass |
| NON_GOALS | Not silent tool sprawl / not an LLM product | Pass — understanding-class path |
| ARCHITECTURE | Planner orchestrates; Context assembles | Pass |
| ROADMAP | C1.3 Coding Review | Pass — Current |

### Residuals

* Review quality depends on WI density (selection text, diagnostics, git dirty paths).
* “Review my changes” reviews GitSnapshot samples — not a full diff dump or git tool execution.
* No Quick Action bar Review button yet (free-text / Conversation First).
