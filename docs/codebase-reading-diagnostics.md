# Codebase Reading Diagnostics Parity (Sprint C2.7.1)

Extends existing Coding Diagnostics / `LastPlannerActivity` so C2.4 Deep
Investigation and C2.5 Codebase Q&A match the observability already provided for
C2.1–C2.3.

## Ownership

| Concern | Owner |
|---------|--------|
| Produce operational report fields | Planner (`DeepInvestigationReport` / `CodebaseQaReport`) |
| Record into activity cache | Application `record_planner_activity` |
| Render observational lines | Coding Diagnostics (`codebase_reading_activity_lines`) |
| Conversation transcript | Unchanged — findings/gaps only; no raw diagnostics dump |

No second diagnostics system. No new UI workspace. Diagnostics never influence
Planner decisions or execute tools.

## Fields exposed

**Deep Investigation (C2.4):** summary, steps, searches, files, symbols, evidence,
context sections (assemble/re-assemble), stop status, elapsed ms, budget line.

**Codebase Q&A (C2.5):** summary, complexity, route, searches, files, symbols,
evidence, conflicts, gaps, investigation_needed.

**Preserved:** C2.1 code search, C2.2 symbol navigation, C2.3 relationship trace.

## Constitutional audit

| Rule | Status |
|------|--------|
| Extend existing LastPlannerActivity / Coding Diagnostics | PASS |
| No second diagnostics system / no new workspace | PASS |
| Not in normal conversation transcript | PASS |
| Observational only (no Planner influence / no tools) | PASS |
| Preserve C2.1–C2.5 behavior | PASS |
| C2.1–C2.3 diagnostics intact | PASS |
| Tests | PASS |

**Verdict: PASS**

### Remaining observability limitations

- Per-step chain detail for deep investigation is summarized (budget + counts),
  not a full step-by-step dump in the panel (full chain remains on the report /
  logs via `chain_diagnostics_lines`).
- Codebase Q&A does not separately list selected ContextBundle section ids
  (those live on deep investigation when that route ran).
