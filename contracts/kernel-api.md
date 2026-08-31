# Kernel API Contract

This is the canonical reference for shared types in `proof-kernel`. All crates must compile against these shapes. Agents must read this file before adding fields, variants, or renaming anything.

## Operation Naming

- Operation identifier: dot-delimited with at least two non-empty segments — e.g. `schema.create`, `content.approve`, `analytics.snapshot.create`.
- Version string: `v<N>` — e.g. `v1`, `v2`.
- Storage composite key: `operation::version` — e.g. `schema.create::v1`. The `version` column in SQLite stores the suffix after `::`.

## RegistryEntry

Located in `crates/proof-kernel/src/registry.rs`.

| Field | Type | Notes |
|---|---|---|
| `operation` | `String` | `domain.action` format |
| `domain` | `String` | e.g. `content`, `test` |
| `version` | `String` | `v<N>` format |
| `action` | `String` | e.g. `content:schema_create` |
| `description` | `String` | |
| `input_schema` | `String` | JSON schema (inline or path) |
| `output_schema` | `String` | JSON schema (inline or path) |
| `required_authority` | `String` | e.g. `delegation-grant` |
| `governance` | `Governance` | `AgentExecutable` or `HumanOnly` |
| `idempotency` | `String` | e.g. `required-uuidv7` |
| `consequence` | `String` | e.g. `content-mutation` |
| `evidence_contract` | `String` | e.g. `operation-effect-v1` |
| `benchmark` | `Option<String>` | optional benchmark ID, e.g. `B1` |
| `status` | `VersionStatus` | `Active` / `Deprecated` / `Sunset` |
| `deprecated_since` | `Option<NaiveDate>` | |
| `replacement_operation` | `Option<String>` | pointer to newer version |

Optional fields use `#[serde(default)]`. Adding new fields must follow this pattern and must be reported to the orchestrator (downstream constructors exist in storage, HTTP, and MCP test helpers).

## ExecutionContext

Located in `crates/proof-kernel/src/executor/context.rs`.

| Field | Type | Notes |
|---|---|---|
| `actor` | `PrincipalId` | |
| `principal_kind` | `Option<PrincipalKind>` | `None` treated as agent; `Some(Human)` allows human-only operations |
| `delegation_id` | `Option<Uuid>` | |
| `delegation_chain` | `Option<DelegationChain>` | |
| `workspace_path` | `PathBuf` | |
| `timestamp` | `DateTime<Utc>` | |

Adding a field here breaks every transport's test context constructors — always report to the orchestrator.

## Proof / ProofBody

Located in `crates/proof-kernel/src/evidence.rs`.

| Field | Type | Notes |
|---|---|---|
| `id` | `Uuid` | UUIDv7 |
| `actor` | `PrincipalId` | |
| `delegation_id` | `Option<Uuid>` | skipped when None |
| `operation` | `String` | stored as `operation::version` composite |
| `input_digest` | `ContentDigest` | BLAKE3 of canonical input |
| `output_digest` | `ContentDigest` | BLAKE3 of canonical output |
| `timestamp` | `DateTime<Utc>` | |
| `expires_at` | `Option<DateTime<Utc>>` | optional expiration |

Proofs are signed with Ed25519 (`ed25519-dalek`). The signature covers the canonical JSON of `ProofBody`.
Version-bound consumers reject legacy proofs whose signed `operation` contains only `domain.action`; those proofs cannot be migrated to the composite form without re-execution and a new signature.

## Principal Persistence

SQLite treats a principal's ID, kind, and public key as its immutable durable
identity. Re-saving the same tuple is idempotent; reusing an ID with a different
kind or public key is a conflict.

## Approval Contracts

Located in `crates/proof-kernel/src/approval.rs`.

- `SignedApprovalRequest` binds the requesting agent, operation, version, canonical input digest, and validity window.
- `SignedApprovalRequest::verify_for_call` rejects retries that change the actor, operation, version, input, or validity window.
- `SignedApprovalDecision` binds an enrolled human approver's approve/deny decision to the signed request digest.
- `ApprovalGrant` bundles the request, decision, and approver principal for independent verification.
- `ExecutionEngine::execute_with_approval` only bypasses a `human-only` gate when both signatures, the trusted approver, actor, operation, version, input digest, and validity window match.
- `ExecutionEngine::execute_evidenced` and `execute_with_approval_evidenced` return `ExecutionOutcome { output, proof }` so runtimes do not reconstruct transport-local evidence.
- `ApprovalExecution.executed_at` equals its signed proof timestamp; recovery and evaluation reject mismatches.
- Approval decisions are single-use at the transport layer: a completed approval request replays its persisted `ApprovalExecution` instead of executing again.

## Exact Execution Replay

Replay types and policy are defined in `crates/proof-kernel/src/executor/store.rs`,
and re-exported from `crates/proof-kernel/src/executor/mod.rs` and
`crates/proof-kernel/src/lib.rs`:

```rust
pub enum IdempotencyPolicy {
    None,
    RequiredUuidV7ExactReplay,
}

pub struct ExecutionReplayKey {
    pub operation: String,
    pub version: String,
    pub idempotency_key: Uuid,
}

pub struct ExecutionReplayClaim {
    pub key: ExecutionReplayKey,
    pub input_digest: ContentDigest,
    pub claim_token: Uuid,
    pub claimed_by: PrincipalId,
    pub claimed_at: DateTime<Utc>,
}

pub enum ExecutionReplayClaimResult {
    Acquired,
    Completed(ExecutionOutcome),
    Conflict,
    InProgress,
    Failed,
    Unsupported,
}
```

`OperationHandler::idempotency_policy` defaults to `None`; only
`edition.create::v1` and `changeset.commit::v1` opt in during E0000. The
default preserves compatibility for existing handlers. `ExecutionStore` adds
these object-safe, default-compatible methods (implemented by `SqliteStore`
and `RecordingStore`):

```rust
fn claim_execution_replay(
    &self,
    claim: &ExecutionReplayClaim,
) -> Result<ExecutionReplayClaimResult, String>;
fn complete_execution_replay(
    &self,
    claim: &ExecutionReplayClaim,
    context: &ExecutionContext,
    outcome: &ExecutionOutcome,
) -> Result<(), String>;
fn fail_execution_replay(
    &self,
    claim: &ExecutionReplayClaim,
    failed_at: DateTime<Utc>,
    failure: &str,
) -> Result<(), String>;
```

The methods are implemented in `crates/proof-storage/src/sqlite/replay.rs`
and delegated by `crates/proof-storage/src/sqlite/store.rs`.

`ExecutionEngine::execute_operation` in
`crates/proof-kernel/src/executor/engine.rs` is the shared path for
`execute`, `execute_evidenced`, `execute_with_approval`, and
`execute_with_approval_evidenced`. For an opted-in handler it performs
authorization/governance/lifecycle/delegation checks first; requires a
top-level string `idempotency_key` that is UUIDv7; canonicalizes the complete
input and computes the domain-separated `ArtifactKind::OperationInput`
digest; requires durable compatible storage; and claims the tuple before the
benchmark gate and handler entry. Canonical JSON equivalence, not caller byte
order, defines exact input equality.

`Acquired` is the sole path allowed to benchmark, mutate, and create proof.
`Completed` returns the stored canonical output and the original proof ID,
body, and signature after structural operation/input/output-digest checks;
the handler is not invoked and no evidence is minted. `Conflict` is a
different input for the tuple, `InProgress` is a retryable busy conflict, and
`Failed` is an indeterminate prior attempt. `Unsupported` and absent durable
storage fail before handler entry. Benchmark, handler, and evidence failures
never produce a completed replay and acquired claims are best-effort marked
`failed`. A completion failure leaves the claim `claimed` and the tuple
blocked; it is not transitioned to `failed`.

`complete_execution_replay` atomically writes the execution context, proof,
canonical output, serialized original proof, and completed ledger state. It
validates the claimant/token, tuple, input digest, proof operation, actor,
delegation/timestamp, and output digest. Equal completion retries are
idempotent; a different completion is a storage conflict. Domain mutation is
outside this transaction because the public handler boundary does not expose
a shared transaction.

Claims in `claimed` or `failed` state never expire, lease-steal, delete, or
automatically re-execute. A crash can leave a claimed row after mutation;
operators must reconcile the domain and then use a new UUIDv7 key. This is
the fail-closed recovery policy approved by D-E0000-006.

## Agent Run Contracts

Located in `crates/proof-kernel/src/agent_run.rs`. Agent runs are the platform-level execution ledger for agent intent and tool attempts; they are distinct from workflow-domain records in `proof-workflow`.

### `AgentRun`

| Field | Type | Notes |
|---|---|---|
| `id` | `Uuid` | UUIDv7 run identity |
| `actor` | `PrincipalId` | Workspace agent that owns the run |
| `agent_id` | `Option<Uuid>` | Immutable agent definition used by native runtime runs; absent for transport-managed legacy runs |
| `mode` | `AgentRunMode` | `OneShot` or `Session` |
| `goal` | `String` | Human-readable intent |
| `status` | `AgentRunStatus` | `Queued`, `Running`, `WaitingForInput`, `Succeeded`, `Failed`, or `Cancelled` |
| `retry_count` | `u32` | Incremented when a failed run resumes |
| `revision` | `u64` | Optimistic concurrency revision |
| `created_at` / `updated_at` | `DateTime<Utc>` | Lifecycle timestamps |
| `completed_at` | `Option<DateTime<Utc>>` | Set only for terminal states |

One-shot runs terminate with their tool call. Session runs remain `Running` after a successful step until explicitly completed or cancelled. Human approval moves a run from `Running` to `WaitingForInput`, then resumes the same run after a decision.

### `AgentRunStep`

Each step records an operation/version, canonical input digest, ordinal, attempt number, optional retry lineage, lifecycle status, optional approval request, and either a successful output/proof or an error. A successful transition verifies that the proof operation, input digest, and output digest match the recorded attempt. A retry creates a new immutable attempt with the same ordinal and input digest.

### Checkpoints and evaluations

- `AgentCheckpoint` appends immutable, sequence-ordered state with an `AgentCheckpoint` canonical digest.
- `AgentCheckpointTail` is the compact checkpoint ID, sequence, and state-digest identity used for compare-and-append. `AgentRunStore::append_agent_checkpoint(expected_tail, checkpoint)` returns `Appended`, `Stale`, or the backward-compatible default `Unsupported`. A well-formed displaced writer returns `Stale` without a write; malformed candidates, corrupt stored current/predecessor evidence, identity conflicts, and noncontiguous sequences return an error. Exact retry is idempotently `Appended` only while the candidate remains current and its exact predecessor still matches. Recording storage performs the decision under its checkpoint mutex; SQLite uses `BEGIN IMMEDIATE` and exact-validates indexed columns, immutable JSON, canonical digests, and the retry predecessor before inserting or accepting a retry. The legacy insert-once save remains available to existing callers.
- `AgentRunEvaluation` appends an immutable pass/fail evaluation to a terminal run, with an optional score from 0 through 10,000 basis points and canonical JSON metrics. Deterministic trace policy objects reject unknown fields at the top level and in nested expected-call and final-output-reference objects. Trace evaluations include canonical policy and trace-snapshot digests in their metrics so the evaluated inputs remain auditable, and may require scalar arguments, outputs, or proof IDs to appear in the final model report. Trace snapshots bind principals by durable ID, kind, and public key rather than non-persisted read timestamps.
- Deterministic lifecycle evaluation validates run and step timestamp windows, contiguous step ordinals and attempts, retry parent lineage and call identity, contiguous event sequences and digests, and approval chronology from tool request through request, decision, resume, and execution.
- Evaluation rows are historical assertions, not mutable current-state slots. Consumers distinguish assertions by run, evaluator, policy digest, and trace digest; a later row does not silently supersede an earlier one.
- `AgentRunStore` implementations enforce monotonic revisions for mutable runs and steps, and insert-once semantics for checkpoints and evaluations.
- `LiveRunStartClaim` is the strict `proof-live-run-start-claim/v1` identity for one live-start authorization. `readiness_binding_digest` is its primary replay key and `setup_digest` is independently unique. `AgentRunStore::claim_live_run_start` atomically persists the claim plus the exact initial Running session run, sequence-zero `agent_runtime_v2` checkpoint, and sequence-zero `Started` event. It returns `Acquired`, `Existing(original_run_id)`, `Conflict`, or the default-compatible `Unsupported`; an exact replay verifies the original immutable bundle before returning its run ID, while either cross-paired digest conflicts before provider construction.
- In SQLite, a matching terminal event seals the run trace: later non-idempotent run, step, checkpoint, or event writes fail, and event sequences must be contiguous. The seal also covers approval request, decision, and execution evidence bound to a sealed step: exact existing evidence may be retried idempotently, but missing or conflicting evidence cannot be inserted after the seal. Cancelled runs seal immediately because the event enum has no cancellation variant.
- MCP results expose the active run and step in `_meta["com.proofplatform/run"]`; callers resume a session or retry attempt with its `runId` and optional `stepId`.

## Native Agent Contracts

Located in `crates/proof-kernel/src/agent.rs` and implemented by
`crates/proof-agent-runtime`.

- `AgentDefinition` is immutable and contains a UUIDv7 ID, unique name, instructions, provider, model, explicit `AgentTool` allowlist, `AgentLimits`, and creation time.
- `AgentTool` stores an operation and `v<N>` version. Its storage/CLI key is `operation::version`.
- `AgentLimits` bounds steps, model calls, total tokens, wall-clock duration, output tokens per call, and optionally cost in micro-USD.
- `AgentRunEvent` is immutable, sequence ordered, canonically digested, and records model requests/responses, tool attempts/results, approvals, budget failures, and terminal outcomes.
- `AgentStore` persists immutable definitions and append-only events. Agent names and per-run event sequences are unique.
- Native runtime checkpoints use `kind: "agent_runtime_v1"` and preserve the provider response cursor, next model input, pending tool/approval, accumulated usage, and terminal result.
- The frozen E0001 `agent_runtime_v2` live path has one active execution writer per private workspace. A crash-released advisory workspace-directory lease is acquired before a resume's first durable read and, for a newly acquired start claim, before every post-claim read and provider boundary; it is held through the complete returned outcome. Resume contention returns `LiveRunBusy` with zero store writes, gateway construction, provider sends, or governed effects. Because a new start claims atomically before lease acquisition, start contention may retain exactly that immutable claim plus its initial run, checkpoint zero, and Started event zero; it still performs no post-claim write, gateway construction, provider send, or governed effect. Exact `Existing` start replay remains deliberately read-only and does not acquire the lease.
- Every post-claim live-v2 checkpoint save validates the complete prospective history and uses expected-tail compare-and-append. A resume epoch transition additionally binds the exact previously loaded state; ordinary saves require the persisted tail to retain the caller's process epoch. `Stale` and `Unsupported` fail closed before immutable insertion or a provider boundary. The lease prevents an active writer's authorized provider response from being orphaned by takeover, while compare-and-append independently prevents a stale or lock-bypassing writer from poisoning checkpoint history.
- Human-only calls transition the run to `WaitingForInput`; `resume` verifies and executes or reconciles the same approval request and step. Request expiration is the earlier of the configured approval TTL and the run's wall-clock deadline. Resume enforces the duration budget before approval validation, reconciliation, or execution, so approval after that deadline records a budget-exceeded terminal outcome without tool execution.
- Resuming a run already sealed by a terminal `Failed` or `BudgetExceeded` event returns the persisted terminal outcome read-only; it does not append another checkpoint or event.

## ExecutionError Variants & HTTP Mapping

| Variant | HTTP Status |
|---|---|
| `OperationNotFound` | 404 |
| `NoHandler` | 500 |
| `HumanOnly` | 403 |
| `Approval` | 403 |
| `Sunset` | 410 |
| `Delegation` | 403 |
| `ScopeViolation` | 403 |
| `HandlerFailed` | 500 |
| `EvidenceFailed` | 500 |
| `BenchmarkExpired` | 409 |
| `Idempotency(MissingKey)` | 400 |
| `Idempotency(InvalidUuidV7)` | 400 |
| `Idempotency(Conflict)` | 409 |
| `Idempotency(InProgress)` | 409 |
| `Idempotency(Indeterminate)` | 409 |
| `Idempotency(StorageRequired)` | 503 |
| `StorageFailed` | 500 |

`IdempotencyError` and the nested `ExecutionError::Idempotency(#[from]
IdempotencyError)` are defined in `crates/proof-kernel/src/executor/error.rs`.
The HTTP mapping is exhaustive in
`crates/proof-transport-http/src/handlers/errors.rs` (`execution_error_response`):
invalid input is 400, tuple conflict/in-progress/indeterminate is 409, and
durable storage required is 503. Corruption, serialization, transaction, and
other storage failures remain `StorageFailed`/500. WebSocket mapping is
exhaustive in `crates/proof-transport-ws/src/lib.rs`: invalid input is
`-32602`, conflict/in-progress/indeterminate is `-32006`, and storage/internal
classes (including storage required) are `-32005`.

## ExecutionStore Trait

Implemented by `SqliteStore` (proof-storage) and `RecordingStore` (kernel test helper).

| Method | Signature |
|---|---|
| `save_proof` | `(&self, &Proof) -> Result<(), String>` |
| `save_execution_context` | `(&self, &ExecutionContext) -> Result<String, String>` |
| `load_delegation` | `(&self, &Uuid) -> Result<Option<Delegation>, String>` (default impl: `Ok(None)`) |
| `claim_execution_replay` | `(&self, &ExecutionReplayClaim) -> Result<ExecutionReplayClaimResult, String>` (default: `Unsupported`) |
| `complete_execution_replay` | `(&self, &ExecutionReplayClaim, &ExecutionContext, &ExecutionOutcome) -> Result<(), String>` (default: unsupported-store error) |
| `fail_execution_replay` | `(&self, &ExecutionReplayClaim, DateTime<Utc>, &str) -> Result<(), String>` (default: unsupported-store error) |

Adding a method must provide a default implementation so existing implementors keep compiling.

### Version-aware handler methods (active; added by AXP-E0001)

`ExecutionEngine` selects handlers by operation name, so version-specific
behavior uses these default-compatible `OperationHandler` hooks:

```rust
fn idempotency_policy_for(&self, version: &str) -> IdempotencyPolicy {
    self.idempotency_policy()
}

fn execute_versioned(
    &self,
    version: &str,
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    self.execute(input, context)
}
```

The engine calls these methods only after normal registry, authority,
governance, lifecycle, and delegation checks. Existing implementations retain
their exact behavior through the defaults. `release.publish` preserves v1's
`None` policy while v2 selects `RequiredUuidV7ExactReplay`.

The E0001 live entry path additionally requires
`ExecutionContext.delegation_id=Some(id)` and a matching chain. It loads that
ID through the execution store and rejects chain-only authority, a missing
row, default/`None`/wildcard/unbounded scope, or any scope other than exactly
`allowed_operations=["release.publish"]` and
`allowed_domains=["content"]` before provider credential access. This is a
journey-specific strictness rule; it does not narrow legacy engine callers.

This addition did not change a shared struct, error, proof, or approval.
AXP-E0001 Gate B approved it, E0001-06 implemented it, and the compatibility
tests require legacy handlers to retain the default operation-wide behavior.

## SQLite Schema

Migrations live in `crates/proof-storage/src/sqlite/migrations.rs`. Current
active version: **13**.

| Version | Contents |
|---|---|
| 1 | proofs, execution_contexts, principals, delegations, registry tables |
| 2 | benchmark results |
| 3 | proof expiration |
| 4 | commerce catalogs, products, orders, and order lines |
| 5 | workflow definitions, runs, and steps |
| 6 | analytics snapshots, queries, and insights |
| 7 | signed approval requests, decisions, and execution replay |
| 8 | agent runs, attempts, checkpoints, and evaluations |
| 9 | immutable agent definitions, append-only run events, and optional run-to-agent linkage |
| 10 | unique single-use approval-request bindings for agent run steps |
| 11 | exact execution replay ledger (`execution_replays`) |
| 12 | strict structured delegation scope persistence (`delegations.scope_json`) |
| 13 | atomic live-run start claims and initial run/checkpoint/event barrier |

### SQLite v13 live-start claim

Migration v13 appends `live_run_start_claims`. Its primary key is the
64-character `readiness_binding_digest`; `setup_digest`, `run_id`, initial
checkpoint ID, and Started-event ID are independently unique. The row retains
the strict claim JSON and immutable initial Running-run JSON, while foreign
keys bind the sequence-zero checkpoint and event.

`claim_live_run_start` uses one `BEGIN IMMEDIATE` transaction. A first claim
inserts the Running revision-1 session run, exact sequence-zero
`agent_runtime_v2` checkpoint, exact sequence-zero `Started` event, and claim
row, or none of them. An exact digest pair verifies the stored claim, initial
bundle, and current run identity before returning `Existing(original_run_id)`.
The same readiness digest with another setup digest, or the same setup digest
with another readiness digest, returns `Conflict` without writing. The down
migration removes only the claim table; operators must quiesce live-start
writers before rollback.

### SQLite v12 delegation scope (added by AXP-E0001)

Migration v12 is appended after versions 1-11 and adds
`scope_json TEXT NOT NULL DEFAULT '{}'` to `delegations`. The default preserves
legacy rows, but `{}` is unbounded and MUST fail the E0001 live journey.
New/updated E0001 grants persist and strictly decode the complete
`Delegation.scope`, including exact operation and domain lists.

`SqliteStore` overrides `ExecutionStore::load_delegation` and reconstructs
all legacy delegation fields plus `scope_json`. Storage first decodes through a
storage-local `#[serde(deny_unknown_fields)]` DTO with only optional
`allowed_operations`, `allowed_domains`, and `resource_scope`; malformed JSON,
unknown keys, or wrong value types are storage errors, not defaults. This MUST
NOT change the shared kernel `DelegationScope` deserializer globally. `{}` is
valid only as a legacy row. The E0001 grant requires singleton operation/domain
lists and no structured `resource_scope` key.

CLI grant/save/load round-trips this field. The v12 down path removes only
this column, preserves all legacy rows/columns, and restores version 11 while
writers are quiescent. Required tests cover v11-to-v12 upgrade, legacy `{}`
default, exact scope round trip, known optional resource scope, malformed and
unknown-key scope, E0001 resource-scope rejection, missing/revoked/expired
grants, operation/domain mismatch, and loaded-grant engine enforcement.

Migration 11 is appended (never edits an earlier migration), with description
`create exact execution replay ledger`. Its `execution_replays` table is keyed
by `(operation, version, idempotency_key)` and contains `input_digest`, state
(`claimed`, `completed`, or `failed`), unique `claim_token`, claimant/time,
completion/failure times and message, canonical `output_json`, unique
`proof_id` plus duplicated immutable `proof_json`, and unique
`execution_context_id` references. SQLite checks the 64-character digest,
valid state, and mutually exclusive nullable columns for each state; an index
on `(state, claimed_at)` supports inspection. UUIDv7 shape and digest
correctness remain application validations. The completion transaction
inserts context and proof and transitions the row atomically. The v10 upgrade
creates an empty ledger; no historical executions are backfilled. The down
migration drops only the replay index and table. Operational rollback requires
quiescing writers, preserving/exporting the ledger, and reverting opted-in
policy before returning to v10.

New migrations append sequentially. Never edit, reorder, or duplicate existing migrations.
