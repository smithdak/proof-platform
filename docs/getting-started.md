# Getting started

This guide installs the CLI, creates an isolated workspace, and produces the
first signed operation evidence. It then points to the agent and transport
paths you can add when needed.

## Prerequisites

- A stable Rust toolchain with Cargo.
- A Unix-like shell for the examples.
- `jq` if you want to capture IDs from JSON output exactly as shown.

All commands begin at the Proof Platform repository root.

## 1. Install the CLI

```bash
cargo install --path crates/proof-transport-cli
proof --version
```

During active development, rebuild the installed binary after pulling source
changes. You can also run the package directly with Cargo.

## 2. Initialize a workspace

Use a disposable directory rather than the repository root:

```bash
PROOF_DEMO="$(mktemp -d)"
export PROOF_DEMO
proof --workspace "$PROOF_DEMO" init
```

Initialization creates a `.proof/` directory containing a new Ed25519
workspace identity, SQLite storage, registry directory, and local data tree. It
fails rather than replacing an existing or partial identity.

Install the repository's registry into that workspace:

```bash
cp -R registry/. "$PROOF_DEMO/.proof/registry/"
proof --workspace "$PROOF_DEMO" capabilities
```

## 3. Create typed content

Create a schema and capture its ID:

```bash
SCHEMA_ID="$(
  proof --workspace "$PROOF_DEMO" schema-create \
    --name Article \
    --fields '[
      {"name":"title","field_type":"text","required":true},
      {"name":"published","field_type":"boolean"}
    ]' \
  | jq -r '.id'
)"
```

Create an object validated by that schema:

```bash
proof --workspace "$PROOF_DEMO" object-create \
  --schema-id "$SCHEMA_ID" \
  --locale en-US \
  --data '{"title":"Hello from Proof","published":false}'
```

Inspect the workspace summary:

```bash
proof --workspace "$PROOF_DEMO" status
```

Both create commands return JSON. The output includes the durable record ID
and signed proof ID.

## 4. Execute through the governed engine

The direct lifecycle commands are convenient local authoring helpers. To use
registry discovery, governance, handler validation and dispatch, idempotency,
and evidence persistence as one path, call `execute`. The current
`schema.create` handler accepts a complete serialized schema definition, so
supply a UUIDv7 identifier, an RFC 3339 timestamp, and every field attribute
explicitly:

```bash
proof --workspace "$PROOF_DEMO" execute schema.create v1 \
  --input '{
    "id":"01a06748-e81b-7e33-a32b-928db60d370f",
    "name":"Brief",
    "version":1,
    "fields":[
      {
        "name":"headline",
        "field_type":"text",
        "required":true,
        "localized":false,
        "default_value":null
      }
    ],
    "created_at":"2026-09-03T12:00:00Z"
  }'
```

If a registry entry is present but its handler is not registered in the
selected binary, Proof returns `NoHandler`. Discovery alone does not grant an
implementation.

## 5. Verify evidence

Copy a proof ID from a successful response and verify it with the workspace
identity:

```bash
proof --workspace "$PROOF_DEMO" verify <PROOF_ID>
```

Verification checks the signed envelope and its canonical digests. Persisted
principals allow storage-backed verification of evidence signed by identities
other than the current workspace key.

## Optional: run a native agent

Native agents require a provider credential and can incur external cost. Start
with explicit model, tool, and budget choices:

```bash
export OPENAI_API_KEY='<key>'
export OPENAI_MODEL='<model>'

proof --workspace "$PROOF_DEMO" agent create \
  --name content-assistant \
  --instructions 'Create only the content requested by the operator.' \
  --model "$OPENAI_MODEL" \
  --tool schema.create::v1 \
  --max-steps 4 \
  --max-model-calls 6 \
  --max-total-tokens 20000 \
  --max-duration-seconds 300
```

Copy the returned agent ID, then start and watch a run:

```bash
proof --workspace "$PROOF_DEMO" agent start <AGENT_ID> \
  --goal 'Create a schema for short editorial briefs'
proof --workspace "$PROOF_DEMO" agent watch <RUN_ID>
```

Tool references use `operation::version`. The runtime rejects tools outside the
definition's allowlist and stops at configured budgets.

### Handle a human-only operation

When a run reaches a human-only tool, it persists a waiting approval request.
Use the terminal signing flow:

```bash
proof --workspace "$PROOF_DEMO" approval approver-init
proof --workspace "$PROOF_DEMO" approval list
proof --workspace "$PROOF_DEMO" approval approve <REQUEST_ID> \
  --approver <APPROVER_ID> \
  --reason 'Reviewed exact operation and arguments'
proof --workspace "$PROOF_DEMO" agent resume <RUN_ID>
```

Use `approval deny` to record a denial. A decision never resumes or executes a
tool automatically.

The standalone browser approval console remains unreleased after AXP-E0006's
deferred Gate C. Do not treat `proof approval ui` as a supported security path;
use the terminal commands above.

## Optional: connect MCP

Install the stdio server:

```bash
cargo install --path crates/proof-transport-mcp --bin proof-mcp
proof-mcp --workspace "$PROOF_DEMO"
```

Use an absolute workspace path in client configuration. The complete setup,
run metadata, and approval protocol are documented in the
[MCP server guide](../crates/proof-transport-mcp/README.md).

## Optional: run the development HTTP transport

```bash
export PROOF_WORKSPACE="$PROOF_DEMO"
cargo run -p proof-transport-http
```

The binary currently binds `0.0.0.0:3000`, exposes development routes without
independent operator authentication, generates a new signing identity at each
start, and uses a different SQLite path from the CLI and MCP server. Do not
expose it to an untrusted network. See the [HTTP API](http-api.md) for the exact
boundary.

## Workspace layout

```text
<workspace>/.proof/
├── config.json          # Workspace actor binding
├── keypair.json         # Private workspace signing identity
├── registry/            # Installed operation manifests and schemas
├── storage/storage.db   # CLI and MCP durable store
├── approvers/           # Local human signing keys, when enrolled
└── data/                # Local content records and proof files
```

Treat `.proof/keypair.json` and `.proof/approvers/` as secrets. Never commit a
workspace directory.

## Troubleshooting

| Symptom | Likely cause | Resolution |
|---|---|---|
| `workspace not initialized` | The selected path has no valid `.proof/` identity | Run `proof --workspace <PATH> init` once |
| Empty capabilities | The workspace registry is empty | Copy `registry/.` into `.proof/registry/` |
| `NoHandler` | The entry is discoverable but the binary has no matching handler | Register the exact logical operation in the embedding binary |
| Human-only rejection | The call lacks signed request and human-decision evidence | Use an agent run and the terminal approval flow |
| HTTP and CLI show different records | The current transports open different SQLite paths | Treat the generic HTTP server as a separate development surface |
| Identity mismatch | `config.json` and `keypair.json` name different actors | Recover the intended workspace; do not overwrite identity files manually |

Next: read [Core concepts](concepts.md) or browse the
[CLI reference](cli-reference.md).
