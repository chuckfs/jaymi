Jaymi

An open-source, offline-first personal AI environment.

Jaymi is a desktop application that transforms your computer into an intelligent partner.

Instead of navigating applications, folders, and search boxes, you simply describe what you want to accomplish. Jaymi understands your projects, files, conversations, documents, and personal knowledge, then helps you find information, generate content, write code, automate tasks, and organize your digital life—all while keeping your data on your own device by default.

⸻

Documentation map

How to read the docs:

* **Current Implementation** — exists in the codebase, wired at boot, covered by tests
* **Partial / Stub** — architecture or API present; behavior incomplete
* **Target Architecture** — intentional future vision (kept, not deleted)

See `ARCHITECTURE.md` for the kernel, `ROADMAP.md` for layer progress, and `docs/` for subsystem detail.

⸻

Why Jaymi Exists

Modern computers are powerful, but they still expect people to think like computers.

They expect us to remember:

* File names
* Folder locations
* Which application contains our information
* Which AI tool is best for a particular task

Our minds don’t work that way.

We remember people, projects, ideas, conversations, and moments.

Jaymi bridges that gap.

Instead of remembering where something is, you simply remember what it is.

⸻

Our Philosophy

Your computer should understand you.

Your knowledge should belong to you.

Your AI should adapt to your workflow—not force you into someone else’s.

Jaymi is built on a few simple principles:

* Offline First — Work locally whenever possible.
* Privacy by Default — Your data never leaves your computer without your permission.
* User Ownership — Your knowledge belongs to you, not to the applications that created it.
* Natural Language First — Talk to your computer instead of navigating it.
* Extensible by Design — Every major capability is modular and replaceable.
* Transparent Actions — Every important action is reviewed before it happens.

⸻

Current Implementation

Jaymi’s architectural kernel is shipped through Layers 0–6, with foundation work on Layers 7–8.

| Area | Status |
|------|--------|
| Planner (orchestration kernel) | Current — every user request enters `handle` |
| Context Engine (`assemble`) | Current — sole request-context path |
| Discovery / Knowledge inventory | Current |
| Understanding / parsers (txt, md, json, pdf, docx, images) | Current |
| Search (FTS, metadata, hybrid, citations) | Current |
| Memory (conversation / project / personal) | Current |
| Project Engine (identity + workspace context) | Current |
| Capability planning + availability + workspace expansion | Current |
| Tools (search files/knowledge, read, inventory, scan) | Current (small set) |
| Providers (filesystem, local embedding, OCR placeholder) | Current / Partial |
| Reasoning (language-model backends) | Stub — not implemented |
| OCR engine | Stub — architecture only |
| Git / Terminal / Messages / Mail / Image generation | Target |
| Installable provider plugins | Target |
| Daily-driver product UX (Layer 9) | Target |

The Context Engine assembles request context for every Planner request.

Administrative Memory and Project CRUD resolve owning engines directly; user retrieval (search, list, read, discover, continue, project knowledge) always goes through the Planner.

⸻

Target Architecture

Jaymi is designed to become the primary interface to your computer.

Long-term it aims to combine:

* Conversational AI
* Project-aware coding
* Image generation
* Semantic search
* Persistent memory
* Local document understanding
* File management
* Terminal automation
* Intelligent project context
* Personal knowledge retrieval (documents, messages, email, archives, …)

Everything accessible through a single conversational interface.

Cloud services remain optional and are only used when the user explicitly chooses them.

⸻

Offline First

Offline-first is not a feature.

It is the foundation of the project.

**Current:** local SQLite knowledge store, filesystem discovery, local document parsing, local lexical embeddings, offline-first and privacy policies enforced on tool candidates.

**Target:** full local AI models for reasoning, vision, OCR, and generation — cloud only when chosen.

⸻

Projects

A project is more than a folder.

**Current:** first-class Project Engine identity; `.jaymi/` layout; conversations, memories, decisions, tasks (as memory kinds), and assembled `ProjectContext`; “Continue working on …” restores workspace context.

**Target:** live Git status, full IDE/editor workspace, artifact pipelines, repository import flows.

⸻

Extensible Architecture

**Current:** ProviderRegistry for discovery; concrete providers bound into tools at boot; Capability Engine soft-matching for plans. There is no ProviderManager.

**Target:** installable providers that connect new tools, services, or local resources without changing the core application.

⸻

Long-Term Vision

Jaymi is not trying to become another chatbot.

The long-term vision is much larger.

Jaymi aims to become the intelligent layer between people and their computers—one place where conversations, creativity, coding, automation, memory, and personal knowledge come together in a single, unified experience.

The computer should no longer feel like a collection of disconnected applications.

It should feel like something you can simply talk to.

See `VISION.md` for the full target narrative.

⸻

Guiding Principle

Your computer should understand you—not the other way around.
