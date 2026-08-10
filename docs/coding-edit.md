# Coding Edit Intent (Sprint C3.1)

## Goal

Begin Chapter IV mutation by teaching Jaymi to **recognize and structure**
coding modification requests — without editing files.

C2 Codebase Reading remains closed. This sprint does not reopen or redesign C2.

## Behavior

| User says | Jaymi does |
|-----------|------------|
| “Fix this.” (with selection/symbol) | Classifies as mutation; builds `CodingEditObjective` (`Understood`) |
| “Fix this.” (no context) | Classifies as mutation; asks targeted clarification |
| “Explain this.” / “Where is…?” / “Review…” | Stays read-only (C1/C2 paths) |

Adequately specified requests surface:

> I understand what you want to change (planning only — no files modified).

Ambiguous requests ask for file / selection / symbol — **never invent a path**.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Sole classification + soft path (`complete_coding_edit_intent`) |
| **Environmental Resolution** | Deixis (“this”) binding from WI |
| **Workspace Intelligence** | Observed file / symbol / selection / references |
| **C2 modules** | Unchanged; ambient WI reused; soft-path order keeps reads first |
| **ContextEngine** | Sole ContextBundle factory (assemble before objective) |
| **ReasoningProvider** | Not invoked for C3.1 intent structuring |

## Soft-path order

After C2 reads and after C1.5 / C1.7; before review / coding_plan / understanding:

```
… → code_search → code_generation → multi_file → coding_edit (C3.1 + C3.2 + C3.3) → review → …
```

C1.7 keeps specialized rename-symbol / move / extract / split.
C1.4 yields in-place mutation verbs to C3.1 via `steals_from_coding_plan_detect`.
C3.2 Change Investigation and C3.3 Coding Plan run inside `complete_coding_edit_intent` (no new detect).

## Structured objective

`CodingEditObjective` carries (when available):

- user objective / requested change / kind
- target project / file / selection / symbols
- relevant files / context notes
- constraints / ambiguities / expected scope
- investigation notes (ambient WI)
- clarification question when needed

Not an Execution Plan. No `write_file`, `manage_path`, terminal/Git mutation, or LSP rename.

## Diagnostics

`LastPlannerActivity` + Coding Diagnostics expose `coding_edit_*` fields from
`to_diagnostics_summary()` — observational only; not dumped into chat as raw
internals (conversation gets the markdown objective / clarification).

## Constitutional lines

Planner Over Models · Conversation First · Intelligence Through Context ·
Projects First-Class · Review Before Action · Not Fully Autonomous · Offline First

## Out of scope (later C3)

- Generating patches / applying edits
- Executable Execution Plans for mutations
- C3.4+ apply / review slices

See also: [Change Investigation (C3.2)](change-investigation.md),
[Coding Plan from Investigation (C3.3)](change-coding-plan.md).
