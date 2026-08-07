# Conversational Complexity Assessment

**Status: Current Implementation** — Planner-owned, deterministic, no AI.

Before Context assemble, the Planner classifies free-text conversational
complexity into a lightweight [`ComplexityAssessment`]. The class is written
onto [`AssembleHints::complexity`] so Context can skip or prioritize providers.
It **never** changes Intent routing or Capability selection.

```text
User Request
    │
    ▼
Planner
    │
    ├─ Intent Resolution          (unchanged)
    ├─ Capability Resolution      (unchanged)
    ├─ Complexity Assessment      ← new, deterministic
    └─ AssembleHints { intent, capability_ids, complexity }
            │
            ▼
        Context Engine assemble
            │
            └─ RelevanceSignals apply complexity participation tiers
```

## Classes

| Class | Stable id | Meaning |
|-------|-----------|---------|
| Greeting | `greeting` | Short social hello |
| SmallTalk | `small_talk` | Thanks / how-are-you / goodbye |
| GeneralQuestion | `general_question` | Default question / statement |
| ProjectQuestion | `project_question` | About this project / repo / workspace |
| CodingQuestion | `coding_question` | Code, tools, errors, implementation |
| ResearchQuestion | `research_question` | Broader research / explain / history |

## Classification rules (ordered, first match wins)

Normalization: lowercase, strip punctuation (keep `?` and `'`), collapse whitespace.

1. **empty_default** — empty / whitespace-only → `general_question`
2. **greeting** — length ≤ 48; no coding/project/research markers; exact or short prefix match on hello/hi/hey/good morning|afternoon|evening/howdy/yo/hiya/greetings (+ optional soft tokens, ≤ 5 words, not a question)
3. **small_talk** — length ≤ 72; no coding/project/research markers; thanks / how are you / what's up / goodbye / see you / take care / you're welcome / good night / have a good day
4. **coding_markers** — contains compile/refactor/stack trace/borrow checker/lsp/typescript/rust/python/git commit/write a function/… (see `complexity.rs` marker list) → `coding_question`
5. **project_markers** — contains this project/repo/codebase, open/close/switch/continue project, workspace root, which files, … → `project_question`
6. **research_markers** — contains research/investigate/literature/history of/explain the concept/compare and contrast/… or `compare` / `difference between` without coding/project markers → `research_question`
7. **coding_workspace_question** — active workspace is coding/code/development **and** text looks like a question → `coding_question` (tie-break only)
8. **general_question** — looks like a question (leading what/why/how/… or contains `?`)
9. **general_default** — everything else → `general_question`

Determinism: same `(text, workspace_kind)` always yields the same assessment. No model calls.

## AssembleHints influence

Context copies `AssembleHints.complexity` onto `RelevanceSignals.complexity` and
maps each provider to a participation tier: **Required**, **Optional**, or **Excluded**.

| Provider | greeting / small_talk | general_question | coding_question |
|----------|----------------------|------------------|-----------------|
| conversation | Required | Required | Required |
| memory | Required | Optional | Optional |
| search | Excluded | Optional | Optional |
| workspace | Excluded | Optional | Required |
| diagnostics | Excluded | Optional | Required |
| project | Excluded | Optional | Required |
| editor | Excluded | Optional | Required |
| git_status | Excluded | Optional | Required |
| workspace_inventory | Excluded | Optional | Optional |
| file_summaries | Excluded | Optional | Required |
| runtime | Excluded | Optional | Required |
| workspace_memory | Excluded | Optional | Required |

**Excluded** providers are skipped before policy evaluation — no `contribute()` call,
inspector outcome `SkippedComplexity`. **Required** providers receive a high relevance
score (always above threshold). **Optional** providers use their normal local relevance
heuristic; policy and budget still apply.

Additional tiers (same mechanism):

| Complexity | Required | Excluded |
|------------|----------|----------|
| project_question | conversation, project, workspace, workspace_inventory, git_status | — |
| research_question | conversation, search | — |

Sprint **B2.8** Context Selection may further omit Required feeds when a refined
profile is stricter (e.g. debug/compile omits Git). See
[context-selection.md](context-selection.md).

Threshold / policy / capability ids are unchanged. Assembly always flows through
`ContextEngine::assemble_with` — complexity never bypasses the engine.

## Ownership

| Owner | Role |
|-------|------|
| **Planner** | Sole author of `ComplexityAssessment` and `AssembleHints.complexity` |
| **DecisionEngine** | Intent + Capabilities (untouched by complexity) |
| **Context** | Consumes complexity label for participation tiers only — never re-classifies free text into complexity or Intent |
| **Experience / UI** | Does not classify complexity |

See also: [planner.md](planner.md), [context.md](context.md).
