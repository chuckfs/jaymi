Roadmap

Jaymi is being built as a collection of layers.

Each layer provides new capabilities while remaining stable enough to support everything built on top of it.

The objective is not to ship features as quickly as possible.

The objective is to build a personal AI environment that can grow for years without requiring major architectural rewrites.

⸻

Layer 0 — Foundation

Objective

Build the foundation that every other component depends on.

Includes

* Desktop application
* Core architecture
* Planner
* Provider framework
* Tool framework
* Permission system
* Configuration
* Logging
* SQLite database

Exit Criteria

Jaymi launches and can execute a simple tool through the planner.

⸻

Layer 1 — Knowledge Engine

Objective

Teach Jaymi what exists on the computer.

Responsibilities

* File discovery
* Metadata indexing
* Background indexing
* Incremental updates
* Local database

Supported Sources

* Files
* Folders
* Downloads
* Documents

Exit Criteria

Jaymi can answer:

“What exists?”

without opening Finder.

Status

Implemented through the conversation shell:

* Recursive filesystem discovery
* SQLite metadata index (`indexed_files`)
* Boot-time background indexing of configured roots
* Incremental root replace on rescan
* Planner intents for existence queries and index refresh

⸻

Layer 2 — Understanding Engine

Objective

Teach Jaymi to understand content rather than filenames.

Responsibilities

* PDF parsing
* DOCX parsing
* Markdown parsing
* OCR
* Image understanding
* Metadata extraction
* Summaries
* Embeddings

Exit Criteria

Jaymi can answer:

“What is this?”

instead of only

“What is this file called?”

⸻

Layer 3 — Search Engine

Objective

Build semantic retrieval across every knowledge source.

Features

* Semantic search
* Hybrid search
* Ranking
* Citations
* Preview

Example

Instead of

biology_final_v7.pdf

Users ask

Find my biology paper about fungi.

Exit Criteria

Search is based on meaning rather than filenames.

⸻

Layer 4 — Memory Engine

Objective

Allow Jaymi to remember over time.

Memory Types

Conversation Memory

Temporary memory for one conversation.

⸻

Project Memory

Long-term memory attached to a project.

⸻

Personal Memory

Persistent user preferences and important facts.

Exit Criteria

Jaymi remembers intentionally instead of accidentally.

⸻

Layer 5 — Project Engine

Objective

Teach Jaymi that work happens inside projects.

A project is not just a folder.

A project includes:

* Files
* Source code
* Conversations
* Documentation
* Architecture
* Decisions
* Tasks
* History

Exit Criteria

Users can simply say:

Continue working on Jaymi.

⸻

Layer 6 — Capability Engine

Objective

Give Jaymi useful abilities.

Capabilities include:

* Chat
* Coding
* Image generation
* Search
* Vision
* Internet
* Automation
* File management
* Terminal

Capabilities are abstract.

They describe what Jaymi can do.

Not how it does it.

⸻

Layer 7 — Tool Engine

Objective

Implement capabilities using interchangeable tools.

Examples

Coding

↓

Local editor

↓

Language server

↓

Terminal

↓

Git

⸻

Images

↓

Local image model

↓

Vision model

⸻

Search

↓

Filesystem

↓

Messages

↓

Documents

↓

Photos

Tools are replaceable.

Capabilities remain constant.

⸻

Layer 8 — Provider Ecosystem

Objective

Allow Jaymi to connect to external systems.

Examples

* Local models
* AI providers
* Git
* Messages
* Calendar
* Email
* Notes
* Browser
* Terminal
* Future providers

Every provider follows the same interface.

⸻

Layer 9 — Daily Driver

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