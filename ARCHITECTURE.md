Architecture

Jaymi is built around a single idea: every interaction begins with understanding intent.

The user never interacts directly with AI models, providers, tools, or services.

The user talks to Jaymi.

Jaymi decides everything else.

This document defines the architectural principles that guide the project.

⸻

The Core Philosophy

Jaymi is not an AI model.

Jaymi is not a chatbot.

Jaymi is not an IDE.

Jaymi is not an automation platform.

Jaymi is an intelligent environment that coordinates all of these systems through a single conversational interface.

The architecture is designed so that any individual component can be replaced without changing how users interact with Jaymi.

⸻

System Overview

                   User
                     │
                     ▼
              Conversation UI
                     │
                     ▼
             Planner (Kernel)
                     │
     ┌───────────────┼────────────────┐
     │               │                │
 Context Engine  Memory Engine  Permission Engine
     │               │                │
     └───────────────┼────────────────┘
                     │
             Capability Engine
                     │
             Tool Orchestrator
                     │
        ┌────────────┼────────────┐
        │            │            │
    Providers     Local AI     External AI
        │
        ▼
 Files • Messages • Git • Terminal • Photos •
 Documents • Browser • Calendar • Notes • etc.

Every request follows this architecture.

There are no shortcuts.

⸻

Planner

The Planner is the heart of Jaymi.

Every request passes through it.

The planner does not generate code.

It does not search files.

It does not execute terminal commands.

Instead, it decides:

* What is the user trying to accomplish?
* What information is required?
* Which memories are relevant?
* Which capabilities are needed?
* Which tools should be used?
* Does this require user approval?
* What should happen next?

The planner coordinates the system.

Nothing bypasses it.

⸻

Context Engine

The Context Engine determines what Jaymi should know before responding.

Rather than loading everything into a model, it builds only the context required for the current request.

Possible context includes:

* Active project
* Previous conversation
* Files
* Search results
* Git status
* Terminal output
* Notes
* Messages
* Browser history
* Retrieved memories

Context is assembled dynamically.

Context is never assumed.

⸻

Memory Engine

Memory allows Jaymi to improve over time while remaining transparent and user-controlled.

Jaymi maintains three independent memory systems.

Conversation Memory

Temporary.

Exists only within the current conversation.

Destroyed when the conversation ends unless promoted.

⸻

Project Memory

Attached to individual projects.

Includes:

* architecture decisions
* conversations
* documentation
* coding decisions
* TODOs
* important project history

Project memory travels with the project.

⸻

Personal Memory

Long-term information intentionally remembered.

Examples:

* preferences
* workflows
* writing style
* favorite tools

Personal memory is always editable.

The user owns every memory.

⸻

Capability Engine

Capabilities define what Jaymi knows how to do.

Examples include:

* Chat
* Search
* Code
* Vision
* Generate Images
* Browse the Web
* Read Documents
* Organize Files
* Execute Terminal Commands
* Automate Tasks

Capabilities describe behavior.

They do not describe implementation.

⸻

Tool Orchestrator

Tools are concrete implementations of capabilities.

A capability may require one tool or many.

For example:

Search

↓

Filesystem

↓

Messages

↓

Documents

↓

Photos

Coding

↓

Editor

↓

Git

↓

Language Server

↓

Terminal

Image Generation

↓

Image Model

↓

Vision Model

The planner selects tools automatically.

Users think in goals.

Jaymi thinks in tools.

⸻

Provider System

Providers connect Jaymi to resources.

Examples include:

* Filesystem
* Git
* Messages
* Mail
* Calendar
* Browser
* Photos
* Notes
* AI Models
* Future integrations

Providers expose consistent interfaces.

The rest of the system never depends on provider-specific behavior.

Providers can be added, removed, or replaced without affecting the planner.

⸻

Permission Engine

Jaymi should never surprise the user.

Potentially destructive actions require review.

Examples include:

* Editing files
* Running terminal commands
* Renaming folders
* Sending messages
* Deleting documents
* Modifying repositories

Every important action should include:

* What will happen
* Why it will happen
* What will change

The user remains in control.

⸻

Knowledge Pipeline

Every piece of information follows the same lifecycle.

Discover
↓
Read
↓
Understand
↓
Index
↓
Store
↓
Retrieve
↓
Reason
↓
Respond

Jaymi reasons over understanding—not raw files.

⸻

Projects

Projects are first-class citizens.

A project is more than a folder.

A project is a living workspace.

A project includes:

* Source code
* Documents
* Conversations
* Architecture
* Decisions
* Tasks
* Git history
* Project memory

When a project is opened, Jaymi restores its context automatically.

The user continues working rather than starting over.

⸻

Offline-First

Jaymi performs as much work locally as possible.

Examples include:

* Search
* Memory
* OCR
* Document parsing
* Embeddings
* Indexing
* Context construction
* Project awareness

Internet access is treated as another capability.

Cloud AI providers are optional.

The system functions without them.

⸻

AI Models

Models are interchangeable.

Jaymi never depends on a specific model.

Instead, the planner requests capabilities.

Providers determine how those capabilities are fulfilled.

As models improve, Jaymi improves without architectural changes.

⸻

Extensibility

Every major subsystem is replaceable.

Developers should be able to extend Jaymi by adding:

* Providers
* Tools
* Capabilities
* Importers
* Memory strategies
* Search engines
* AI models

The planner should not require modification to support new functionality.

⸻

Guiding Principles

Every architectural decision should reinforce these principles.

* The planner is the center of the system.
* The user owns their data.
* Local execution comes first.
* Every action should be explainable.
* Every important action should be reviewable.
* Components should be modular.
* Providers should be interchangeable.
* Intelligence comes from context, not model size.
* The conversation is the primary interface.

⸻

The Long-Term Vision

Jaymi is not built around a language model.

It is built around a planner capable of understanding people, coordinating tools, managing context, and orchestrating work.

Models will improve.

Tools will evolve.

Providers will change.

The planner remains.

Everything else is replaceable.

The architecture should ensure that, no matter how the technology changes, interacting with Jaymi always feels the same:

You simply talk to your computer.

It understands what you mean.

Then it figures out the rest.