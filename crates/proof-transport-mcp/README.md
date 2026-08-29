# Proof MCP Server

`proof-mcp` is the stdio agent transport for Proof Platform. It exposes registry
operations as MCP tools, executes them through the governed kernel, and attaches
signed Proof evidence to each successful tool result.

## Start

Initialize and populate a Proof workspace first:

```bash
proof init
cp -R /path/to/proof-platform/registry/. .proof/registry/
```

Build or install the server:

```bash
cargo install --path crates/proof-transport-mcp --bin proof-mcp
```

Configure an MCP client with an absolute workspace path:

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

The server writes only newline-delimited JSON-RPC messages to stdout. Runtime
diagnostics use stderr.

## Protocol

- MCP `2026-07-28`: stateless discovery through `server/discover`, per-request
  protocol metadata, cacheable tool lists, and typed result metadata.
- MCP `2025-11-25`: legacy `initialize` compatibility for existing clients.
- Stable identity: the server uses the workspace Ed25519 keypair for every call.
- Evidence: successful calls persist proofs under `.proof/data/proofs` and return
  the signed envelope in `com.proofplatform/evidence` result metadata.
- Runs: every tool call persists a run and exact step attempt in
  `.proof/storage/storage.db`, returned in `com.proofplatform/run` metadata.

## Agent runs

Calls without run metadata execute as one-shot runs and terminate automatically.
To compose several tools into one session, start a run with the CLI:

```bash
proof --workspace /absolute/path/to/workspace run start \
  --goal "Prepare and publish a release"
```

Merge the returned `mcpMeta` into the MCP request `_meta`. You can also request
a new session directly with this namespaced metadata fragment:

```json
{
  "com.proofplatform/run": {
    "mode": "session",
    "goal": "Prepare and publish a release"
  }
}
```

Every tool result includes this shape:

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

Inspect, checkpoint, complete, cancel, retry, or evaluate runs with `proof run`.
`proof run retry <RUN_ID> <STEP_ID>` returns MCP metadata containing both the
run ID and the new pending attempt ID. The MCP server verifies that the retry's
operation, version, and canonical input digest match that recorded attempt.

## Human approval

Human-only operations use a resumable, signed approval flow rather than trusting
an MCP client's accept button:

1. The server persists the active run as `waiting_for_input` and its step as
   `waiting_for_approval`, then returns `resultType: "input_required"`, a signed
   approval request, and a `requestState` UUID. The request binds the agent,
   operation, version, canonical input digest, and a 15-minute validity window.
2. Enroll a local human signing identity once:

   ```bash
   proof --workspace /absolute/path/to/workspace approval approver-init
   ```

3. Review pending requests and sign an approve or deny decision:

   ```bash
   proof --workspace /absolute/path/to/workspace approval list
   proof --workspace /absolute/path/to/workspace approval approve <REQUEST_ID> --approver <APPROVER_ID> --reason "Reviewed"
   # or
   proof --workspace /absolute/path/to/workspace approval deny <REQUEST_ID> --approver <APPROVER_ID> --reason "Policy blocked"
   ```

4. Retry the same `tools/call` with the unchanged arguments and returned
   `requestState`. Proof resumes the same run and step, verifies both signatures,
   and dispatches only an exact approved call. Completed requests replay the
   persisted output, proof, run ID, and step ID instead of dispatching again.

Approval requests, decisions, and replay records live in
`.proof/storage/storage.db`. Public approval evidence is returned in
`com.proofplatform/approval`; human private keys remain under
`.proof/approvers/` with owner-only permissions on Unix.
