# Change Investigation (Sprint C3.2)

## Goal

After a Planner-owned [`CodingEditObjective`](coding-edit.md) (C3.1), investigate
what the requested mutation would **touch** — before any coding plan or patch.

C2 Codebase Reading remains closed. This sprint **reuses** C2 helpers; it does
not create a second investigation system.

## Pipeline

```
Coding Edit Intent (C3.1)
  → C2-mediated investigation (search / read / LSP / relationship ambient)
  → identify relevant files / symbols
  → inspect relationships
  → determine affected code
  → Change Investigation report
  → (later) C3.3 Coding Plan
```

Example: “Add image generation to Settings.” → investigate Settings architecture,
providers, capability registration, UI, config, tests, docs — then report findings.

## Ownership

| Component | Role |
|-----------|------|
| **Planner** | Sole orchestration (`run_change_investigation` inside `complete_coding_edit_intent`) |
| **C2 deep / relationship / symbol / search** | Reused helpers + caps — no parallel detect soft path |
| **ContextEngine** | Sole ContextBundle factory (re-assemble after search inject) |
| **Workspace Intelligence** | Observational seeds (file / symbol / references / deps) |
| **ReasoningProvider** | Not invoked for C3.2 investigation |

No new soft-path detect. Investigation runs only after C3.1 classification on the
existing `coding_edit` soft path.

## Modes

| Mode | When |
|------|------|
| **Full** | `CodingEditStatus::Understood` — Planner-mediated Search / read / optional LSP refs + ambient relationship seed |
| **Light** | `NeedsClarification` (or insufficient target) — ambient WI + explicit unknowns only |

## Report contents

Where evidence supports it, `ChangeInvestigationReport` records:

- affected files / symbols
- related dependencies
- implementation patterns
- tests / configuration that may be affected
- likely side effects
- architectural constraints
- existing related functionality
- evidence rows, searches, steps
- **explicit unknowns** when evidence is insufficient

Bounded by existing C2 limits (`MAX_STEPS`, `MAX_FILES`, `SEARCH_HIT_LIMIT`, …).

## Hard lines

1. Reuse C2 — do not redesign reading.
2. No second investigation system / soft-path detect.
3. Planner owns orchestration and tool mediation.
4. ContextEngine remains the sole ContextBundle factory.
5. Workspace Intelligence remains observational.
6. Investigation is **read-only**.
7. Do not modify files.
8. Do not execute mutation tools (`write_file`, `manage_path`, mutating LSP, …).
9. Do not generate final patches.
10. Do not produce an executable Execution Plan for the mutation.
11. Insufficient evidence → explicit unknowns.
12. Stay within C2 budgets.
13. Reuse existing diagnostics / investigation observability patterns.

## Diagnostics

`LastPlannerActivity` + Coding Diagnostics expose `change_investigation_*`
fields from `to_diagnostics_summary()` — observational only.

## Soft-path order

Unchanged detect order; C3.2 runs **inside** the C3.1 complete path:

```
… → coding_edit (C3.1 + C3.2 + C3.3) → review → …
```

## Out of scope (later C3)

- C3.3 Coding Plan — see [change-coding-plan.md](change-coding-plan.md)
- Patch generation / apply
- Mutation Execution Plans

## Constitutional audit

| Line | Status |
|------|--------|
| Planner Over Models | **PASS** — Planner orchestrates; no Reasoning for investigation |
| Conversation First | **PASS** — markdown report in conversation; diagnostics observational |
| Intelligence Through Context | **PASS** — ContextEngine assemble / re-assemble |
| Projects First-Class | **PASS** — project / WI ambient; missing target → unknowns / light |
| Review Before Action | **PASS** — no mutation; review deferred |
| Not Fully Autonomous | **PASS** — bounded, no writes, clarification path |
| Offline First | **PASS** — local search / read / LSP only |
| C2 reuse (no second system) | **PASS** — deep / relationship / symbol helpers |
| Read-only investigation | **PASS** — no write / manage_path / mutating LSP |
| Explicit unknowns | **PASS** — gaps + ambiguities surfaced |
| Bounded (C2 caps) | **PASS** — shared constants |
| Hands off to C3.3 | **PASS** — plan authored after investigation; no patch leak |
