# Performance (Context / Conversation)

**Status: Current Implementation** — observational notes for Developer Diagnostics
and conversational prepare latency.

## Conversational prepare (Sprint B2.13.2)

`Application::prepare_context_session` must stay cheap on the request path:

* Merges the latest **completed** ambient snapshots only
* Never rebuilds a `WorkspaceSnapshot`
* Never calls `observe_toolchain` / marker-file probes
* If no completed WorkspaceSnapshot exists, **schedules** ambient refresh and
  continues (conversation never waits)

Toolchain / marker observation runs inside ambient
`ContextMaintenance` WorkspaceSnapshot jobs triggered by Coding / project /
editor activity. See [context-maintenance.md](context-maintenance.md) and
[workspace-snapshot.md](workspace-snapshot.md).

## Related surfaces

* Developer Diagnostics **Performance** dashboard — pipeline / TTFT / provider
  timings (never in the conversation transcript)
* Context Inspector — assemble duration, cache hit/miss, per-provider timings
* Workspace Intelligence diagnostics — snapshot freshness / maintenance status

## Related

* [context.md](context.md)
* [context-maintenance.md](context-maintenance.md)
* [workspace-snapshot.md](workspace-snapshot.md)
* [experience.md](experience.md) — Developer Diagnostics
