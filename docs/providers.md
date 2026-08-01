Providers

Providers are Jaymi’s connection to the outside world.

Providers expose resources, services, and functionality to the Planner through a standardized interface.

The Planner never communicates directly with external systems.

Instead, every interaction flows through one or more providers.

This abstraction allows Jaymi to remain modular, extensible, and independent of any specific technology or vendor.

⸻

Philosophy

Providers answer one question:

“Where can Jaymi get information or perform work?”

A provider may expose:

* Local resources
* AI models
* External services
* Hardware
* Operating system features

Providers never make decisions.

They simply expose capabilities in a predictable way.

⸻

Responsibilities

A provider is responsible for:

* Connecting to a resource
* Advertising available capabilities
* Validating requests
* Executing supported operations
* Returning structured results
* Reporting errors
* Declaring required permissions
* Managing provider-specific configuration

Providers do not:

* Plan
* Reason
* Store memory
* Build context
* Make autonomous decisions

Those responsibilities belong elsewhere.

⸻

Provider Lifecycle

Every provider follows the same lifecycle.

Discover
↓
Register
↓
Initialize
↓
Health Check
↓
Ready
↓
Execute Requests
↓
Shutdown

The Planner only interacts with providers that are healthy and registered.

⸻

Provider Categories

Jaymi groups providers into logical categories.

Local Providers

Interact with the local computer.

Examples:

* Filesystem
* Terminal
* Git
* Messages
* Mail
* Calendar
* Photos
* Notes

⸻

AI Providers

Provide reasoning or generation.

Examples:

* Local language models
* Local image models
* Future cloud AI providers

⸻

Internet Providers

Retrieve online information.

Examples:

* Search
* HTTP requests
* Documentation lookup

⸻

Import Providers

Import external knowledge.

Examples:

* Conversation archives
* Documents
* Projects
* Messages
* Future migration sources

⸻

Automation Providers

Perform actions.

Examples:

* Launch applications
* Execute shortcuts
* Move files
* Rename files
* Run scripts

⸻

Provider Identity

Every provider must expose:

* Unique identifier
* Human-readable name
* Version
* Description
* Category
* Author
* Supported capabilities
* Required permissions
* Configuration schema

The Planner should be able to discover every provider without special-case logic.

⸻

Capabilities

Providers advertise what they can do.

Examples include:

* Search
* Read
* Write
* Generate
* Execute
* Import
* Index
* Summarize
* Analyze

The Planner selects providers based on capabilities—not provider names.

⸻

Execution Model

The Planner never asks:

“Use the Filesystem Provider.”

Instead it asks:

“I need a provider capable of searching files.”

The Provider Manager resolves the best match.

This allows providers to be replaced without changing Planner logic.

⸻

Configuration

Every provider owns its own configuration.

Examples:

* Enabled / Disabled
* Local storage paths
* Model selection
* Indexing rules
* Resource limits
* Authentication
* Provider-specific settings

Jaymi should never hardcode provider behavior.

⸻

Permissions

Every provider declares the permissions it requires.

Examples:

Filesystem

* Read files
* Write files
* Delete files

Terminal

* Execute commands

Internet

* Network access

Messages

* Read messages

The Planner enforces permissions.

Providers never grant permissions themselves.

⸻

Failure Handling

Providers are expected to fail gracefully.

Common failures include:

* Resource unavailable
* Permission denied
* Network unavailable
* Provider disabled
* Invalid request
* Corrupt data

Failures should return structured errors.

The Planner decides how to recover.

⸻

Provider Independence

Providers should never depend directly on one another.

If a workflow requires multiple providers, coordination happens in the Planner.

Example:

User

↓

Planner

↓

Filesystem Provider

↓

Vision Provider

↓

Memory Provider

↓

Response

Providers remain isolated.

⸻

Installation

Providers are installable modules.

Users should be able to:

* Install
* Enable
* Disable
* Configure
* Update
* Remove

without affecting the rest of the system.

⸻

Versioning

Every provider should declare:

* Provider version
* Minimum supported Jaymi version
* Supported capabilities

This ensures compatibility as the platform evolves.

⸻

Design Principles

Every provider should be:

* Small
* Focused
* Replaceable
* Testable
* Self-contained
* Well documented

A provider should solve one problem well.

⸻

Long-Term Vision

Providers are what make Jaymi adaptable.

As new operating systems, AI models, services, and technologies emerge, they should integrate by implementing the provider interface rather than changing the Planner.

Jaymi’s architecture should evolve through new providers—not through rewrites of the core system.

The Planner coordinates.

Providers execute.

That separation is fundamental to the architecture.
