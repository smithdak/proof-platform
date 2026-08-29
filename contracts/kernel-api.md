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

Located in `crates/proof-kernel/src/executor.rs`.

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
- `AgentRunEvaluation` appends an immutable pass/fail evaluation to a terminal run, with an optional score from 0 through 10,000 basis points and canonical JSON metrics. Deterministic trace policy objects reject unknown fields at the top level and in nested expected-call and final-output-reference objects. Trace evaluations include canonical policy and trace-snapshot digests in their metrics so the evaluated inputs remain auditable, and may require scalar arguments, outputs, or proof IDs to appear in the final model report. Trace snapshots bind principals by durable ID, kind, and public key rather than non-persisted read timestamps.
- Deterministic lifecycle evaluation validates run and step timestamp windows, contiguous step ordinals and attempts, retry parent lineage and call identity, contiguous event sequences and digests, and approval chronology from tool request through request, decision, resume, and execution.
- Evaluation rows are historical assertions, not mutable current-state slots. Consumers distinguish assertions by run, evaluator, policy digest, and trace digest; a later row does not silently supersede an earlier one.
- `AgentRunStore` implementations enforce monotonic revisions for mutable runs and steps, and insert-once semantics for checkpoints and evaluations.
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
| `StorageFailed` | 500 |

Every new variant must be registered in this table AND mapped in `crates/proof-transport-http/src/lib.rs` (`execution_error_response`). Adding a variant without the HTTP mapping is an integration defect.

## ExecutionStore Trait

Implemented by `SqliteStore` (proof-storage) and `RecordingStore` (kernel test helper).

| Method | Signature |
|---|---|
| `save_proof` | `(&self, &Proof) -> Result<(), String>` |
| `save_execution_context` | `(&self, &ExecutionContext) -> Result<String, String>` |
| `load_delegation` | `(&self, &Uuid) -> Result<Option<DelegationGrant>, String>` (default impl: `Ok(None)`) |

Adding a method must provide a default implementation so existing implementors keep compiling.

## SQLite Schema

Migrations live in `crates/proof-storage/src/sqlite/migrations.rs`. Current version: **10**.

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

New migrations append sequentially. Never edit, reorder, or duplicate existing migrations.
