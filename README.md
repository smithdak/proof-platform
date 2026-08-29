# Proof Platform

Proof Platform is a governed, agent-native Rust platform in which autonomous software discovers and executes operations through a data-driven registry, bounded delegation, and domain handlers, while successful governed transitions produce signed cryptographic proof of execution. Its kernel binds the actor, authority, operation, input digest, output digest, and timestamp into independently verifiable evidence. Three domains — content governance, commerce, and workflow — are fully implemented and conformance-tested.

- **Full architecture:** [ARCHITECTURE.md](ARCHITECTURE.md)
- **Changelog:** [CHANGELOG.md](CHANGELOG.md)
- **Rust version:** 1.75 or newer
- **License:** MIT

## Architecture

```text
┌────────────────────────────────────────────────────────────────────┐
│                           Humnan/Agents                            │
└──────────────┬──────────────────┬───────────────────┬──────────────┘
               │                  │                   │
┌──────────────▼──────┐ ┌─────────▼─────────┐ ┌───────▼────────────┐
│       CLI           │ │    HTTP / REST    │ │        MCP         │
│ proof               │ │  axum transport   │ │  registry-derived  │
└──────────────┬──────┘ └─────────┬─────────┘ └───────┬────────────┘
               │                  │                   │
┌──────────────▼──────────────────▼───────────────────▼──────────────┐
│                    Execution Engine (proof-kernel)                 │
│  registry → governance → delegation → handler dispatch → evidence   │
│   benchmarks · canonical JSON · digests · optional operation spans   │
└──────────────┬──────────────────┬───────────────────┬──────────────┘
               │                  │                   │
┌──────────────▼──────┐ ┌─────────▼─────────┐ ┌───────▼────────────┐
│ Operation Registry  │ │  Content Domain   │ │      Storage       │
│ versioned JSON rows │ │ lifecycle handlers │ │  SQLite + blobs   │
└─────────────────────┘ └───────────────────┘ └────────────────────┘

Observability supplies structured JSON operation spans and HTTP request metrics.
```

The transports are intentionally thin. The registry is the source of capability metadata and governance policy; the engine is the authority for execution and evidence; storage adapters persist proofs, execution contexts, and domain records. Optional observability records the same operations and HTTP requests without changing authorization or evidence semantics.

## Crates

| Crate                  | Purpose                                                                                                                                    |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `proof-kernel`         | Registry, execution engine, benchmarks, delegation, canonical JSON, Ed25519 identity, proofs                                               |
| `proof-content`        | Content models, lifecycle handlers, and release pipeline                                                                                   |
| `proof-storage`        | SQLite migrations and storage for proofs, contexts, principals, delegations, registry entries, and domain records; content-addressed blobs |
| `proof-transport-cli`  | Developer-oriented `proof` binary                                                                                                          |
| `proof-transport-http` | Axum HTTP API on `0.0.0.0:3000`                                                                                                            |
| `proof-transport-mcp`  | Registry-derived MCP tool schemas and governed tool-call execution                                                                         |
| `proof-observability`  | Structured JSON tracing, operation spans, and HTTP request middleware                                                                      |

## Quickstart

Build and install the CLI from the repository root:

```bash
cargo install --path crates/proof-transport-cli
```

Initialize a workspace. The command creates `.proof/`, a local Ed25519 workspace keypair, SQLite storage, and workspace data directories:

```bash
proof init
```

Copy the repository registry into the workspace. The CLI loads `.proof/registry`, so this step makes its current content operations available:

```bash
mkdir -p .proof/registry
cp -R registry/. .proof/registry/
```

Run the content lifecycle:

```bash
SCHEMA_ID="$(
  proof schema-create \
    --name 'Article' \
    --fields '[{"name":"title","field_type":"text","required":true}]' \
  | jq -r .id
)"

OBJECT_ID="$(
  proof object-create \
    --schema-id "$SCHEMA_ID" \
    --locale en-US \
    --data '{"title":"Hello"}' \
  | jq -r .id
)"

CHANGESET_ID="$(
  proof changeset-create --intent 'Initial content' | jq -r .id
)"

proof edition-create --changeset-id "$CHANGESET_ID"
proof release-publish --edition-id "$EDITION_ID" --environment preview
```

Execute a registry operation directly through the governed engine:

```bash
proof execute schema.create v1 --input '{
  "name": "Article",
  "version": 1,
  "fields": [
    { "name": "title", "field_type": "text", "required": true }
  ]
}'
```

The repository registry currently declares these operations. The library content-handler set covers the first six; the CLI’s legacy direct path also handles `changeset.create`:

| Operation          | Version | Library handler |
| ------------------ | ------- | --------------- |
| `schema.create`    | `v1`    | Yes             |
| `object.create`    | `v1`    | Yes             |
| `object.edit`      | `v1`    | Yes             |
| `content.approve`  | `v1`    | Yes             |
| `content.release`  | `v1`    | Yes             |
| `release.publish`  | `v1`    | Yes             |
| `changeset.commit` | `v1`    | No              |

Registry entries without a registered handler will fail with `NoHandler`. Register a handler for the exact logical operation before executing it through the engine.

### Run the HTTP server

Build the server:

```bash
cargo build --release -p proof-transport-http
./target/release/proof-transport-http
```

Set `PROOF_WORKSPACE` to an initialized workspace directory; it defaults to `.`. The server listens on port `3000` and uses `<workspace>/.proof/data/proofs/proofs.sqlite3` for evidence, audit, principal, and registry persistence.

```bash
export PROOF_WORKSPACE="$PWD"
./target/release/proof-transport-http
```

The repository registry must be copied to `<workspace>/.proof/registry`, as shown above.

### Run the MCP transport

`proof-transport-mcp` is a library for embedding MCP tool execution. Generate the tool list and route calls through the governed engine:

```rust
use std::path::PathBuf;

use proof_kernel::{ExecutionContext, ExecutionEngine, Registry};
use proof_transport_mcp::{handle_tool_call, tools_from_registry};

let registry = Registry::load_from_directory("registry")?;
let tools = tools_from_registry(&registry);

let mut engine = ExecutionEngine::new(registry);
for handler in proof_content::handlers::content_handlers() {
    engine.register_handler(handler);
}

let context = ExecutionContext {
    actor: keypair.principal_id,
    delegation_id: None,
    delegation_chain: None,
    workspace_path: PathBuf::from("."),
    timestamp: chrono::Utc::now(),
};

let call = proof_transport_mcp::McpToolCall {
    name: "proof_content_v1_schema_create".to_string(),
    arguments: serde_json::json!({
        "name": "Article",
        "version": 1,
        "fields": [{ "name": "title", "type": "text", "required": true }]
    }),
};

let result = handle_tool_call(&call, &engine, context.actor, PathBuf::from("."));
```

MCP tools expose input/output schemas and annotations derived from the registry’s governance and consequence fields. Tool calls are validated, routed through `ExecutionEngine`, and return structured result content with a signed proof.

MCP tool names use this form:

```text
proof_<domain>_<version>_<operation.with.dots.encoded.as.underscores>
```

## Platform Capabilities

### Governance and execution

- Versioned JSON operations are discovered from `.proof/registry`.
- `ExecutionEngine` enforces registry governance, validates optional delegation chains, dispatches handlers, and returns typed kernel errors.
- Successful engine executions persist the execution context and signed proof when an `ExecutionStore` is configured.
- Operation inputs and outputs use canonical JSON and BLAKE3 content digests.
- Benchmark contracts measure duration and validate operation output against JSON Schema success criteria.

### Content management

- The content domain provides schema, object, changeset, edition, release, and principal models.
- Governed handlers cover schema creation, object creation and editing, approval, content release, and release publication.
- `ReleasePipeline` applies create/edit changes, invokes the governed release operation, creates a release manifest, and emits proofs for both content changes and the release.
- `verify_release` validates a canonical manifest against the supplied objects.

### Evidence and identity

- Ed25519 identities represent humans, agents, and services.
- Proofs bind actor, delegation, operation, input digest, output digest, and timestamp.
- Proofs can be signed, independently signature-verified, and verified as ordered digest chains.
- Persisted principals allow verification of proofs not signed by the current transport identity.

### Delegation

- Delegations bound action authority, resource scope, validity interval, and revocation state.
- Ordered delegation chains are validated from a trusted root to the executing agent.
- Child grants must preserve or narrow parent action and resource-scope patterns.
- Exact, universal (`*`), and trailing-wildcard patterns are supported.

### Storage

- SQLite persists proofs, execution contexts, principals, delegations, registry entries, and content records.
- Schema migrations are versioned, idempotent, and support rollback helpers.
- Proof-chain queries and context expiration helpers are available on `SqliteStore`.
- The content-addressed store persists blobs on the filesystem with SQLite metadata, references, and garbage collection.

### Observability

- `proof-observability` provides structured JSON logging to stderr and four verbosity levels.
- Operation spans record start/completion, actor, proof ID, duration, and success state.
- HTTP middleware emits request ID, path, status, and duration metrics.
- Kernel operation spans are optional and enabled with the `proof-kernel/tracing` feature.

## HTTP API

### `GET /`

Returns service metadata:

```json
{
  "name": "proof",
  "description": "Governed agent-native content platform",
  "api_version": "v1"
}
```

### `GET /health`

Returns `{"status":"ok"}`.

### `GET /capabilities`

Returns registry-derived capabilities with operation name, version, domain, and governance level.

### `POST /v1/operations/:name/:version`

Executes a registry operation. The request body is the operation input.

```bash
curl -X POST http://localhost:3000/v1/operations/schema.create/v1 \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "Article",
    "version": 1,
    "fields": [
      { "name": "title", "type": "text", "required": true }
    ]
  }'
```

Successful response shape:

```json
{
  "operation": "schema.create",
  "version": "v1",
  "status": "executed",
  "result": { "schema_id": "..." },
  "proof": {
    "body": {
      "id": "...",
      "actor": "...",
      "operation": "schema.create",
      "input_digest": "...",
      "output_digest": "...",
      "timestamp": "..."
    },
    "signature": [64-byte Ed25519 signature array]
  }
}
```

Error responses:

| Status | Meaning                                                                       |
| ------ | ----------------------------------------------------------------------------- |
| `404`  | Operation/version is not in the registry                                      |
| `403`  | Operation is `human-only`                                                     |
| `500`  | No handler, handler failure, invalid delegation, evidence, or storage failure |

### `GET /v1/schemas` and `GET /v1/objects`

List JSON files currently in `<workspace>/.proof/data/schemas` and `.proof/data/objects`, respectively.

### `GET /v1/proofs`

Returns the full proofs collection from storage.

### `GET /proofs`

Returns proofs with these optional query parameters:

| Parameter   | Type        | Meaning                                                                               |
| ----------- | ----------- | ------------------------------------------------------------------------------------- |
| `operation` | String      | Exact proof operation filter                                                          |
| `actor`     | UUID string | Exact actor filter                                                                    |
| `version`   | String      | Acceptable parameter; combining it with `operation` currently returns an empty result |

Example:

```bash
curl 'http://localhost:3000/proofs?operation=object.create&actor=<uuid>'
```

### `GET /proofs/:id`

Returns a proof and its verification status (`"verified"` or `"invalid"`), checked against the workspace keypair or a stored signing principal.

### `POST /proofs/verify`

Verifies a stored proof against the HTTP server’s keypair.

```bash
curl -X POST http://localhost:3000/proofs/verify \
  -H 'Content-Type: application/json' \
  -d '{"proof_id":"<uuid>"}'
```

Response:

```json
{ "proof_id": "<uuid>", "valid": true }
```

### `GET /audit`

Returns saved execution contexts ordered by descending timestamp. Each context includes storage ID, actor, workspace path, and timestamp.

## CLI Reference

Global options apply before the subcommand:

```bash
proof --workspace <PATH> <COMMAND>
proof -w <PATH> <COMMAND>
proof --verbose <COMMAND>
```

The default workspace is `.`.

| Command                                     | Arguments                                                                    | Purpose                                                                            |
| ------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `proof init`                                | none                                                                         | Initialize `.proof`, create workspace keypair and data directories                 |
| `proof schema-create`                       | `--name <name>`, `--fields <json>`                                           | Validate, save, and prove a schema creation                                        |
| `proof object-create`                       | `--schema-id <uuid>`, `--locale <locale>` (default `en-US`), `--data <json>` | Validate against schema, save, and prove object creation                           |
| `proof changeset-create`                    | `--intent <text>`                                                            | Create a changeset and signed proof                                                |
| `proof edition-create`                      | `--changeset-id <uuid>`                                                      | Snapshot current objects into an edition                                           |
| `proof release-publish`                     | `--edition-id <uuid>`, `--environment <name>`                                | Publish an edition to an environment                                               |
| `proof status`                              | none                                                                         | Count saved schemas, objects, changesets, editions, releases, and proofs           |
| `proof capabilities`                        | none                                                                         | Print registry operations and governance levels                                    |
| `proof registry list`                       | none                                                                         | List operation, version, domain, action, and governance                            |
| `proof registry inspect <operation>`        | operation name                                                               | Print matching registry entries                                                    |
| `proof verify <proof-id>`                   | proof ID                                                                     | Verify a saved proof with the workspace keypair                                    |
| `proof execute <operation> <version>`       | `--input <json>`                                                             | Execute through the engine with a local content handler, sign, and persist a proof |
| `proof workspace init <path>`               | workspace path                                                               | Initialize an additional workspace                                                 |
| `proof workspace status`                    | none                                                                         | Report registry, proof, and principal counts for the selected workspace            |
| `proof keypair export`                      | none                                                                         | Print the principal ID and public key                                              |
| `proof keypair rotate`                      | none                                                                         | Archive the current keypair and generate a new workspace identity                  |
| `proof delegation grant <agent-id>`         | `--scope <json>`                                                             | Persist a bounded delegation grant                                                 |
| `proof delegation list`                     | none                                                                         | List persisted delegations                                                         |
| `proof delegation revoke <delegation-id>`   | none                                                                         | Revoke a grant issued by the workspace identity                                    |
| `proof delegation validate <delegation-id>` | none                                                                         | Validate a grant as a delegation chain                                             |

`schema-create` field objects accept:

| Field        | Type                                                                               | Required               |
| ------------ | ---------------------------------------------------------------------------------- | ---------------------- |
| `name`       | string                                                                             | Yes                    |
| `field_type` | `text`, `rich_text`, `number`, `boolean`, `date`, `date_time`, `json`, `reference` | No; defaults to `text` |
| `required`   | boolean                                                                            | No                     |
| `localized`  | boolean                                                                            | No                     |
| `default`    | JSON value                                                                         | No                     |

CLI outputs are JSON.

## Add an Operation

Add a JSON entry under a domain directory in the workspace or repository registry. The loader accepts files recursively and rejects duplicate `operation`/`version` pairs.

Create `registry/content/object-edit.json`:

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

Field contract:

| Field                | Meaning                                                              |
| -------------------- | -------------------------------------------------------------------- |
| `operation`          | Stable logical name, such as `object.edit`                           |
| `domain`             | Domain namespace used by MCP tool names and discovery                |
| `version`            | Registry/API version, such as `v1`                                   |
| `action`             | Delegation authority token, such as `content:object_edit`            |
| `description`        | Human- and agent-readable description                                |
| `input_schema`       | Inline JSON Schema document or referenced JSON Schema string         |
| `output_schema`      | Inline JSON Schema document or referenced JSON Schema string         |
| `required_authority` | Authority policy token; use `delegation-grant` for bounded authority |
| `governance`         | `agent-executable` or `human-only`                                   |
| `idempotency`        | Idempotency policy string                                            |
| `consequence`        | Consequence classification string                                    |
| `evidence_contract`  | Proof/evidence contract identifier                                   |
| `benchmark`          | Optional performance or conformance benchmark ID                     |

Then register an `OperationHandler` for the exact logical operation. A minimal Rust handler:

```rust
use std::sync::Arc;

use proof_kernel::{ExecutionContext, ExecutionError, OperationHandler};
use serde_json::{json, Value};

#[derive(Debug)]
struct ObjectEditHandler;

impl OperationHandler for ObjectEditHandler {
    fn operation(&self) -> &str {
        "object.edit"
    }

    fn execute(
        &self,
        input: &Value,
        _context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        Ok(json!({ "edited": input }))
    }
}

fn register(engine: &mut proof_kernel::ExecutionEngine) {
    engine.register_handler(Arc::new(ObjectEditHandler));
}
```

MCP tool discovery updates automatically from the registry. HTTP `capabilities` is also registry-derived via `ExecutionEngine`.

### Benchmarks

Benchmark definitions are caller-supplied contracts tied to a registry entry’s optional `benchmark` ID. `BenchmarkRunner::run` measures execution, validates output against `success_criteria`, compares elapsed time with `max_duration_ms`, and returns a structured `BenchmarkResult`. Use `ExecutionEngine::verify_benchmark` to reject contracts that do not match the operation’s declared benchmark ID.

## Delegation Chains

Every principal has an Ed25519 identity represented by `PrincipalId`. A `Delegation` grants authority from an issuer to a recipient and includes:

- `allowed_actions`: action patterns such as `content:*`
- `resource_scope`: resource patterns such as `site/a/pages/*`
- `valid_from` and `valid_until`
- `revoked`
- Unique delegation UUID

A `DelegationChain` contains the trusted root `PrincipalId` and an ordered set of grants. `validate_chain(root, executing_agent, grants, now)` enforces:

1. The chain is non-empty.
2. The first grant’s issuer is the trusted root.
3. Every later issuer equals the previous recipient.
4. The final recipient is the executing agent.
5. Every grant is unrevoked and valid at the supplied time.
6. Every child action and resource scope is within the parent grant’s authority.

The first grant is treated as the root grant for action/resource authority checks; subsequent grants must narrow or preserve authority. Pattern matching supports exact values, the universal wildcard `*`, and trailing wildcards such as `content:*`.

At execution, `ExecutionContext.delegation_chain` is optional. When present, the engine validates it against the context actor and timestamp before dispatching the handler. If absent, the engine’s registry governance check still applies, but callers must establish authority through the embedding transport or deployment policy.

## Testing and Status

```bash
cargo fmt --check
cargo test --workspace
```

### Current status

- Kernel registry, execution engine, benchmarks, canonical JSON, Ed25519 identities, delegation validation, and proofs are implemented.
- SQLite stores proofs, execution contexts, principals, delegations, registry entries, and domain records.
- CLI, HTTP, and MCP transports route supported operations through the governed engine.
- The content, commerce, and workflow domains are implemented and tested.
- WebSocket transport is functional but has no dedicated tests yet.
- Proof envelopes do not yet support rotation/expiry links; workspace keypair rotation is available in the CLI.
- Full multi-workspace management is not yet implemented; the CLI supports initializing and inspecting additional workspaces with `--workspace`.
