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
**Status:** in-progress
**Thesis:** Governed creation, editing, approval, and release of structured content objects with signed evidence for every transition.

### Operations

The governed Content v1 registry is frozen at exactly these eight operations.
Every row is active; `changeset.create` is not a governed operation.

| Operation | Version | Status | Governance | Consequence |
|---|---|---|---|---|
| `schema.create` | `v1` | active | agent-executable | content-mutation |
| `object.create` | `v1` | active | agent-executable | content-mutation |
| `object.edit` | `v1` | active | agent-executable | content-mutation |
| `content.approve` | `v1` | active | human-only | content-approval |
| `content.release` | `v1` | active | human-only | content-release |
| `changeset.commit` | `v1` | active | agent-executable | content-mutation |
| `release.publish` | `v1` | active | human-only | content-release |
| `edition.create` | `v1` | active | agent-executable | content-mutation |

### Content v1 mutation contract

`edition.create::v1` accepts an object with required UUIDv7
`idempotency_key` and required UUID `changeset_id`. Its canonical output is:

```json
{
  "operation": "edition.create",
  "data": {
    "edition": {
      "id": "<uuid>",
      "changeset_id": "<uuid>",
      "objects": [],
      "created_at": "<rfc3339 timestamp>",
      "content_digest": "sha256:<hex>"
    }
  }
}
```

`changeset.commit::v1` accepts an object with required UUIDv7
`idempotency_key`, required UUID `changeset_id`, and optional string `notes`.
Its canonical output is:

```json
{
  "operation": "changeset.commit",
  "data": {
    "changeset": {
      "id": "<uuid>",
      "intent": "<string>",
      "base_state_digest": "sha256:<hex>",
      "edits": [],
      "created_at": "<rfc3339 timestamp>",
      "status": "committed"
    },
    "objects_count": 0
  }
}
```

For each operation version, the idempotency tuple is
`(operation, version, idempotency_key)`. The UUIDv7 key is bound to the
canonical JSON of the complete input, including the key itself. Repeating a
completed call with the same tuple and byte-equivalent canonical input MUST
return the original persisted output and signed proof without executing the
mutation again. Reusing the tuple with different canonical input MUST fail as
an idempotency conflict before mutation. Failed calls do not create a completed
replay record.

Content snapshot identifiers (`base_state_digest`, edition `content_digest`,
and existing content-domain digest fields) remain algorithm-qualified SHA-256
for v1 compatibility. Kernel proof input/output digests remain the canonical
BLAKE3-256 `ContentDigest` values defined by `contracts/kernel-api.md`. Changing
content snapshot identifiers requires a later, explicitly versioned contract.

`changeset.create` may remain a local authoring helper that prepares a
ChangeSet. It MUST NOT appear in the governed registry or eight-operation
conformance surface and MUST NOT mint a Proof claiming an unregistered
`changeset.create::v1` execution.

### AXP-E0001 Content addition (active implementation; Gate C pending)

AXP-E0001 Gate B approved and implemented active `release.publish::v2`
alongside the unchanged active v1 row. It remains the same governed operation,
so the v1 eight-operation name surface does not gain a ninth operation. The
edition's live journey and Gate C remain pending; that does not make the
shipped registry row or its API prerequisites provisional.

| RegistryEntry field | Exact value |
|---|---|
| `operation` | `release.publish` |
| `domain` | `content` |
| `version` | `v2` |
| `action` | `content:release_publish` |
| `description` | `Publish one existing content edition as an immutable local preview artifact` |
| `input_schema` | `content/release-publish-v2.input.json` |
| `output_schema` | `content/release-publish-v2.output.json` |
| `required_authority` | `delegation-grant` |
| `governance` | `human-only` |
| `idempotency` | `required-uuidv7` |
| `consequence` | `content-release` |
| `evidence_contract` | `operation-effect-v1` |
| `benchmark` | `B1` |
| `status` | `active` |
| `deprecated_since` | null |
| `replacement_operation` | null |

The v2 input requires UUIDv7 `idempotency_key`, existing `edition_id`, literal
`preview` environment, validated `version_label`, and exact canonical
`manifest_digest`. Its handler selects `RequiredUuidV7ExactReplay`; the v1
handler remains `None` for compatibility. V2 creates at most one immutable
artifact inside the disposable workspace after signed approval and returns the
engine's original proof. Canonical request, manifest, artifact, output, proof,
replay, recovery, and evaluation shapes are defined in
`contracts/release-manager-live-v1.md`.

This active row uses the default-compatible version-aware handler API,
SQLite v12 delegation-scope persistence, `SqliteStore::load_delegation`, and
CLI exact-scope grant/load plus preflight-before-credential behavior noted
there. The live path requires an explicit delegation ID whose loaded grant has
exactly `allowed_operations=["release.publish"]` and
`allowed_domains=["content"]`; chain-only, default, wildcard, or unbounded
scope is invalid, and structured `resource_scope` is forbidden for this exact
grant. V12 storage rejects unknown scope keys locally without changing the
shared kernel deserializer. V1 remains active and unchanged; do not emulate v2
by changing v1 or by adding a ninth Content operation.

### Non-goals

- No visualization, rendering, or frontend
- No user-facing UI for content editing
- No integration with external CMS platforms

### Decision log

| # | Date | Decision | Rationale |
|---|---|---|---|
| 1 | 2026-08-27 | Domain 1 scoped as Content Governance, 8-operation surface | Walking skeleton from architecture doc; first domain proves kernel generality |
| 2 | 2026-08-28 | Wave 10 completes Domain 1: full registry coverage, handlers, HTTP, idempotency | Domain 1 is production-ready for the governance narrative |
| 3 | 2026-08-29 | D-E0000-005 freezes exactly eight active v1 operations and the `edition.create` / `changeset.commit` replay and output contracts | The prior complete status overstated seven-handler implementation coverage; Gate B preserves names and SHA-256 snapshot compatibility while closing governed execution without adding `changeset.create` as operation nine. |
| P-E0001-1 | 2026-08-30 | **PROPOSED, Gate B pending:** add `release.publish::v2` without changing v1 | The live preview needs existing-edition identity, exact replay, a durable local artifact, and original-proof binding; changing v1 would break callers. |
| E0001-2 | 2026-08-30 | Gate B approved and implementation activated `release.publish::v2`; v1 remains unchanged; E0001 Gate C remains pending | The registry, schemas, version-aware handler, exact replay, immutable local artifact, and delegation-scope prerequisites now pass their scoped and reverse-impact gates. |

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
