# Code Talk (Sprint C0.2)

**Status: Current** — Teach Jaymi how to talk about code.

This is **not** ChatGPT-style prompt tinkering. Code Talk owns the voice and
shape of coding conversation so Jaymi answers like an engineering partner.

## Before → after

**Before (narrator / hedged):**

> The current file appears to be responsible for the game runtime…

**After (partner):**

```text
script.js

Purpose
Implements the runtime for a browser-based Pong game.

Responsibilities
• Canvas rendering
• Game loop
• Audio playback
• Input handling

Interesting observations
• Rendering and game logic are tightly coupled.
• Global state is used extensively.
• The file is becoming large (~360 lines).

Recommendations
• Extract Renderer
• Introduce Game class
• Move audio into AudioManager

Next actions
[Review]
[Refactor]
[Generate Tests]
[Explain Architecture]
```

## Voice rules

| Do | Don't |
|----|-------|
| Lead with the file / symbol name | Title the reply “Coding Understanding” |
| Direct Purpose sentences | “appears to be”, “seems to”, “likely”, “I think” |
| Capability Responsibilities | Metadata dumps (`Observed language: js`) |
| Concrete Interesting observations | Soft narrator hedges |
| Actionable Recommendations | Vague “consider looking into…” |
| Bracketed Next action chips | Long prose “suggested next actions” |

Banned hedging phrases live in `crates/jaymi-planner/src/code_talk.rs`
(`BANNED_PHRASES`) and are injected into Reasoning instructions.

## Ownership

| Concern | Owner |
|---------|--------|
| Voice rules + chip helpers | **Planner** (`code_talk`) |
| Soft-path scaffold content | **Planner** (`coding_understanding` scaffold) |
| Reasoning instruction | Code Talk rules + WI starter scaffold |
| Mutations / tools | Forbidden on understanding turns |

Still WI-only: soft path never invents unread file bodies. When a file summary
exists, Purpose prefers that summary. Recommendations stay observational until
Review / Planning turns.

## Pipeline

```text
Explain / “what is this file responsible for?”
        │
        ▼
 Coding Understanding detect (C1.1)
        │
        ▼
 scaffold_from_bundle (Code Talk voice)
        │
        ├─ soft path → render_markdown (filename title + sections + [chips])
        │
        └─ Reasoning
             instruction includes voice_rules_block()
             + starter scaffold
```

## Related

- [coding-understanding.md](coding-understanding.md) — C1.1 Understand Before Acting
- [pair-programming.md](pair-programming.md) — C1.8 next-action partner flow
- [prompt.md](prompt.md) — PromptBuilder sections
- [experience.md](experience.md) — Conversation First surface

## Constitutional audit (C0.2)

### Ownership

```text
Code Talk voice
        │
        ▼
 Planner owns structure + banned hedges     ★ not UI copywriting
        │
        ▼
 Soft scaffold / Reasoning instruction
        │
        ▼
 Conversation First (still no tools / edits)
```

| Concern | Owner | Verdict |
|---------|--------|---------|
| How Jaymi talks about code | Planner Code Talk | Pass |
| Invented file bodies | Forbidden (WI only) | Pass |
| Hedged narrator voice | Banned | Pass |
| Next actions as partner chips | `[Review]` style | Pass |
| Intent / Capability ownership | Unchanged | Pass |

### Matrix vs constitution

| Document | Check | Result |
|----------|-------|--------|
| VISION | Engineering partner | Pass |
| PRINCIPLES | Conversation First; honest | Pass |
| NON_GOALS | Not chatbot filler | Pass |
| ARCHITECTURE | Planner owns coding scaffolds | Pass |
| ROADMAP | C0.2 Code Talk | Pass — Current |

### Residuals

* Soft path Recommendations are WI-derived heuristics (size, enclosing type) —
  richer Extract/Introduce names still need file summaries or Reasoning.
* Bracketed Next actions are conversational chips today; clicking them still
  goes through the C1.8 / Conversation First path (not in-markdown buttons).
