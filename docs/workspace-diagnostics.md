# Workspace Diagnostics (Sprint B2.11)

**Status: Current Implementation** · Developer Diagnostics only

Workspace Diagnostics exposes a read-only aggregate of Workspace Intelligence
for developers: snapshot freshness, provider timings, maintenance status,
candidate selection, policy decisions, and context budget.

It never writes to the conversation transcript, Memory turns, or Planner
routing. Assembling the report does not schedule maintenance or re-assemble
Context.

## Surface

| Surface | Role |
| --- | --- |
| Developer Diagnostics (nav rail) | Primary UI — **Workspace Intelligence** section |
| `DiagnosticsSnapshot.workspace_inspector` | Headless / CLI dashboard |
| Coding dock Diagnostics | Unchanged (execution inspection); no transcript pollution |

## Pipeline

```text
ContextEngine::inspect_last  →  ContextInspectorReport
  (provider timings, policy, candidates, budget)
ContextMaintenance
  (generation, jobs, inflight, completed snapshot timestamps)
        │
        ▼
WorkspaceDiagnosticsReport::assemble / from_maintenance
        │
        ▼
Application::workspace_diagnostics()
        │
        ▼
DiagnosticsSnapshot.workspace_inspector
        │
        ▼
Developer Diagnostics UI  ·  render_dashboard()
```

Observation only — no side effects on paint.

## What is shown

1. **Snapshot freshness** — present / missing plus age labels (`fresh` ≤30s,
   `warm` ≤2m, `aging` ≤10m, `stale`) for workspace / editor / project / git /
   runtime snapshots; presence for inventory, diagnostics, file summaries.
2. **Provider timings** — last assemble per-provider contribute timings from the
   Context Inspector.
3. **Maintenance status** — per-kind inflight + completed flags; generation and
   job counters.
4. **Candidate selection** — proposed / selected / rejected counts and
   per-candidate decisions (B2.7).
5. **Policy decisions** — selection profile / rules (B2.8) and per-provider
   Included/Excluded reasons.
6. **Context budget** — used / max characters, estimated tokens, truncated /
   skipped providers.

## Guarantees

* Developer-only — gated by the same nav-rail Developer Diagnostics toggle.
* No transcript pollution — never appended to conversation turns.
* No assemble / maintenance side effects when painting diagnostics.
* Context Engine remains the sole ContextBundle factory; this report only reads
  the last inspection and maintenance store.

## Related

* [docs/context.md](context.md) — Context Engine / Inspector / Policy
* [docs/context-maintenance.md](context-maintenance.md) — ambient maintenance
* [docs/context-candidates.md](context-candidates.md) — candidate graph (B2.7)
* [docs/context-selection.md](context-selection.md) — selection profiles (B2.8)
* [docs/experience.md](experience.md) — Developer Diagnostics surface
