Projects

**Status: Current Implementation** (core) · **Target:** Git, artifacts, import/convert, recommendations

Projects are the primary workspace in Jaymi.

A project is more than a folder containing files.

It is a persistent environment that combines code, documents, conversations, memories, tasks, decisions, and context into a single unit of work.

Projects allow users to stop working at any time and later resume exactly where they left off.

⸻

Philosophy

People work on projects.

Not folders.

Not repositories.

Not conversations.

A project represents everything related to accomplishing a goal.

Jaymi should understand projects the same way people do.

⸻

What Is a Project?

A project is a collection of related resources.

A project may contain:

### Current

* Source code / documents under a root
* Conversations
* Memories
* Tasks (as project memory kinds)
* Architecture / decision log entries
* Configuration under `.jaymi/`

### Target

* Images / Notes as first-class project resources
* Git repository integration
* Generated artifacts pipelines

Every project has its own identity.

⸻

Project Architecture

Project
    │
    ├── Files
    ├── Conversations
    ├── Memories
    ├── Tasks
    ├── Artifacts
    ├── Configuration
    ├── Git
    └── Metadata

Everything associated with the project belongs here.

⸻

Project Structure

Every project contains a hidden .jaymi directory.

Example:

MyProject/
src/
docs/
assets/
.jaymi/
    project.json
    conversations/
    memories/
    tasks/
    artifacts/
    cache/

The .jaymi directory stores project-specific data.

Project files remain separate from Jaymi’s internal metadata.

⸻

Global Index

Although each project owns its data, Jaymi maintains a global index.

The global index allows:

* Cross-project search
* Fast discovery
* Global memory retrieval
* Recently opened projects
* Project recommendations

The global index never replaces project ownership.

It simply points to project resources.

⸻

Project Identity Ownership

The Project Engine is the only owner of project identity.

Create, delete, register, list, and lookup by id or name exist only inside the Project Engine.

Other subsystems reference projects exclusively by `project_id`:

* Memory Engine — stores and restores memories keyed by `project_id` (no project registry)
* Search Engine — scopes search to a project root / path; never owns project records
* Knowledge Store — inventory is filtered through Project Engine by `project_id`

There is a single `ProjectContext` type, owned by the Project Engine. Memory returns a `ProjectMemoryBundle` of categorized memories for a given `project_id`.

⸻

Session Ownership

Exactly one project session lifecycle exists:

Application (`open_project` / `close_project` / `set_active_project`)
↓
Planner (`handle` → open/close orchestration)
↓
Project Engine (owns open state)
+ Memory (mirrors active project id for context assembly)

Continue / Open-by-id / Close intents use the same Planner helpers. Application never mutates Project Engine or Memory session state directly for open/close.

⸻

Project Lifecycle

Every project follows the same lifecycle.

Create
↓
Initialize
↓
Index
↓
Work
↓
Update
↓
Archive
↓
Restore

Projects should remain recoverable at every stage.

⸻

Creating Projects

Projects may be created by:

* Creating a new project
* Opening an existing folder
* Importing a repository
* Converting an existing conversation

The Planner should automatically initialize the .jaymi directory when appropriate.

⸻

Conversations

Projects own their conversations.

Conversations are stored separately from general chat history.

Every conversation may be attached to:

* One project
* Multiple tasks
* Related memories
* Generated artifacts

Conversations become part of project context.

⸻

Project Memory

Project Memory belongs exclusively to the project.

Examples include:

* Architecture decisions
* Design discussions
* Coding conventions
* TODOs
* Technical notes
* Meeting summaries

Project Memory should never become Personal Memory automatically.

⸻

Active Context

When a project is opened, Jaymi restores:

### Current

* Project Memory
* Recent Conversations
* Active Tasks (project memory kinds)
* Working Files / indexed project knowledge
* Planner Context (via Context Engine)

### Target

* Git Status
* Live IDE / working-tree file state

Users should not need to manually reload their workspace.

⸻

Tasks

Projects may contain tasks.

Examples:

* Current objective
* Future work
* Bugs
* Ideas
* Milestones

Tasks remain attached to the project.

⸻

Git Integration

**Status: Target**

Git is optional.

Projects may exist with or without version control.

When Git is available, Jaymi may understand:

* Branches
* Commits
* Diffs
* Status
* History

Git information becomes additional project context.

Git does not define the project.

⸻

Artifacts

**Status: Target**

Projects should retain generated outputs.

Examples:

* Images
* Documents
* Reports
* Diagrams
* Code patches

Artifacts remain searchable.

⸻

Project Search

### Current

Searching a project includes:

* Files / documents under the project root
* Memories
* Conversations
* Tasks (project memory)
* Architecture / decision entries

### Target

* Artifacts
* Git History

Project search should prioritize project context before global context.

⸻

Portability

A project should remain portable.

Copying the project folder should preserve:

* Conversations
* Memories
* Tasks
* Metadata
* Configuration

Opening the project on another Jaymi installation should restore the workspace automatically.

⸻

Isolation

Projects should remain isolated by default.

The Planner should only retrieve information from outside the active project when:

* The user requests it.
* A policy allows it.
* Cross-project reasoning is beneficial.

Project boundaries should be respected.

⸻

Design Principles

Projects should be:

* Portable
* Persistent
* Self-contained
* Searchable
* Recoverable
* Understandable
* Independent

A project should feel like a living workspace rather than a directory of files.

⸻

Long-Term Vision

Projects are where work happens.

Jaymi should understand projects deeply enough that users never need to explain the same context twice.

Opening a project should feel less like opening a folder and more like resuming a conversation that never truly ended.
