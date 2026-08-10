# Pair Programming Experience (Sprint C1.8)

**Status: Current** — Make Jaymi feel like an engineering partner.

Conversation First remains primary. Suggested next actions are
**Planner-generated** from Planner state + Workspace Intelligence. Quick Actions
mirror that list — they are not hardcoded UI buttons.

## Natural transitions

```text
Understanding ──► Review ──► Planning ──► Generation
       ▲              │                        │
       │              ▼                        ▼
       └──── Modification ◄──────────── Execution
```

| Phase | Typical turn | Next suggestions (examples) |
|-------|--------------|-------------------------------|
| Understanding | Explain Architecture | Review Changes · Generate Tests · Refactor |
| Review | Review Changes | Generate Tests · Refactor · Explain Architecture |
| Planning | Generate Tests | Generate Code · Review Changes |
| Generation | Generate the code. | Review Card Approve / Modify / Cancel |
| Modification | Refactor / multi-file | Review Changes · Run Build |
| Execution | Run Build (reviewed) | Review Changes · Find Dead Code |
| Ready | Coding opens | Baseline partner actions |

## Examples

| Suggestion | Conversation text | Routes to |
|------------|-------------------|-----------|
| Explain Architecture | `Explain the architecture of this project.` | C1.2 Understanding |
| Review Changes | `Review my changes.` | C1.3 Review |
| Generate Tests | `Generate tests.` | C1.4 Coding Plan |
| Refactor | typed `RefactorSelection` | Conversational proposal (no silent edits) |
| Run Build | typed `RunProject` | Reviewed terminal Execution Plan |
| Find Dead Code | `Find dead code.` | Soft Understanding orientation |

## Ownership

```text
Planner state + WI
        │
        ▼
 synthesize PairProgrammingSurface     ★ Planner owns the list
        │  phase + SuggestedAction[]
        ▼
 PlannerResponse.pair_programming
        │
        ▼
 Experience mirrors (never invents)
        │
        ▼
 Quick Action menu renders list
        │  click → Conversation First turn
        ▼
 Existing C1.x detectors / Coding Actions
```

| Concern | Owner |
|---------|--------|
| Suggested action list | **Planner** (`pair_programming`) |
| Phase inference | **Planner** |
| Mirror / display | Experience + UI |
| Routing | Existing Decision Engine / C1.x detect |
| Mutations | Execution Plan + Review (unchanged) |

## Constraints

| Forbidden | Required |
|-----------|----------|
| Hardcoded UI action catalog as source of truth | Planner-authored `SuggestedAction[]` |
| UI inventing next steps | Experience mirrors `PlannerResponse` |
| Direct editor / tool / provider from Quick Actions | Conversation First turns only |
| Skipping Review for mutating paths | C1.5 / C1.6 / C1.7 gates unchanged |
| New Intent / Capability ownership | Unchanged |

## Pipeline

```text
Coding Workspace opens
        │
        ▼
 Planner baseline_surface()            ★ still Planner-authored
        │
User turn (chat or Quick Action)
        │
        ▼
 C1.x mode (Understanding / Review / …)
        │
        ▼
 finalize_response → synthesize next actions
        │
        ▼
 Experience + Quick Action menu update
```

## API surface

| Entry | Behavior |
|-------|----------|
| `PlannerResponse.pair_programming` | Phase + suggested actions for this turn |
| `Planner::pair_programming_suggestions()` | Current / baseline list for Coding chrome |
| `ExperienceSession::suggested_actions()` | Mirrored list for UI |
| Quick Action `···` menu | Renders Planner list (baseline fallback) |

## Related

- [coding-actions.md](coding-actions.md) — C0.1 typed Coding Actions
- [coding-understanding.md](coding-understanding.md) — Understanding
- [coding-review.md](coding-review.md) — Review
- [coding-plan.md](coding-plan.md) — Planning
- [code-generation.md](code-generation.md) — Generation
- [coding-execution-plans.md](coding-execution-plans.md) — Review Card
- [multi-file-intelligence.md](multi-file-intelligence.md) — Modification
- [experience.md](experience.md) — Conversation First UX

## Constitutional audit (C1.8)

### Ownership

```text
Partner suggestions
        │
        ▼
 Planner synthesizes from state + WI     ★ not UI / not LLM inventing paths
        │
        ▼
 Conversation First turn
        │
        ▼
 Existing C1.x / tools / Review          ★ no bypass
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Suggested next actions | Planner | Pass |
| Quick Actions source of truth | Planner surface (UI renders) | Pass |
| Conversation First primary | Yes | Pass |
| Intent / Capability ownership | Unchanged | Pass |
| Mutation Review gate | Unchanged | Pass |
| Hardcoded UI catalog | Fallback only via Planner `baseline_actions` | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Engineering partner, transparent | Pass |
| PRINCIPLES | Conversation First; local; reviewed mutations | Pass |
| NON_GOALS | Not IDE toolbar sprawl / silent agents | Pass |
| ARCHITECTURE | Planner orchestrates; Experience presents | Pass |
| ROADMAP | C1.8 Pair Programming Experience | Pass — Current |

### Residuals

* Baseline actions are Planner-authored constants, not UI-owned — still the
  starting partner menu until scaffolds enrich the list.
* Find Dead Code is an honest orientation pass until richer WI unused-symbol
  observations exist; it does not scan the filesystem outside Context assemble.
* Understanding still does not auto-gate Edit/Refactor into Execution Plans —
  suggestions recommend Review / Plan first; mutations remain Review-gated.
