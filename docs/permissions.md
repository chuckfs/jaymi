Permissions

**Status: Current Implementation** (rule engine) · **Target:** conversational approval UX, history, revoke

Permissions define what Jaymi is allowed to do on behalf of the user.

Permissions exist to protect the user.

Every meaningful action performed by Jaymi must be authorized according to a clear, transparent, and predictable permission model.

Permissions are independent of Policies.

Policies influence how Jaymi behaves.

Permissions determine whether Jaymi may perform an action.

⸻

Philosophy

Jaymi should never surprise the user.

Before performing actions that affect the user’s computer, data, or accounts, Jaymi must have permission.

The user always remains in control.

⸻

Permission Model

Every action belongs to a permission category.

Examples include:

* Read
* Write
* Execute
* Delete
* Network
* Import
* Export

The Planner evaluates permissions before execution.

Providers never grant themselves permission.

⸻

Permission Categories

Filesystem

Examples:

* Read files
* Create files
* Modify files
* Move files
* Rename files
* Delete files

⸻

Terminal

Examples:

* Execute commands
* Install software
* Modify environment variables

⸻

Internet

Examples:

* Search the web
* Download files
* Upload files
* Connect to external APIs

⸻

Communication

Examples:

* Read Messages
* Read Mail
* Send Messages
* Send Email

⸻

System

Examples:

* Launch applications
* Close applications
* Access clipboard
* Access camera
* Access microphone
* Access location

⸻

AI Providers

Examples:

* Send prompts
* Send attachments
* Generate images
* Upload documents

Cloud providers always require explicit user approval before transmitting personal data unless the user has configured otherwise.

⸻

Permission Scope

Permissions may be granted at different scopes.

Once

Valid for one action only.

Example:

Run this terminal command.

⸻

Conversation

Valid until the current conversation ends.

Example:

Allow internet access for this conversation.

⸻

Project

Valid only within the active project.

Example:

Allow Git operations in this repository.

⸻

Global

Persistent until revoked.

Example:

Always allow Jaymi to read my Downloads folder.

⸻

Permission Request

Whenever permission is required, Jaymi should explain:

* What it wants to do
* Why it needs permission
* Which resources will be affected
* Whether the action is reversible

The request should be written in plain language.

Example:

Jaymi would like to rename 12 files in your Downloads folder to make them easier to find. You can review every proposed filename before anything changes.

⸻

Approval Workflow

**Status: Partial** — Permission + Action Policy emit Allowed / RequiresApproval / Denied · Review Card resume · **Target:** full conversational approval UX

Permission Engine and Action Policies share the same decision triad. The Planner
combines them (Denied > RequiresApproval > Allowed) and may also escalate from
ToolRisk (Modify / Destructive / External).

| Decision | Planner |
|----------|---------|
| Allowed | Execute |
| RequiresApproval | Review Card → await user → resume same plan |
| Denied | Explain why → do not execute |

Approval never bypasses the Planner. Tools never execute themselves.

```text
Planner
  → Action Policy (Allowed / RequiresApproval / Denied)
  → Permission Check (Allowed / RequiresApproval / Denied)
  → Combine (+ ToolRisk escalate)
  → Allowed → Execute
  → RequiresApproval → Review Card → User → Resume
  → Denied → Explain → Stop
```

Offline First requires approval for internet/cloud tools. Privacy Maximum
hard-denies non-local tools (overrides Offline First's approval path).

No protected action bypasses this workflow.

⸻

Destructive Actions

Actions that could permanently modify user data require additional safeguards.

Examples include:

* Permanent deletion
* Repository reset
* Bulk file modifications
* System configuration changes

Whenever possible, Jaymi should prefer reversible actions.

Examples:

Instead of deleting:

Move to Trash.

Instead of overwriting:

Create a backup.

Instead of replacing:

Generate a preview.

⸻

Preview Before Action

Whenever practical, Jaymi should present a preview.

Examples include:

* File diffs
* Document changes
* Image edits
* File moves
* Folder reorganizations

The user should understand exactly what will happen before approving.

⸻

Permission History

**Status: Target**

Jaymi should maintain a local history of permission decisions.

History may include:

* Timestamp
* Action
* Resource
* Decision
* Scope

Users should be able to review and revoke previously granted permissions.

This is distinct from **Approval History** (Review Card Approve / Modify /
Cancel on Execution Plans — see `docs/planner.md`), which is Current.

⸻

Revocation

Permissions are never permanent unless the user explicitly chooses.

Users may revoke:

* Individual permissions
* Provider permissions
* Project permissions
* Global permissions

Revocation should take effect immediately.

⸻

Explainability

Jaymi should always be able to answer:

* Why was permission required?
* Why wasn’t this action automatic?
* Which permission blocked the request?
* What would change if permission were granted?

Transparency builds trust.

⸻

Security Principles

Permissions should always:

* Default to least privilege.
* Prefer reversible actions.
* Require approval for meaningful changes.
* Be easy to revoke.
* Be understandable.
* Never rely on hidden behavior.

The safest action is the default action.

⸻

Long-Term Vision

Permissions are not obstacles.

They are conversations.

Rather than interrupting the user, permissions should build confidence by making Jaymi’s intentions clear, predictable, and reviewable.

Users should never wonder what Jaymi is doing.

They should always know, always understand, and always have the final say.
