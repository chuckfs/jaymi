# Coding Plan / Generation Planning (Sprint C1.4)

**Status: Current** — Structured generation planning without code generation.

Examples:

* `"Build Pong."`
* `"Create a parser."`
* `"Write tests."`

## Constraints

| Forbidden | Required |
|-----------|----------|
| Code generation | Planning only |
| Tool / command execution | Conversational Coding Plan path |
| File writes | Proposal paths only |
| Execution Plans | Coding Plan ≠ ExecutionPlan |
| New context systems | Assembled ContextBundle / WI |
| Provider bypasses / FS scans | ContextEngine sole factory |

## Structured response

```text
## Coding Plan
### Plan
### Files to Create
### Files to Modify
### Dependencies
### Estimated Risk
### Summary
```

`PlannerResponse.coding_plan` carries the WI scaffold. Soft path (no Reasoning
backend) returns the structured markdown directly.

## Kind

| Kind | Triggers (examples) |
|------|---------------------|
| New project | “Build Pong.” / build … game / app / project |
| Feature | “Create a parser.” / implement … module / component |
| Tests | “Write tests.” / add unit tests |
| Generic | Other short build/create/implement generation asks |

Priority among observational modes: **review → coding plan → understanding**.

## Pipeline

```text
UserRequest ("Build Pong." / "Create a parser." / "Write tests.")
    │
    ▼
Intent → Capability → Complexity → Environmental Resolution
    │
    ▼
Coding Plan detect                 ★ Sprint C1.4 (after Review, before Understanding)
    │  AssembleHints.coding_plan
    ▼
ContextEngine assemble_with
    │  notes: coding_plan=…
    ▼
scaffold_from_bundle (WI only)
    │
    ├─ soft markdown (## Coding Plan)
    └─ LlmContext.extensions["coding_plan"]
         → Prompt section: Coding Plan
         → Reasoning elaborates (still no tools / writes / codegen / Execution Plans)
```

## Ownership

| Concern | Owner |
|---------|--------|
| Detect generation-plan mode / kind | **Planner** (`coding_plan::detect_coding_plan_request`) |
| WI scaffold | **Planner** (`scaffold_from_bundle`) |
| Assemble / stamp notes | Context Engine |
| Prompt section | PromptBuilder (`CodingPlan`) |
| Elaborate | Reasoning (optional) |
| Tools / writes / codegen / Execution Plans | **Forbidden** |

Planner Intent / Capability ownership is unchanged. Coding Plan only annotates
`AssembleHints` and instructs Reasoning.

## Related

- [coding-understanding.md](coding-understanding.md) — C1.1 understand before acting
- [project-understanding.md](project-understanding.md) — C1.2 project orientation
- [coding-review.md](coding-review.md) — C1.3 review only
- [workspace-intelligence.md](workspace-intelligence.md) — WI sources
- [planner.md](planner.md) — orchestration; Execution Plans are separate (tool-gated)
- [prompt.md](prompt.md) — Coding Plan prompt section
- [code-generation.md](code-generation.md) — C1.5 apply plan → reviewed writes

## Constitutional audit (C1.4)

### Ownership

```text
"Build Pong." / "Create a parser." / "Write tests."
        │
        ▼
 Planner detect_coding_plan_request       ★ owns mode (Intent unchanged)
        │  AssembleHints.coding_plan
        ▼
 ContextEngine assemble_with              ★ sole ContextBundle factory
        │
        ▼
 scaffold from project / file / git /
 languages / package manager / memory     ★ no FS / tools / writes / codegen
        │
        └─ ## Coding Plan (six sections)
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Mode detect / scaffold / instruct | Planner | Pass |
| ContextBundle factory | ContextEngine | Pass |
| WI observation | Assembled sections only | Pass |
| Tools / Execution Plans | Not invoked | Pass |
| Codegen / file writes | None | Pass |
| Intent / Capability | Unchanged | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Help users plan before mutation | Pass — plan before generate |
| PRINCIPLES | Local, transparent, offline soft path | Pass |
| NON_GOALS | Not silent tool sprawl / not an LLM product | Pass — planning-class path |
| ARCHITECTURE | Planner orchestrates; Context assembles | Pass |
| ROADMAP | C1.4 Generation Planning | Pass — Current |

### Residuals

* Plan quality depends on WI density (project languages, dirs, open files, git).
* Coding Plan proposes paths — it does not create files or run tools.
* Distinct from `Intent::PlanWork` / Execution Plans (capability composition / tool review).
* Apply generation via C1.5 (“Generate the code.”) after the plan is accepted.
