# Kernel API Contract

This is the canonical reference for shared types in `proof-kernel`. All crates must compile against these shapes. Agents must read this file before adding fields, variants, or renaming anything.

## Operation Naming

- Operation identifier: `domain.action` — e.g. `schema.create`, `object.create`, `content.approve`.
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

## ExecutionError Variants & HTTP Mapping

| Variant | HTTP Status |
|---|---|
| `OperationNotFound` | 404 |
| `NoHandler` | 500 |
| `HumanOnly` | 403 |
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

Migrations live in `crates/proof-storage/src/sqlite.rs`. Current version: **3**.

| Version | Contents |
|---|---|
| 1 | proofs, execution_contexts, principals, delegations, registry tables |
| 2 | delegation scope columns |
| 3 | benchmark_results table |

New migrations append sequentially. Never edit, reorder, or duplicate existing migrations.
