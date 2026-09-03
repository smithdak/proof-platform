# Proof MCP server

`proof-mcp` exposes Proof registry operations as Model Context Protocol tools.
It runs over stdio, routes calls through the governed execution engine, and
returns signed proof evidence plus durable run metadata.

## What you get

- Registry-derived tool names, descriptions, and input/output schemas.
- Governance and consequence annotations derived from the same registry entry.
- Strict input and output validation around the registered operation handler.
- One-shot runs by default and explicit multi-call sessions when requested.
- Signed approval requests for human-only tools.
- Persisted result replay for an exact completed approval or retry.

## Quickstart

Create and populate a workspace from the repository root:

```bash
PROOF_DEMO="$(mktemp -d)"
export PROOF_DEMO
proof --workspace "$PROOF_DEMO" init
cp -R registry/. "$PROOF_DEMO/.proof/registry/"
```

Install the server:

```bash
cargo install --path crates/proof-transport-mcp --bin proof-mcp
```

Configure your MCP client with an absolute workspace path:

```json
{
  "mcpServers": {
    "proof": {
      "command": "proof-mcp",
      "args": ["--workspace", "/absolute/path/to/workspace"]
    }
  }
}
```

The server reserves stdout for newline-delimited JSON-RPC. Diagnostics are
written to stderr so logging cannot corrupt the protocol stream.

## Protocol compatibility

| Protocol | Support |
|---|---|
| MCP `2026-07-28` | Stateless `server/discover`, per-request protocol metadata, cacheable tool lists, and typed result metadata |
| MCP `2025-11-25` | Legacy `initialize` compatibility |

Tool names are deterministic:

```text
proof_<domain>_<version>_<operation.with.dots.as.underscores>
```

Discovery is not execution authority. Every tool call still passes through
registry governance, schema validation, delegation checks when present, and a
registered handler.

## Call lifecycle

```text
tools/list
   │
   └─ registry-derived tools and annotations

tools/call
   │
   ├─ validate request and run metadata
   ├─ create or load durable run + step
   ├─ execute governed operation
   ├─ persist result and signed proof
   └─ return structured content + namespaced metadata
```

Calls without run metadata become one-shot runs and terminate automatically.
Calls attached to a session preserve one goal and ordered step history across
multiple tools.

## Result metadata

Every tool result identifies its durable run and exact step. For example, a
successful step in a still-active multi-call session has this shape:

```json
{
  "com.proofplatform/run": {
    "runId": "019...",
    "stepId": "019...",
    "run": { "status": "running" },
    "step": { "status": "succeeded", "attempt": 1 },
    "replay": false
  }
}
```

Successful governed calls also return signed evidence in
`com.proofplatform/evidence`. Human-approval calls use
`com.proofplatform/approval`. Treat these namespaced objects as protocol data;
do not infer authority from a client's local UI state.

## Multi-call sessions

Start a durable session with the CLI:

```bash
proof --workspace /absolute/path/to/workspace run start \
  --goal 'Prepare and publish a release'
```

Merge the returned `mcpMeta` into subsequent request `_meta`, or request a new
session directly:

```json
{
  "com.proofplatform/run": {
    "mode": "session",
    "goal": "Prepare and publish a release"
  }
}
```

Inspect or administer the run with `proof run list`, `run inspect`,
`run checkpoint`, `run retry`, `run complete`, `run cancel`, and
`run evaluate`.

`proof run retry <RUN_ID> <STEP_ID>` returns metadata for a new linked attempt.
The MCP server verifies that the operation, version, and canonical input digest
match that attempt before execution.

## Human approval

Human-only operations use a signed, resumable workflow:

1. The server persists the run as `waiting_for_input` and the step as
   `waiting_for_approval`.
2. The result returns `resultType: "input_required"`, a signed approval request,
   and a `requestState` UUID.
3. An enrolled human reviews and signs an approve or deny decision.
4. The client repeats the same `tools/call` with unchanged arguments and the
   returned `requestState`.
5. Proof verifies both signatures and either dispatches the exact approved call
   or reports the denial. An already completed request replays its persisted
   result, proof, run ID, and step ID.

Enroll and use a local human identity:

```bash
proof --workspace /absolute/path/to/workspace approval approver-init
proof --workspace /absolute/path/to/workspace approval list

proof --workspace /absolute/path/to/workspace approval approve <REQUEST_ID> \
  --approver <APPROVER_ID> \
  --reason 'Reviewed exact operation and arguments'

# Or deny without granting execution authority.
proof --workspace /absolute/path/to/workspace approval deny <REQUEST_ID> \
  --approver <APPROVER_ID> \
  --reason 'Policy blocked'
```

An approval decision does not execute a tool automatically. The exact call
must be resumed.

## Persistence and identity

The MCP server uses the selected workspace's Ed25519 identity for stable calls.
Run, approval, replay, principal, and proof ledger records are stored in
`.proof/storage/storage.db`; each successful proof envelope is also written to
`.proof/data/proofs/<PROOF_ID>.json`. Human private keys remain under
`.proof/approvers/` with owner-only permissions on supported Unix systems.

Never commit `.proof/` or pass private key material through MCP arguments,
result metadata, or logs.

## Troubleshooting

| Symptom | Check |
|---|---|
| No Proof tools appear | Confirm the absolute workspace path and copy `registry/.` into `.proof/registry/` |
| A listed tool returns `NoHandler` | The embedding server has no handler registered for that logical operation |
| JSON-RPC parsing fails | Ensure wrappers and diagnostics write to stderr, never stdout |
| A human-only call remains waiting | Sign the exact request, then retry the same call with unchanged arguments and `requestState` |
| A retry is rejected | Confirm the operation, version, input digest, run ID, and pending attempt all match |

See [Getting started](../../docs/getting-started.md),
[Core concepts](../../docs/concepts.md), and the
[Security model](../../docs/security-model.md) for platform-wide context.
