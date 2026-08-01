Planner

The Planner is the orchestration kernel of Jaymi.

The Planner is responsible for coordinating every interaction within Jaymi.

It understands user goals, gathers context, delegates work, enforces permissions, and manages execution.

The Planner does not perform the work itself.

Instead, it coordinates the systems that do.

This separation allows Jaymi to evolve independently of any individual AI model, provider, or tool.

⸻

Core Philosophy

Jaymi is not built around a language model.

It is built around decision making.

Language models help Jaymi reason.

The Planner decides what happens next.

This distinction allows intelligence to improve over time without changing the architecture.

⸻

Responsibilities

The Planner is responsible for:

* Understanding the user’s goal
* Determining whether additional reasoning is required
* Building execution plans
* Loading context
* Retrieving memories
* Selecting capabilities
* Choosing tools
* Coordinating execution
* Managing permissions
* Recovering from failures
* Producing the final response

The Planner never directly edits files, generates images, executes terminal commands, or searches documents.

Those responsibilities belong to specialized tools.

⸻

Architecture

User
   │
   ▼
Conversation
   │
   ▼
Planner
   │
   ├───────────────┐
   │               │
Decision Engine    Reasoning Engine
   │               │
   └──────┬────────┘
          │
     Context Engine
          │
     Memory Engine
          │
 Capability Manager
          │
    Tool Orchestrator
          │
      Provider Layer
          │
 Local Resources / AI Models / Internet

The Planner remains deterministic.

Reasoning is delegated.

Execution is delegated.

⸻

Decision Engine

The Decision Engine contains deterministic application logic.

It answers questions such as:

* Does this request require reasoning?
* Which project is active?
* Is internet access allowed?
* Does this action require approval?
* Which capabilities are required?
* Which tools are available?
* Has the user already granted permission?

These decisions should never depend on a language model.

⸻

Reasoning Engine

The Reasoning Engine exists to solve problems that require language understanding.

Examples include:

* Understanding ambiguous requests
* Summarizing information
* Planning complex workflows
* Explaining results
* Generating natural language
* Choosing between multiple reasonable interpretations

The Reasoning Engine may use any compatible language model.

It is replaceable.

⸻

Request Lifecycle

Every request follows the same pipeline.

Receive Request
↓
Determine Intent
↓
Determine Context Requirements
↓
Retrieve Memory
↓
Retrieve Knowledge
↓
Reason (if necessary)
↓
Build Execution Plan
↓
Select Capabilities
↓
Select Tools
↓
Execute
↓
Request Approval (if required)
↓
Respond
↓
Update Memory (optional)

No request bypasses this process.

⸻

Intent

Every interaction begins with a goal.

Examples:

Continue working on Jaymi.
↓
Resume Project
Generate a logo.
↓
Create Image
Find Heather's Canva login.
↓
Search Personal Knowledge

The Planner cares about goals, not wording.

⸻

Context

Context is assembled intentionally.

The Planner determines which information is required before asking the Reasoning Engine to think.

Possible sources include:

* Active project
* Previous conversation
* Project memory
* Personal memory
* Search results
* Files
* Git history
* Terminal state
* Messages
* Documents

Only relevant information should be retrieved.

⸻

Planning

The Planner converts user intent into executable tasks.

Example:

User
↓
Find the ChatGPT images I downloaded yesterday.
↓
Execution Plan
Search Downloads
↓
Filter Yesterday
↓
Identify Images
↓
Analyze Metadata
↓
Generate Results

Execution plans should remain simple, deterministic, and explainable.

⸻

Capabilities

Capabilities describe what Jaymi can accomplish.

Examples include:

* Chat
* Search
* Code
* Vision
* Generate Images
* Read Documents
* Browse Internet
* Terminal
* File Management
* Automation

Capabilities are stable.

They should not depend on specific tools.

⸻

Tool Selection

Every capability may be fulfilled by one or more tools.

Example:

Search

↓

Filesystem Tool

↓

Messages Tool

↓

Documents Tool

↓

Photos Tool

Tools may change over time.

Capabilities should not.

⸻

Execution

Execution is managed step by step.

The Planner tracks:

* Progress
* Dependencies
* Success
* Failure
* Recovery

If execution fails, the Planner may:

* Retry
* Select another tool
* Request clarification
* Continue with partial results
* Explain the failure

Execution should always remain predictable.

⸻

Permissions

The Planner determines whether approval is required before execution.

Typical approval points include:

* Editing files
* Running terminal commands
* Sending messages
* Installing software
* Modifying repositories
* Deleting information

Approval requests should explain:

* What will happen
* Why it is needed
* Expected outcome

The user always makes the final decision.

⸻

Memory Integration

The Planner determines:

* Which memories are relevant
* Whether new information should be remembered
* Whether information belongs to conversation, project, or personal memory

Memory should always be intentional.

⸻

Explainability

Every decision made by the Planner should be explainable.

Jaymi should always be able to answer questions such as:

* Why did you search there?
* Why did you use this tool?
* Why did you ask for permission?
* Why did you retrieve this memory?
* Why was internet access required?

Transparency is required for trust.

⸻

Design Principles

The Planner should always:

* Prefer deterministic logic over AI when possible.
* Retrieve before reasoning.
* Build context before thinking.
* Prefer local execution.
* Ask before meaningful actions.
* Keep execution understandable.
* Separate orchestration from intelligence.

⸻

Long-Term Vision

The Planner is designed to remain stable for the lifetime of the project.

Language models will improve.

Providers will evolve.

Tools will change.

Capabilities will grow.

The Planner should not.

It remains the orchestration kernel that connects every part of Jaymi into a single intelligent system.
