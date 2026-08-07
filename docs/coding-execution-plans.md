# Coding Execution Plans (Sprint C1.6)

**Status: Current** — Review Before Action for coding workflows uses the shared
Execution Plan + Review Card architecture.

## Example

```text
Rename variable
        │
        ▼
Execution Plan
Files affected
Diff Preview
Risk
Approve · Modify · Cancel
```

Save, Explorer rename, LSP rename, write/delete, and Code Generation (C1.5) all
pause the same way. There is **no** coding-only approval path and **no** editor
bypass.

## Surface (universal Review Card)

| Section | Source |
|---------|--------|
| **Execution Plan** | Plan steps / conversational bullets |
| **Files affected** | `ExecutionPlan.affected_resources` ∪ `ActionPreview.resources` |
| **Diff Preview** | `ActionPreview` (`UnifiedDiff` / `LspWorkspaceEdit`); other kinds stay **Preview** |
| **Risk** | `EstimatedRisk` (+ deletion method when relevant) |
| **Approve / Modify / Cancel** | `ReviewIntent` only → `Planner::resolve_review` |

## Constraints

| Forbidden | Required |
|-----------|----------|
| Special coding approval bypass | Existing `prepare_execution` → pause → Review Card |
| Editor / Monaco applying mutations directly | Tools only after Approved plan |
| Modal-only coding review | In-conversation Review Card |
| Skipping Review Before Action | Universal for mutating tools |

## Pipeline

```text
Coding mutation (rename / write / generate / …)
        │
        ▼
 Planner prepare_execution                 ★ same gate as every tool mutation
        │  ExecutionPlan + ActionPreview
        │  affected_resources ← preview.resources (C1.6)
        ▼
 Pause + ReviewCardModel::from_plan
        │  Execution Plan · Files affected · Diff Preview · Risk
        ▼
 ReviewIntent::Approve | Modify | Cancel
        │
        ▼
 resolve_review → tools (Approve only)
```

## Ownership

| Concern | Owner |
|---------|--------|
| Plan + pause + resume | **Planner** |
| Preview body | Tool `preview()` → Planner attaches |
| Card text / UI | Review Card model + Experience UI |
| Mutations | Tools after Approve |
| Editor / Monaco | Never applies workspace edits itself |

## Related

- [code-generation.md](code-generation.md) — C1.5 ops → reviewed plans
- [planner.md](planner.md) — Execution Plans / Review Cards
- [experience.md](experience.md) — in-conversation card
- [coding-actions.md](coding-actions.md) — toolbar intents (still no direct edits)

## Constitutional audit (C1.6)

### Ownership

```text
Rename variable / Save / Generate / …
        │
        ▼
 Existing Execution Plan architecture     ★ no special coding plan type
        │
        ▼
 Review Card (Files · Diff · Risk · A/M/C) ★ presentation only
        │
        ▼
 resolve_review → Tool → Provider         ★ editor never bypasses
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Mutation gate | Execution Plan + Review | Pass |
| Coding-specific bypass | None | Pass |
| Editor bypass | None | Pass |
| Review Before Action | Universal | Pass |
| Files / Diff / Risk surface | Shared Review Card | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Safe, transparent changes | Pass — richer review surface |
| PRINCIPLES | Local, reviewed mutations | Pass |
| NON_GOALS | Not silent tool sprawl | Pass — same pause path |
| ARCHITECTURE | Planner orchestrates | Pass |
| ROADMAP | C1.6 Coding Execution Plans | Pass — Current |

### Residuals

* Gesture flows may auto-submit Approve after an explicit user action; still `ReviewIntent` on the same plan.
* Path/git previews keep the **Preview** label; text/LSP edits use **Diff Preview**.
* Conversational “Rename variable” without an LSP/tool request still needs editor or structured rename input — free text alone does not invent a bypass.
