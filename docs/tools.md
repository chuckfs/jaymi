Tools

Tools are the executable building blocks of Jaymi.

A tool performs one specific operation on behalf of the Planner.

Tools do not make decisions.

They do not reason.

They do not remember.

They simply perform work.

The Planner decides what should happen.

The Tool performs it.

⸻

Philosophy

Tools answer one question:

“How can this task be completed?”

Every tool performs exactly one well-defined job.

Examples include:

* Search files
* Read a document
* Execute a terminal command
* Generate an image
* Create a Git commit
* Parse a PDF
* Index a folder

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

Long-Term Vision

As Jaymi grows, hundreds of Tools may exist.

The Planner should not need to know how they work.

It only needs to know:

* What the Tool does.
* Which Capability it satisfies.
* Which Provider executes it.

This separation allows Jaymi to grow indefinitely while keeping the architecture clean, modular, and predictable.
