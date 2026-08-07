# Environmental Resolution (Sprint B2.10)

**Status: Current** — Planner resolves ambiguous workspace deixis before Reasoning.

Examples:

* `"rename this"`
* `"fix it"`
* `"why?"`
* `"clean this up"`

The Planner binds these references from **Workspace Intelligence** already on
`ContextSessionInputs`. LLMs **never** invent workspace paths, files, or
symbols independently — they consume Planner bindings only.

## Pipeline

```text
UserRequest
    │
    ▼
Intent Resolution
    │
    ▼
Capability Resolution
    │
    ▼
Complexity Assessment          (class only; never routing)
    │
    ▼
Environmental Resolution       ★ Sprint B2.10
    │  session Workspace Intelligence
    │  deterministic deixis → evidence chain
    │
    ▼
AssembleHints {
  intent, capability_ids, complexity,
  environmental: EnvironmentalHints,
  understanding: Option<String>   // C1.1 when Explain / understanding
}
    │
    ▼
Context Engine assemble_with
    │  stamps PlannerMetadata.environmental
    │
    ▼
LlmContext.providers.environmental
    │
    ▼
Prompt section: Environmental Resolution
    │  + system rule: do not invent workspace refs
    ▼
Reasoning Engine
```

## Ownership

| Concern | Owner |
|---------|--------|
| Detect deixis / bind referents | **Planner** (`environmental::resolve_environment`) |
| Workspace Intelligence facts | Coding host → `ContextSessionInputs` (snapshots, editor, diagnostics, workspace memory) |
| Carry bindings into assemble | `AssembleHints.environmental` |
| Bundle / LLM stamp | Context Engine (sole `ContextBundle` factory) |
| Render for the model | PromptBuilder (`EnvironmentalResolution` section) |
| Invent paths / symbols | **Forbidden** for LLMs |

Environmental Resolution does **not** invent Intent or Capabilities. It only
annotates workspace binding for Context Policy bias and Reasoning.

## Deixis detection (deterministic)

Normalization: lowercase; keep `?` / `'`; collapse whitespace.

Triggers include:

* Bare `why` / `why?`
* Phrases: `rename this`, `fix it`, `fix this`, `clean this up`, `look at this`, …
* Short requests (≤ 12 tokens) containing `this` / `that` / `it` / `here` / `these` / `those`
* Soft cues: `the file`, `the error`, `the bug`

Structured requests (`file`, `write_file`, `terminal`, `git`, `lsp`, …) skip
resolution — paths are already explicit.

## Evidence chain (first match wins per cue)

Ordered Workspace Intelligence:

1. **Current selection** (non-empty text on `ContextSessionInputs.current_selection`,
   or `editor_snapshot.selection` when session selection text is empty) → `Selection`
2. **Primary diagnostic** (for `why?` / `fix it` / error-shaped text)
3. **Active / current file**
4. **Editor symbol** (when present on `EditorSnapshot`)
5. **Workspace Memory recent edit**
6. **Active open tab**
7. Else → `Unresolved` (ambiguous)

Competing distinct paths mark the resolution `ambiguous=true`.

Selection text is host-observed (Monaco → CodingState → snapshots). The Planner
binds `"Explain this."` / `"Rename this."` / `"Clean this up."` from that
observation — models do not invent the referent.

## Prompt contract

When bindings exist, PromptBuilder emits **Environmental Resolution** with:

* Explicit instruction: use only Planner-resolved references; do not invent paths
* `primary_path`, selection, symbol, diagnostic, binding lines, rule ids

Default system instructions reinforce the same rule.

## Related

* [editor-snapshot.md](editor-snapshot.md) — Monaco selection → EditorSnapshot
* [workspace-memory.md](workspace-memory.md) — activity rings used as evidence
* [context-selection.md](context-selection.md) — feed selection (orthogonal)
* [complexity.md](complexity.md) — conversational class (orthogonal)
* [planner.md](planner.md) — orchestration kernel
