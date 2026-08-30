# E0000 Decisions

## D-E0000-001 — Edition-first swarm records

Status: adopted for scaffolding · Date: 2026-08-29 · Decision owner: product owner

Track charter, graph, ownership, status, decisions, evidence, and retrospective
under `editions/`. These records are the durable control plane for low-cost
agent assignments and owner oversight.

## D-E0000-002 — Tracked `.proof` material is a P0

Status: Gate B approved · Date: 2026-08-29 · Decision owner: product owner

Treat tracked key/config/database material as potentially exposed. Preserve the
local files, remove the three known runtime paths from Git tracking, harden
private directories to `0700` and files to `0600`, and document rotation impact
without reading or displaying private contents. Do not rotate the workspace
identity, rewrite history, delete files, or notify external parties in this
action. Those remain separate owner decisions after impact review.

Execution result: completed within the approved boundary. The files remain
local and ignored, and current Git tracking was removed. Historical copies
remain in two commits, so the key is still classified as compromised. Rotation
is deferred until identity continuity and old-proof verification are designed.

## D-E0000-003 — One writer per path

Status: adopted · Date: 2026-08-29 · Decision owner: orchestrator

Root files, contracts, migrations, transport integration, and edition records
have one named writer. Workers submit cross-scope requests.

## D-E0000-004 — Gate A approval

Status: approved · Date: 2026-08-29 · Decision owner: product owner

The product owner approved the E0000 charter, success policy, delivery budget,
non-goals, and conflict-safe wave plan. This authorizes bounded edition work
within the frozen charter. D-E0000-002 separately records the subsequently
approved security boundary.

## D-E0000-005 — Content v1 contract freeze

Status: Gate B approved · Date: 2026-08-29 · Decision owner: product owner

The E0000-05 read-only mismatch matrix found three compatibility-sensitive
choices. The contract steward recommends:

1. Add a UUIDv7 `idempotency_key` to `edition.create::v1` and
   `changeset.commit::v1`. Bind it to canonical input: identical key and input
   replays the original persisted output/proof; the same key with different
   input fails. Standardize `{operation,data}` outputs containing the actual
   immutable `Edition` or committed `ChangeSet`, with commit `objects_count`.
2. Keep the frozen eight governed operations. Retain `changeset.create` only as
   a local authoring helper and stop it from minting a proof for an unregistered
   ninth operation. Do not expand the v1 registry in E0000.
3. Preserve existing SHA-256 content snapshot identifiers for v1 and clarify
   that architecture D7's BLAKE3 requirement governs kernel evidence digests.
   A content-digest migration requires an explicitly versioned later edition.

The product owner approved all three recommendations. Canonical contract and
architecture edits may proceed within this exact boundary. Detailed evidence
is in `handoffs/E0000-05.md`.

## D-E0000-006 — Shared exact-replay ledger and recovery

Status: Gate B approved · Date: 2026-08-29 · Decision owner: product owner

E0000-13 determined that exact original output/proof replay cannot live in a
content handler because the kernel creates evidence after handler mutation. It
recommends this complete boundary:

1. Add default-compatible public kernel replay policy, key, claim, claim-result,
   and idempotency error types plus three object-safe `ExecutionStore` methods.
   Only `edition.create::v1` and `changeset.commit::v1` opt in during E0000.
2. Append migration 11, `create exact execution replay ledger`, with the exact
   `execution_replays` tuple key, claim token, state constraints, immutable
   canonical output/original proof, and atomic context/proof/completion behavior
   specified in `handoffs/E0000-13.md`. Historical executions are not backfilled.
3. Enforce fail-closed recovery: `claimed` and `failed` tuples never expire,
   steal, delete, or automatically re-execute. They require reconciliation and
   then a new UUIDv7 key. Operational rollback quiesces writers and preserves
   the ledger before reverting application policy or schema.
4. Map invalid keys to HTTP 400, conflicts/in-progress/indeterminate to 409,
   missing durable storage to 503, and corruption/storage failures to 500;
   provide equivalent exhaustive WebSocket classes.

Approval authorizes only the three disjoint W4 packets: kernel API/engine
E0000-14, SQLite migration/storage E0000-16, and HTTP/WebSocket error mapping
E0000-15. It does not authorize automatic recovery, registry-wide rollout,
backfill, PostgreSQL, content/operation edits, or transaction redesign.

The product owner approved this complete boundary. W4 may fan out against the
frozen E0000-13 interface and must stop on any deviation.

## D-E0000-007 — Gate C edition acceptance

Status: Gate C approved · Date: 2026-08-29 · Decision owner: product owner

The product owner approved AXP-E0000 after the final quiescent workspace gate
passed 405 tests across 46 suites and every work item had linked evidence. This
accepts the edition as the process-hardening and governed walking-skeleton
baseline defined by its charter.

Approval does not reclassify the historical workspace identity as safe. The
three local `.proof` runtime files remain present, ignored, untracked, and
permission-hardened, while copies in prior commits mean the key remains
compromised. Production use of that identity, key rotation, identity
succession, old-proof migration, and history rewriting remain prohibited until
a separate security-gated decision defines them.

The owner also accepts the written E0000 dispositions for the split transport
database paths, fail-closed gap between filesystem mutation and replay
completion, process-generated HTTP/WebSocket identities, and fixture-only
Release Manager evaluation. Acceptance does not waive these risks or authorize
E0001 implementation; the next edition requires its own charter and Gate A.
Repository changes remain uncommitted under the project Git policy.
