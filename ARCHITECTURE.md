# Proof Platform: Foundational Architecture

**Status:** Draft — project-owner review required
**Date:** 2026-08-27
**Supersedes:** Proof CMS implementation (archived as reference)

## What Proof is

Proof is a governed, agent-native platform where autonomous software discovers,
composes, and executes operations across any domain, producing cryptographic
evidence for every governed transition. Content governance is the first domain
module; the kernel is domain-agnostic.

## Core thesis

Every consequential action taken by software should produce independently
verifiable evidence. Every agent should operate under bounded authority. Every
system should compose from a shared operation registry rather than hard-coded
interfaces. These three principles — evidence, authority, composability —
define the platform.

## Architecture layers

```
┌──────────────────────────────────────────────────────────┐
│                    Proof Platform                         │
├──────────────────────────────────────────────────────────┤
│            SAM / Mesh Transport (adapter layer)           │
├─────────────┬─────────────┬─────────────┬───────────────┤
│   Content   │  Analytics  │  Commerce   │   Workflow    │
│  Governance │  & Insight  │  & Orders   │  & Approvals  │
│  (domain 1) │  (domain 2) │  (domain 3) │   (domain 4)  │
├─────────────┴─────────────┴─────────────┴───────────────┤
│              Evidence & Audit Kernel                      │
├──────────────────────────────────────────────────────────┤
│           Operation Registry (data-driven JSON)           │
│    Discovery · Authority · Composition · Execution       │
├──────────────────────────────────────────────────────────┤
│         Identity & Delegation (Ed25519 + SAM/OIDC)       │
├──────────────────────────────────────────────────────────┤
│         Storage (SQLite dev / PostgreSQL prod)            │
└──────────────────────────────────────────────────────────┘
```

## Layer definitions

### Identity & Delegation (foundation)

- Every actor (human or agent) has a cryptographic identity (Ed25519 keypair).
- Delegations are bounded: issuer, recipient, allowed actions, resource scope,
  validity interval, revocation state.
- Sub-delegation is forbidden unless explicitly granted.
- SAM's OIDC enrollment and Biscuit credentials map to Proof's Principal model.
- Identity is environment-agnostic: the same key works on cloud, local, or edge.

### Operation Registry (data-driven)

The registry is a JSON manifest — not a Rust enum, not compiled code. Adding an
operation means adding a JSON row. The runtime discovers capabilities at startup.

```json
{
  "operation": "object.create",
  "domain": "content",
  "version": "v1",
  "action": "content:object_create",
  "description": "Create one immutable Object with typed fields",
  "input_schema": "content/operations/object-create.input.json",
  "output_schema": "content/operations/object-create.output.json",
  "required_authority": "delegation-grant",
  "governance": "agent-executable",
  "idempotency": "required-uuidv7",
  "consequence": "content-mutation",
  "evidence_contract": "operation-effect-v1",
  "benchmark": "B1"
}
```

Registry entries are versioned. Old entries are never removed — they are
deprecated. The runtime loads the registry from disk, discovers all available
operations, and generates the HTTP, MCP, and CLI surfaces automatically.

### Evidence & Audit Kernel

- Every governed operation produces a signed Proof.
- Proofs bind: who acted, under what authority, with what input, producing what
  output, validated by which deterministic validators.
- Proofs are independently reconstructable without the producing system's
  private keys (offline verification).
- Evidence is domain-agnostic: a content mutation and an analytics query use
  the same proof envelope.

### Domain Modules

Each domain (content, analytics, commerce, workflow) is a self-contained module
that:

1. Registers its operations in the shared registry
2. Uses the shared evidence pipeline
3. Uses the shared identity and delegation model
4. May define domain-specific validators, policies, and storage extensions

Domains do NOT modify the kernel. They extend it through the registry.

### Transport (adapter layer)

The kernel exposes operations through pluggable transports:

- **SAM/Sovereign Agent Mesh** — preferred for agent-to-agent and
  cross-environment operation. Zero-trust P2P mesh with MCP sidecar routing.
- **HTTP/REST** — standard web API for human-facing applications and SDKs.
- **MCP (Model Context Protocol)** — for LLM-based agents.
- **CLI** — for developers and scripting.
- **Embedded** — in-process for single-binary deployments.

No transport is privileged. All transports call the same kernel operations.

## Rust workspace structure

```
proof-platform/
├── crates/
│   ├── proof-kernel/          # Identity, delegation, evidence, registry
│   │   ├── src/
│   │   │   ├── identity.rs    # Ed25519 keys, Principal types
│   │   │   ├── delegation.rs  # Bounded delegation model
│   │   │   ├── evidence.rs    # Proof generation, signing, verification
│   │   │   ├── registry.rs    # Data-driven operation registry loader
│   │   │   ├── canonical.rs   # RFC 8785 canonical JSON + digests
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── proof-content/         # Content governance domain (first domain)
│   │   ├── src/
│   │   │   ├── schema.rs      # Content schema definitions
│   │   │   ├── object.rs      # Object lifecycle (create, mutate, archive)
│   │   │   ├── changeset.rs   # Atomic ChangeSet pipeline
│   │   │   ├── edition.rs     # Immutable Edition snapshots
│   │   │   ├── release.rs     # Release to Environments
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── proof-transport-http/  # HTTP/REST adapter
│   ├── proof-transport-mcp/   # MCP adapter
│   ├── proof-transport-cli/   # CLI adapter
│   ├── proof-transport-sam/   # SAM mesh adapter (optional)
│   └── proof-storage/         # SQLite + PostgreSQL storage adapters
├── registry/                  # Operation registry manifests (JSON)
│   ├── content/               # Content domain operations
│   ├── kernel/                # Platform-level operations
│   └── schema.json            # Registry entry schema
├── schemas/                   # Input/output JSON Schemas
│   ├── content/
│   └── kernel/
├── conformance/               # Conformance vectors and benchmarks
└── Cargo.toml                 # Workspace root
```

## Design constraints

These constraints are non-negotiable. Any change that violates one is rejected.

### D1. Registry is data, not code

Operations are defined in JSON manifests. Adding an operation never requires
recompilation of the kernel. Domain modules register at startup by reading
their manifest files.

### D2. Evidence is structural, not optional

Every governed operation MUST produce evidence. There is no "evidence-free"
path. Operations that cannot produce evidence are not registered.

### D3. Authority is bounded and evaluated at execution time

No operation executes without current authorization. Authority is checked
immediately before commit, not at proposal time.

### D4. All transports are peers

HTTP, MCP, CLI, and SAM all call the same kernel operations. No transport has
privileged access or special-case logic.

### D5. Domains extend, never modify

Domain modules register operations and validators through the registry. They
do not patch the kernel, bypass the evidence pipeline, or add private paths.

### D6. Idempotency is required for mutations

Every mutating operation requires an idempotency key. Retrying a completed
mutation with the same key returns the original result. Retrying with a
different key or input fails explicitly.

### D7. Canonical JSON is the wire format

All structured data uses RFC 8785 canonical JSON. Digests are
algorithm-qualified (BLAKE3-256). This ensures byte-identical serialization
across all transports and implementations.

### D8. Storage is pluggable

The kernel defines storage traits. SQLite and PostgreSQL are reference
implementations. Any storage backend that implements the traits is compatible.

### D9. Independent verification

Proofs can be verified without access to the producing system. The verifier
needs only: the proof envelope, the public keys it references, and the
canonical data the proof covers.

### D10. Performance is measured, not assumed

Every capability has benchmark targets (latency, throughput, concurrency).
A capability that cannot meet its benchmark does not close.

## Competitive positioning

| Dimension | Sitecore | Contentful | Contentstack | Umbraco | Proof |
|---|---|---|---|---|---|
| Governance | Opaque audit log | None | Basic | None | Cryptographic evidence, independently verifiable |
| Agent operability | Bolted-on AI | MCP server, no authority | Chatbot wrapper | None | First-class agents with bounded delegation |
| Performance | C#/.NET, slow | Node.js, moderate | Node.js, moderate | C#/.NET, moderate | Rust, measurably faster |
| Deployment | .NET monolith + Azure | SaaS only | SaaS only | .NET self-hosted | Single binary → SQLite → PostgreSQL → mesh |
| Registry | Compiled code | API config | API config | Compiled code | Data-driven JSON, zero recompilation |
| Lock-in | Extreme | High | High | Moderate | None — open contracts, portable evidence |
| Cost | $500K+/yr + implementation | Per-seat/per-call | Per-seat/per-call | Free but limited | Free core, pay for hosted + metered agents |

## Monetization model

1. **Free open-source core** — kernel + content governance domain. Distribution
   channel and trust builder. No license revenue.
2. **Hosted Workspaces** — managed Proof instances with SLAs, backups, scaling,
   SSO. Priced per Workspace.
3. **Agent operation metering** — charge for governed agent execution outcomes
   (bulk imports, migrations, content pipelines), not per API call.
4. **Domain expansion** — each new domain (analytics, commerce, workflow) is a
   new product line on the same platform.
5. **Ecosystem marketplace** — third-party validators, workflows, connectors,
   UI components. Revenue share model.

## Go-to-market sequence

1. **Win developers**: single binary, `proof init`, running in 60 seconds.
   Great DX, TypeScript/Python SDK. Agents can create schemas and content.
2. **Make agents productive**: agent imports 50K objects, localizes into 12
   languages, publishes to production. Bounded authority, batch approval.
3. **Governance becomes the enterprise gate**: once agents do real work,
   evidence and bounded authority become requirements. Now we charge.
4. **Kill incumbents on cost**: 1/10th infrastructure cost, 1/10th
   implementation time, no per-API-call pricing, no lock-in.

## Walking skeleton (first milestone)

The smallest working system that proves every layer:

1. `proof init` — initialize a Workspace with embedded SQLite
2. `proof schema create` — define a content Schema
3. `proof object create` — create an Object conforming to the Schema
4. `proof changeset commit` — commit a ChangeSet with deterministic validation
5. `proof edition create` — snapshot the committed state
6. `proof release publish` — release to an Environment
7. `proof verify` — independently verify the Release evidence

Every step produces a signed Proof. The entire loop runs from a single binary
with zero external dependencies. MCP tools are auto-generated from the registry.

## What we leave behind

The previous Proof CMS implementation is archived as a reference
implementation. Its intellectual assets are preserved:

- ADR-0014 (45-capability canon) → content domain registry entries
- ADR-0015 (performance benchmark spec) → conformance targets
- Pain-point register → competitive positioning
- Gap matrix → content domain implementation waves
- Constitutional invariants → kernel design constraints

Its implementation debt (god modules, frozen enums, tight coupling) is NOT
carried forward.
