# Project Understanding (Sprint C1.2)

**Status: Current** — Understand entire software projects.

Teach Jaymi to orient on a whole project **before** any edits, tools, plans, or
execution. Examples:

* `"Explain this project."`
* `"What architecture does this use?"`
* `"Where should this feature live?"`
* `"What modules are most important?"`

## Constraints

| Forbidden | Required |
|-----------|----------|
| File modifications | Observation only |
| Tool execution | Conversational understanding path |
| Execution Plans / planning | Understanding responses only |
| New context systems | Assembled ContextBundle / WI sections |
| Provider bypasses / FS scans | ContextEngine remains sole factory |

## Sources (assembled only)

| Source | Consumed via |
|--------|----------------|
| WorkspaceSnapshot | `ActiveWorkspace` / open files / project root (bundle sections) |
| ProjectSnapshot | `ProjectIntelligence` (layout, languages, deps, members) |
| GitSnapshot | `GitStatus` (branch, dirty paths, recent commits) |
| Workspace Memory | `WorkspaceMemory` (objective, edits, opens, builds, failures) |
| Conversation | `Conversation` (title, message count, project binding) |

Planner never reads raw host snapshots directly and never walks the filesystem
for this sprint.

## Structured response

```text
## Project Understanding
### Overview
### Architecture
### Important Modules
### Relationships
### Activity & Risks
### Suggested Next Actions
```

Angles (Planner-owned):

| Angle | Triggers (examples) |
|-------|---------------------|
| Overview | “Explain this project.” / “How does this project work?” |
| Architecture | “What architecture does this use?” |
| FeaturePlacement | “Where should this feature live?” |
| ImportantModules | “What modules are most important?” |

`PlannerResponse.coding_understanding` carries the WI scaffold. Soft path
(no Reasoning backend) returns the structured markdown directly.

## Pipeline

```text
UserRequest (project understanding phrase)
    │
    ▼
Intent → Capability → Complexity → Environmental Resolution
    │
    ▼
Coding Understanding detect          ★ Project angle (C1.2)
    │  AssembleHints.understanding=understanding:project:<angle>
    ▼
ContextEngine assemble_with
    │
    ▼
scaffold_from_bundle_with_angle      ★ WI sections only
    │
    ├─ soft markdown (## Project Understanding)
    └─ LlmContext extension → Prompt section → Reasoning
         (elaborates; still no tools / plans / edits)
```

## Ownership

| Concern | Owner |
|---------|--------|
| Detect angle / feature hint | Planner |
| Scaffold from bundle | Planner (observation) |
| Assemble ContextBundle | ContextEngine |
| Elaborate | Reasoning (optional) |
| Tools / plans / edits | **Forbidden** |

Extends [coding-understanding.md](coding-understanding.md) (C1.1). Planner
ownership of Intent / Capability selection is unchanged.

## Related

- [coding-understanding.md](coding-understanding.md) — C1.1 base path
- [workspace-intelligence.md](workspace-intelligence.md) — WI snapshots
- [project-snapshot.md](project-snapshot.md) — ProjectSnapshot
- [git-snapshot.md](git-snapshot.md) — GitSnapshot
- [workspace-memory.md](workspace-memory.md) — Workspace Memory
- [planner.md](planner.md) — orchestration

## Constitutional audit (C1.2)

### Ownership

```text
"Explain this project." / architecture / modules / feature placement
        │
        ▼
 Planner detect_project_understanding     ★ angle + feature hint
        │  AssembleHints only
        ▼
 ContextEngine assemble_with              ★ sole ContextBundle factory
        │
        ▼
 scaffold from ProjectIntelligence +
 GitStatus + WorkspaceMemory +
 Conversation + ActiveWorkspace           ★ no FS / tools / plans
        │
        └─ structured Project Understanding markdown
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| Mode / angle detect | Planner | Pass |
| ContextBundle factory | ContextEngine | Pass |
| WI sources | Assembled sections only | Pass — snapshots via providers |
| Tools / Execution Plans | Not invoked | Pass |
| File modifications | None | Pass |
| Planning / execution | None | Pass |
| Intent / Capability | Unchanged | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Understand work before acting | Pass — project orientation first |
| PRINCIPLES | Local, transparent, privacy | Pass — soft scaffold without LLM |
| NON_GOALS | Not an LLM product; no tool sprawl | Pass — understanding-only path |
| ARCHITECTURE | Planner orchestrates; Context assembles | Pass |
| ROADMAP | C1.2 Project Understanding | Pass — Current |

### Residuals

* Feature placement candidates are layout heuristics from observed dirs/members — not authoritative ownership maps.
* Thin ProjectSnapshot / missing open project yields honest “not observed” sections.
* Conversation contribution is metadata (title / counts), not a full transcript dump into the scaffold.
