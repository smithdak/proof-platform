# Proof Platform

Governed execution for autonomous software.

Proof Platform is an Agent Experience Platform (AXP): a Rust runtime and
control foundation where agents discover typed operations, act with bounded
authority, pause for signed human decisions, and return independently
verifiable evidence.

Every successful governed operation binds the actor, authority, operation,
canonical input, canonical output, and execution time into a signed proof.
Durable runs add checkpoints, approvals, retries, budgets, and evaluations
without creating a second execution path.

> **Project status:** Proof is pre-1.0 and under active development. The kernel,
> CLI, MCP transport, generic HTTP transport, durable agent runtime, SQLite
> persistence, and four domain modules are implemented. The independently
> authenticated operator control plane tracked in AXP-E0002 is not yet a
> released product surface.

## Start here

| Goal | Read this |
|---|---|
| Install Proof and complete a local walkthrough | [Getting started](docs/getting-started.md) |
| Understand evidence, authority, approvals, and runs | [Core concepts](docs/concepts.md) |
| See component boundaries and data flow | [Architecture](ARCHITECTURE.md) |
| Look up CLI commands and common workflows | [CLI reference](docs/cli-reference.md) |
| Run or integrate the development HTTP transport | [HTTP API](docs/http-api.md) |
| Connect an MCP client | [MCP server guide](crates/proof-transport-mcp/README.md) |
| Understand trust boundaries and safe deployment | [Security model](docs/security-model.md) |
| Build, test, or contribute | [Development guide](docs/development.md) |
| Follow product delivery | [AXP Editions](editions/README.md) |

The [documentation index](docs/README.md) provides the complete map.

## Why Proof

- **Evidence by construction.** Governed operations return signed proofs over
  canonical inputs and outputs; evidence is part of execution, not a log added
  afterward.
- **Bounded authority.** Registry governance and delegation chains constrain
  what an actor can do, to which resources, and for how long.
- **Durable agent work.** Runs preserve steps, checkpoints, approval waits,
  retry lineage, budget use, and terminal evaluations.
- **Transport parity.** CLI, MCP, HTTP, and WebSocket adapters use the same
  registry and execution engine rather than inventing separate policy paths.
- **Domain extensibility.** Content, commerce, workflow, and analytics build on
  shared kernel contracts while retaining domain-owned models and handlers.

## How it works

```text
CLI · MCP · HTTP · WebSocket
             │
             ▼
   Operation registry lookup
             │
             ▼
 Governance + delegation checks
             │
             ▼
     Domain handler dispatch
             │
             ▼
 Canonical result + signed proof
             │
             ▼
 SQLite evidence · run history · audit
```

For an agent run, the runtime wraps that operation path with model decisions,
tool allowlists, hard budgets, checkpoints, and signed human approval. A
recovered process reuses durable results where safe and fails closed around an
interrupted mutation.

## Quickstart

You need a stable Rust toolchain. [`jq`](https://jqlang.github.io/jq/) is useful
for the JSON examples but is not required by Proof itself.

Install the CLI from the repository root:

```bash
cargo install --path crates/proof-transport-cli
proof --version
```

Create a disposable workspace outside the repository and install the bundled
operation registry:

```bash
PROOF_DEMO="$(mktemp -d)"
export PROOF_DEMO
proof --workspace "$PROOF_DEMO" init
cp -R registry/. "$PROOF_DEMO/.proof/registry/"
proof --workspace "$PROOF_DEMO" capabilities
```

Create a typed content schema and an object. CLI output is JSON, so the schema
ID can be passed directly into the next command:

```bash
SCHEMA_ID="$(
  proof --workspace "$PROOF_DEMO" schema-create \
    --name Article \
    --fields '[{"name":"title","field_type":"text","required":true}]' \
  | jq -r '.id'
)"

proof --workspace "$PROOF_DEMO" object-create \
  --schema-id "$SCHEMA_ID" \
  --locale en-US \
  --data '{"title":"Hello from Proof"}'

proof --workspace "$PROOF_DEMO" status
```

This creates a local Ed25519 workspace identity, persists the records, and
produces signed operation evidence. Continue with the
[full walkthrough](docs/getting-started.md) to execute registry operations,
configure an agent, or connect MCP.

## Choose an integration surface

| Surface | Best for | Current boundary |
|---|---|---|
| `proof` CLI | Local development, scripts, workspace and run administration | Local process and filesystem authority |
| `proof-mcp` | MCP clients and model-driven tool use | Stdio; workspace selected at launch |
| Rust crates | Embedded runtimes and custom domain handlers | In-process APIs and explicit dependency injection |
| Generic HTTP | Local API development and transport tests | Binds `0.0.0.0:3000` with a process-local signer; not an authenticated operator plane |
| WebSocket | Registry-derived tool discovery and governed execution | Development transport |

> **Do not expose the generic HTTP binary to an untrusted network.** Its current
> routes are development surfaces, it binds all interfaces by default, and it
> generates a new signing identity at each start. It does not provide the
> independent operator authentication required by AXP-E0002. See the
> [HTTP security notes](docs/http-api.md#security-boundary).

## Core guarantees

1. Registry entries use stable `domain.action` operation names and `v<N>`
   versions.
2. Mutating registry entries declare contract-defined idempotency; handlers
   that select exact replay enforce it with durable UUIDv7 claims.
3. Structured inputs and outputs are canonicalized before digesting.
4. Successful governed execution returns a signed proof.
5. Human-only operations require signed request and decision evidence; a UI
   acknowledgement alone grants no authority.
6. Agent attempts belong to durable runs with explicit status and lineage.
7. Secrets and raw custody tokens are not part of public evidence.

The precise shared types and invariants live in
[`contracts/kernel-api.md`](contracts/kernel-api.md) and
[`contracts/domain-definitions.md`](contracts/domain-definitions.md).

## Repository guide

| Path | Responsibility |
|---|---|
| `crates/proof-kernel` | Identity, delegation, registry, engine, canonical data, proofs, and durable contracts |
| `crates/proof-agent-runtime` | Provider-neutral model/tool loop, budgets, checkpoints, recovery, and evaluation |
| `crates/proof-storage` | SQLite persistence and content-addressed blob storage |
| `crates/proof-content` | Content schemas, objects, changesets, editions, and releases |
| `crates/proof-commerce` | Catalog, product, order, and fulfillment operations |
| `crates/proof-workflow` | Workflow definitions, runs, steps, and approvals |
| `crates/proof-analytics` | Snapshots, queries, execution, and insights |
| `crates/proof-transport-*` | CLI, HTTP, MCP, and WebSocket adapters |
| `registry` | Data-driven operation manifests and wire schemas |
| `contracts` | Canonical cross-crate and product contracts |
| `evals` | Deterministic evaluator policies and fixtures |
| `editions` | Governed product delivery records and evidence |

## Development

Format and test only the crate you changed:

```bash
cargo fmt --check -p proof-kernel
cargo test -p proof-kernel
```

Repository agents and orchestrated edition work must follow
[`AGENTS.md`](AGENTS.md), including scoped reverse-impact checks and exclusive
path ownership. Human contributors can find the full local workflow in the
[development guide](docs/development.md).

## Delivery and release status

Product work is tracked as auditable AXP Editions. Each edition records its
charter, owner decisions, task ownership, acceptance evidence, and release
gate. A completed implementation task is not automatically a released product.

- [Edition backlog](editions/BACKLOG.md)
- [Edition operating model](editions/README.md)
- [Current AXP-E0002 status](editions/AXP-E0002/status.md)
- [Changelog](CHANGELOG.md)

The workspace packages declare the MIT license in Cargo metadata.
