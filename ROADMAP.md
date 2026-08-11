Roadmap

Jaymi is being built as a collection of layers.

Each layer provides new capabilities while remaining stable enough to support everything built on top of it.

The objective is not to ship features as quickly as possible.

The objective is to build a personal AI environment that can grow for years without requiring major architectural rewrites.

How to read this document:

* **Current** — implemented, wired at boot, covered by tests
* **Partial** — foundation present; incomplete relative to the layer objective
* **Target** — planned; keep the objective, do not treat as shipped

⸻

Progress

| Layer | Name | Status |
|-------|------|--------|
| 0 | Foundation | **Current** |
| 1 | Knowledge Engine | **Current** |
| 2 | Understanding Engine | **Current** (OCR stub; lexical embeddings) |
| 3 | Search Engine | **Current** |
| 4 | Memory Engine | **Current** (core; graph/export/aging Target) |
| 5 | Project Engine | **Current** (core; deep Git history / artifacts Target — ambient GitSnapshot status is Current via Layer 6 WI) |
| 6 | Workspace & Capability Engine | **Current** (engine + Coding shell + thin UX; rich IDE/canvas Target) |
| 7 | Tool Engine | **Partial** (framework + 12 tools; broader catalog Target) |
| 8 | Provider Ecosystem | **Partial** (Registry + 6 local providers; plugins Target) |
| 9 | Daily Driver | **Target** |

Architectural Integrity (orthogonal to layers): Context sole assemble path, ContextBundle sole request-context contract on `PlannerResponse`, Project ownership, Planner responsibilities, Planner integrity (requests via `handle`), Provider simplification (no ProviderManager), Capability availability (Ready / Experimental / Planned / Unavailable), canonical request pipeline (Intent → Capability → Context Policy → assemble → Action Policy → Permission → Tool), Session ownership (one project open/close lifecycle), Documentation & Diagnostics (Operational / Experimental / Stub / Disabled), Settings Workspace preferences ownership (snapshots/intents only; Model Registry remains catalog SoT) — **Current**. Behavior stage after ContextBundle — **Planned**.

⸻

Layer 0 — Foundation

Status: **Current**

Objective

Build the foundation that every other component depends on.

Includes

* Desktop application (egui shell + diagnostics)
* Core architecture
* Planner
* Provider framework (Registry + bound instances)
* Tool framework
* Permission system
* Policy system
* Configuration
* Logging
* SQLite database

Exit Criteria

Jaymi launches and can execute a simple tool through the planner.

Shipped when: boot sequence completes; list/read/search pipelines pass through Planner → Intent → Capability → Context → Action Policy → Permission → Tool.

⸻

Layer 1 — Knowledge Engine

Status: **Current**

Objective

Teach Jaymi what exists on the computer.

Responsibilities

* File discovery
* Metadata indexing
* Background indexing / watcher
* Incremental updates
* Local database inventory

Supported Sources (Current)

* Files
* Folders
* Downloads
* Documents

Exit Criteria

Jaymi can answer:

“What exists?”

without opening Finder.

⸻

Layer 2 — Understanding Engine

Status: **Current** (with stubs)

Objective

Teach Jaymi to understand content rather than filenames.

Responsibilities

* PDF / DOCX / Markdown / text / JSON parsing — **Current**
* Image content pipeline — **Current** (OCR engine **Stub**)
* Metadata extraction — **Current**
* Embeddings — **Current** (local lexical hashed embeddings, not a neural model)
* Summaries / deep image understanding — **Target**

Exit Criteria

Jaymi can answer:

“What is this?”

instead of only

“What is this file called?”

⸻

Layer 3 — Search Engine

Status: **Current**

Objective

Build semantic retrieval across every knowledge source.

Features

* Full-text search — **Current**
* Metadata search — **Current**
* Semantic / hybrid ranking — **Current** (over local embeddings)
* Citations — **Current**
* Preview — **Partial**

Exit Criteria

Search is based on meaning rather than filenames.

⸻

Layer 4 — Memory Engine

Status: **Current** (core)

Objective

Allow Jaymi to remember over time.

Memory Types

Conversation Memory — temporary for one conversation — **Current**

Project Memory — long-term memory attached by `project_id` — **Current**

Personal Memory — persistent preferences and important facts — **Current**

Target (not yet productized)

* Full relationship graph
* Merge / split memories
* Automatic aging
* Export of all memories

Exit Criteria

Jaymi remembers intentionally instead of accidentally.

⸻

Layer 5 — Project Engine

Status: **Current** (core)

Objective

Teach Jaymi that work happens inside projects.

A project is not just a folder.

A project includes (Current):

* Files under a root (via Knowledge / Search)
* Conversations
* Documentation / architecture entries in context
* Decisions
* Tasks (as project memory kinds)
* History via memory and conversations

Target:

* Deep Git history / project-owned Git Integration productization (working-tree
  status observation is **Current** via [`GitSnapshot`](docs/git-snapshot.md) —
  Workspace Intelligence, not Project Engine)
* Artifact pipelines
* Repository import / conversation convert flows
* Richer live working-tree / IDE file productization beyond the Coding shell

Exit Criteria

Users can simply say:

Continue working on Jaymi.

⸻

Layer 6 — Workspace & Capability Engine

Status: **Current** (engine + thin UX)

Objective

Give Jaymi useful abilities that reshape the experience — not just backend concepts.

Capabilities include (catalog):

* Chat, Coding, Image generation, Search, Vision, OCR, Embeddings, Internet, Automation, File management, Terminal, and related aliases

**Boot registration:** the **full capability catalog** is registered. Availability (Ready / Experimental / Planned / Unavailable) distinguishes conceptual support from what is currently executable. Planned capabilities stay registered.

**Currently executable (inventory-backed):** Search, ReadDocuments, Discover, Index (Ready); Embeddings (Experimental — local lexical). Code / Vision remain Experimental catalog but Unavailable without coding/vision tools. OCR is **Planned** (placeholder provider only; not executable).

Capabilities are abstract.

They describe what Jaymi can do.

Not how it does it.

Capabilities also change the user experience.

Conversation stays permanent. Workspaces expand beside it.

**Current:** conversation shell + Coding Workspace shell (five dock pages — Terminal / Problems / Search / Git / Diagnostics — plus an Output placeholder not listed in `pages()`) + `CodingState` + workspace expansion model + capability state / inspector + **WorkspaceSnapshot** (B2.1 / B2.2 / B2.13.2) + **EditorSnapshot** (B2.3 / B2.13.3 editor intelligence) + **ProjectSnapshot** (B2.4 project intelligence) + **GitSnapshot** (B2.5 — **Current**) + **RuntimeSnapshot** (B2.6 — **Current**) + **Context Candidate Graph** (B2.7 / B2.13.1 — **Current**) + **Context Selection** (B2.8) + **Workspace Memory** (B2.9) + **Environmental Resolution** (B2.10) + **Workspace Diagnostics** (B2.11) + **Constitutional Audit** (B2.12) + **docs sync** (B2.13.4) + **Coding Actions** (C0.1) + **Code Talk** (C0.2) + **Coding Understanding** (C1.1) + **Project Understanding** (C1.2) + **Coding Review** (C1.3) + **Coding Plan / Generation Planning** (C1.4) + **Code Generation** (C1.5) + **Coding Execution Plans** (C1.6) + **Multi-file Intelligence** (C1.7) + **Pair Programming Experience** (C1.8) + **Intelligent Code Search** (C2.1) + **Symbol Navigation** (C2.2) + **Code Relationship Tracing** (C2.3) + **Deep Codebase Investigation** (C2.4) + **Codebase Q&A** (C2.5) + **Codebase Evidence Handoff** (C2.7.2) + **Coding Edit Intent** (C3.1) + **Change Investigation** (C3.2) + **Coding Plan from Investigation** (C3.3) + **Patch Generation** (C3.4) + **Coding Diff Review** (C3.5) + **Apply Approved Coding Changes** (C3.6) + **Coding Mutation Recovery** (C3.7) + **Build Awareness** (C4.1) + **Test Execution** (C4.2) + **Verification Diagnostics Feedback** (C4.3) + **Fix From Verification Failure** (C4.4) + **Verification Summary** (C4.5) + **Bounded Coding Verification Loop** (C4.6) + **Coding Goal Model** (C5.1) + **Goal Decomposition** (C5.2) + **Agent State Machine** (C5.3) + **Multi-Step Coding Execution** (C5.4) + **Agent Context Maintenance** (C5.5) + **Approval and Autonomy Policy** (C5.6) + **Agent Progress UX** (C5.7) + **Agent Pause, Resume, and Cancellation** (C5.8) + **Agent Failure and Recovery** (C5.9) + **Coding Goal Completion** (C5.10) + **Long-Running Coding Sessions** (C5.11) + **Agentic Coding Experience** (C5.12) + **Resume Integrity Hardening** (C5.14).

**Target UX:**

Conversation → chat-only interface

Coding → conversation stays; fuller IDE polish on top of the shipped shell
(Monaco / Terminal / Git panel / Workspace Intelligence are **Current**; deeper
LSP tooling and Git merge/rebase remain Target)

Creation → conversation stays; canvas appears

Research → conversation stays; sources and notes appear

**B2.1:** `WorkspaceSnapshot` is the immutable observation of the live Coding
workspace (project / root / kind / files / selection / cursor / branch /
language / package manager / build system / timestamp). Host observation is
ambient (`ContextMaintenance`); prepare merges the latest completed snapshot.
Context Engine consumes via session inputs. See
[docs/workspace-snapshot.md](docs/workspace-snapshot.md).

**B2.2:** Ambient `ContextMaintenance` refreshes `WorkspaceSnapshot` on editor /
selection / git / diagnostics / terminal / project changes without blocking
conversation. Prepare merges the latest **completed** snapshot only — never
rebuilds a ContextBundle, never reasons, never calls LLMs, never executes tools,
and (Sprint **B2.13.2**) never probes toolchain markers on the conversational
path. Planner remains the sole request owner. See
[docs/context-maintenance.md](docs/context-maintenance.md).

**B2.3:** `EditorSnapshot` is the read-only editor intelligence observation
(active file / open editors / cursor / selection / symbol / enclosing function /
enclosing type / semantic tokens / references / diagnostics / code lens / hover).
Context providers consume it; Planner and Reasoning never call LSP for assemble.
Ambient refresh may use read-only `LspProvider` enrichment. See
[docs/editor-snapshot.md](docs/editor-snapshot.md).

**B2.4:** `ProjectSnapshot` is the read-only project intelligence observation
(metadata / languages / frameworks / package manager / dependency summary /
cargo·npm metadata / repository metadata / workspace layout). Ambient
`ContextMaintenance` owns FS observation; `ProjectProvider` consumes the session
snapshot. Planner never scans projects; no filesystem scanning during requests.
See [docs/project-snapshot.md](docs/project-snapshot.md).

**B2.5:** `GitSnapshot` is the read-only Git intelligence observation (branch /
HEAD / dirty / staged / untracked / conflicts / recent commits). Ambient
`ContextMaintenance` owns read-only `GitProvider` refresh; `GitStatusProvider`
exposes capped summaries. Reasoning never runs git commands. See
[docs/git-snapshot.md](docs/git-snapshot.md).

**B2.6:** `RuntimeSnapshot` is the read-only runtime intelligence observation
(latest cargo check / build / tests, terminal output summary, running processes,
recent failures). Ambient `ContextMaintenance` observes Coding terminal sessions;
TerminalProvider owns PTY updates; `RuntimeProvider` exposes summaries.
Conversation never blocks waiting for runtime; observe never re-runs cargo.
See [docs/runtime-snapshot.md](docs/runtime-snapshot.md).

**B2.7:** Workspace Intelligence exposes `ContextCandidate` nodes instead of
raw bundle sections. Every workspace feed proposes candidates; Context Policy
evaluates relevance, recency, importance, privacy, and budget; only selected
candidates become a `ContextBundle`. Providers never assemble bundles; Planner
ownership unchanged. Migration completed in **B2.13.1** (every provider
implements `propose_candidates`). See
[docs/context-candidates.md](docs/context-candidates.md).

**B2.8:** Context Policy chooses workspace context with deterministic selection
profiles (no AI scoring). Examples: `hello` → Conversation + Memory; `why won't
this compile?` → Conversation + Diagnostics + Current file + Terminal +
Selection; `summarize this project` → Project + Filesystem + Architecture + Git.
All heuristics are documented. See
[docs/context-selection.md](docs/context-selection.md).

**B2.9:** Workspace Memory remembers Coding workspace activity (recent edits,
recently opened files, recent builds, recent failures, current coding
objective). Distinct from Conversation Memory. Context Policy decides when to
include it. See [docs/workspace-memory.md](docs/workspace-memory.md).

**B2.10:** Environmental Resolution — Planner resolves ambiguous workspace
deixis (`rename this`, `fix it`, `why?`, `clean this up`) from Workspace
Intelligence before Reasoning. LLMs never invent workspace references.
See [docs/environmental-resolution.md](docs/environmental-resolution.md).

**B2.11:** Workspace Diagnostics — Developer Diagnostics expose Workspace
Intelligence (snapshot freshness, provider timings, maintenance status,
candidate selection, policy decisions, context budget). Never written to the
conversation transcript. See
[docs/workspace-diagnostics.md](docs/workspace-diagnostics.md).

**B2.12:** Constitutional Audit — Workspace Intelligence (B2.1–B2.11) verified
against VISION / PRINCIPLES / NON_GOALS / ARCHITECTURE / ROADMAP / docs/.
Ownership holds (Planner orchestrates, Workspace observes, providers passive,
ContextEngine sole ContextBundle factory). Residuals noted at audit time were
closed by **B2.13.1**–**B2.13.3** (candidate migration, ambient-only prepare,
Monaco selection). Documentation synchronized in **B2.13.4**.

**B2.13.1:** Complete Context Candidate Migration — every Context Provider
exposes `propose_candidates()`; production assemble no longer relies on
contribution→candidate trait fallback; Context Policy evaluates candidates
uniformly; ContextEngine remains sole bundle factory. See
[docs/context-candidates.md](docs/context-candidates.md).

**B2.13.2:** Remove Synchronous Prepare Probes — conversational
`prepare_context_session` never rebuilds WorkspaceSnapshot or calls
`observe_toolchain`; ambient ContextMaintenance owns observation; prepare
merges the latest completed snapshot (or schedules refresh if missing). See
[docs/workspace-snapshot.md](docs/workspace-snapshot.md) and
[docs/context-maintenance.md](docs/context-maintenance.md).

**B2.13.3:** Monaco Selection Intelligence — Monaco text selection (range +
text) synchronizes into CodingState and the ambient Workspace/EditorSnapshot
pipeline; Environmental Resolution binds `"Explain this."` / `"Rename this."` /
`"Clean this up."` from Workspace Intelligence. Cursor tracking preserved; no
Monaco types in Planner/ContextEngine; no sync LSP. See
[docs/editor-snapshot.md](docs/editor-snapshot.md) and
[docs/environmental-resolution.md](docs/environmental-resolution.md).

**B2.13.4:** Documentation Synchronization — ARCHITECTURE / ROADMAP /
context / workspace-intelligence / providers / experience updated so shipped
Workspace Intelligence (including **Current** GitSnapshot, RuntimeSnapshot, and
Context Candidate Graph) is documented accurately; stale B2 Target residuals
removed. Docs only — no implementation changes. See
[docs/workspace-intelligence.md](docs/workspace-intelligence.md).

**C0.1:** Coding Actions — Quick Action Bar submits typed `CodingAction`
intents (`ExplainSelection` / `ExplainFile` / `EditSelection` /
`RefactorSelection` / `SearchWorkspace` / `RunProject` / `OpenCodingActions`)
as Conversation First turns. UI emits intents only; Planner routes;
Workspace Intelligence supplies context; Run uses reviewed Execution Plans.
Honest replies when search/run cannot proceed. See
[docs/coding-actions.md](docs/coding-actions.md).

**C0.2:** Code Talk — teach Jaymi how to talk about code (engineering-partner
voice). File explanations lead with the filename, then Purpose ·
Responsibilities · Interesting observations · Recommendations · Next actions
(`[Review]` chips). Bans hedged narrator phrases (“appears to be…”). Soft
scaffolds and Reasoning instructions share the same rules. See
[docs/code-talk.md](docs/code-talk.md).

**C1.1:** Coding Understanding — Understand Before Acting. Explain actions and
free-text understanding questions return structured Purpose / Responsibilities /
Key Symbols / Relationships / Potential Issues / Suggested Next Actions built
only from already-assembled Workspace Intelligence. No new context systems,
provider bypasses, filesystem scans, tools, or edits. Constitutional audit in
[docs/coding-understanding.md](docs/coding-understanding.md).

**C1.2:** Project Understanding — whole-project orientation from
WorkspaceSnapshot / ProjectSnapshot / GitSnapshot / Workspace Memory /
Conversation (assembled sections only). Structured Overview · Architecture ·
Important Modules · Relationships · Activity & Risks · Suggested Next Actions.
Angles: overview, architecture, feature placement, important modules. No file
modifications, tools, planning, or execution. Constitutional audit in
[docs/project-understanding.md](docs/project-understanding.md).

**C1.3:** Coding Review — structured Strengths · Weaknesses · Potential Bugs ·
Complexity · Performance · Maintainability · Architecture for file / function /
changes review requests. No edits, execution, or Execution Plans. Constitutional
audit in [docs/coding-review.md](docs/coding-review.md).

**C1.4:** Generation Planning — Coding Plans (Plan · Files to Create · Files to
Modify · Dependencies · Estimated Risk · Summary) before any code generation.
Examples: “Build Pong.”, “Create a parser.”, “Write tests.” No codegen, tool
execution, file writes, or Execution Plans. Constitutional audit in
[docs/coding-plan.md](docs/coding-plan.md).

**C1.5:** Code Generation — approved Coding Plans become CreateFile /
ModifyFile / DeleteFile ops; Planner converts them into Execution Plans with
mandatory Review Before Action. No provider writes or LLM edits directly.
Constitutional audit in [docs/code-generation.md](docs/code-generation.md).

**C1.6:** Coding Execution Plans — extend the universal Review Card for coding
workflows (Execution Plan · Files affected · Diff Preview · Risk ·
Approve / Modify / Cancel). Every mutation still uses the existing Execution
Plan architecture; no coding or editor bypass. Constitutional audit in
[docs/coding-execution-plans.md](docs/coding-execution-plans.md).

**C1.7:** Multi-file Intelligence — reason across the project for rename
symbol / move module / extract class / split file. Planner determines affected
files from Workspace Intelligence; Execution Plans summarize changes; no
direct LLM editing; Review required. Constitutional audit in
[docs/multi-file-intelligence.md](docs/multi-file-intelligence.md).

**C1.8:** Pair Programming Experience — Conversation First partner flow across
Understanding · Review · Planning · Generation · Modification · Execution.
Suggested next actions are Planner-generated; Quick Actions mirror Planner
state (not hardcoded UI). Constitutional audit in
[docs/pair-programming.md](docs/pair-programming.md).

**C2.1:** Intelligent Code Search — Planner-owned NL project search via existing
Search Knowledge / citations / Context pipeline (lexical + WI references).
Read-only; Conversation First; capped evidence; diagnostics for what was
searched. Constitutional audit in [docs/code-search.md](docs/code-search.md).

**C2.2:** Symbol Navigation — Planner-owned definition / references /
implementations / callers / enclosing / dependents via Workspace Intelligence
and read-only `language_server` (no rename, no Reasoning→LSP). Constitutional
audit in [docs/symbol-navigation.md](docs/symbol-navigation.md).

**C2.3:** Code Relationship Tracing — bounded multi-hop investigation
(identify → define → callers → related → optional Search/LSP) with hard limits
and an investigation-chain diagnostic. Constitutional audit in
[docs/relationship-tracing.md](docs/relationship-tracing.md).

**C2.4:** Deep Codebase Investigation — general read-only multi-source
investigation (Planner plans evidence → Search / symbols / project → reads →
relationships → Context Policy → ContextEngine → Reasoning). Bounded,
observable; diagnostics not dumped into chat. Constitutional audit in
[docs/deep-investigation.md](docs/deep-investigation.md).

**C2.5:** Codebase Q&A — natural conversational questions routed by complexity
across Search / symbols / relationship / investigation; grounded claims;
conflicts surfaced; no special workspace. Constitutional audit in
[docs/codebase-qa.md](docs/codebase-qa.md).

**C2.7.1:** Codebase Reading Diagnostics Parity — `deep_investigation` and
`codebase_qa` operational data surface on existing `LastPlannerActivity` /
Coding Diagnostics (same ownership model as C2.1–C2.3). No second diagnostics
system; conversation transcript unchanged.

**C2.7.2:** Unified Codebase Evidence → Reasoning Handoff — C2.1–C2.5 share one
evidence representation and Planner-owned
`retrieval → evidence → ContextPolicy / ContextEngine → PromptBuilder → Reasoning`
path when Reasoning is available and the ask is non-trivial; deterministic
markdown otherwise. No second assembler / prompt builder; C2 never calls
ReasoningProvider. Constitutional audit in
[docs/codebase-evidence-handoff.md](docs/codebase-evidence-handoff.md).

**C2 closed.** Codebase reading is permanently closed (C2.7.3 hardening audit).

**C3.1:** Coding Edit Intent — classify mutation vs read requests; structure a
Planner-owned `CodingEditObjective` (targets, scope, ambiguities, clarification).
No file writes, no mutation tools, no executable Execution Plan. Soft path after
C2 / C1.5 / C1.7. Constitutional audit in
[docs/coding-edit.md](docs/coding-edit.md).

**C3.2:** Change Investigation — after Understood edit intent, reuse C2 search /
symbol / relationship / deep helpers (Planner-mediated, read-only) to map
affected files, symbols, deps, tests, config, side effects, and unknowns.
Light mode on clarification. No patches / Execution Plan. Spec:
[docs/change-investigation.md](docs/change-investigation.md).

**C3.3:** Coding Plan from Change Investigation — Planner-owned `CodingPlan`
(objective, rationale, create/modify/remove, symbols, deps, tests, docs,
ordering, risk, unresolved questions, constitutional conflicts). No file
contents, tools, or executable mutation plan. Spec:
[docs/change-coding-plan.md](docs/change-coding-plan.md).

**C3.4:** Patch Generation — targeted create/modify/delete/rename proposals from
an approved mutation Coding Plan; validate vs current file state; convert valid
patches into the existing reviewed ExecutionPlan path. No automatic apply. Spec:
[docs/patch-generation.md](docs/patch-generation.md).

**C3.5:** Coding Diff Review — multi-file +/− Diff Preview on the **same**
ExecutionPlan / ReviewCard / ReviewIntent path (files, ops, rationale, risk,
dependencies, validation). Approve / Modify / Cancel unchanged; no pre-approval
writes; no coding-only approval bypass. Spec:
[docs/coding-diff-review.md](docs/coding-diff-review.md).

**C3.6:** Apply Approved Coding Changes — after Approve, Planner executes the
frozen generation batch via existing `write_file` / `manage_path` → providers,
preserving order, stopping on failure with partial `ExecutionSummary`, and
recording history. No coding bypass / retry loops. Spec:
[docs/coding-apply.md](docs/coding-apply.md).

**C3.7:** Coding Mutation Recovery — baseline preflight before writes, refuse
stale/invalid sources, actionable recovery on fail/partial/cancel, no silent
retry or destructive auto-rollback; further mutation needs a new reviewed plan.
Spec: [docs/coding-mutation-recovery.md](docs/coding-mutation-recovery.md).

**C4.1:** Build Awareness — Planner-owned, read-only discovery of how the active
project is built and which verification commands are appropriate (`VerificationTarget`
from ProjectSnapshot / WI evidence). No command execution, no second discovery
system, no Reasoning shell. Spec: [docs/build-awareness.md](docs/build-awareness.md).

**C4.2:** Test Execution — Planner selects grounded test commands (C4.1), runs them
via Terminal Tool under permission/ToolRisk review, captures exit/output/duration,
bounds large logs, and publishes outcomes to RuntimeSnapshot for Context. No
auto-mutation or silent retries. Spec: [docs/test-execution.md](docs/test-execution.md).

**C4.3:** Verification Diagnostics Feedback — parse build/test output into structured
`VerificationEvidence` (kind, errors, warnings, files, tests, bounded excerpts),
feed existing Context diagnostics (`source=verification`), and explain failures
conversationally without raw log dumps or automatic repair. Spec:
[docs/verification-diagnostics.md](docs/verification-diagnostics.md).

**C4.4:** Fix From Verification Failure — bridge structured verification failures into
C2 investigation + C3 Coding Plan (proposal only). No auto-mutation, auto-approve,
or silent retries; mutations still use Review Before Action. Spec:
[docs/fix-from-verification.md](docs/fix-from-verification.md).

**C4.5:** Verification Summary — concise conversational outcome from ExecutionSummary
+ VerificationEvidence (what changed / verified / passed / failed / remains / next).
Never invents runs; recommended fixes are chips into C4.4, not auto-executed. Spec:
[docs/verification-summary.md](docs/verification-summary.md).

**C4.6:** Bounded Coding Verification Loop — Planner-owned Apply→Verify→(optional
reviewed Fix) cycles with hard `MAX_CYCLES`, cancel/stop, and no auto-approve or
silent mutation retry. Not C5. Spec:
[docs/coding-verification-loop.md](docs/coding-verification-loop.md).

**C5.1:** Coding Goal Model — Planner-owned multi-step coding objective
(`CodingGoal` / `CodingGoalStore`). Represents *what* the user wants across turns;
not a Coding Plan, Execution Plan, or verification result. Create/cancel/complete/block
only — no tools, mutation, decomposition, autonomy, or progress UX. Spec:
[docs/coding-goal.md](docs/coding-goal.md).

**C5.2:** Goal Decomposition — Planner-owned proposed task graph on an active
Coding Goal (statuses, deps, cycle detection). Never executes tasks, mutates files,
or auto-approves; C2/C3/C4 reused as later hints only. Spec:
[docs/goal-decomposition.md](docs/goal-decomposition.md).

**C5.3:** Agent State Machine — Planner-owned bounded lifecycle for an active
Coding Goal (Idle→…→Completed/Failed/Cancelled, Pause, Blocked, Correcting).
Explicit events only; LLM/UI cannot invent transitions; Completed needs evidence;
AwaitingApproval/Verifying do not mutate. Spec:
[docs/agent-state-machine.md](docs/agent-state-machine.md).

**C5.4:** Multi-Step Coding Execution — Planner-owned orchestration linking Goals →
Tasks → C2/C3/C4 (one in-progress task; continue/advance; evidence-gated complete;
no second plan/review; no auto-approve / auto-correct mutate). Spec:
[docs/multi-step-execution.md](docs/multi-step-execution.md).

**C5.5:** Agent Context Maintenance — compact Planner-owned goal context for
multi-step work without stuffing full history into every model request; reuses
ContextEngine + ContextPolicy + Workspace Intelligence (no AgentContextEngine).
Spec: [docs/agent-context-maintenance.md](docs/agent-context-maintenance.md).

**C5.6:** Approval and Autonomy Policy — default READ/OBSERVE vs MUTATION/SIDE_EFFECT
boundaries; reuses Permission + ToolRisk + Review (no AgentApproval, no autonomy
modes); approvals bind to plan_id + task_id. Spec:
[docs/approval-autonomy-policy.md](docs/approval-autonomy-policy.md).

**C5.7:** Agent Progress UX — derive-only progress view of an active Coding Goal
(goal / phase / tasks / approval / verification / blockers) without CoT; Experience
mirrors Planner (no second agent state); Diagnostics stay separate. Spec:
[docs/agent-progress-ux.md](docs/agent-progress-ux.md).

**C5.8:** Agent Pause, Resume, and Cancellation — reliable user control over
Coding Goals (stop new ops on pause; fingerprint revalidation on resume; cancel
invalidates approvals, preserves history, never silent rollback); reuses C5.3
transitions + existing Tool/stream cancel (no parallel engine). Spec:
[docs/agent-pause-resume-cancel.md](docs/agent-pause-resume-cancel.md).

**C5.9:** Agent Failure and Recovery — goal-scoped failure classification and
bounded disposition (read-only investigate / new reviewed C3 plan / AwaitUser→Blocked /
Failed); never silent mutate, retry, approval reuse, or rollback. Spec:
[docs/agent-failure-recovery.md](docs/agent-failure-recovery.md).

**C5.10:** Coding Goal Completion — evidence-based completion ownership (acceptance
criteria Satisfied/Unsatisfied/Unknown/Waived; failed verification blocks success;
blocked cannot Complete; user-ended ≠ successfully completed). Spec:
[docs/coding-goal-completion.md](docs/coding-goal-completion.md).

**C5.11:** Long-Running Coding Sessions — bounded Planner store continuity across
many tasks/turns/pauses (C5.5 compact, C5.8 revalidate, C5.6 approval invalidate;
serde hitch; no agent DB; no background work). Spec:
[docs/agent-session-continuity.md](docs/agent-session-continuity.md).

**C5.12:** Agentic Coding Experience — compose C5.1–C5.11 into one Conversation First
path for complex objectives (“Build me a Pong game.” → goal → decompose → … →
evidence-based completion); Planner-orchestrated; no parallel agent; no CoT /
hidden FS / model→tool. Spec:
[docs/agentic-coding-experience.md](docs/agentic-coding-experience.md).

**C5.14:** Resume Integrity Hardening — pause/resume capture real ContextEngine
fingerprints; unavailable fingerprints are unknown/stale (never “unchanged”);
stale state invalidates approvals before continue; no second context assembler.
Spec: [docs/agent-pause-resume-cancel.md](docs/agent-pause-resume-cancel.md)
(C5.14 section).

⸻

Layer 7 — Tool Engine

Status: **Partial**

Objective

Implement capabilities using interchangeable tools.

Current tools (12; registered at boot)

* `search_files`
* `list_project_tree`
* `search_knowledge`
* `search_project_knowledge`
* `read_file`
* `write_file`
* `manage_path`
* `terminal`
* `git`
* `language_server`
* `query_inventory`
* `scan_filesystem`

Target tools (examples)

Coding → dedicated editor tool (beyond LSP rename / workspace shell)

Images → Local image model, Vision model

Search → Messages, Photos, and other sources

Git → merge / rebase / cherry-pick (status / stage / unstage / discard / commit are Current)

Tools are replaceable.

Capabilities remain constant.

⸻

Layer 8 — Provider Ecosystem

Status: **Partial**

Objective

Allow Jaymi to connect to external systems.

Current providers (6; bound at boot)

* `filesystem`
* `terminal` (PTY)
* `git` (status / stage / unstage / discard / commit)
* `lsp` (Rust Analyzer)
* `embedding.local`
* `ocr.placeholder` (architecture only — OCR capability remains Planned)

Target examples

* Local models / AI providers
* Messages, Calendar, Email, Notes, Browser
* Git merge / rebase / cherry-pick
* Installable / enableable provider plugins

Every provider follows the same interface.

There is no ProviderManager — discovery uses ProviderRegistry; execution uses tool-bound instances.

⸻

Layer 9 — Daily Driver

Status: **Target**

Objective

Make Jaymi the first application opened every day.

Users should be able to:

* Chat
* Code
* Search
* Create
* Organize
* Automate

without needing to think about which application to open.

The computer becomes conversational.

⸻

Engineering Principles

Every contribution should improve one or more of the following:

* Simplicity
* Privacy
* Extensibility
* Transparency
* Local-first execution
* Project awareness
* Context awareness
* User ownership

⸻

Non-Goals

Jaymi is not trying to:

* Build a proprietary language model
* Replace every application overnight
* Depend on a single AI provider
* Require cloud services
* Lock users into one ecosystem

⸻

Success

Jaymi succeeds when interacting with a computer feels like talking to someone who understands your work.

The user should no longer think:

“Which application should I open?”

Instead, they simply ask:

“Can you help me do this?”

Jaymi figures out the rest.
