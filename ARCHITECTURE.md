# Proof Platform architecture

**Status:** Active implementation architecture

**Last reviewed:** 2026-09-03

**Audience:** Contributors, integrators, and technical reviewers

Proof Platform is a governed execution system for autonomous software. It
separates capability discovery, authority, domain behavior, durable agent
coordination, and cryptographic evidence so each layer can evolve without
creating a policy bypass.

This document describes the current repository. Future directions are labeled
explicitly and are not claims of released functionality.

## Architectural thesis

Three principles shape the platform:

1. **Evidence:** every successful governed operation produces independently
   verifiable evidence over canonical inputs and outputs.
2. **Authority:** execution applies registry governance and rechecks any
   bounded delegation at the effect boundary; human approval is signed
   authority, not UI state.
3. **Composability:** agents and transports discover versioned operations from
   shared data contracts rather than hard-coded capability lists.

The result is one execution path that can serve local scripts, agent runtimes,
MCP clients, HTTP integrations, and future control surfaces.

## System context

```text
┌─────────────────────────────────────────────────────────────────────┐
│ Callers                                                             │
│ CLI scripts · MCP clients · HTTP clients · embedded applications    │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ protocol adaptation
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Transport layer                                                     │
│ CLI · MCP stdio · HTTP · WebSocket                                  │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ typed operation request
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Kernel                                                              │
│ registry → governance → delegation → handler → canonical evidence   │
└──────────────┬───────────────────────────────┬──────────────────────┘
               │                               │
               ▼                               ▼
┌──────────────────────────────┐  ┌───────────────────────────────────┐
│ Domain handlers              │  │ Durable coordination              │
│ content · commerce           │  │ runs · approvals · budgets        │
│ workflow · analytics         │  │ checkpoints · leases · evaluation│
└──────────────┬───────────────┘  └─────────────────┬─────────────────┘
               │                                    │
               └─────────────────┬──────────────────┘
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Persistence                                                         │
│ SQLite records · content-addressed blobs · append-only evidence      │
└─────────────────────────────────────────────────────────────────────┘
```

The agent runtime sits above the kernel: it decides when to call an allowlisted
operation, but it does not bypass that operation's governance or evidence path.

## Component responsibilities

| Component | Owns | Does not own |
|---|---|---|
| `proof-kernel` | Principals, delegations, registry loading, execution, canonical JSON, proofs, shared durable contracts | Domain persistence, protocol parsing, model-provider behavior |
| `proof-agent-runtime` | Agent definitions, model/tool loop, budgets, checkpoints, approvals, recovery, trace evaluation | Registry authority, domain mutations, transport authentication |
| `proof-storage` | SQLite implementations, migrations, evidence records, domain persistence, blobs | Policy decisions or domain semantics |
| Domain crates | Domain models, validation, handlers, lifecycle rules | Kernel changes or private execution paths |
| Transport crates | Protocol adaptation, transport-owned request/response behavior | Alternate governance or evidence rules |
| `proof-operator-auth` | Independent operator identity/session primitives under its accepted contract | Generic transport authentication by implication |
| `proof-operator-control` | Control-shell primitives and protected composition interfaces | A released assembled operator product |
| `proof-observability` | Structured tracing and HTTP request instrumentation | Authorization or durable evidence |
| Registry and schemas | Discoverable capabilities and wire validation | Handler implementation |
| Evaluators and fixtures | Deterministic acceptance policy and test evidence | Runtime authority |

## Governed operation flow

The kernel's direct execution path follows these stages:

1. **Discover.** Resolve the exact `operation::version` registry entry.
2. **Authorize.** Apply registry governance and validate any supplied
   delegation chain against the actor and execution time.
3. **Claim replay state.** Enforce the handler's idempotency policy against the
   configured execution store.
4. **Dispatch and validate.** Invoke the registered handler, which owns input
   decoding and domain validation.
5. **Canonicalize.** Serialize the result using the canonical JSON contract.
6. **Prove.** Sign evidence binding actor, authority, operation, input digest,
   output digest, and timestamp.
7. **Persist.** Store the execution context, proof, and replay result when an
   execution store is configured.
8. **Return.** Give the caller both the typed result and its evidence.

Schema enforcement currently differs by entry point. MCP validates input and
output against the adjacent registry JSON Schemas. Prepared operator execution
uses an immutable schema catalog. The legacy direct CLI and generic HTTP paths
rely on handler-owned decoding and validation; the generic kernel path does not
load adjacent schema files itself. This is an implementation boundary, not
permission for a transport to weaken governance.

Missing registry entries, insufficient authority, idempotency conflicts,
absent handlers, domain-validation failures, and evidence failures stop before
a successful result is claimed.

## Registry model

The operation registry is a recursive tree of JSON manifests and schemas. An
entry identifies the domain, stable operation name, `v<N>` version, authority
action, governance, consequence, schemas, idempotency rule, evidence contract,
and optional benchmark.

```json
{
  "operation": "object.edit",
  "domain": "content",
  "version": "v1",
  "action": "content:object_edit",
  "description": "Edit one Object revision",
  "input_schema": "content/object-edit.input.json",
  "output_schema": "content/object-edit.output.json",
  "required_authority": "delegation-grant",
  "governance": "agent-executable",
  "idempotency": "required-uuidv7",
  "consequence": "content-mutation",
  "evidence_contract": "operation-effect-v1",
  "benchmark": "B1"
}
```

Adding an entry changes discovery, not implementation. A domain or embedding
binary must still register a compatible handler for the exact logical
operation. Each transport derives tool or capability metadata from the same
registry data.

Normative operation sets and wire contracts live in
[`contracts/domain-definitions.md`](contracts/domain-definitions.md).

## Identity, delegation, and human authority

Ed25519 principals represent humans, agents, and services. A delegation binds
issuer, recipient, actions, resource scopes, validity, and revocation. A chain
must connect the trusted root to the executing actor, and each child must
preserve or narrow its parent's authority.

Human-only operations introduce a second signature boundary:

```text
Agent signs exact request
          │
          ▼
Runtime persists approval wait
          │
          ▼
Human signs approve or deny decision
          │
          ▼
Runtime verifies both records
          │
          ▼
Explicit resume → governed dispatch or denial
```

Approval does not resume automatically. Reusing a completed exact request
returns its persisted result rather than dispatching again.

## Durable agent lifecycle

An immutable agent definition binds instructions, provider/model selection, an
operation allowlist, and hard budgets. A run is the durable attempt to satisfy
one goal with that definition.

```text
queued → running → waiting_for_input ── explicit resume ─┐
   │         │                                           │
   │         ├─ checkpoint → model call → tool request ──┘
   │         │
   │         ├─ succeeded
   │         ├─ failed (including budget exhaustion)
   │         └─ cancelled
   └─ run state is durable and lifecycle events are append-only
```

Each tool attempt is an `AgentRunStep`. Immutable checkpoints preserve the
next model input, provider cursor, pending tool, and accumulated usage before
effect boundaries. Retries create linked attempts; they do not replace prior
history.

Recovery reuses completed steps and persisted approval execution. It does not
blindly replay an interrupted mutation when no durable result proves the
outcome. Leases and fences reject stale workers and writes in operator-aware
paths.

## Evidence model

Proofs bind versioned operation execution to canonical data. Kernel evidence
uses domain-separated BLAKE3-256 `ContentDigest` values, serialized in proof
envelopes as 64 lowercase hexadecimal characters. Content snapshot identifiers
already versioned as SHA-256 retain their algorithm-qualified `sha256:<hex>`
form until a later contract changes them explicitly.

Evidence layers are complementary:

- a **proof** attests to one operation result;
- an **execution context** records its actor and authority boundary;
- **run events** record lifecycle chronology;
- **approval records** bind human intent to one request; and
- an **evaluation** asserts whether a sealed trace satisfies a named policy.

No “evidence-free” successful governed path exists.

## Persistence and workspace layout

The kernel depends on storage traits; `proof-storage` supplies the current
SQLite implementation and content-addressed blob storage.

For CLI and MCP workspaces:

```text
<workspace>/.proof/
├── config.json
├── keypair.json
├── registry/
├── storage/storage.db
├── approvers/
└── data/
```

The generic HTTP transport currently opens
`.proof/data/proofs/proofs.sqlite3`, not the CLI/MCP database. That split is a
known development boundary. It also creates a process-local signer at startup,
so its actor identity changes across restarts. AXP-E0002 requires one
authoritative store and stable identity handling for its future assembled
operator product; accepted lower-layer components do not by themselves
complete that composition.

SQLite migrations are ordered and append-only. Existing migration bytes and
version numbers are immutable.

## Design invariants

### 1. Registry is data, not a kernel enum

Capabilities are described in versioned manifests. Domains register handlers
without adding domain-specific branches to the kernel.

### 2. Evidence is structural

A governed success includes its evidence. Logging or transport metadata cannot
substitute for a proof.

### 3. Authority is checked at the effect boundary

Proposal-time authority is insufficient. Expiry, revocation, lease, fence, and
budget state must still permit the operation immediately before an effect.

### 4. Transports are peers

No transport may weaken kernel governance or invent a privileged handler path.
Transport-specific authentication remains explicit rather than assumed.

### 5. Domains extend the platform

Domains own their models and lifecycle rules while using shared identity,
execution, evidence, and storage contracts.

### 6. Mutations declare idempotency by contract

Registry mutations declare their idempotency requirement. A handler that
selects durable exact replay can return a completed mutation only for the same
scoped UUIDv7 key and canonical input; key reuse with different input fails
before mutation. Handler policy remains the current enforcement boundary.

### 7. Canonical representation is part of the API

Operation names, versions, JSON canonicalization, digest algorithms, and proof
storage keys are contract values, not formatting preferences.

### 8. Recovery fails closed

Durable state may prove a safe replay. Ambiguous mutation state is never treated
as permission to try again.

### 9. Verification is independent

A verifier needs the proof envelope, referenced public identity, and canonical
data—not the producer's private key.

### 10. Acceptance is measured

Benchmarks, deterministic evaluators, scoped tests, and edition evidence decide
whether a capability is accepted. Implementation presence alone is not release.

## Extending Proof

To add or change an operation:

1. Read the owning domain definition and shared kernel contract.
2. Add or version the registry manifest and input/output schemas.
3. Implement the domain-owned handler without modifying the kernel for domain
   semantics.
4. Register that handler in each embedding binary that should execute it.
5. Add unit, transport, storage round-trip, and conformance coverage required
   by the affected layer.
6. Verify idempotency, consequence, authority, and evidence behavior.
7. Update user-facing reference documentation and the changelog.

Public contracts, migrations, security boundaries, and external effects also
require the applicable AXP owner gate.

## Repository structure

```text
proof-platform/
├── crates/
│   ├── proof-kernel/            # Shared contracts and governed engine
│   ├── proof-agent-runtime/     # Durable model/tool runtime
│   ├── proof-storage/           # SQLite and blob persistence
│   ├── proof-operator-auth/     # Operator authentication primitives
│   ├── proof-operator-control/  # Control-shell composition primitives
│   ├── proof-content/           # Content domain
│   ├── proof-commerce/          # Commerce domain
│   ├── proof-workflow/          # Workflow domain
│   ├── proof-analytics/         # Analytics domain
│   ├── proof-observability/     # Tracing and request instrumentation
│   └── proof-transport-*/       # CLI, HTTP, MCP, and WebSocket adapters
├── registry/                    # Operation manifests and adjacent schemas
├── schemas/                     # Shared JSON Schemas
├── contracts/                   # Normative public and cross-crate contracts
├── evals/                       # Deterministic policies and fixtures
├── conformance/                 # Cross-component conformance harness
├── docs/                        # Task-oriented documentation
└── editions/                    # Governed product delivery records
```

## Current and future boundaries

| Area | Current repository | Future direction, not yet released |
|---|---|---|
| Execution | Registry-backed kernel with signed proofs | Broader remote and mesh composition |
| Storage | SQLite and filesystem blobs | Additional production storage backends |
| Transports | CLI, MCP stdio, generic HTTP, WebSocket | Hardened remote operator access |
| Agents | Provider-neutral runtime with OpenAI adapter | Additional providers and richer orchestration |
| Operator experience | Accepted lower-layer auth/control/storage/runtime work | Assembled protected API, UI, and Gate C release |
| Domains | Content, commerce, workflow, analytics | Additional registry-driven domains |

Use [AXP Edition status](editions/README.md) to distinguish planned, accepted,
and released product work. Strategic direction does not override a missing
contract, task dependency, or Gate C decision.

## Related documentation

- [Core concepts](docs/concepts.md)
- [Security model](docs/security-model.md)
- [Kernel API contract](contracts/kernel-api.md)
- [Domain definitions](contracts/domain-definitions.md)
- [AXP experience contract](contracts/axp-experience.md)
- [Development guide](docs/development.md)
