# CLI reference

The `proof` CLI manages local workspaces, executes governed operations, runs
agents, records human decisions, and inspects evidence. Output is JSON unless a
command explicitly documents otherwise.

Use built-in help as the version-matched source for exact flags:

```bash
proof --help
proof <COMMAND> --help
proof <COMMAND> <SUBCOMMAND> --help
```

## Global syntax

Global options appear before the command:

```bash
proof [--verbose] [--workspace <PATH>] <COMMAND>
proof -w <PATH> <COMMAND>
```

The default workspace is the current directory.

## Command map

| Command | Purpose |
|---|---|
| `init` | Initialize the selected workspace without replacing an identity |
| `schema-create` | Create and prove a local content schema |
| `object-create` | Validate, create, and prove a local content object |
| `changeset-create` | Create a local authoring changeset |
| `changeset-commit` | Commit a changeset through its governed contract |
| `edition-create` | Create an immutable content edition |
| `release-publish` | Disabled legacy shortcut for a human-only operation |
| `status` | Summarize local content and proof counts |
| `capabilities` | List registry-derived operations and governance |
| `registry` | List or inspect registry entries |
| `execute` | Execute one operation and version through the engine |
| `verify` | Verify a persisted proof |
| `benchmark` | Run or summarize benchmark contracts |
| `workspace` | Initialize or inspect another workspace |
| `keypair` | Export public identity data or rotate the workspace key |
| `delegation` | Grant, list, revoke, or validate bounded authority |
| `approval` | Enroll an approver and sign approve/deny decisions |
| `agent` | Define, start, resume, watch, or evaluate an agent |
| `run` | Administer durable run records directly |
| `export` / `import` | Move supported portable workspace data |

## Workspaces and identity

```bash
proof --workspace /absolute/path/to/workspace init
proof --workspace /absolute/path/to/workspace workspace status
proof --workspace /absolute/path/to/workspace keypair export
```

Initialization creates identity material only when no workspace identity
exists. It fails closed on partial or conflicting identity state. Use
`keypair rotate` for an intentional rotation; do not edit `config.json` or
`keypair.json` by hand.

## Registry and execution

```bash
proof -w <PATH> capabilities
proof -w <PATH> registry list
proof -w <PATH> registry inspect object.create
proof -w <PATH> execute object.create v1 --input '<JSON>'
```

Operation names use `domain.action`; versions use `v<N>`. `execute` validates
the registry entry, applies governance and applicable idempotency, and then
dispatches a registered handler. The legacy direct path relies on that handler
for input decoding and domain validation; it does not independently load the
adjacent registry JSON Schema. Entries without a handler return `NoHandler`.

## Proofs and delegation

```bash
proof -w <PATH> verify <PROOF_ID>
proof -w <PATH> delegation grant <AGENT_ID> --scope '<JSON>'
proof -w <PATH> delegation list
proof -w <PATH> delegation validate <DELEGATION_ID>
proof -w <PATH> delegation revoke <DELEGATION_ID>
```

Delegation scopes are authority contracts, not filters applied after
execution. Validate the chain before relying on a grant, and treat revocation
as a state transition that later execution must recheck.

## Agent definitions and runs

Create an agent with an explicit tool allowlist and budgets:

```bash
proof -w <PATH> agent create \
  --name <NAME> \
  --instructions '<INSTRUCTIONS>' \
  --provider openai \
  --model <MODEL> \
  --tool object.create::v1 \
  --max-steps 8 \
  --max-model-calls 12 \
  --max-total-tokens 50000 \
  --max-duration-seconds 600
```

Start, observe, and resume the agent-owned run:

```bash
proof -w <PATH> agent start <AGENT_ID> --goal '<GOAL>'
proof -w <PATH> agent watch <RUN_ID>
proof -w <PATH> agent resume <RUN_ID>
```

Use the lower-level `run` family when a transport or operator needs to manage
run records directly:

```bash
proof -w <PATH> run start --goal '<GOAL>'
proof -w <PATH> run list
proof -w <PATH> run inspect <RUN_ID>
proof -w <PATH> run checkpoint <RUN_ID> --state '<JSON>'
proof -w <PATH> run retry <RUN_ID> <FAILED_STEP_ID>
proof -w <PATH> run complete <RUN_ID>
proof -w <PATH> run cancel <RUN_ID>
proof -w <PATH> run evaluate <RUN_ID> \
  --evaluator <EVALUATOR_ID> \
  --outcome passed \
  --score-bps 9500 \
  --metrics '{"proof_valid":true}'
```

Agent and run commands are intentionally distinct: `agent` drives the native
model/tool runtime, while `run` administers persisted lifecycle records used by
transports and operators. Likewise, `run evaluate` records a caller-supplied
outcome and metrics, while `agent evaluate` below executes a deterministic
trace policy.

## Human approval

```bash
proof -w <PATH> approval approver-init
proof -w <PATH> approval list
proof -w <PATH> approval approve <REQUEST_ID> \
  --approver <APPROVER_ID> \
  --reason '<REASON>'
proof -w <PATH> approval deny <REQUEST_ID> \
  --approver <APPROVER_ID> \
  --reason '<REASON>'
```

Approval records a signed decision only. Resume the same agent run explicitly
after an approval. A denial grants no authority.

The `approval ui` subcommand exists as an unreleased candidate from AXP-E0006,
whose Gate C was deferred. It is not a supported security path.

## Evaluation

Evaluate a sealed agent trace against a deterministic policy:

```bash
proof -w <PATH> agent evaluate <RUN_ID> \
  --evaluator <EVALUATOR_ID> \
  --policy-file <POLICY.json>
```

Evaluation policies reject unknown fields, verify lifecycle and evidence
bindings, and persist both passing and failing results. An evaluation does not
rewrite or replace prior evidence.

For a complete first-run sequence, use the [getting-started guide](getting-started.md).
