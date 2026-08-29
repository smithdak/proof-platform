# Changelog

All notable changes to Proof Platform are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

#### Storage

- SQLite-backed persistence for proofs, execution contexts, principals, delegations, registry entries, schemas, objects, changesets, editions, and releases.
- Ordered, idempotent schema migrations with version tracking and rollback support.
- Content-addressed blob storage backed by SQLite metadata and filesystem objects.
- Proof lookup by operation, operation/version, actor, and proof ID.
- Signature and digest-chain verification for ordered proof sequences.
- Expired execution-context cleanup and proof/context counters.
- Registry save/load with deterministic ordering and transactional persistence.
- Workflow definition, run, and step persistence with list, load, and delete support.

#### Transports

##### HTTP

- Axum transport with registry-backed engine state, workspace keypair, SQLite store, and handler registration.
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
- Workspace creation, listing, switching, removal, and status commands.
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

- Centralized execution authority in `proof-kernel::ExecutionEngine` across CLI, HTTP, and MCP transports.
- HTTP execution now uses the engine’s registry and governance checks rather than an ad-hoc operation path.
- MCP tool discovery is now driven directly by registry entries and exposes registry schemas.
- SQLite storage is now shared across proof persistence, execution audit, principals, delegations, and registry state.
- README architecture and usage documentation was expanded and reconciled with the implemented platform surface.

### Known Limitations

- The WebSocket transport is functional but has no dedicated test coverage.
- Rotating proof envelopes and full multi-workspace CLI management are not yet present in the committed kernel/CLI surface.

[Unreleased]: https://github.com/example/proof-platform/compare/v0.1.0...HEAD
