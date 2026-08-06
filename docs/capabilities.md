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

### Catalog vs effective availability

Two assessments must not be conflated:

| Path | API | Meaning |
| --- | --- | --- |
| **Catalog / request validate** | `CapabilityEngine::validate` | Catalog maturity only (Ready / Experimental / Planned). Planner request gating uses this tier. |
| **Effective / discover** | `discover` / Inspector / capability plans | Catalog + live tool/provider inventory via `effective_availability` |

Effective availability can demote Ready / Experimental to **Unavailable** when required tools or providers are missing. Planned stays Planned until the product promotes it.

**Effective at boot (approximate):**

| Capability | Catalog | Effective discover | Why |
| --- | --- | --- | --- |
| Search, Read Documents, Discover, File Management, Execute Terminal Commands | Ready | Ready | Required tools/providers advertise the capability |
| Index | Ready | **Unavailable** | `scan_filesystem` ads Index; no provider currently ads Index |
| Code | Experimental | **Experimental** | `terminal` / `git` / `language_server` + matching providers |
| Embeddings | Experimental | Experimental | Provider only (`embedding.local`); no tool required |
| Vision | Experimental | **Unavailable** | Requires a vision tool; none registered |
| OCR / Chat / Generate Images / … | Planned | Planned | Planned never promotes via inventory alone |

Preferred tool hints (e.g. Code prefers `editor`) are diagnostics only — there is no `editor` tool yet; LSP / terminal / git fulfill Code inventory.

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
