# Proof Platform

Proof Platform is a governed, agent-native Rust platform in which autonomous software discovers and executes operations through a data-driven registry, bounded delegation, and domain handlers, while every successful governed transition produces a signed cryptographic proof of execution. Its kernel binds the actor, authority, operation, input digest, output digest, and timestamp into independently verifiable evidence, while content governance provides the first domain implementation.

- **Full architecture:** [ARCHITECTURE.md](ARCHITECTURE.md)
- **Rust version:** 1.75 or newer
- **License:** MIT

## Architecture

```text
┌────────────────────────────────────────────────────────────────────┐
│                             Clients                                │
└──────────────┬──────────────────┬───────────────────┬──────────────┘
               │                  │                   │
┌──────────────▼──────┐ ┌─────────▼─────────┐ ┌───────▼────────────┐
│       CLI           │ │    HTTP / REST    │ │        MCP         │
│ proof               │ │  axum transport   │ │  registry-derived  │
└──────────────┬──────┘ └─────────┬─────────┘ └───────┬────────────┘
               │                  │                   │
┌──────────────▼──────────────────▼───────────────────▼──────────────┐
│                    Execution Engine (proof-kernel)                 │
│   registry lookup → governance → delegation → handler dispatch     │
│   canonical input/output → proof generation → execution audit      │
└──────────────┬──────────────────┬───────────────────┬──────────────┘
               │                  │                   │
┌──────────────▼──────┐ ┌─────────▼─────────┐ ┌───────▼────────────┐
│ Operation Registry  │ │  Content Domain   │ │      Storage       │
│ versioned JSON rows │ │  handlers/models  │ │  SQLite adapter    │
└─────────────────────┘ └───────────────────┘ └────────────────────┘
```

The transports are intentionally thin. The registry is the source of capability metadata and governance policy; the engine is the authority for execution and evidence; storage adapters persist proofs, execution contexts, and domain records.

## Crates

| Crate | Purpose |
|---|---|
| `proof-kernel` | Registry, execution engine, delegation, canonical JSON, Ed25519 identity, proofs |
| `proof-content` | Content models and governed content operation handlers |
| `proof-storage` | SQLite storage for proofs, execution contexts, principals, and domain records |
| `proof-transport-cli` | Developer-oriented `proof` binary |
| `proof-transport-http` | Axum HTTP API on `0.0.0.0:3000` |
| `proof-transport-mcp` | Registry-derived MCP tool schemas and governed tool-call execution |

## Quickstart

Build and install the CLI from the repository root:

```bash
cargo install --path crates/proof-transport-cli
```

Initialize a workspace. The command creates `.proof/`, a local Ed25519 workspace keypair, and workspace data directories:

```bash
proof init
```

Copy the repository registry into the workspace. The CLI loads `.proof/registry`, so this step makes its current content operations available:

```bash
mkdir -p .proof/registry
cp -R registry/. .proof/registry/
```

Run the lifecycle:

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

The repository registry currently declares these operations:

| Operation | Version | Action | Governance | Required authority |
|---|---|---|---|---|
| `changeset.commit` | `v1` | `content:changeset_commit` | `agent-executable` | `delegation-grant` |
| `object.create` | `v1` | `content:object_create` | `agent-executable` | `delegation-grant` |
| `schema.create` | `v1` | `content:schema_create` | `agent-executable` | `delegation-grant` |

The engine currently exposes handlers for `schema.create`, `object.create`, and `changeset.create`; registry-only entries require the handler noted below before they can execute.

### Run the HTTP server

Build the server:

```bash
cargo build --release -p proof-transport-http
./target/release/proof-transport-http
```

Set `PROOF_WORKSPACE` to an initialized workspace directory; it defaults to `.`. The server listens on port `3000` and uses `<workspace>/.proof/data/proofs/proofs.sqlite3` for evidence persistence.

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

MCP tool names use this form:

```text
proof_<domain>_<version>_<operation.with.dots.encoded.as.underscores>
```

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

Returns a static capability list with operation name, version, domain, and governance level.

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

| Status | Meaning |
|---|---|
| `404` | Operation/version is not in the registry |
| `403` | Operation is `human-only` |
| `500` | No handler, handler failure, invalid delegation, evidence, or storage failure |

### `GET /v1/schemas` and `GET /v1/objects`

List JSON files currently in `<workspace>/.proof/data/schemas` and `.proof/data/objects`, respectively.

### `GET /v1/proofs`

Returns the full proofs collection from storage.

### `GET /proofs`

Returns proofs with these optional query parameters:

| Parameter | Type | Meaning |
|---|---|---|
| `operation` | String | Exact proof operation filter |
| `actor` | UUID string | Exact actor filter |
| `version` | String | Acceptable parameter; combining it with `operation` currently returns an empty result |

Example:

```bash
curl 'http://localhost:3000/proofs?operation=object.create&actor=<uuid>'
```

### `GET /proofs/:id`

Returns a proof and its local verification marker. The implementation currently reports `"verification":"unverified"`; use `POST /proofs/verify` for signature verification.

### `POST /proofs/verify`

Verifies a stored proof against the HTTP server’s keypair.

```bash
curl -X POST http://localhost:3000/proofs/verify \
  -H 'Content-Type: application/json' \
  -d '{"proof_id":"<uuid>"}'
```

Response:

```json
{"proof_id":"<uuid>","valid":true}
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

| Command | Arguments | Purpose |
|---|---|---|
| `proof init` | none | Initialize `.proof`, create workspace keypair and data directories |
| `proof schema-create` | `--name <name>`, `--fields <json>` | Validate, save, and prove a schema creation |
| `proof object-create` | `--schema-id <uuid>`, `--locale <locale>` (default `en-US`), `--data <json>` | Validate against schema, save, and prove object creation |
| `proof changeset-create` | `--intent <text>` | Create a changeset and signed proof |
| `proof edition-create` | `--changeset-id <uuid>` | Snapshot current objects into an edition |
| `proof release-publish` | `--edition-id <uuid>`, `--environment <name>` | Publish an edition to an environment |
| `proof status` | none | Count saved schemas, objects, changesets, editions, releases, and proofs |
| `proof capabilities` | none | Print registry operations and governance levels |
| `proof registry list` | none | List operation, version, domain, action, and governance |
| `proof registry inspect <operation>` | operation name | Print matching registry entries |
| `proof verify <proof-id>` | proof ID | Verify a saved proof with the workspace keypair |
| `proof execute <operation> <version>` | `--input <json>` | Execute through the engine, sign, and persist a proof |

`schema-create` field objects accept:

| Field | Type | Required |
|---|---|---|
| `name` | string | Yes |
| `field_type` | `text`, `rich_text`, `number`, `boolean`, `date`, `date_time`, `json`, `reference` | No; defaults to `text` |
| `required` | boolean | No |
| `localized` | boolean | No |
| `default` | JSON value | No |

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

| Field | Meaning |
|---|---|
| `operation` | Stable logical name, such as `object.edit` |
| `domain` | Domain namespace used by MCP tool names and discovery |
| `version` | Registry/API version, such as `v1` |
| `action` | Delegation authority token, such as `content:object_edit` |
| `description` | Human- and agent-readable description |
| `input_schema` | Inline JSON Schema document or referenced JSON Schema string |
| `output_schema` | Inline JSON Schema document or referenced JSON Schema string |
| `required_authority` | Authority policy token; use `delegation-grant` for bounded authority |
| `governance` | `agent-executable` or `human-only` |
| `idempotency` | Idempotency policy string |
| `consequence` | Consequence classification string |
| `evidence_contract` | Proof/evidence contract identifier |
| `benchmark` | Optional performance or conformance benchmark ID |

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

MCP tool discovery updates automatically from the registry. HTTP `capabilities` is currently a static list and should be updated alongside the registry until it is derived directly from `ExecutionEngine`.

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

- Kernel registry, engine, canonical JSON, Ed25519 identities, delegation validation, and proofs are implemented.
- SQLite stores proofs, execution contexts, principals, delegations, and domain records.
- CLI, HTTP, and MCP transports execute registry operations through the governed engine.
- Workspace is in active development. The repository has unreconciled changes around kernel test storage exports and content-handler feature wiring, so `cargo check --workspace` may fail until those changes are integrated.
- `GET /proofs?version=...` and the `/capabilities` static list are known compatibility gaps.
- `GET /proofs/:id` currently reports `unverified`; use the verification endpoint.
