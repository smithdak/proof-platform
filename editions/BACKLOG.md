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

AXP-E0001 is blocked and quiescent after its Gate C defer in D-E0001-020.
AXP-E0006 is blocked and quiescent after its P0 security work; Gate A/B and its
candidate implementation are complete. Its five-minute Human decision/revoke path and later
credential-free single-tab attachment probe passed separately. The final
D-E0006-013 run reached terminal verification, but the current attached tab
failed closed before session/app evidence. The product owner then authorized
D-E0006-015: one bounded CLI remediation wave, independent review, and one
post-review exact ceremony under the unchanged frozen contract. The remediation
and source/test review passed, but the Human-visible product page and isolated
verifier target were different browser contexts in that single ceremony. Its
authority is consumed. A later credential-free launcher-first diagnostic passed
without product state and isolated the tooling failure. D-E0006-018 then
authorized exactly one launcher-first provider-free product ceremony. Its
launcher and same-tab preflight passed, but the product run persisted an
unexpected signed approval instead of the authorized denial. Execution remained
null, and D-E0006-019 consumed the run without retry. AXP-E0006 is blocked and
quiescent at that boundary. The owner then directed either a fix or a move;
D-E0006-020 selects a bounded UI-only Human-intent guard in W8, followed by
non-author source/test review and no product runtime. Both passed. D-E0006-023
then authorized one final E0006-11 ceremony, but its credential-free launcher
never became the current document in the sole headed tab. D-E0006-024 consumed
the run before product state, with complete cleanup and no retry. The product
owner deferred Gate C in D-E0006-025. E0006 is quiescent and unreleased, its
standalone UI remains unsupported, and terminal approve/deny is the rollback.
The same decision creates a narrow roadmap exception for E0002 Gate A
scaffolding only; no E0002 implementation or contract authority follows. The
tracked `editions/AXP-E0006/assignments.tsv` remains E0006's dispatch source of
truth. E0003-E0005 remain ranked candidates without active writers.

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

**Status:** Blocked and quiescent after the product-owner Gate C defer in
D-E0001-020. The exact authorized live start reached the direct provider, then
sealed failed after two dispatches and its one allowed retry, before any
committed response, approval, tool, artifact, proof, publication, or mutation.
The live evaluation is 5/17; the deterministic 10/10 and final host-context
614/614 verification remain valid but do not make the edition released. Its
B5 one-run authority is exhausted, and no diagnostic, replacement, or retry is
authorized without a new explicit Gate B decision.

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

**Status:** Blocked, quiescent, and unreleased after the D-E0006-020
Human-intent remediation, D-E0006-022 independent PASS, D-E0006-024 terminal
launcher-gate failure, and D-E0006-025 product-owner Gate C defer/no-go.
Gate A/B are approved; E0006-02 is quiescent with 117/117 host tests and an
independent source/contract PASS. E0006-03 passed pre-session browser/header
checks and deterministic rejection coverage. A follow-up co-located Human
journey then completed the secure handoff, one exact signed denial, explicit
revoke, controlled shutdown, and TTY restoration with zero execution/resume.
The prior headed W3 ceremony proved browser secrecy/capture but expired before
decision/revoke. The owner-authorized five-minute rerun then passed the exact
signed denial, explicit revoke, shutdown, TTY restoration, and zero
execution/resume path, but automation exposed a distinct fresh tab instead of
the Human-visible post-decision tab. A subsequent credential-free probe proved
the corrected cross-agent single-tab attachment pattern and showed that tab
inventory metadata can remain stale after asynchronous navigation. Exact
14/14, E0006-04, and Gate C remain blocked until the product path and corrected
attachment pass in one ceremony. The final D-E0006-013 ceremony reached
terminal verification but its current attached tab returned a generic
rejection with hidden app controls; it created no decision or execution and
its one-run authority was consumed. D-E0006-015 authorized a bounded
current-document delivery remediation, non-author review, and one final exact
14/14 ceremony without changing the frozen contract. E0006-05 passed its
deterministic remediation, independent review, and 120/120 host tests. The one
post-remediation ceremony then failed its same-visible-tab boundary: the Human
used the product page in a regular browser while the isolated verifier target
remained `New Tab`. It stopped before a decision or execution, and its authority
is consumed. E0006-07 then proved a credential-free launcher-first pattern in
one visible headed tab: direct reads reached a pending target while inventory
retained stale New Tab metadata. D-E0006-018 activated exactly one E0006-08
provider-free ceremony with no retry. Its launcher preflight passed, but the
product run persisted an unexpected approval instead of the authorized denial;
execution remained null and D-E0006-019 consumed the run. D-E0006-020 now
authorized a UI-only exact request/outcome Human-intent challenge plus
independent source/test review. E0006-09 and E0006-10 passed focused 7/7,
host/scoped 124/124, JavaScript, formatting, frozen-hash, and distinct review
gates. D-E0006-022 accepts that fix. D-E0006-023 then authorized one exact
E0006-11 run, but the launcher returned GET 200 while the sole headed tab
remained empty `about:blank`; D-E0006-024 consumed the authority before product
state. D-E0006-025 records the product owner's Gate C defer/no-go. The
standalone UI remains unreleased, E0006-11 and E0006-04 are blocked and
quiescent, and terminal approval commands are the rollback path.

**Dispatch source of truth:** `editions/AXP-E0006/assignments.tsv`. The
backlog rows below are descriptive only and cannot authorize work.

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
| E0006-01 | Freeze the standalone-console threat model, bootstrap/session contract, and Gate A/B decision | D-E0001-020 + E0001 quiescent/writer-free | new E0006 records plus one narrowly scoped approval-session contract (orchestrator only) | Contract covers clean URL/output, secure handoff, one-use exchange, lifetime/scope/revocation, constant-time comparison, Host/Origin, and fail-closed recovery | critical | `gpt-5.6-sol` |
| E0006-02 | Replace the reusable fragment bearer with the approved bootstrap/session flow | E0006-01 | `crates/proof-transport-cli/**` (one security owner) | Process tests prove secret-free output/argv; Tower tests prove one successful exchange, concurrent/replay/expiry/cross-instance rejection, no unauthorized signature, and unchanged v1/v2 actionability | critical | `gpt-5.6-sol` |
| E0006-03 | Independently verify browser secrecy and signing boundaries | E0006-02 | non-author browser/security verifier; redacted dogfood evidence only | Clean loopback URL; no query/fragment/secret storage/cookie/referrer; no-store/CSP/no-referrer/frame/nosniff headers; secret-sentinel scan; exact 14/14 non-author PASS | critical | `gpt-5.6-sol` |
| E0006-05 | Remediate the current-document bootstrap/session delivery ambiguity | E0006-02, D-E0006-015 | `crates/proof-transport-cli/**` (same single security owner, later wave) | Deterministic reproduction/root cause, legitimate one-document path corrected, and reload/replay/lost-response paths remain fail closed | critical | `gpt-5.6-sol` |
| E0006-06 | Independently review remediation and run one exact ceremony | E0006-05 | non-author browser/security verifier; redacted dogfood evidence only | Candidate source/test PASS followed by one same-tab, same-session exact 14/14 journey with zero execution/resume | critical | `gpt-5.6-sol` |
| E0006-07 | Prove launcher-first isolated-browser attachment without product state | E0006-05 + D-E0006-016 failure context | orchestrator-owned transient diagnostic and edition records | One headed tab reaches a pending credential-free target through direct reads; stale inventory classified; exact cleanup passes | high | `gpt-5.6-sol` |
| E0006-08 | Run one launcher-first exact product ceremony under D-E0006-018 | E0006-07 + D-E0006-018 | non-author browser/security verifier; redacted dogfood evidence only | One already-visible headed tab supplies the complete same-session 14/14 journey; no retry, provider, execution, or retained credential | critical | `gpt-5.6-sol` |
| E0006-09 | Require explicit request-bound Human intent before browser decision POST | E0006-08 + D-E0006-020 | `crates/proof-transport-cli/**` plus unique handoff | Exact `DENY/APPROVE <request-id>` challenge, frozen form identity, cancellation, and unchanged backend/frozen hashes pass deterministic tests | critical | `gpt-5.6-sol` |
| E0006-10 | Independently verify the intent guard without a product runtime | E0006-09 | non-author source/test verifier; redacted dogfood evidence only | Source review and scoped host tests pass with no fixture, credential, decision, provider, or effect | critical | `gpt-5.6-sol` |
| E0006-11 | Run one intent-bound exact ceremony after separate owner authorization | E0006-10 + D-E0006-023 | non-author browser/security verifier; redacted dogfood evidence only | Failed at launcher gate under D-E0006-024; no product state, complete cleanup, authority consumed | critical | `gpt-5.6-sol` |
| E0006-04 | Reconcile public guidance and record the owner Gate C disposition | E0006-01,02,05,07,08,09,10,11 | orchestrator-owned public guidance and release records | Gate C defer/no-go, unreleased status, terminal rollback, absent 14/14, and intentionally unclaimed final verifier are durable | critical | `gpt-5.6-sol` |

### AXP-E0002 — One Human Oversees Many Runs

**Outcome:** An operator can inspect, approve, deny, revoke, resume, and audit
multiple governed runs from one authenticated control plane.

**Dependency and exception:** AXP-E0006 Gate C was deferred/no-go in
D-E0006-025; E0006 is not released. That owner decision permits this E0002
Gate A planning scaffold only. E0002 must define an independent operator
authentication boundary and may use E0006 only as read-only failure/design
input. It may not inherit, wrap, relabel, or claim the E0006 session, evidence,
release status, bootstrap, or authority.

Read-only operator-control discovery completed on 2026-08-31 and was refined by
three independent path-level audits on 2026-09-01 without activating an E0002
writer. The repository has durable per-run records but no authenticated
multi-run projection, scoped operator identity, bounded keyset pagination,
append-only cancellation/resume command record, durable fenced lease, aggregate
budget ledger, or complete operator audit chronology. The generic HTTP audit is
an unauthenticated execution-context view, and current cancellation changes
only run status. Kernel, storage, runtime, and shared operator-auth work must
therefore be explicit tasks rather than hidden inside an HTTP assignment.

The current authorization ends after the owner-ready Gate A packet. It creates
no public contract, schema, evaluator, source, migration, provider run, browser
ceremony, or external effect. After a separate Gate A, E0002-13 may freeze the
operator contract/schema/evaluation and request Gate B. After Gate B, kernel,
independent auth, and mechanical fixtures may fan out in parallel. Storage
follows kernel; runtime follows storage; authenticated HTTP reads follow
storage/auth and the dedicated control-plane assembly; mutations follow
runtime/auth; UI and independent verification follow the protected APIs. The
two HTTP waves use one named crate owner and are strictly sequential.

| ID | Assignable work item | Depends on | Exclusive path suggestion | Acceptance evidence | Risk | Tier |
|---|---|---|---|---|---|---|
| E0002-01 | Scaffold the owner-ready Gate A direction packet only | D-E0006-025 planning exception | `editions/AXP-E0002/**`; E0002 backlog rows | Complete charter, journey, metric, non-goals, budgets, acceptance proposal, workgraph, exclusive paths, stop gates, and owner decision text; edition validates with every product task non-dispatchable | high | `gpt-5.6-sol` |
| E0002-13 | Freeze the independent operator authority, projection, pagination, command, audit, recovery, and evaluation contract | E0002 Gate A | `contracts/operator-control-plane.md`; `schemas/operator-control/**`; `evals/operator-control-v1.json`; W2 edition decision/status records | Gate B packet binds Human/workspace/instance/capability scopes, exact routes/DTOs, cursor bindings, mutation ordering, no-remote boundary, and frozen required-check/vector digests | critical | `gpt-5.6-sol` |
| E0002-05 | Add kernel operator-command, audit, cursor, lease/fence, and aggregate-budget contracts | E0002-13 + Gate B | `crates/proof-kernel/**` | Strict public types reject unknown fields; canonical cancel/resume records, transition/idempotency conflicts, cursor bindings, fences, and budget values pass unit tests | critical | `gpt-5.6-sol` |
| E0002-08 | Implement independent scoped Human operator authentication without reusing E0006 authority | E0002-13 + Gate B | `crates/proof-operator-auth/**` plus explicitly delegated W3 root manifest/lock delta | Fresh enrolled-Human signed challenge and volatile session bind exact Human/workspace/instance plus least granted scopes; replay, self-escalation, auth-first, revoke/expiry, cross-scope, Host/Origin, and persistence vectors pass | critical | `gpt-5.6-sol` |
| E0002-12 | Build mechanical contract fixtures and rejection-vector harness | E0002-13 + Gate B | `evals/fixtures/operator-control/**` | Frozen valid/invalid DTO, cursor, scope, receipt, and chronology fixtures replay deterministically with no security-policy judgment or product effect | medium | `gpt-5.6-luna` |
| E0002-06 | Persist filtered multi-run projections and an atomic append-only operator-command ledger | E0002-05 | `crates/proof-storage/**` | Next migration and reopen round trips pass; bounded keyset pagination has no skips/duplicates; cancel/complete, lease/reclaim, stale-fence, command replay, and aggregate-budget races linearize exactly | critical | `gpt-5.6-sol` |
| E0002-07 | Enforce runtime cancel/resume, fenced recovery, and aggregate budgets | E0002-06 | `crates/proof-agent-runtime/**` | Barrier tests prove cancel-before-dispatch has zero provider/tool effect, cancel/resume has one winner, stale epochs cannot write, recovery is single-owner, and concurrent runs cannot exceed aggregate limits | critical | `gpt-5.6-sol` |
| E0002-11 | Build the loopback operator-control launcher, signed-challenge adapter, synthetic same-origin assembly interface, revoke, and shutdown | E0002-07, E0002-08 | `crates/proof-operator-control/**` plus explicitly delegated W6 root manifest/lock delta | Process/Tower tests prove clean loopback delivery, independent issuance, generic failures, injected-router/static-source assembly, no persistence, explicit revoke, control-plane restart invalidation, runtime-worker restart continuity, and fail-closed shutdown | critical | `gpt-5.6-sol` |
| E0002-02 | Add authenticated operator read APIs for attention inbox, run detail, approvals, and audit | E0002-06, E0002-08, E0002-11 | `crates/proof-transport-http/**` in the first HTTP wave | Tower tests prove auth-before-enumeration, scoped/redacted DTOs, stable cursor/filter behavior under inserts, generic unauthorized/not-found results, and exact Host/security headers | high | `gpt-5.6-terra` with `gpt-5.6-sol` security review |
| E0002-09 | Add authenticated approve/deny, cancel, and explicit-resume APIs | E0002-02, E0002-07, E0002-08, E0002-11 | `crates/proof-transport-http/**` in a later sequential wave | Deterministic mutation races preserve one decision/command winner, no automatic resume, no key/provider/write after lost authority, expected revisions, idempotent receipts, and uncertain-response recovery | critical | `gpt-5.6-sol` |
| E0002-03 | Build the multi-run operator console against frozen schemas and scoped APIs | E0002-02, E0002-09, E0002-11 | `apps/operator-console/**` | Browser evidence covers attention filters, exact run/approval/audit detail, distinct decision/cancel/session-revoke challenges, accessibility, stale controls, session loss, uncertain-write recovery, and no credential persistence | high | `gpt-5.6-terra` |
| E0002-04 | Independently verify API/browser concurrency, recovery, authority, and zero-effect boundaries | E0002-02, E0002-03, E0002-07, E0002-08, E0002-09, E0002-11 | verifier-only dogfood evidence and unique handoff | Frozen all-required evaluation passes exact auth, pagination, decision, cancel/resume, lease/recovery, budget, browser secrecy, audit chronology, separate worker/control-plane restart, and sentinel vectors | critical | `gpt-5.6-sol` |
| E0002-10 | Quiescent integration and product-owner Gate C | E0002-01,02,03,04,05,06,07,08,09,11,12,13 | root integration, public guidance, edition release records | Scoped/reverse-impact and quiescent gates pass; limitations, rollback, spend, independent verdict, and dated owner accept/defer/reject are recorded without claiming E0006 release evidence | high | `gpt-5.6-sol` |

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
