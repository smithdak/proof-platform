# Changelog

All notable changes to Proof Platform are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

#### Agent Experience Platform

- Tracked AXP Product Edition contracts, model-tier routing, assignment packets,
  ownership validation, evidence templates, and a populated E0000-E0005 backlog
  for continuous owner-governed agent swarms.
- Runnable dual-era MCP stdio server with registry-derived tools, stable workspace identity, structured results, and persisted signed proofs.
- Signed approval requests and human approve/deny decisions bound to the exact actor, operation, version, input digest, and validity window.
- Durable SQLite approval requests, decisions, and execution replay records, with terminal trace seals that permit only exact idempotent retries of bound approval evidence.
- CLI approver enrollment, pending-request listing, and signed approve/deny commands.
- Resumable MCP `input_required` flow that ignores unsigned client acceptance and replays completed approved operations without redispatch.
- Durable `AgentRun` and `AgentRunStep` lifecycle contracts for one-shot calls, multi-step sessions, approval waits, failures, cancellations, and retry lineage.
- Immutable agent checkpoints and terminal evaluations with canonical metrics and optional basis-point scores.
- SQLite agent-run control-plane persistence with optimistic revisions and process-safe approval-to-step linkage.
- CLI run start, list, inspect, checkpoint, retry, complete, cancel, and evaluate commands.
- MCP run metadata that creates one-shot runs automatically, composes calls into sessions, and resumes the exact persisted approval or retry attempt.
- Immutable agent definitions with instructions, provider/model selection, explicit registry-tool allowlists, and step/model/token/time/output/cost limits.
- Provider-neutral `proof-agent-runtime` planner/tool loop with an OpenAI Responses API adapter and sequential function calling.
- Durable model cursors, pending tool calls, usage counters, terminal results, and digest-addressed agent-run events.
- Fail-closed crash recovery that reconciles persisted tool or approval results without blindly replaying interrupted mutations, and replays terminal failed or budget-exceeded outcomes without appending new checkpoints or events.
- Approval expiration capped by the run duration deadline, with deadline enforcement before approval validation, reconciliation, or execution so late decisions cannot dispatch tools.
- Native CLI `agent create`, `list`, `inspect`, `start`, `resume`, and `watch` commands.
- Deterministic `agent evaluate` policies with strict unknown-field rejection that verify signed tool, approval, lifecycle, final-report, and failure evidence; validate run/step topology, retry lineage, timestamps, and approval chronology; and bind each result to canonical policy and trace digests that remain stable across repeated storage reads.
- A loopback-only browser approval console that shows the exact governed context and records signed human decisions without executing the tool.
- Evidence-returning execution engine APIs used by runtimes to receive the exact persisted signed proof.

#### Execution Engine

- Registry-backed operation discovery for versioned operations.
- Governed execution with operation-not-found, human-only, missing-handler, invalid-delegation, handler-failure, evidence-failure, and storage-failure error paths.
- Optional `ExecutionStore` integration that persists execution contexts and signed proofs after successful execution.
- Deterministic operation keypairs and transport-configurable engine state.
- Canonical JSON serialization and BLAKE3 content digests for operation inputs and outputs.
- `BenchmarkRunner` for operation-level benchmarks with input execution, output JSON Schema validation, duration thresholds, and structured pass/fail results.
- `ExecutionEngine::verify_benchmark` for validating a benchmark against the benchmark ID declared by a registry operation.
- Optional kernel tracing integration through the `proof-observability` crate and the `tracing` feature.

#### Content Management

- Schema, object, changeset, edition, release, and principal content models.
- Generic governed content handlers for `schema.create`, `object.create`, `object.edit`, `content.approve`, `content.release`, and `release.publish`.
- Registry-derived input schema validation for content handlers.
- Canonical content digests for schemas, objects, and release-manifest entries.
- Object revision and lifecycle state transitions.
- `ReleasePipeline` for creating and editing governed objects, invoking `release.publish`, and producing release, object, change, and release-proof artifacts.
- Canonical release manifests with deterministic entries and a release digest.
- Independent release-manifest verification against a set of objects.

#### Workflow Management

- Workflow definition, run, and step models with lifecycle state-machine validation.
- Governed handlers for `workflow.define`, `workflow.trigger`, `workflow.step.complete`, and `workflow.approve`.
- Canonical JSON digests for workflow definitions and runs.
- SQLite persistence for workflow definitions, runs, and steps with ordered migrations and round-trip tests.
- HTTP endpoints for workflow and workflow-run listing, with registry-backed execution for all four operations.
- Workflow governance conformance suite covering human-only approval rejection, lifecycle sequencing, and UUIDv7 idempotency-key enforcement.

#### Commerce Management

- Catalog, product, and order models with governed lifecycle handlers.
- Governed handlers for `catalog.create`, `catalog.update`, `order.create`, `order.approve`, and `order.fulfill`.
- SQLite persistence for catalogs, products, and orders with stale-timestamp conflict detection.
- HTTP endpoints for catalog and order listing, with registry-backed execution for all five operations.

#### Analytics Management

- Snapshot, query, and insight models with governed lifecycle handlers.
- Governed handlers for `analytics.snapshot.create`, `analytics.query.create`, `analytics.query.execute`, and `analytics.insight.approve`.
- SQLite persistence for snapshots, queries, and insights with ordered migrations and round-trip tests.
- HTTP endpoints for snapshot and query listing, with registry-backed execution for all four operations.
- Analytics governance conformance suite covering human-only insight approval rejection, lifecycle sequencing, and UUIDv7 idempotency-key enforcement.

#### Unified Storage Persistence

- Domain operations persist through `SqliteStore` rather than ad-hoc JSON files, making SQLite the authoritative system of record for commerce, workflow, and analytics records.
- SQLite WAL journal mode and foreign-key enforcement enabled for concurrent read safety and referential integrity.

#### Storage

- SQLite-backed persistence for proofs, execution contexts, principals, delegations, registry entries, schemas, objects, changesets, editions, and releases.
- Immutable durable principal bindings: repeated saves of the same ID, kind, and public key are idempotent, while conflicting identity material is rejected.
- Ordered, idempotent schema migrations with version tracking and rollback support.
- Content-addressed blob storage backed by SQLite metadata and filesystem objects.
- Proof lookup by operation, operation/version, actor, and proof ID.
- Signature and digest-chain verification for ordered proof sequences.
- Expired execution-context cleanup and proof/context counters.
- Registry save/load with deterministic ordering and transactional persistence.
- Workflow definition, run, and step persistence with list, load, and delete support.

#### Transports

##### HTTP

- Axum transport with registry-backed engine state, a process-local keypair,
  SQLite storage, and handler registration.
- `POST /v1/operations/:name/:version` routed through `ExecutionEngine` with generated signed proofs.
- Operation execution mappings for registry, governance, dispatch, delegation, handler, evidence, and storage errors.
- Proof collection, filtering, retrieval, and signature verification endpoints.
- Execution-context audit listing.
- Schema and object listing endpoints.

##### MCP

- Registry-derived MCP tool schemas, including input and output schemas.
- Registry governance and consequence mappings to MCP annotations for destructive, idempotent, and read-only behavior.
- Cursor-based MCP tool pagination with a 20-tool page size.
- Governed tool calls routed through `ExecutionEngine` with input and output schema enforcement.
- Structured tool results containing execution output and signed proof evidence.

##### CLI

- Content lifecycle commands for schema creation, object creation, changeset creation, edition creation, and release publication.
- Workspace initialization, keypair persistence, status reporting, and JSON output.
- Registry listing and per-operation registry inspection.
- Governed operation execution with local content handlers and proof persistence.
- Cryptographic proof verification using persisted workspace identities.
- Delegation grant, listing, revocation, and validation commands backed by SQLite.
- Explicit workspace selection plus workspace initialization and status
  commands.
- Keypair export and rotation with archived prior keypairs.

#### Delegation

- Ed25519-backed principal identity for human, agent, and service actors.
- Bounded delegations with allowed actions, resource scopes, validity windows, unique IDs, and revocation state.
- Ordered delegation-chain validation for root connectivity, terminal actor, time windows, revocation, and connected authority links.
- Authority-narrowing checks for child action and resource-scope patterns.
- Pattern matching for exact values, universal wildcards, and trailing wildcards.
- Execution-time delegation-chain validation before handler dispatch.

#### Proof Management

- Signed Ed25519 proof envelopes binding actor, delegation, operation, input digest, output digest, and timestamp.
- Canonical proof signing payloads and stable proof digests.
- Proof signature and actor verification.
- Persistent principal storage for verifying proofs signed by identities other than the transport workspace identity.
- Ordered proof-chain verification.

#### Observability

- Optional structured JSON tracing to stderr with configurable verbosity.
- Operation spans recording operation, version, actor, proof ID, duration, and success/failure.
- HTTP request middleware generating UUIDv7 request IDs, path/status/duration metrics, and structured completion events.

### Changed

- Reorganized the public documentation into a concise project overview,
  task-oriented getting-started guide, core concepts, CLI and HTTP references,
  security model, development guide, and an implementation-focused
  architecture document.
- Clarified the current security and maturity boundaries for the generic HTTP
  transport, the unreleased browser approval console, split development
  storage paths, and the not-yet-assembled AXP-E0002 operator surface.
- Centralized execution authority in `proof-kernel::ExecutionEngine` across CLI, HTTP, and MCP transports.
- HTTP execution now uses the engine's registry and governance checks rather than an ad-hoc operation path.
- MCP tool discovery is now driven directly by registry entries and exposes registry schemas.
- Each configured SQLite transport store now composes proof persistence,
  execution audit, principals, delegations, and registry state; the generic
  HTTP binary still uses a separate development database path.
- HTTP registry schema loading and record listing are unified across content, commerce, workflow, and analytics domains.
- Unauthenticated HTTP execution no longer trusts the caller-controlled `X-Principal-Kind` header; human-only operations fail closed until routed through signed approval evidence.
- The legacy `proof release-publish` shortcut now fails closed instead of bypassing HumanOnly governance and writing a release directly.
- Proof operation filters treat SQL `%` and `_` characters literally across storage and HTTP queries.
- MCP approval execution timestamps are identical to their signed proof timestamps, matching native recovery and evaluation invariants.
- Cancelled native runs use SQLite's immediate terminal seal and can persist a deterministic failing task evaluation without a synthetic failure event.
- MCP governance conformance tests now cover all four domains (content, commerce, workflow, analytics).
- Registry directory loading now ignores adjacent input/output JSON Schema files and accepts nested dot-delimited operation names.
- Domain handlers resolve schemas from workspace-local `.proof/registry` directories for installed runtimes.
- Execution proofs now sign the canonical `operation::version` composite. Proofs emitted by older builds with a bare operation remain signature-valid legacy bytes, but fail closed in version-bound run/evaluation checks and must be regenerated.

### Known Limitations

- Rotating proof envelopes and full multi-workspace CLI management are not yet present in the committed kernel/CLI surface.

[Unreleased]: https://github.com/smithdak/proof-platform/compare/v0.1.0...HEAD
