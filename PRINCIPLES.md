Principles

Every decision made in Jaymi should reinforce these principles.

Technology changes.

Models improve.

Frameworks come and go.

These principles should remain constant.

Implementation status for features implied by a principle (export, conversational approval UX, universal citations) may still be Target — see `ARCHITECTURE.md` and `ROADMAP.md`. The principles themselves do not wait for those features.

⸻

1. Offline First

Jaymi should work without an internet connection whenever possible.

Local execution is the default.

Cloud services are optional enhancements—not requirements.

If a task can be completed locally, it should be.

⸻

2. Privacy by Default

User data belongs to the user.

Jaymi never sends information to external services without explicit user consent.

Every external request should be intentional, visible, and understandable.

Privacy is a design requirement, not a feature.

⸻

3. User Ownership

Knowledge belongs to the user.

Not to Jaymi.

Not to an AI provider.

Not to a cloud service.

Conversations, projects, memories, documents, and generated content should always remain portable and under the user’s control.

Users should always be able to export, inspect, edit, or delete their own data.

⸻

4. Conversation First

Conversation is the primary interface.

Users should describe goals rather than applications.

Instead of asking:

“Which program should I use?”

Users should simply say:

“Help me accomplish this.”

Jaymi determines the rest.

⸻

5. Intelligence Through Context

Jaymi should not rely on remembering everything.

Instead, it should retrieve the right information at the right time.

Understanding comes from context, not brute force.

Better context is more valuable than larger models.

⸻

6. Planner Over Models

Language models are replaceable.

The planner is not.

Jaymi’s identity comes from how it understands intent, builds context, coordinates capabilities, and orchestrates work—not from any particular AI model.

Models improve over time.

Architecture should not depend on them.

⸻

7. Transparency Builds Trust

Users should always understand:

* Why something happened.
* Where information came from.
* What Jaymi is about to do.

Answers should include sources whenever possible.

Actions should be explainable.

Automation should never feel mysterious.

⸻

8. Review Before Action

Jaymi should never perform meaningful actions silently.

Potentially destructive or irreversible actions require review.

Examples include:

* Editing files
* Running terminal commands
* Moving documents
* Sending messages
* Deleting information

The user always has the final decision.

⸻

9. Projects Are First-Class

Work happens inside projects.

Jaymi should understand projects rather than isolated files.

A project includes:

* Code
* Documents
* Conversations
* Decisions
* Tasks
* History
* Memory

Context should naturally follow the project.

⸻

10. Modular by Design

Every major system should be replaceable.

Capabilities.

Providers.

Tools.

Memory.

Models.

Search engines.

Nothing should require rewriting the rest of the application.

⸻

11. Extensible by Default

Jaymi should be designed for growth.

New capabilities should be added through extension rather than modification.

Every new provider or tool should integrate into the existing architecture without changing the planner.

⸻

12. Simplicity Wins

The simplest solution that satisfies the architecture is usually the correct one.

Avoid unnecessary abstraction.

Avoid premature optimization.

Avoid complexity that exists only for future possibilities.

Build only what is needed today while leaving room for tomorrow.

⸻

13. One Conversation, Many Capabilities

Regardless of the task:

* Chatting
* Coding
* Searching
* Researching
* Creating
* Automating
* Organizing

The experience should remain consistent.

Users should not think about tools.

They should think about goals.

⸻

14. Build for the User, Not the Benchmark

Jaymi is not competing to have the largest model, the fastest benchmark, or the most features.

Success is measured by one question:

Does this make the user’s computer easier to use?

If the answer is no, the feature should be reconsidered.

⸻

Final Principle

Every design decision should move Jaymi toward a future where interacting with a computer feels less like operating software and more like collaborating with an intelligent partner.

If a feature supports that vision, it belongs.

If it does not, it probably doesn’t.