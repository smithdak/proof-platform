# HTTP API

`proof-transport-http` is the generic Axum development transport. It exposes
registry discovery, governed operation execution, domain listings, proof
queries, and audit records.

## Security boundary

> **Development use only.** The binary binds `0.0.0.0:3000` by default and its
> generic read routes do not provide the independently authenticated operator
> boundary specified by AXP-E0002. Do not expose it directly to an untrusted
> network or use it as the operator control plane.

The HTTP transport also opens
`<workspace>/.proof/data/proofs/proofs.sqlite3`, while the CLI and MCP server
use `<workspace>/.proof/storage/storage.db`. Until the planned authoritative
store composition is released, treat those as separate development data
planes.

The binary generates a new Ed25519 signing identity when it starts. Its actor
identity is therefore not stable across process restarts, and a restarted
server cannot use its new in-memory key to verify proofs signed by the prior
process. This is another reason to treat the transport as disposable local
infrastructure.

## Start the server

Initialize and populate a disposable workspace as shown in
[Getting started](getting-started.md), then select it explicitly before
running from the repository root:

```bash
export PROOF_WORKSPACE=/absolute/path/to/disposable-workspace
cargo run -p proof-transport-http
```

The process defaults to `.` when `PROOF_WORKSPACE` is unset, but explicit
selection prevents accidental writes to the repository-root workspace. Install
the repository registry into `<workspace>/.proof/registry/` before starting if
you need capability discovery or operation execution.

## Routes

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/` | Service metadata |
| `GET` | `/health` | Liveness response |
| `GET` | `/capabilities` | Registry-derived operations |
| `POST` | `/v1/operations/:name/:version` | Governed operation execution |
| `GET` | `/v1/schemas` | Local schema records |
| `GET` | `/v1/objects` | Local object records |
| `GET` | `/catalog` | Commerce catalog records |
| `GET` | `/orders` | Commerce order records |
| `GET` | `/workflows` | Workflow definitions |
| `GET` | `/workflow-runs` | Workflow run records |
| `GET` | `/analytics-snapshots` | Analytics snapshots |
| `GET` | `/analytics-queries` | Analytics query records |
| `GET` | `/v1/proofs` | Full proof collection |
| `GET` | `/v1/proofs/:id` | One proof |
| `GET` | `/proofs` | Filtered proof collection |
| `GET` | `/proofs/:id` | One proof with verification state |
| `POST` | `/proofs/verify` | Verify a stored proof |
| `GET` | `/audit` | Execution contexts, newest first |

These are the current generic transport routes. They are not the protected
operator route set planned by AXP-E0002.

## Service metadata and health

```bash
curl http://127.0.0.1:3000/
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3000/capabilities
```

The health response is:

```json
{"status":"ok"}
```

## Execute an operation

The request body is the operation input. The route name and version must match
one registry entry. The current `schema.create` handler expects a complete
serialized schema definition, including a UUIDv7 identifier and RFC 3339
timestamp:

```bash
curl --request POST \
  http://127.0.0.1:3000/v1/operations/schema.create/v1 \
  --header 'Content-Type: application/json' \
  --data '{
    "id":"01a06748-e81b-7e33-a32b-928db60d370f",
    "name":"Article",
    "version":1,
    "fields":[
      {
        "name":"title",
        "field_type":"text",
        "required":true,
        "localized":false,
        "default_value":null
      }
    ],
    "created_at":"2026-09-03T12:00:00Z"
  }'
```

A successful response contains the operation, version, `executed` status,
typed result, and signed proof envelope.

### Execution errors

| Status | Meaning |
|---|---|
| `400` | Invalid JSON, content type, or idempotency key |
| `403` | Governance rejects the call, including human-only execution without signed approval evidence |
| `404` | The operation/version is not found |
| `409` | Idempotency state conflicts or required benchmark evidence expired |
| `410` | The registry entry is sunset |
| `413` | The request exceeds the configured body limit |
| `429` | The client exceeds the configured request rate |
| `500` | Input/output schema validation, handler, evidence, or storage processing fails |
| `503` | Durable storage required by the idempotency contract is unavailable |

Do not use status codes alone as durable command receipts. Product mutation
APIs require the command/idempotency semantics defined by their contract.

## Query proofs

Filter the `/proofs` collection by exact operation or actor:

```bash
curl 'http://127.0.0.1:3000/proofs?operation=object.create'
curl 'http://127.0.0.1:3000/proofs?actor=<PRINCIPAL_UUID>'
```

Available query parameters are `operation`, `version`, `actor`, `limit`,
`offset`, `sort`, and `order`. The maximum page size is 100; `sort` accepts
`timestamp` or `id`, and `order` accepts `asc` or `desc`. Filters can be
combined.

Verify a stored proof:

```bash
curl --request POST \
  http://127.0.0.1:3000/proofs/verify \
  --header 'Content-Type: application/json' \
  --data '{"proof_id":"<PROOF_UUID>"}'
```

```json
{"proof_id":"<PROOF_UUID>","valid":true}
```

## Operational guidance

- Bind or firewall the process so only intended local callers can reach it.
- Tune `PROOF_RATE_LIMIT_PER_MINUTE` and `PROOF_REQUEST_BODY_LIMIT` when the
  defaults of 100 requests per minute and 1 MiB are unsuitable.
- Never infer human authority from caller-controlled headers.
- Do not place a reverse proxy in front of this binary and call it the operator
  control plane.
- Keep workspace signing keys and proof databases out of source control.
- Use the [security model](security-model.md) before embedding or deploying a
  transport.
