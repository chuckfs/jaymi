Tools

**Status: Partial** — framework + twelve tools · **Target:** broad catalog

Tools are the executable building blocks of Jaymi.

A tool performs one specific operation on behalf of the Planner.

Tools do not make decisions.

They do not reason.

They do not remember.

They simply perform work.

The Planner decides what should happen.

The Tool performs it.

Adding a new tool-backed path requires **tool registration** (boot) and **route
registration** (`ToolRouteTable` / `IntentToolHandler`) plus Intent → Capability
mapping in the Decision Engine. It must **not** require a new `Planner::handle`
match arm. The Planner still owns Execution Plan → Review → Execute.

⸻

Current tools

All twelve are registered at boot and available to the Planner (tool availability = registered + healthy). Capability fulfillment is separate — see [capabilities.md](capabilities.md).

| Tool | Base risk | Capability ads | Bound provider id | Review (effective) |
|------|-----------|----------------|-------------------|--------------------|
| `search_files` | Workspace | Search | `filesystem` | No (unless escalated) |
| `list_project_tree` | Workspace | Search | `filesystem` | No |
| `search_knowledge` | Safe | Search | *(SearchEngine; metadata may say filesystem)* | No |
| `search_project_knowledge` | Workspace | Search | `filesystem` | No |
| `read_file` | Workspace | ReadDocuments | `filesystem` | No |
| `write_file` | Modify | FileManagement | `filesystem` | Yes |
| `manage_path` | Modify | FileManagement | `filesystem` | Yes (permanent delete → Destructive) |
| `terminal` | Destructive | ExecuteTerminalCommands, Code | `terminal` | Yes |
| `git` | Workspace | Code | `git` | Mutate → Yes; discard → Destructive |
| `language_server` | Workspace | Code | `lsp` | Rename → Yes |
| `query_inventory` | Safe | Discover | *(SearchEngine; metadata may say filesystem)* | No |
| `scan_filesystem` | Workspace | Index | *(Discovery; metadata may say filesystem)* | No |

`manage_path` — mkdir / rename / delete; delete executes Planner-chosen
`DeletionMethod` (Trash by default, Permanent when requested or Trash is
unavailable). Tools never choose the strategy.

`terminal` — ensure / run / create / rename / kill PTY sessions (`TerminalOperation`) through the Terminal Provider.

Every tool declares a **ToolRisk**:

| Risk | Meaning | Review |
|------|---------|--------|
| Safe | Read-only indexes / inventory | No |
| Workspace | Browse / open / switch workspace | No |
| Modify | Edit local user data | Yes |
| Destructive | Delete or permanently change data | Yes |
| External | Internet, APIs, cloud | Yes |

| Tool | Risk |
|------|------|
| `query_inventory`, `search_knowledge` | Safe |
| `search_files`, `list_project_tree`, `search_project_knowledge`, `read_file`, `scan_filesystem`, `git` (base), `language_server` (base) | Workspace |
| `write_file`, `manage_path` (base) | Modify |
| `terminal` | Destructive |

Effective risk may escalate per invocation (e.g. permanent `manage_path` delete → Destructive; Trash delete → Modify; git discard → Destructive; git mutate / LSP rename → Modify; network-required → External).

The Planner derives review from ToolRisk (not from PermissionEngine hardcodes). Every approval path shares one lifecycle: ExecutionPlan → Review → Planner → Approved → Execution via `Application::submit_review` (`ReviewIntent`). Conversation Review Cards leave the card pending for explicit Approve / Cancel / Modify. Coding / Git / Terminal / Explorer / LSP rename gestures may auto-submit `ReviewIntent::Approve` after an explicit user action (Save, Delete, Run, Commit, …) through `complete_user_initiated` — still the same Planner gate; tools never execute directly.

⸻

Philosophy

Tools answer one question:

“How can this task be completed?”

Every tool performs exactly one well-defined job.

### Current examples

* Search files
* Read a document
* Index a folder / query inventory
* Git status / stage / unstage / discard / commit (via `git` → Git Provider)
* Ensure / run / create / rename / kill a terminal session (via `terminal` → Terminal Provider); `cargo test`, `git status`, `npm test`, `python …` all run the same way — a persistent local PTY shell

### Target examples

* Generate an image
* Search messages
* Parse a PDF (understanding pipeline exists; dedicated tool catalog expands)
* Git merge / rebase / cherry-pick

Tools should remain focused, deterministic, and reusable.

⸻

Architecture

User
    │
    ▼
Planner
    │
    ▼
Capability
    │
    ▼
Tool
    │
    ▼
Provider
    │
    ▼
Resource

The Planner never executes Providers directly.

Every interaction passes through a Tool.

⸻

Responsibilities

A Tool is responsible for:

* Validating input
* Calling the appropriate Provider
* Handling execution
* Returning structured output
* Reporting failures

A Tool is not responsible for:

* Planning
* Choosing providers
* Memory
* Permissions
* User interaction

⸻

Tool Lifecycle

Every Tool follows the same lifecycle.

Request
↓
Validate
↓
Execute
↓
Return Result
↓
Cleanup

Tools should not maintain long-term state.

⸻

One Tool, One Responsibility

Every Tool should perform exactly one operation.

Good examples:

* Search Files
* Read File
* Write File
* Rename File
* Parse PDF
* OCR Image
* Execute Command
* Generate Image
* Search Messages

Avoid tools that try to perform multiple unrelated tasks.

⸻

Inputs

Every Tool receives structured input.

Typical input may include:

* Parameters
* Context
* Active Project
* Working Directory
* User Options

Tools should never parse natural language directly.

That responsibility belongs to the Planner and Reasoning Engine.

⸻

Outputs

Every Tool returns structured results.

A result may contain:

* Success / Failure
* Data
* Metadata
* Citations
* Errors
* Suggested next actions

The Planner determines how those results are presented to the user.

⸻

Stateless Design

Tools should remain stateless whenever possible.

A Tool should not remember previous executions.

Persistent information belongs in the Memory Engine.

⸻

Tool Categories

Jaymi organizes Tools into logical categories.

Search

Examples:

* Search Files
* Search Messages
* Search Notes
* Search Projects

⸻

Reading

Examples:

* Read File
* Parse PDF
* Read DOCX
* OCR Image
* Analyze Metadata

⸻

Writing

Examples:

* Create File
* Update File
* Rename File
* Move File

⸻

Coding

Examples:

* Edit Code
* Apply Patch
* Run Formatter
* Execute Tests
* Commit Changes

⸻

AI

Examples:

* Generate Text
* Summarize
* Explain
* Translate
* Generate Image

⸻

Automation

Examples:

* Launch Application
* Execute Shortcut
* Run Terminal Command
* Open URL

⸻

Import

Examples:

* Import Conversations
* Import Documents
* Import Project
* Import Archive

⸻

Provider Relationship

Tools execute work through Providers.

Providers are bound into tools at boot (concrete `Arc` instances). There is no
runtime ProviderManager that selects a provider for each request.

Example:

Search Files Tool
↓
Filesystem Provider
↓
Local Filesystem

Another example:

Generate Image Tool
↓
Image Provider
↓
Local Image Model

The Tool knows which Provider to call.

The Provider knows how to interact with the resource.

⸻

Failure Handling

A Tool should never crash the Planner.

Instead it should return structured failures.

Examples:

* Invalid input
* Permission denied
* Resource unavailable
* Provider offline
* Timeout
* Corrupt data

The Planner decides how to recover.

⸻

Composability

Complex tasks should be composed from multiple simple Tools.

Example:

Find yesterday's AI-generated images.
↓
Search Files Tool
↓
Read Metadata Tool
↓
Analyze Image Tool
↓
Generate Results

Small tools are easier to test, replace, and reuse.

⸻

Design Principles

Every Tool should be:

* Focused
* Deterministic
* Stateless
* Testable
* Reusable
* Replaceable

A Tool should do one thing exceptionally well.

⸻

Tool Metadata

Every Tool should describe itself using metadata.

This allows the Planner to make intelligent decisions without hardcoding provider-specific logic.

Rather than asking:

“Should I use Tool A or Tool B?”

The Planner asks:

“Which available Tool best satisfies this request according to the current policy?”

Every Tool should expose the following information.

⸻

Identity

* Tool ID
* Human-readable name
* Version
* Description
* Provider
* Supported capabilities

⸻

Execution

Describe how the Tool behaves during execution.

Execution Mode

Possible values include:

* Synchronous
* Asynchronous
* Streaming

⸻

Estimated Runtime

Approximate execution time.

Examples:

* Instant (<100 ms)
* Fast (<1 s)
* Medium (<10 s)
* Slow (>10 s)

This value is only an estimate and should not be treated as a guarantee.

⸻

Resource Cost

Represents the relative computational expense of running the Tool.

Possible values:

* Very Low
* Low
* Medium
* High
* Very High

This allows the Planner to prefer lightweight solutions whenever possible.

⸻

Memory Usage

Estimated memory requirements.

Examples:

* Tiny
* Small
* Moderate
* Large
* Extreme

This enables future scheduling decisions on memory-constrained systems.

⸻

GPU Requirements

Examples:

* None
* Optional
* Recommended
* Required

⸻

Privacy

Every Tool declares where execution occurs.

Possible values:

* Local Only
* Cloud Only
* Local Preferred
* Cloud Optional
* Hybrid

The Planner uses this information to respect the user’s privacy preferences.

⸻

Internet Requirements

Possible values:

* Never
* Optional
* Required

The Planner should avoid unnecessary internet access whenever possible.

⸻

Permissions

Every Tool declares the permissions required before execution.

Examples:

* Read Files
* Write Files
* Delete Files
* Read Messages
* Run Terminal Commands
* Network Access

The Planner remains responsible for enforcing permissions.

⸻

Reliability

Tools should describe their expected reliability.

Possible values:

* Experimental
* Stable
* Production

The Planner may prefer more stable tools when multiple options exist.

⸻

Result Type

Every Tool should describe the type of data it returns.

Examples:

* Text
* Image
* Structured Data
* File
* Stream
* Diff
* Search Results

This allows downstream components to process results consistently.

⸻

Supported Context

Tools should declare which contextual information they can use.

Examples:

* Active Project
* Conversation
* Memory
* Files
* Search Results
* Git Repository
* Terminal State

The Planner should avoid supplying unnecessary context.

⸻

Tool Selection

When multiple Tools satisfy the same Capability, the Planner evaluates them using their metadata.

Typical considerations include:

* User preferences
* Privacy policy
* Available hardware
* Internet availability
* Required permissions
* Estimated execution time
* Resource cost
* Reliability
* Provider availability

Example:

Generate Image

Available Tools

• Local FLUX

* Local Only
* High GPU Cost
* Fast

• Cloud Image Provider

* Cloud Only
* Higher Quality
* Internet Required

If the user’s policy is “Offline First,” the Planner selects the local Tool automatically.

If local execution is unavailable or unsuitable, the Planner may request permission to use an alternative.

⸻

Design Principle

The Planner should never contain provider-specific rules.

Instead, every Tool should describe itself well enough that intelligent decisions emerge naturally from metadata.

Adding a new Tool should not require changing the Planner.

The Planner learns about new capabilities by inspecting Tool metadata rather than relying on hardcoded logic.

⸻

Long-Term Vision

As Jaymi grows, hundreds of Tools may exist.

The Planner should not need to know how they work.

It only needs to know:

* What the Tool does.
* Which Capability it satisfies.
* Which Provider executes it.

This separation allows Jaymi to grow indefinitely while keeping the architecture clean, modular, and predictable.
