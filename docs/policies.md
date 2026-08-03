Policies

**Status: Partial** — Offline First and Privacy Maximum enforced · other builtins declared

Policies define how Jaymi behaves.

The Planner uses policies to make consistent decisions without hardcoding behavior.

Policies do not grant permissions.

Policies express preferences.

Permissions answer:

“Can this action happen?”

Policies answer:

“How should this action happen?”

This separation allows Jaymi to remain predictable while adapting to different users and situations.

### Current enforcement (boot-active)

* Offline First — default; constrains internet / cloud-only candidates

### Declared enforcement (constraint logic exists; not boot-active)

* Privacy Maximum — rejects non-local-only candidates when activated

### Declared / Target (builtins registered; no constraint logic yet)

Highest Quality, Fastest Response, Battery Saver, Developer / Creative / Research modes, rich multi-scope resolution, user-custom policies.

⸻

Philosophy

Jaymi should make decisions according to user-defined preferences rather than hidden internal rules.

The Planner should never contain logic such as:

* Always use this model.
* Always search the internet.
* Always use local AI.
* Always choose the fastest provider.

Instead, those decisions emerge from active policies.

⸻

Policy Engine

Every request passes through the Policy Engine before execution.

User
↓
Planner
↓
Policy Engine
↓
Capability Selection
↓
Tool Selection
↓
Execution

Policies influence planning.

They never perform work themselves.

⸻

Built-in Policies

Offline First

**Status: Current** — boot default; enforced on tool candidates.

Default policy.

Priorities:

1. Local execution
2. Local models
3. Local search
4. Local memory

Only use internet resources when:

* Required
* Explicitly approved
* No local alternative exists

⸻

Highest Quality

**Status: Declared / Target** — builtin identity exists; no constraint logic yet.

Prioritize the highest quality result regardless of execution time.

May choose:

* Larger models
* Slower reasoning
* Higher quality image generation
* More expensive tools

⸻

Fastest Response

**Status: Declared / Target** — builtin identity exists; no constraint logic yet.

Prioritize speed.

Prefer:

* Cached results
* Smaller models
* Lightweight tools
* Local execution

⸻

Privacy Maximum

**Status: Declared** — constraint logic exists; not enabled at boot (activate explicitly).

Never use cloud resources.

Never send data outside the device.

Always prefer local providers.

⸻

Battery Saver

**Status: Declared / Target** — builtin identity exists; no constraint logic yet.

Optimize for efficiency.

Avoid:

* GPU-intensive workloads
* Large models
* Long-running background tasks

Prefer lightweight execution.

⸻

Developer Mode

**Status: Declared / Target** — builtin identity exists; no constraint logic yet.

Optimize for software development.

Prefer:

* Project context
* Terminal tools
* Git integration
* Code-aware models

Reduce unnecessary confirmations for approved development actions.

⸻

Creative Mode

**Status: Declared / Target** — builtin identity exists; no constraint logic yet.

Optimize for ideation.

Prefer:

* Larger context
* Creative language models
* Image generation
* Brainstorming tools

⸻

Research Mode

**Status: Declared / Target** — builtin identity exists; no constraint logic yet.

Optimize for information gathering.

Prefer:

* Search providers
* Document readers
* Summarization tools
* Citation-rich responses

Internet access may be requested automatically according to permission settings.

⸻

Policy Categories

Policies may influence:

* Tool selection
* Model selection
* Provider selection
* Internet usage
* Context size
* Memory retrieval
* Execution order
* Parallel execution
* Retry strategy

Policies never replace permissions.

⸻

Policy Scope

**Status: Partial** — Global scope is Current (boot-active Offline First). Conversation / Project / Task scoped resolution and override order are Target.

Policies may exist at multiple levels.

Global

Applies to all conversations.

Example:

Offline First

⸻

Conversation

Applies only to the current conversation.

Example:

Research Mode

⸻

Project

Applies to one project.

Example:

Developer Mode

⸻

Task

Applies to one request.

Example:

Highest Quality

⸻

The most specific policy overrides broader policies.

Task

↓

Project

↓

Conversation

↓

Global

⸻

Policy Resolution

Multiple policies may be active simultaneously.

Example:

Global

Offline First

Project

Developer Mode

Conversation

Research Mode

The Planner combines these policies when selecting Tools.

When conflicts occur, the most specific policy wins.

⸻

User Customization

Users may:

* Enable policies
* Disable policies
* Create custom policies
* Assign default policies
* Override policies temporarily

Policies should be transparent and editable.

⸻

Design Principles

Policies should be:

* Predictable
* Explainable
* Composable
* User-controlled
* Independent of providers
* Independent of models

The Planner should rely on policies rather than hardcoded behavior.

⸻

Long-Term Vision

Policies allow Jaymi to adapt without changing its architecture.

As new capabilities, providers, and models are added, existing policies continue to guide behavior.

This keeps Jaymi consistent while allowing the ecosystem to evolve.

The Planner remains deterministic.

Policies shape decisions.

Tools perform work.

Providers connect to resources.
