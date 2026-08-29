# Domain Definitions Contract

This file tracks the canonical scope, operation set, and decision history for each domain module. Every wave spec must reference the domain definition below. Changes to scope, operations, or naming require a new decision entry — the swarm builds toward what is written here, not toward chat.

## Format

Each domain has a definition block with:

- **Name** — the domain identifier used in registry entries (`domain` field) and crate names
- **Crate** — the Rust crate that owns the domain's models and handlers
- **Status** — `draft`, `in-progress`, `complete`, or `deprecated`
- **Thesis** — one sentence explaining what the domain does and why it exists
- **Operations** — the registry surface, with governance level and consequence
- **Non-goals** — what this domain explicitly does not do (prevents scope creep)
- **Decision log** — dated, numbered decisions that changed the domain's scope or direction

---

## Domain 1: Content

**Name:** `content`
**Crate:** `proof-content`
**Status:** complete
**Thesis:** Governed creation, editing, approval, and release of structured content objects with signed evidence for every transition.

### Operations

| Operation | Governance | Consequence |
|---|---|---|
| `schema.create` | agent-executable | content-mutation |
| `object.create` | agent-executable | content-mutation |
| `object.edit` | agent-executable | content-mutation |
| `content.approve` | human-only | content-approval |
| `content.release` | human-only | content-release |
| `changeset.commit` | agent-executable | content-mutation |
| `release.publish` | human-only | content-release |
| `edition.create` | agent-executable | content-mutation |

### Non-goals

- No visualization, rendering, or frontend
- No user-facing UI for content editing
- No integration with external CMS platforms

### Decision log

| # | Date | Decision | Rationale |
|---|---|---|---|
| 1 | 2026-08-27 | Domain 1 scoped as Content Governance, 8-operation surface | Walking skeleton from architecture doc; first domain proves kernel generality |
| 2 | 2026-08-28 | Wave 10 completes Domain 1: full registry coverage, handlers, HTTP, idempotency | Domain 1 is production-ready for the governance narrative |

---

## Domain 2: Commerce

**Name:** `commerce`
**Crate:** `proof-commerce`
**Status:** in-progress
**Thesis:** Governed catalog, order, and fulfillment operations with signed evidence — the second domain proving the kernel's domain-agnosticism and the clearest migration path for Sitecore OrderCloud customers.

### Operations (planned)

| Operation | Governance | Consequence |
|---|---|---|
| `catalog.create` | agent-executable | catalog-mutation |
| `catalog.update` | agent-executable | catalog-mutation |
| `order.create` | agent-executable | order-mutation |
| `order.approve` | human-only | order-approval |
| `order.fulfill` | agent-executable | order-mutation |

### Non-goals

- No payment processing (external payment providers, not Proof's concern)
- No inventory management beyond catalog mutation
- No storefront or customer-facing UI

### Decision log

| # | Date | Decision | Rationale |
|---|---|---|---|
| 1 | 2026-08-28 | Domain 2 initially scoped as "Data" (Proof-hosted datasets) | Explored data-hosting market |
| 2 | 2026-08-28 | Dropped "Data" as a domain; promoted Commerce to Domain 2 | Data ownership is an infrastructure commitment (self-host, export, Postgres), not a domain module. Commerce is the original architecture sequence and directly displaces Sitecore OrderCloud. Avoids scope creep of a generic query engine. |
| 3 | 2026-08-28 | Commerce operations: catalog.create, catalog.update, order.create, order.approve, order.fulfill | Clear lifecycle mapping to evidence pipeline; mirrors content's create/edit/approve/release pattern |

---

## Domain 3: Workflow (not started)

**Name:** `workflow`
**Crate:** `proof-workflow`
**Status:** draft
**Thesis:** Governed multi-step approval and orchestration chains with signed evidence at each step.

### Operations (planned)

| Operation | Governance | Consequence |
|---|---|---|
| `workflow.define` | agent-executable | workflow-mutation |
| `workflow.trigger` | agent-executable | workflow-mutation |
| `workflow.step.complete` | agent-executable | workflow-mutation |
| `workflow.approve` | human-only | workflow-approval |

### Non-goals

- No cron/scheduler (kernel or transport concern, not domain)
- No UI for workflow builder

### Decision log

| # | Date | Decision | Rationale |
|---|---|---|---|
| 1 | 2026-08-28 | Domain 3 placeholder scoped as Workflow | Matches architecture doc layer diagram; no decisions made yet |

---
