# Context Selection (Sprint B2.8)

**Status: Current** — deterministic workspace context selection. **No AI scoring.**

Context Policy chooses which workspace feeds participate for a request using
ordered, documented heuristics. Providers still propose
[`ContextCandidate`](context-candidates.md) nodes; selection decides the
allowlist before candidates are scored and packed.

```text
"hello"
  → Conversation, Memory

"why won't this compile?"
  → Conversation, Diagnostics, Current file, Terminal, Selection

"summarize this project"
  → Project, Filesystem, Architecture, Git
```

## Ownership

| Owner | Role |
|-------|------|
| **Planner** | Intent, Capabilities, optional `AssembleHints.complexity` |
| **Context Selection** | Maps complexity + RequestKind + lexical cues → profile |
| **Context Policy** | Enforces profile allowlists on providers / candidate kinds |
| **Context Engine** | Sole `ContextBundle` factory |
| **Context Selection must not** | Invent Intent or Complexity for the Planner; use AI |

When `AssembleHints.complexity` is present it is the **coarse class**. Lexical
cues only **refine** within that class (e.g. `coding_question` →
`debug_compile`) or provide a fallback when no complexity hint is attached
(tests / direct assemble).

## Profiles

| Profile | Stable id | Typical feeds |
|---------|-----------|---------------|
| Greeting | `greeting` | Conversation, Memory |
| SmallTalk | `small_talk` | Conversation, Memory |
| DebugCompile | `debug_compile` | Conversation, Diagnostics, Editor (file + selection), Runtime/Terminal, Workspace |
| ProjectOverview | `project_overview` | Conversation, Project (+ architecture/intelligence), Filesystem inventory, Git, File summaries |
| CodingGeneral | `coding_general` | Broader coding set (diag / editor / runtime / workspace_memory / project / git / memory) |
| Research | `research` | Conversation, Search, Memory, Project |
| Search | `search` | Conversation, Search, Memory, Workspace (capabilities), Inventory |
| Git | `git` | Conversation, Git, Project, Workspace, Editor |
| Terminal | `terminal` | Conversation, Runtime, Workspace Memory, Workspace, Editor, Project |
| FileEdit | `file_edit` | Conversation, Editor, Workspace Memory, File summaries, Diagnostics, Project |
| ProjectSession | `project_session` | Conversation, Project, Workspace, Memory, Workspace Memory |
| GeneralChat | `general_chat` | Conversation, Memory, Project, Workspace |

`permission` is always allowed.

**Planner capability override:** capability ids ride on the `workspace`
provider. When `AssembleHints` carry non-empty capability ids, Context Policy
keeps `workspace` allowed even under a strict profile that would otherwise omit
it — so selection never drops Planner-owned capabilities. Workspace *kind* /
inventory feeds remain profile-gated via candidate kinds.

**Workspace Memory** (`workspace_memory`) is allowed on coding-shaped profiles
and omitted on greeting / small-talk. See [workspace-memory.md](workspace-memory.md).

## Classification order (first match wins)

1. **request_kind_*** — structured `RequestKind` (Search / Git / Terminal / File /
   Lsp / ProjectSession) maps directly to a profile.
2. **complexity_*** — Planner `AssembleHints.complexity` when present:
   - `greeting` / `small_talk` → Greeting / SmallTalk
   - `coding_question` + debug/compile cues → **DebugCompile**
   - `coding_question` → CodingGeneral
   - `project_question` → ProjectOverview
   - `research_question` → Research
   - `general_question` + debug cues → DebugCompile
   - `general_question` + project-overview cues → ProjectOverview
   - `general_question` → GeneralChat
3. **lexical_*** fallback (no complexity hint):
   - greeting phrases → Greeting
   - small-talk phrases → SmallTalk
   - debug/compile cues → DebugCompile
   - project-overview cues → ProjectOverview
   - coding workspace / Code intent → CodingGeneral
   - else → GeneralChat

Normalization: lowercase; strip punctuation except `?` and `'`; collapse
whitespace.

## Lexical heuristics (documented)

### Greeting cues

Exact: `hello`, `hi`, `hey`, `howdy`, `yo`, `hiya`, `greetings`, `hello jaymi`, …
Prefixes (≤ 32 chars, ≤ 5 words, not a question): `hello `, `hi `, `hey `,
`good morning`, `good afternoon`, `good evening`.

Disqualified when debug/compile or project-overview cues are present.

### Debug / compile cues

Phrases include: `won't compile`, `doesn't compile`, `compile error`,
`build failed`, `cargo check` / `build` / `test`, `type error`, `syntax error`,
`borrow checker`, `stack trace`, `why won't this compile`, `failing test`,
`panic`, `segfault`, …

Also: `compile` with `why` / `error` / `fail` / `broken` / `fix`; or `error`
with `rustc` / `cargo` / `typescript` / `build`.

### Project overview cues

Phrases include: `summarize this project`, `project overview`,
`what is this project`, `architecture of this project`, `repo structure`,
`filesystem layout`, …

Also: (`summarize` | `summarise` | `overview` | `architecture`) **and**
(`project` | `repo` | `codebase` | `workspace`).

## Candidate kind preferences

Selective profiles also omit kinds:

| Profile | Preferred | Omitted |
|---------|-----------|---------|
| Greeting | Conversation, Memory | Diagnostics, editor, runtime, git, inventory, search, … |
| DebugCompile | Conversation, Diagnostic(s), CurrentFile, Selection, Runtime, EditorIntelligence | ProjectIntelligence, Inventory, Git, Search, FileSummaries, OpenFiles |
| ProjectOverview | Conversation, ProjectIdentity, ProjectIntelligence (architecture), Inventory, Git, … | Diagnostics, Runtime, Selection, Search |

Preferred kinds receive a deterministic importance/relevance boost under budget
packing. Omitted kinds are denied unless marked `required`.

## Complexity tier alignment

Planner complexity tiers (see [complexity.md](complexity.md)) were aligned so
greeting includes **Memory**, coding includes **Runtime**, and project questions
include **Git** — matching the examples above. Context Selection may still omit
feeds that complexity marked Required when the refined profile is stricter
(e.g. DebugCompile omits Git even under `coding_question`).

## Explainability

`PolicyReport` records:

* `selection_profile` — stable profile id
* `selection_rules` — matched rule ids (first is decisive)
* per-provider deny reasons include `Context selection profile '…' omits …`

## Related

* [context-candidates.md](context-candidates.md) — candidate graph
* [complexity.md](complexity.md) — Planner complexity classes
* [context.md](context.md) — Context Engine contract
