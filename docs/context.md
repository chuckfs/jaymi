# Context Engine

**Status: Current Implementation**

The Context Engine is the sole request-context assembler for the Planner. This document describes shipped behavior.

The Context Engine assembles only the knowledge required for the current request.

The Planner calls a single method:

```text
ContextEngine::assemble(request) -> ContextBundle
```

The Planner does not coordinate Memory, Project, Search, or session workspace state itself.

---

## Responsibilities

* Assemble relevant memories through the Memory Engine
* Attach promotion suggestions (never auto-applied)
* Include open project context through the Project Engine
* Coordinate Search Engine hints when appropriate (without executing search tools)
* Include active UX workspace / session state when set

---

## ContextBundle

A unified bundle returned for every Planner request:

* `memory` — relevant memories only
* `promotion_suggestions` / `promotion_ask`
* `project` — open project workspace, when any
* `active_workspace` — experience session workspace kind id, when set
* `search` — lightweight search coordination hint (structured query pending / project index summary)
* `sources` — which context sources contributed
* `assemble_generation` — monotonic counter for diagnostics and tests

---

## Boot

1. Context Engine initializes after the Memory Engine (lifecycle dependency).
2. After Project Engine and Search Engine are ready, Application binds sources:
   - Memory Engine
   - Project Engine
   - Search Engine
3. Planner receives `Arc<ContextEngine>` and calls `assemble` at the start of every `handle`.

---

## What this is not

* Not a Reasoning Engine
* Not a language model
* Not a replacement for tool-backed search / read / discover execution

Search tools still execute through the Tool Orchestrator. The Context Engine only coordinates when search-related context should be noted for the request.

---

## Status

Implemented as the sole request-context assembler for the Planner.
