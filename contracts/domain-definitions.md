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
**Status:** complete
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
| 4 | 2026-08-28 | Wave 12 completes Domain 2: FulfillmentPipeline, MCP tools, commerce idempotency, human-principal kernel policy, benchmark conformance | Domain 2 is production-ready; kernel gained `ExecutionContext.principal_kind` to allow humans on human-only operations |

---

## Domain 3: Workflow

**Name:** `workflow`
**Crate:** `proof-workflow`
**Status:** complete
**Thesis:** Governed multi-step approval and orchestration chains with signed evidence at each step.

### Operations

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
| 2 | 2026-08-29 | Wave 13 completes Domain 3: WorkflowDefinition/Run/Step models, SQLite storage, HTTP wiring, governance conformance | All four operations (`workflow.define`, `workflow.trigger`, `workflow.step.complete`, `workflow.approve`) are registry-covered, storage-backed, HTTP-exposed, and conformance-tested; `workflow.approve` is human-only with UUIDv7 idempotency enforcement |

---

## Domain 4: Analytics

**Name:** `analytics`
**Crate:** `proof-analytics`
**Status:** complete
**Thesis:** Governed query and insight operations over workspace data, producing signed proof that a query was executed against a specific dataset snapshot and returned a specific result.

### Operations

| Operation | Governance | Consequence |
|---|---|---|
| `analytics.query.create` | agent-executable | analytics-query |
| `analytics.query.execute` | agent-executable | analytics-query |
| `analytics.snapshot.create` | agent-executable | analytics-mutation |
| `analytics.insight.approve` | human-only | analytics-approval |

### Non-goals

- No general-purpose SQL or query-language engine
- No external data-source connectors (import/export is a transport or CLI concern)
- No real-time streaming or time-series ingestion
- No dashboard or visualization UI

### Decision log

| # | Date | Decision | Rationale |
|---|---|---|---|
| 1 | 2026-08-28 | Domain 4 placeholder scoped as Analytics & Insight | Matches architecture doc layer diagram; no decisions made yet |
| 2 | 2026-08-29 | Initial operation set: snapshot, query creation, query execution, human-only insight approval | Snapshot separates dataset versioning from live data; query create/execute separates definition from execution (mirroring content define/edit pattern); insight approval is the governance hook where a human signs off on derived conclusions |
| 3 | 2026-08-29 | Wave 14 completes Domain 4: AnalyticsSnapshot/Query/Insight models, SQLite storage, HTTP wiring, governance conformance | All four operations (`analytics.snapshot.create`, `analytics.query.create`, `analytics.query.execute`, `analytics.insight.approve`) are registry-covered, storage-backed, HTTP-exposed, and conformance-tested; `analytics.insight.approve` is human-only with UUIDv7 idempotency enforcement |

---
