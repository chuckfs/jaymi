# Capabilities

Capabilities describe **what Jaymi knows how to do** — never how work is performed.

Tools and providers implement capabilities. The Capability Engine does not execute work.

---

## Conceptual vs executable

Jaymi keeps a **full conceptual catalog** of capabilities. Future abilities stay registered so planning, discovery, and diagnostics can talk about them honestly.

**Availability** says how far each capability is along the path from concept to execution:

| Availability | Meaning |
| --- | --- |
| **Ready** | Catalogued and currently executable with stable fulfillment |
| **Experimental** | Catalogued and currently executable with partial / stub fulfillment |
| **Planned** | Catalogued for the product vision; intentionally not executable yet |
| **Unavailable** | Known and usually registered, but blocked right now (engine down, missing tools/providers) |
| **Unknown** | Not a recognized capability id |

Registration is not the same as executability. Planned capabilities remain registered. They do not disappear from the catalog when tools are not ready.

---

## Current catalog defaults

**Ready:** Search, Read Documents, Discover, Index, File Management, Execute Terminal Commands

**Experimental:** Code, Embeddings, Vision

**Planned:** OCR (placeholder provider only — not executable), Chat, Generate Images, Browse the Web, Organize Files, Automate Tasks, Internet, Automation

Effective availability can demote Ready / Experimental to Unavailable when required tools or providers are missing. Planned stays Planned until the product promotes it.

---

## Planning

Capability planning considers availability before treating a plan as executable:

* Plans may include Planned steps so the user sees the full intended work
* A plan is **ready** only when every step is Ready or Experimental
* A plan is **executable** only when every step is ready *and* live tool/provider requirements are satisfied
* The Planner rejects tool execution for capabilities that are not in an executable tier

---

## Diagnostics

Diagnostics and the Capability Inspector show:

* Registered catalog size (conceptual)
* Active / executable ids (Ready + Experimental with inventory)
* Planned ids (including OCR until a real engine exists)
* Per-capability availability labels in status detail lines

Subsystem diagnostics use a separate readiness vocabulary: **Operational / Experimental / Stub / Disabled**.

---

## Architecture

```
User Request → Intent → Capability → Context Policy → Context Engine → ContextBundle
  → Behavior (Planned) → Action Policy → Permission → Tools → Providers → Response
```

Preserve the Capability Engine as the owner of catalog metadata and availability assessment. Tools remain interchangeable implementations under stable capability ids.
