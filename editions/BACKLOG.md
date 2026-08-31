# AXP Edition Backlog

This is the ranked, assignable backlog for the Agent Experience Platform (AXP).
An edition is a product outcome with a fixed acceptance policy, not a loose
collection of code tasks. Only one edition has active writers at a time. Each
edition is executed by one orchestrator plus at most three concurrent workers;
the orchestrator owns integration, root files, contracts, and release gates.

## Assignment contract

Every assignment must name one owner, exclusive paths, dependencies, a bounded
budget, and an evidence-producing acceptance check. Workers do not commit,
push, or edit another owner's paths. Shared/root paths, migrations, public
contracts, security, and integration are orchestrator-owned unless explicitly
assigned to one worker for the whole wave. Recommended tiers are
`gpt-5.6-luna` for bounded read-only or mechanical work, `gpt-5.6-terra` for
normal implementation, and `gpt-5.6-sol` only for contracts, security, or
cross-crate integration.

AXP-E0001 is activated at its W16 live credential boundary after the W15
one-shot-start and operator-recovery repair. Its tracked
`editions/AXP-E0001/assignments.tsv` is the dispatch source of truth;
`workgraph.md` and the exact task packets are required supporting dispatch
context. The E0000 rows preserve the earlier plan, while E0006 and E0002-E0005
remain ranked candidates that require an owner-approved charter, assignments,
and exact task packets before any writer starts.

## Ranked editions

### AXP-E0000 — One Product Truth

**Outcome:** A repeatable, auditable swarm can ship the complete walking
skeleton with tracked planning records, reconciled contracts, automated test
impact, safe repository hygiene, and deterministic release-manager evidence.

**Exit evidence:** approved security-remediation record; tracked edition
artifacts; reverse-dependency scoped-test report; reconciled domain and kernel
contract diff; governed `edition.create` and `changeset.commit` conformance;
deterministic release-manager evaluation; owner sign-off.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0000-01 | Establish edition records and wave status protocol | none | `editions/AXP-E0000/**` | Eight required artifacts are tracked and status schema is complete | low | `gpt-5.6-luna` |
| E0000-02 | Remediate tracked `.proof` key/config/database exposure without deleting files; rotate or quarantine only with owner approval | E0000-01 | `.gitignore`, `.proof/**` (security owner) | Owner decision, permissions/secret scan, and recovery note; files remain present | critical | `gpt-5.6-sol` |
| E0000-03 | Make swarm launcher ownership-aware and emit handoff/evidence status | E0000-01 | `scripts/swarm.sh` | Dry-run shows root + three-worker assignment, rejects overlap, emits status | medium | `gpt-5.6-terra` |
| E0000-04 | Replace scoped-test map with reverse dependency impact automation | E0000-01 | `scripts/test-scoped.sh` | Cargo-graph report proves changed package plus dependents are selected | medium | `gpt-5.6-terra` |
| E0000-05 | Reconcile architecture, kernel API, domain definitions, registry, and implementation drift | E0000-01 | `contracts/**`, `ARCHITECTURE.md` (contract owner) | Decision log and zero unexplained operation/name/status mismatches | high | `gpt-5.6-sol` |
| E0000-13 | Design shared exact-output/proof replay and its storage migration | E0000-05 | read-only kernel/storage; unique handoff | API/migration, atomicity, conflict, recovery, and test plan ready for Gate B | critical | `gpt-5.6-sol` |
| E0000-14 | Implement approved execution replay API and engine policy | E0000-13 | `crates/proof-kernel/**` | Same key/input returns the original proof; conflicts fail before handler entry | critical | `gpt-5.6-sol` |
| E0000-16 | Implement migration 11 and SQLite replay ledger | E0000-13 | `crates/proof-storage/**` | Claim/complete/fail concurrency and migration round trips pass | critical | `gpt-5.6-sol` |
| E0000-15 | Map idempotency errors in HTTP and WebSocket peers | E0000-13 | HTTP/WS transport crates | Exhaustive mappings and transport tests cover 400/409/503 behavior | high | `gpt-5.6-luna` |
| E0000-18 | Reconcile the canonical kernel contract with approved replay API and migration 11 | E0000-14,15,16 | `contracts/kernel-api.md` | Public shapes, mappings, paths, and schema version match the approved implementation | medium | `gpt-5.6-luna` |
| E0000-06 | Implement governed `edition.create` and finish `changeset.commit` in the content/registry layer | E0000-14,15,16,18 | `crates/proof-content/**`, `registry/content/**`, `schemas/content/**` | Registry, handler, proof, idempotency, and content tests pass | high | `gpt-5.6-terra` |
| E0000-10 | Route the two operations through the CLI without a legacy bypass | E0000-06 | `crates/proof-transport-cli/**` | CLI tests prove engine dispatch and persisted evidence | high | `gpt-5.6-terra` |
| E0000-11 | Verify HTTP peer behavior for the two operations | E0000-06 | `crates/proof-transport-http/**` | Tower tests cover discovery, execution, errors, and proof output | high | `gpt-5.6-terra` |
| E0000-12 | Verify MCP peer behavior for the two operations | E0000-06 | `crates/proof-transport-mcp/**` | MCP tests cover registry-derived tools, execution, and evidence | high | `gpt-5.6-terra` |
| E0000-17 | Verify WebSocket peer behavior and original-proof replay for the two operations | E0000-06 | `crates/proof-transport-ws/**` | WS tests cover discovery, execution, errors, and identical proof replay | high | `gpt-5.6-terra` |
| E0000-07 | Add executable walking-skeleton conformance vectors for all eight content operations | E0000-10,11,12,17 | `conformance/**` | Clean run verifies operation/version, authority, proof, and replay invariants | high | `gpt-5.6-terra` |
| E0000-08 | Harden deterministic Release Manager automation and publish its evaluation fixture | E0000-07 | `crates/proof-agent-runtime/**`, `docs/dogfood/**`, `evals/**` | Repeated run has identical trace digest, approval pause/resume, and 10/10 evaluation | medium | `gpt-5.6-terra` |
| E0000-09 | Integrate, run scoped and quiescent workspace verification, and record owner release decision | E0000-02..08,10..18 | root manifests/release records (orchestrator) | Evidence index, risk disposition, full final gate, and dated owner approval | high | `gpt-5.6-sol` |

### AXP-E0001 — One Live Agent Does Real Work

**Outcome:** A real model completes a bounded release journey, pauses for
signed human approval, resumes durably, and publishes a verified preview.

**Status:** Activated. The authoritative twenty-task W1-W17 graph has
completed E0001-01..03 and E0001-06..20. W16 E0001-04 is stopped only at the
approved credential boundary and is preassigned to `e0001-live-operator` with
a distinct non-author verifier; W17 E0001-05 and Gate C remain blocked on the
live journey and independent 17/17 verification. E0001-20's final recovery
barrier includes a crash-released active-writer lease and atomic expected-tail
checkpoint append, so stale/concurrent resumers cannot poison history or
orphan an authorized provider response.

**Dispatch source of truth:** `editions/AXP-E0001/assignments.tsv`.
`workgraph.md` and `tasks/E0001-*.md` are required supporting dispatch context.
The original seed rows are non-authoritative and cannot assign or authorize
work.

| ID | Current work item | Depends on | Exclusive path | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0001-04 | Live dogfood and independent verification (`e0001-live-operator`; distinct `e0001-live-verifier`) | E0001-02,03,06..20 plus paid-use Gate B | `docs/dogfood/release-manager-live.md`<br>`editions/AXP-E0001/handoffs/E0001-04.md` | Final-source host suite (93/93), Runtime 122/122, Kernel 106/106, Storage 133/133, immutable 10/10 replay; one exact approved live run; signed approval chronology; same-run live resume; independent 17/17; cost and rollback evidence | critical | `gpt-5.6-sol` |
| E0001-05 | Quiescent integration and owner release gate | E0001-01..04,06..20 | edition release records and root integration files listed in the task packet | Quiescent verification passes and the product owner records a dated Gate C accept, defer, or reject decision | high | `gpt-5.6-sol` |

**Historical seed decomposition (not dispatch authority):**

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0001-01 | Define live release-manager contract and sealed evaluation policy | E0000 | `evals/**`, `contracts/**` | Versioned policy rejects unknown fields and binds trace digest | high | `gpt-5.6-sol` |
| E0001-02 | Provider-backed model/tool continuation and failure recovery | E0001-01 | `crates/proof-agent-runtime/**` | Live run pauses, resumes same step, never blindly replays mutation | high | `gpt-5.6-terra` |
| E0001-03 | Preview release adapter with safe side-effect boundary | E0001-01 | `crates/proof-content/**` release paths | Preview artifact and signed proof match requested version | high | `gpt-5.6-terra` |

### AXP-E0006 — Secure Standalone Approval Console

**Outcome:** A local Human can open the standalone approval console without a
reusable signing credential appearing in a URL, ordinary process output,
process arguments, browser history, Web Storage, cookies, referrers, logs, or
test artifacts.

**Status:** P0-next critical security candidate, ranked ahead of AXP-E0002. It
depends on E0001-05 and the recorded E0001 Gate C decision. Until then, only
this backlog candidate may be refined: do not create `editions/AXP-E0006/**`,
dispatch a writer, or modify the frozen E0001 CLI source. The current
standalone UI is not an approved operator path because its printed fragment is
a reusable workspace-wide Human signing capability for the server lifetime.

**Gate B security contract:** the public loopback URL is clean; a secure
non-URL bootstrap is one-use, short-lived, replay-resistant, and unavailable
without a verified local handoff; the resulting session credential is
distinct, memory-only, server-instance/workspace scoped, and bounded by
absolute and idle expiry. Exact Host/Origin/content-type checks remain, secret
comparison is constant-time, and malformed, duplicate, expired, replayed,
cross-instance, and cross-workspace credentials fail before signing. No
automatic browser launcher may place either credential in child argv.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0006-01 | Freeze the standalone-console threat model, bootstrap/session contract, and Gate A/B decision | E0001-05 + E0001 Gate C | new E0006 records plus one narrowly scoped approval-session contract (orchestrator only) | Contract covers clean URL/output, secure handoff, one-use exchange, lifetime/scope/revocation, constant-time comparison, Host/Origin, and fail-closed recovery | critical | `gpt-5.6-sol` |
| E0006-02 | Replace the reusable fragment bearer with the approved bootstrap/session flow | E0006-01 | `crates/proof-transport-cli/**` (one security owner) | Process tests prove secret-free output/argv; Tower tests prove one successful exchange, concurrent/replay/expiry/cross-instance rejection, no unauthorized signature, and unchanged v1/v2 actionability | critical | `gpt-5.6-sol` |
| E0006-03 | Independently verify browser secrecy, signing boundaries, public guidance, and Gate C | E0006-02 | non-author browser/security verifier; orchestrator owns `README.md` and release records | Clean loopback URL; no query/fragment/secret storage/cookie/referrer; no-store/CSP/no-referrer/frame/nosniff headers; full CLI and scoped-impact gates; secret-sentinel scan; dated non-author PASS and owner decision | critical | `gpt-5.6-sol` |

### AXP-E0002 — One Human Oversees Many Runs

**Outcome:** An operator can inspect, approve, deny, revoke, resume, and audit
multiple governed runs from one authenticated control plane.

**Dependency:** AXP-E0006 Gate C. E0002 may reuse only the released approval
session contract; the current standalone fragment bearer is not a bootstrap or
authentication primitive for the operator control plane.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0002-01 | Define operator run-list, approval, revocation, and audit contract | E0001 | `contracts/**`, `schemas/**` | Contract covers authority, pagination, and terminal states | high | `gpt-5.6-sol` |
| E0002-02 | Add batch run/approval query APIs | E0002-01 | `crates/proof-transport-http/**` | Tower integration tests cover auth, filtering, and pagination | high | `gpt-5.6-terra` |
| E0002-03 | Build operator console surfaces | E0002-01 | new operator-UI path reserved by the charter | Browser evidence shows pending, decision, and audit state | medium | `gpt-5.6-terra` |
| E0002-04 | Cross-run budget, revoke, and audit verification | E0002-02,03 | `crates/proof-agent-runtime/**`, `evals/**` | Concurrent runs stop on revoke and retain append-only evidence | critical | `gpt-5.6-sol` |

### AXP-E0003 — Governed Agent Teams

**Outcome:** A parent run can safely fan out work to bounded child agents,
aggregate budgets/evidence, cancel work, and evaluate the task tree.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0003-01 | Specify parent/child run and work-item DAG contract | E0002 | `contracts/**` | Versioned topology, delegation, lease, and cancellation rules | critical | `gpt-5.6-sol` |
| E0003-02 | Persist child-run topology, leases, and narrowed delegation contracts | E0003-01 | `crates/proof-kernel/**`, `crates/proof-storage/**` in dependency-ordered waves | Storage and authority tests bind parent, child, lease, and delegation | critical | `gpt-5.6-sol` |
| E0003-03 | Runtime coordination, heartbeats, aggregate limits, and crash recovery | E0003-02 | `crates/proof-agent-runtime/**` | Restart/reclaim tests show no duplicate mutation and enforce tree budgets | critical | `gpt-5.6-terra` |
| E0003-04 | Task-tree evaluation and swarm operator view | E0003-03 | `evals/**` plus the charter-reserved operator-UI path | Sealed evaluation catches missing/late/unauthorized child evidence | high | `gpt-5.6-terra` |

### AXP-E0004 — Production Workspaces

**Outcome:** AXP supports durable, recoverable, observable multi-tenant
workspaces with production storage and key handling.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0004-01 | PostgreSQL storage parity and migration discipline | E0003 | `crates/proof-storage/**` | Round-trip and migration tests pass against PostgreSQL | critical | `gpt-5.6-terra` |
| E0004-02 | Workspace auth, isolation, and tenancy boundaries | E0004-01 | auth/transport paths | Cross-workspace access tests fail closed | critical | `gpt-5.6-sol` |
| E0004-03 | Backup/restore, key rotation, secrets, and recovery runbook | E0004-01 | ops/docs/security paths | Restore drill verifies proofs and rotated identities | critical | `gpt-5.6-sol` |
| E0004-04 | Observability, metering, SLOs, and failover | E0004-01,02 | `crates/proof-observability/**` | Load report, dashboards, and declared SLO evidence | high | `gpt-5.6-terra` |

### AXP-E0005 — Extensible, High-Volume AXP

**Outcome:** External builders can add registry operations through SDKs and
run measured high-volume governed workloads without kernel modification.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0005-01 | Stable registry extension and validator contract | E0004 | `contracts/**`, registry tooling | Third-party operation loads without kernel enum/code change | high | `gpt-5.6-sol` |
| E0005-02 | TypeScript and Python SDKs for discovery/execution/evidence | E0005-01 | SDK directories | SDK conformance runs match HTTP/MCP proofs | medium | `gpt-5.6-terra` |
| E0005-03 | Batch approval and high-volume execution controls | E0003, E0004 | runtime/transport paths | 50K-object benchmark meets declared latency/error/budget targets | critical | `gpt-5.6-terra` |
| E0005-04 | Ecosystem example connector and independent verifier | E0005-01,02 | examples/conformance paths | External package produces offline-verifiable proof | high | `gpt-5.6-terra` |
