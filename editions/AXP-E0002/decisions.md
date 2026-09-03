# Decisions and Exceptions

## D-E0002-001 — Planning-only roadmap exception; E0006 is not a dependency primitive

Status: adopted · Date: 2026-09-01 · Decision owner: product owner

The product owner directed:

> Defer AXP-E0006 Gate C, keep the standalone approval UI unreleased with
> terminal approve/deny as rollback, and authorize AXP-E0002 Gate A scaffolding
> only. E0002 must define independent scoped operator authentication and may not
> claim or reuse E0006 as released.

This permits creation and review of the tracked E0002 charter, assignments,
task packets, handoffs, workgraph, ownership, status, decisions, evidence,
retro, and backlog reconciliation. It does not approve Gate A or Gate B and
does not authorize a public contract, schema, evaluator, migration, source,
root-manifest change, provider/browser/product run, external effect, commit, or
push.

AXP-E0006 remains quiescent and unreleased. E0002 may cite its failure modes as
read-only design input only. No E0006 bootstrap, session, code, evidence,
digest, approval, release state, or authority is inherited or relabeled.

## D-E0002-002 — Proposed Gate A direction packet

Status: superseded by D-E0002-007 · Date: 2026-09-01 · Decision owner: product
owner

The proposed direction is the chartered eight-step journey: one independently
authenticated Human uses a fresh E0002-owned signed challenge to oversee at
least four differently situated durable runs,
makes exact signed decisions, explicitly resumes, cancels before dispatch,
observes distinct runtime-worker and control-plane restart behavior plus stable
audit/budgets, reauthenticates after control-plane restart, and revokes the
operator session.
The evaluator is all-required and deterministic with zero live provider spend.
Remote access, E0006 reuse, auto-resume, bulk approval, decision withdrawal,
native swarm scheduling, and external effects are non-goals.

If accepted without revision, the bounded authorization is:

> Approve AXP-E0002 Gate A as chartered and authorize E0002-13 to draft and
> freeze the independent operator contract, schemas, and evaluator for a
> separate Gate B decision. Do not dispatch implementation.

No approval is inferred from this proposed text.

## D-E0002-003 — Proposed Gate B boundary

Status: proposed / non-dispatchable · Date: 2026-09-01 · Decision owner:
product owner

After Gate A, E0002-13 must present exact digests for the independent operator
contract, strict schemas, all-required evaluator, and rejection-vector set. The
packet must also name the new auth crate dependency/root-manifest delta, next
SQLite migration number and shapes, the authoritative workspace database and
trusted-open/signer lifecycle, fresh disposable identity policy, auth/session secret handling, exact
challenge/issuance/expiry/revoke flow, capability-grant and separation-of-duties
policy, failure order, DTO redaction, cursor bindings, append-only
commands/audit, approval/cancel/resume ordering, fenced recovery, aggregate
budgets, dedicated protected route inventory with legacy-route exclusion, and
rollback. It must avoid new global `ExecutionError` variants or bind their
same-wave HTTP mappings. Gate B must explicitly accept or reject those
artifacts before W3.

## D-E0002-004 — Proposed routing and retry policy

Status: adopted for planning only · Date: 2026-09-01 · Decision owner:
orchestrator

One mechanical frozen-fixture task starts on Luna; bounded HTTP reads and UI
start on Terra; contract, auth/security, kernel authority, migration/storage,
runtime races, loopback control-plane assembly, mutations, verification, and
integration use Sol. Each task has at most one bounded retry; a failed
evaluation escalates only that task and preserves its handoff. This routing
does not dispatch any task. Per-attempt ceilings are 45 minutes/25,000 combined
tokens for Luna, 90 minutes/50,000 for Terra, 120 minutes/80,000 for Sol high,
and 180 minutes/120,000 for the Sol xhigh integration task.

## D-E0002-005 — Independent Gate A scaffold reviews accepted

Status: adopted for planning evidence · Date: 2026-09-01 · Decision owner:
orchestrator

Three distinct read-only reviewers accepted the corrected packet:

- security/gates PASS after E0002-01 and E0002-13 gained explicit dated
  Gate A/Gate B completion rules and status/dependency locks;
- workgraph/ownership/model PASS after E0002-13 metadata, manual governance-lock
  wording, synthetic-only W6 assembly, actual composition ownership, and the
  dependency diagram were reconciled; and
- journey/evaluation PASS after E0002-11 gained the loopback launcher,
  signed-challenge adapter, same-origin assembly, revoke/shutdown ownership and
  runtime-worker versus control-plane restart became separate required vectors.

Both structural validation and diff checking passed. These reviews establish
only that the proposal is owner-ready. They do not approve Gate A/B, dispatch
E0002-13 or implementation, create product evidence, or change E0006's
unreleased status.

This record is historical evidence for the pre-D-E0002-006 scaffold only.
Later review found that its supposed “actual composition ownership” ended at
documentation/evidence rather than a source writer. D-E0002-009 corrects that
gap with E0002-15. D-E0002-005 is superseded for current Gate A readiness and
cannot substitute for the fresh review recorded after this revision.

## D-E0002-006 — Owner authorizes a bounded planning revision

Status: adopted · Date: 2026-09-01 · Decision owner: product owner

After reviewing the repository/current-state analysis, the product owner
directed: “yes, proceed. plan then work.” This authorizes the orchestrator to
revise the E0002 Gate A planning records, validate them, and obtain independent
read-only reviews. It is interpreted conservatively because the revised packet
did not yet exist when the direction was given: it is not Gate A acceptance,
does not activate E0002-13, and does not authorize contracts, schemas,
evaluators, source, migrations, manifests, product/browser/provider runs,
external effects, commits, or pushes.

The revision must explicitly address the compromised repository-root `.proof`
identity, authoritative workspace store and signer lifecycle, protected-router
isolation, per-task budgets, same-wave error mapping, root-manifest staging, and
the dependency-safe storage/runtime/control fan-out with later integration.

## D-E0002-007 — Revised proposed Gate A direction packet

Status: accepted as Gate A by D-E0002-011 · Date: 2026-09-01 · Decision owner:
product owner

This proposal preserves the eight-step operator journey and all-required,
zero-live-spend evaluation from D-E0002-002 while tightening the direction:

- every product/evaluation path uses a disposable trusted workspace and fresh
  enrolled Human/Agent identities; repository-root `.proof` authority is
  prohibited;
- Gate B freezes one authoritative database/trusted-open/signer lifecycle and
  a dedicated protected router with no unauthenticated legacy fallback;
- fifteen budgeted tasks run across W1-W10; W3 fans out kernel/auth/fixtures,
  W4 fans out storage/runtime-core/control-shell, W5 independently composes
  the backend before mutations, and W8 assembles the real runnable product
  before non-author verification;
- every global execution error is mapped in HTTP in the same wave or avoided;
  root manifest/lock deltas stabilize before concurrent Cargo commands; and
- transport-parity and delivery-hygiene repairs remain separate outcomes unless
  required to isolate the E0002 security boundary.

If accepted without revision, the bounded authorization is:

> Approve the revised AXP-E0002 Gate A charter and workgraph in
> D-E0002-007. Mark E0002-01 done and authorize E0002-13 only to draft and
> freeze the independent operator contract, strict schemas, evaluator, exact
> migration 14/store/signer/router policy, and Gate B digest packet. Do not
> dispatch implementation.

No approval is inferred from D-E0002-006 or from the existence/validation of
this proposal.

## D-E0002-008 — Accelerated graph remains contract-first and integration-gated

Status: adopted for planning only · Date: 2026-09-01 · Decision owner:
orchestrator

Cargo topology shows `proof-agent-runtime` depends on kernel/content and uses
kernel store traits rather than depending on `proof-storage`. After W3 freezes
and implements those traits, storage and runtime-core can therefore proceed in
parallel. The control security shell depends on the independent auth API and
frozen synthetic router/store-opener boundaries rather than W4 storage/runtime
source, so it can occupy the third W4 lane.

E0002-14 is added as a distinct W5 non-author integration owner under
`conformance/**`. Runtime-core cannot claim durable acceptance by itself;
protected mutation work remains blocked until E0002-14 proves real SQLite
restart, fence, command, aggregate-budget, and control-plane composition. This
acceleration changes scheduling, not product scope or Gate B authority.

## D-E0002-009 — Product assembly and build-state ownership are explicit

Status: adopted for planning only · Date: 2026-09-01 · Decision owner:
orchestrator

E0002-15 is a later sequential `proof-operator-control` source owner after the
protected reads, protected mutations, backend integration, and UI are complete.
It alone binds the real protected router, trusted authoritative store/runtime,
independent session state, and static app into the runnable process. The
general legacy HTTP router is never a fallback. Independent E0002-04 product
verification now depends on this source task rather than treating verifier or
release evidence as composition.

Build-state ownership is serialized without giving concurrent agents shared
write authority. In W5, E0002-02 and E0002-14 first freeze their disjoint
package-manifest deltas without Cargo; E0002-14 then alone reconciles
`Cargo.lock` with the Gate-B-frozen serialized lock-generation Cargo command
before either runs build/test Cargo. In W7, E0002-03 freezes a deterministic
static bundle, build command, and digest under `apps/operator-console/dist/**`.
In W8, E0002-15 owns only that generated output, the control crate, and
`Cargo.lock`; it may reproduce the frozen bundle but cannot edit UI source or
root `Cargo.toml`. These barriers are task protocol, not dispatch authority.

## D-E0002-010 — Revised Gate A planning reviews pass

Status: adopted for planning evidence · Date: 2026-09-01 · Decision owner:
orchestrator

Three distinct read-only reviewers assessed the settled fifteen-task revision:

- security/gates/journey PASS: fresh identities, authoritative store/signer,
  protected-router isolation, error mapping, restart vectors, and Gate A/B
  locks are complete;
- edition consistency PASS: assignments, tasks, handoffs, ownership, status,
  evidence, budgets, waves, generated UI artifact, and source assembly agree;
  and
- crate topology/ownership/model PASS: accelerated W4 is dependency-safe,
  final dependency direction is acyclic, manifest/lock and reverse-impact
  barriers are dispatch-readable, paths are disjoint, and model/budget routing
  is correct.

`rtk scripts/swarm.sh validate AXP-E0002`, assignment validation, all fifteen
packet resolutions, forbidden-product-path inspection, and
`rtk git diff --check` pass. These verdicts make the revised proposal
owner-ready only. They do not approve Gate A/B/C, mark E0002-01 done, activate
E0002-13, dispatch implementation, or supply product/release evidence.

## D-E0002-011 — Product owner accepts revised Gate A

Status: adopted / Gate A accepted · Date: 2026-09-01 · Decision owner: product
owner

After receiving the settled packet and D-E0002-010 review results, the product
owner explicitly directed:

> Approve the revised AXP-E0002 Gate A charter and workgraph in D-E0002-007.
> Mark E0002-01 done and authorize E0002-13 only to draft and freeze the
> independent operator contract, strict schemas, evaluator, exact migration
> 14/store/signer/router policy, and Gate B digest packet. Do not dispatch
> implementation.

This dated decision completes E0002-01 and activates E0002-13 only. E0002-13
must return to `review` after freezing its artifacts and cannot become `done`
without explicit product-owner Gate B acceptance of their exact digests and
material-risk choices. W3 and every implementation task remain pending and
non-dispatchable. There is no provider, browser/product runtime, external
effect, commit, push, Gate B, or Gate C authority.

Record future scope changes, escalations, cross-owner requests, Gate B
decisions, and explicit exceptions here. Never silently broaden a task.

## D-E0002-012 — Proposed digest-bound Gate B packet

Status: proposed / owner decision required / non-dispatchable · Date:
2026-09-01 · Decision owner: product owner

E0002-13 has frozen the independent operator contract, strict schemas and
manifest, all-required evaluator, exact migration 14/store/signer/router
policy, dependency deltas, and rollback. The following JSON object is the
immutable Gate B decision packet. Its digest is SHA-256 over UTF-8 compact JSON
with recursively lexicographically sorted object keys, separators `,` and `:`,
and no trailing newline.

<!-- gate-b-packet-v1:start -->
```json
{
  "schema": "proof.operator.gate-b-packet/v1",
  "edition": "AXP-E0002",
  "task": "E0002-13",
  "date": "2026-09-01",
  "status": "pending_owner",
  "implementation_dispatch": false,
  "artifacts": [
    {"ordinal": 1, "role": "contract", "path": "contracts/operator-control-plane.md", "sha256": "sha256:4f51a81e6c75a1c09bbc4362087ab2d574c37e4b3a98a98ec3f2ca9998be85e8"},
    {"ordinal": 2, "role": "schema", "path": "schemas/operator-control/common-v1.schema.json", "sha256": "sha256:e64b2278a81db61ccf333005274990366047e133c9a507915c4123e641c3412b"},
    {"ordinal": 3, "role": "schema", "path": "schemas/operator-control/auth-v1.schema.json", "sha256": "sha256:8ec5777fbb9c7a36484f3503a04f36dd297a880f6cf9c0ba7737384461c5c37d"},
    {"ordinal": 4, "role": "schema", "path": "schemas/operator-control/reads-v1.schema.json", "sha256": "sha256:5072e5c3586267c1dc4de4cdb7944d617f1534a2e4ebbb3da6cb1c094636598f"},
    {"ordinal": 5, "role": "schema", "path": "schemas/operator-control/mutations-v1.schema.json", "sha256": "sha256:0d0ea379971e673fc45cd47d38896b18ddf06712f872ca5a6363d6b07ecf940c"},
    {"ordinal": 6, "role": "schema", "path": "schemas/operator-control/durable-v1.schema.json", "sha256": "sha256:20c7380cad0692f7459bbb6b4d06d2a9b18810f99e45963fa1941d4f5733f719"},
    {"ordinal": 7, "role": "schema", "path": "schemas/operator-control/store-v1.schema.json", "sha256": "sha256:afb321e63ae8884ace0be83c7cad08f1a22e60876a082cf092dc7081d66ef4ab"},
    {"ordinal": 8, "role": "schema", "path": "schemas/operator-control/evaluator-v1.schema.json", "sha256": "sha256:1fae78f5c452ae35a4ab536d1e6e12cf8ae9ec51de4915cd8ff4180a36c9d2c6"},
    {"ordinal": 9, "role": "schema", "path": "schemas/operator-control/manifest-v1.schema.json", "sha256": "sha256:685f683910f0956987db08a1646aa2aee5f5d884fc13634fb80fa4bbf08467d6"},
    {"ordinal": 10, "role": "schema_manifest", "path": "schemas/operator-control/manifest-v1.json", "sha256": "sha256:3a2a100651a55ba39d667b4d12f8342c279486b9d499a6aa02f25eb35634da09"},
    {"ordinal": 11, "role": "evaluator", "path": "evals/operator-control-v1.json", "sha256": "sha256:d1a10306ece122cb771fc9aa3b51437887040649c117a4716b1b0b9795f4a339"}
  ],
  "semantic_digests": {
    "valid_scenarios": "sha256:769717b791bdc78220cb006b5d6c5c0b17d368562ab60218eb3bd51fc8cd1c5f",
    "checks": "sha256:43b5e0daa4fd2f810c04a8ecaf61b295e6069bf8b43a92d111accc4201ac958d",
    "rejection_vectors": "sha256:933001d356fae11f227759044c2ea98ad5105ddf9789b87a5e68bd6dc7063a5c",
    "backend_subset": "sha256:5298e900fec2d3314cde366a2ec90ca11ea6dd812459bf8e6af0ffb6dd980c3b",
    "store_error_matrix": "sha256:cd99bd03809a467a9478b77ebe2e73fd2959db98cafa038f98744d7491beaa6c"
  },
  "evaluation": {
    "required_score_basis_points": 10000,
    "replay_count": 2,
    "valid_scenarios": 16,
    "ordered_checks": 20,
    "rejection_vectors": 105,
    "backend_subset_scenarios": 4,
    "backend_subset_vectors": 16,
    "store_boundaries": 21,
    "store_error_variants": 9,
    "store_matrix_cells": 189,
    "typed_absence_cases": 4,
    "manifest_logical_shapes": 206,
    "protected_and_public_routes": 15,
    "schema_self_tests": 4
  },
  "migration_14": {
    "contract_section": 14,
    "description": "create governed operator control, projection, fence, budget, command, and audit schema",
    "prior_versions_unchanged": "1-13",
    "validated_up_down": true,
    "operator_tables": 14,
    "immutable_triggers": 20,
    "indexes": 19,
    "pre_14_objects_preserved": true
  },
  "material_choices": [
    {"ordinal": 1, "id": "independent_terminal_signed_human_challenge_and_volatile_session"},
    {"ordinal": 2, "id": "six_capability_intersection_exact_human_no_delegation"},
    {"ordinal": 3, "id": "loopback_request_error_secret_boundary_with_same_uid_root_limits"},
    {"ordinal": 4, "id": "disposable_workspace_forbidden_repository_root_identity_single_schema14_database_trusted_open_persisted_signers"},
    {"ordinal": 5, "id": "dedicated_router_exact_inventory_and_legacy_route_exclusion"},
    {"ordinal": 6, "id": "migration14_immutable_provisioning_atomic_store_and_legacy_write_rejection"},
    {"ordinal": 7, "id": "approval_explicit_resume_cancel_dispatch_idempotency_revoke_and_signer_order"},
    {"ordinal": 8, "id": "lease30s_renew10s_fenced_recovery_and_distinct_restart_semantics"},
    {"ordinal": 9, "id": "five_dimension_aggregate_reservation_and_forfeit_rules"},
    {"ordinal": 10, "id": "append_only_projection_cursor_mac_audit_redaction_and_static_constraints"},
    {"ordinal": 11, "id": "exact_root_member_dependency_and_lock_deltas"},
    {"ordinal": 12, "id": "no_new_global_execution_error_and_all_required_zero_effect_evaluation"}
  ]
}
```
<!-- gate-b-packet-v1:end -->

Packet SHA-256:
`sha256:eaff3d4d78ca3e6e4fe521f53b12b9598765db50ffd38fde0d6bf3aeb4c42dd4`

A distinct read-only post-fix reviewer returned PASS. The prior V38-V45
provisioning-outcome blocker is resolved: all eight are fail-before-bind launch
refusals with exact closed error reasons. The reviewer found no remaining
release blocker and independently rechecked schema/meta-schema validity,
references, 206/206 manifest shapes, frozen digests, and migration 14
up/down/FK behavior. This review is evidence for owner consideration, not Gate
B acceptance.

The twelve `material_choices` identifiers map in ordinal order to the complete
normative descriptions in contract section 20; neither the short identifiers
nor this decision abridge that contract. The packet authorizes no source,
manifest, lockfile, database, fixture, browser, provider, external effect,
commit, push, or implementation dispatch.

The product owner may record exactly one of:

> Accept AXP-E0002 Gate B exactly as frozen in D-E0002-012 at packet digest
> `sha256:eaff3d4d78ca3e6e4fe521f53b12b9598765db50ffd38fde0d6bf3aeb4c42dd4`,
> including every constituent artifact/semantic digest and material choice
> 1-12. Mark E0002-13 done. Make only E0002-05, E0002-08, and E0002-12 eligible
> for separate explicit dispatch; do not start later waves.

> Revise AXP-E0002 Gate B in D-E0002-012 as follows: [exact requested changes].
> Keep E0002-13 in review and every implementation task pending.

> Reject AXP-E0002 Gate B in D-E0002-012. Keep E0002-13 not done and every
> implementation task pending.

Until a dated owner decision cites the exact packet digest, Gate B remains
unchecked, E0002-13 remains review, and all implementation remains pending.

## D-E0002-013 — Product owner accepts Gate B and dispatches W3

Status: adopted / Gate B accepted · Date: 2026-09-01 · Decision owner: product
owner

After receiving D-E0002-012 and its validation/review results, the product
owner directed:

> approve and proceed

In the immediate decision context, this accepts AXP-E0002 Gate B exactly at
packet digest
`sha256:eaff3d4d78ca3e6e4fe521f53b12b9598765db50ffd38fde0d6bf3aeb4c42dd4`,
including every constituent artifact and semantic digest and material choice
1-12. It marks E0002-13 done. “Proceed” supplies the separate explicit dispatch
for only the dependency-ready W3 tasks E0002-05, E0002-08, and E0002-12 under
their frozen paths, budgets, tests, and serialized manifest/lock barrier.

This decision does not dispatch W4 or later work, permit contract/schema/
evaluator drift, authorize E0006 reuse, remote access, live provider work,
external effects, destructive work, release, commit, or push. Any change to a
Gate-B-bound artifact or material choice requires a revised digest-bound owner
decision.

## D-E0002-014 — W3 bounded retries stop at failed acceptance

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-01 · Decision owner: orchestrator

W3 obeyed the serialized root-manifest, kernel-manifest, and offline-lock
barrier before source fan-out. The three lanes then reached these independently
checked outcomes:

- E0002-12 is done. Its exact 16 valid envelopes, 105 rejection envelopes, 468
  typed recipe documents, artifact links, canonical digests, and 121/121 frozen
  seed derivations reproduce under the Gate-B artifacts.
- E0002-05 used its one permitted task-level retry after its initial independent
  review failed. The retry passes kernel formatting and 149 local tests, but
  final read-only review still found two frozen-scope blockers: post-permit
  `DispatchTokenCustody` can be consumed and dropped by caller-controlled
  validation before an atomic commit/forfeit settlement request exists, and
  `OperatorCursorCodec::open_page`/`seal_page` borrow `OperatorReadScope`
  instead of taking the frozen by-value parameter. Required adversarial and
  reverse-impact acceptance evidence is absent.
- E0002-08 used its one permitted task-level retry after its initial independent
  review failed. Its corrected source received a provisional static security
  PASS, but `rtk cargo test -p proof-operator-auth` exited 101 during test
  compilation: six owned calls pass `&Zeroizing<[u8; 32]>` where `&[u8]` is
  required, and the compiler reports two unused imports. No scoped test or
  reverse-impact acceptance exists.

Both implementation owners stopped without a post-failure source edit or
additional Cargo command. Their tasks are `blocked`; E0002-12 alone becomes
`done`. Local green tests do not override failed independent acceptance.
`rtk scripts/test-scoped.sh` is intentionally not run while the W3 candidate
cannot compile and satisfy its frozen contract.

This reconciliation grants no repair retry, source edit, command, contract or
digest change, W4 dispatch, provider/browser/external effect, commit, push,
Gate C, or release authority. W4 and every later wave remain pending and
non-dispatchable.

The product owner may now either keep W3 stopped or explicitly authorize one
exceptional final repair attempt for one or both blocked tasks. A bounded
authorization should preserve every Gate-B artifact and limit E0002-05 to
custody-safe atomic settlement, the by-value cursor signature, their direct
tests, with any downstream compile repair kept under existing crate ownership;
limit E0002-08 to the six owned test slice-borrow corrections and two unused-
import removals; require scoped format/tests, quiescent reverse-impact checks,
and fresh independent review; and stop again on any failure. No such
authorization is inferred from D-E0002-013.

## D-E0002-015 — Product owner authorizes exceptional final W3 repairs

Status: adopted / narrowly dispatchable · Date: 2026-09-01 · Decision owner:
product owner

After receiving D-E0002-014 and the exact failed-acceptance evidence, the
product owner directed:

> Authorize one exceptional final repair attempt for E0002-05 and E0002-08
> under D-E0002-014, limited exactly to the recorded blockers. Preserve all
> Gate B artifacts and digests; require scoped formatting/tests, quiescent
> reverse-impact checks, and fresh independent review; stop again on any
> failure. Keep W4 closed until both tasks are done.

This reactivates only E0002-05 and E0002-08. E0002-05 may repair custody-safe
atomic settlement, the frozen by-value `OperatorCursorCodec` scope signature,
and their direct adversarial/recording tests within its existing paths. Any
downstream compile repair remains under existing crate ownership. E0002-08 may
make only the six recorded owned test slice-borrow corrections and remove the
two recorded unused imports. Neither task may change a Gate-B artifact, digest,
manifest dependency, lockfile resolution, public behavior outside the frozen
contract, or another owner's path.

The W3 Cargo barrier remains in force. E0002-08 may make its mechanical source
edits concurrently but runs no Cargo command until E0002-05 reports source and
local Cargo quiescence. Reverse-impact checks begin only after both writers are
source-quiescent and their local scoped checks pass. Fresh independent review
must accept each result before either task becomes done. Any failed edit scope,
format, compile, test, impact check, or review stops both lanes for a new owner
decision.

This decision does not dispatch W4 or later work, reopen E0002-12, modify Gate
B, authorize provider/browser/external effects, permit E0006 reuse, commit,
push, Gate C, or release.

## D-E0002-016 — Exceptional W3 attempt stops on format failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-01 · Decision owner: orchestrator

E0002-05 made only the two substantive repairs authorized by D-E0002-015 and
their direct kernel tests. Its first mandated acceptance command,
`rtk cargo fmt --check -p proof-kernel`, exited 1 because rustfmt requires the
`control_digest_serialized("Proof-Operator-Runtime-Failure-v1", &failure)`
call in `operator/prepared.rs` to be reflowed. The owner made no post-failure
edit, ran no unit or reverse-impact test, did not update another owner's path,
and did not signal `W3 kernel exceptional quiescent`.

E0002-08 had already made exactly its six authorized explicit slice-borrow
test corrections and two unused-import removals. It ran no Cargo command: the
kernel quiescence signal never released its barrier. It made no further edit
and did not signal auth quiescence.

D-E0002-015 explicitly requires both lanes to stop on any format failure.
Therefore both tasks return to `blocked`; their current changes are candidate
work without acceptance evidence. No reverse-impact check or fresh independent
review is authorized or meaningful at this state. E0002-12 remains done, and
W4 plus every later wave remain pending and non-dispatchable.

This record grants no formatting repair, Cargo command, source edit, Gate-B
change, later-wave dispatch, provider/browser/external effect, E0006 reuse,
commit, push, Gate C, or release. Continuing requires a new product-owner
decision that explicitly authorizes the one rustfmt-required kernel reflow,
then the remaining kernel local checks, the auth local checks after the exact
kernel quiescence signal, quiescent reverse-impact checks, and fresh independent
reviews, with another stop on any failure.

## D-E0002-017 — Product owner authorizes serialized W3 acceptance continuation

Status: adopted / narrowly dispatchable · Date: 2026-09-01 · Decision owner:
product owner

After receiving D-E0002-016, the product owner directed:

> Authorize one D-E0002-016 continuation limited to the rustfmt-required kernel
> reflow and the remaining acceptance sequence: kernel format/tests, then auth
> format/tests after kernel quiescence, then quiescent reverse-impact checks
> and fresh independent reviews. Preserve Gate B and keep W4 closed; stop on
> any failure.

This authorizes E0002-05 to make only rustfmt's recorded reflow of the
`control_digest_serialized("Proof-Operator-Runtime-Failure-v1", &failure)` call
and then run its scoped format and unit checks. It authorizes no other kernel
source change. Only after the exact `W3 kernel D017 quiescent` signal may
E0002-08 run its scoped format and unit checks; it may make no additional auth
source edit. Only after the exact `W3 auth D017 quiescent` signal may the
orchestrator run the two quiescent reverse-impact checks serially and obtain
fresh independent reviews.

Any edit-scope, format, compile, unit, impact, or independent-review failure
stops the sequence immediately for another owner decision. Passing local checks
does not mark either task done. This decision preserves every Gate-B artifact
and digest and does not dispatch W4 or later work, modify manifests or lockfile,
authorize provider/browser/external effects, permit E0006 reuse, commit, push,
Gate C, or release.

## D-E0002-018 — Serialized W3 continuation stops on kernel test compile

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-01 · Decision owner: orchestrator

E0002-05 applied exactly the rustfmt-required reflow authorized by
D-E0002-017. `rtk cargo fmt --check -p proof-kernel` then passed. The next
mandated command, `rtk cargo test -p proof-kernel`, exited 101 during
compilation with E0277 in the new adversarial test near
`operator/prepared.rs:1694`: `.unwrap_err()` requires its success type
`RuntimeFailureRequest<'_>` to implement `Debug`, which that secret-bearing
request intentionally does not implement.

The kernel owner made no post-failure correction, ran no reverse-impact check,
and did not signal `W3 kernel D017 quiescent`. Consequently E0002-08 ran no
Cargo command and made no additional edit; its eight D-E0002-015 mechanical
changes remain uncompiled and untested.

D-E0002-017 requires a stop on any compile failure. Both tasks therefore return
to `blocked`. No fresh independent review occurred. E0002-12 remains done, and
W4 plus every later wave remain pending and non-dispatchable.

This record grants no test assertion rewrite, Cargo command, source edit,
Gate-B change, later-wave dispatch, provider/browser/external effect, E0006
reuse, commit, push, Gate C, or release. Continuing requires a new
product-owner decision authorizing only a test assertion that inspects the
error without requiring `RuntimeFailureRequest<'_>: Debug`, followed by the
remaining D-E0002-017 serialized local checks, quiescent reverse-impact checks,
and fresh independent reviews, with another stop on any failure.

## D-E0002-019 — Product owner authorizes test-only W3 continuation

Status: adopted / narrowly dispatchable · Date: 2026-09-01 · Decision owner:
product owner

After receiving D-E0002-018, the product owner directed:

> Authorize one D-E0002-018 continuation limited to replacing the adversarial
> test's `.unwrap_err()` with an equivalent error assertion that imposes no
> `Debug` bound, then resume the remaining kernel/auth checks, quiescent
> reverse-impact checks, and fresh independent reviews. Preserve Gate B, keep
> W4 closed, and stop on any failure.

This authorizes E0002-05 to change only that one adversarial test assertion in
`operator/prepared.rs`, preserving its exact expected error while avoiding a
`Debug` requirement on `RuntimeFailureRequest<'_>`. It then runs kernel scoped
format and unit checks. Only after the exact `W3 kernel D019 quiescent` signal
may E0002-08 run scoped format and unit checks; E0002-08 may make no additional
source edit. Only after the exact `W3 auth D019 quiescent` signal may the
orchestrator run both reverse-impact checks serially and obtain fresh
independent reviews.

Any edit-scope, format, compile, unit, impact, or independent-review failure
stops the sequence immediately for another owner decision. Passing local checks
does not mark either task done. This decision preserves every Gate-B artifact
and digest and does not dispatch W4 or later work, modify manifests or lockfile,
authorize provider/browser/external effects, permit E0006 reuse, commit, push,
Gate C, or release.

## D-E0002-020 — W3 acceptance completes; W4 remains closed

Status: recorded / W3 complete / owner dispatch required · Date: 2026-09-01 ·
Decision owner: orchestrator

The D-E0002-019 serialized sequence completed without a product failure:

- E0002-05 format and 151 scoped kernel tests pass. Its quiescent reverse-impact
  check passes 715 tests across 13 impacted packages and 53 suites. The initial
  sandbox execution was blocked only by CLI filesystem/process permissions;
  the exact host-permission rerun passed. Fresh independent source review
  passes custody-safe conversion, commit-barrier invalid prepared handling,
  adversarial coverage, exact by-value cursor signatures, prior accepted
  surfaces, all eleven Gate-B artifact hashes, and manifest/lock stability.
- E0002-08 format and 26 scoped auth tests pass. Its quiescent one-package
  reverse-impact check passes 26 tests across two suites. Fresh independent
  source review passes strict UUIDv7/UTC wire handling, zeroizing and redacted
  borrow-only secrets, consuming constant-work auth-first/race behavior,
  correctly bounded disposable-workspace evidence, no E0006 authority, exact
  six-slice/two-import repair scope, all frozen hashes, and no manifest/lock/API
  or behavior drift.
- E0002-12 remains done with its independently reproduced frozen corpus.

E0002-05 and E0002-08 become `done`; therefore every W3 task is done and
quiescent. No Gate-B artifact or digest changed.

This completion record does not dispatch W4. D-E0002-013 dispatched only W3,
and D-E0002-015/D-E0002-017/D-E0002-019 explicitly kept W4 closed. E0002-06,
E0002-07, and E0002-11 are dependency-ready but remain `pending` and
non-dispatchable until a separate product-owner decision activates exactly
those tasks under their frozen paths, budgets, stop gates, and serialized W4
root/storage manifest and offline-lock barrier. No later wave, provider/browser
external effect, E0006 reuse, commit, push, Gate C, or release is authorized.

## D-E0002-021 — Product owner dispatches only W4

Status: adopted / W4 dispatch · Date: 2026-09-01 · Decision owner: product
owner

After receiving D-E0002-020, the product owner directed:

> Dispatch only W4 tasks E0002-06, E0002-07, and E0002-11 under their frozen
> packets and Gate B. Enforce the serialized W4 root/storage-manifest and
> offline-lock barrier before source fan-out; require scoped and reverse-impact
> tests, handoffs, and independent reviews. Do not dispatch W5 or later.

This activates exactly E0002-06 storage, E0002-07 runtime, and E0002-11 control
within their existing writable paths, budgets, acceptance rules, and one-retry
stop gates. The pre-source barrier is mandatory:

1. E0002-11 writes only the exact root rustix/member entries, complete control
   package manifest, and inert two-line control `src/lib.rs` scaffold; it runs
   no Cargo command and signals `W4 root ready`.
2. E0002-06 then adds only `rustix.workspace = true` to the storage package
   manifest without source work or Cargo and signals
   `W4 storage manifest frozen`.
3. With all W4 source trees quiescent, E0002-11 alone runs
   `rtk cargo check -p proof-operator-control -p proof-storage --offline`,
   changes only the lockfile as required by that exact frozen graph, and
   signals `W4 lock stable`.
4. Only then may the three owners fan out source work or run another Cargo
   command. Any later dependency change stops all writers and repeats the full
   barrier.

Local scoped checks may run after lock stability. Reverse-impact checks wait
until all W4 writers are quiescent and run serially. Each result requires a
durable handoff and fresh independent review before becoming done.

This decision does not alter Gate B, dispatch W5 or later, authorize live
provider/tool/browser/external effects, permit E0006 reuse, commit, push, Gate
C, or release.

## D-E0002-022 — Proposed bounded kernel integration repair for W4 storage

Status: proposed / owner decision required / non-dispatchable · Date:
2026-09-01 · Decision owner: product owner

After `W4 lock stable`, E0002-06 stopped on a real missing-kernel-API boundary.
Fresh lease claim, reclaim, and begin-dispatch requests hold noncloneable raw
custody and expose only constant-work verification against an already-known
digest. For the first persisted claim/reclaim/dispatch, no expected digest yet
exists. Storage cannot lawfully derive it because raw-token access and a
storage-local crypto seam are both forbidden.

A distinct read-only kernel audit confirms that Gate B already requires claim,
reclaim, and begin-dispatch transactions to derive and persist these nonsecret
digests. It found no existing lawful path and no non-kernel Rust call site. The
exact additive repair is:

- `LeaseClaimRequest::lease_token_digest(&self) -> ControlDigest`;
- `ReclaimRequest::new_lease_token_digest(&self) -> ControlDigest`; and
- `BeginDispatchRequest::dispatch_token_digest(&self) -> ControlDigest`.

Kernel implements them through private infallible proof `digest()` helpers
using the already-frozen `Proof-Operator-Lease-Token-v1` and
`Proof-Operator-Dispatch-Token-v1` domains, retains every existing constant-
work verifier, and adds exact known-token/verifier/wrong-domain tests to the
three existing custody paths. E0002-06 then consumes only those accessors when
persisting the initial lease, replacement lease, and dispatch reservation/
permit.

This repair changes no serialized shape, Gate-B artifact/digest, dependency,
manifest, lockfile, or existing call site; therefore it does not repeat the W4
manifest/lock barrier. Because runtime and control compile the shared kernel,
the kernel edit must nevertheless wait until E0002-07 and E0002-11 report local
source quiescence. E0002-06 remains stopped in the interim; its partial work is
unaccepted. E0002-07 and E0002-11 may finish their storage-independent local
scopes under D-E0002-021.

The product owner may authorize exactly:

> After E0002-07 and E0002-11 are source-quiescent, reopen E0002-05 only for
> the three D-E0002-022 kernel-derived `ControlDigest` accessors and their exact
> direct tests. Preserve Gate B and all manifests/lockfiles. Require scoped
> kernel format/tests and a focused independent review; stop on any failure.
> If accepted, resume E0002-06 only to consume those accessors and finish its
> frozen task, then run quiescent W4 impact checks and reviews. Keep W5 closed.

Until that exact or equivalent bounded decision is recorded, no kernel or
storage continuation is authorized. This proposal does not dispatch W5 or
later, authorize external effects, E0006 reuse, commit, push, Gate C, or
release.

## D-E0002-023 — Proposed consolidated W4 continuation

Status: proposed / owner decision required / non-dispatchable · Date:
2026-09-01 · Decision owner: product owner

All W4 writers are now source-quiescent:

- E0002-06 remains blocked on the exact D-E0002-022 three-accessor kernel gap.
  Its migration-14 and partial lifecycle work is preserved but unaccepted.
- E0002-07 format and 133 scoped runtime tests pass. It has no kernel gap or
  cross-owner edit. Reverse-impact and review intentionally wait until the
  shared kernel is stable.
- E0002-11 exhausted its bounded source retry. Format passes, but scoped test
  compilation reports four owned mechanical mismatches: keyed BLAKE3 needs an
  exact `[u8; 32]` key reference, kernel canonicalization needs a parsed JSON
  `Value` rather than a string, base64 input needs an explicit byte slice, and
  UUIDv7 construction needs an exact `[u8; 10]` random array reference. No test
  ran after the failure.

The product owner may authorize exactly:

> Authorize the consolidated D-E0002-023 W4 continuation. First reopen
> E0002-05 only for the three D-E0002-022 nonsecret kernel-derived
> `ControlDigest` accessors and exact direct tests; change no Gate-B artifact,
> manifest, or lockfile. Require kernel format/tests and focused independent
> review, stopping on any failure. After that kernel API is accepted, resume
> E0002-06 only to consume the accessors and finish its frozen storage task,
> and resume E0002-11 only for its four recorded owned compile corrections and
> remaining frozen control checks. Keep E0002-07 source unchanged. When all
> three W4 lanes are locally green and quiescent, run serial reverse-impact
> checks and fresh independent reviews. Stop the affected continuation on any
> failure. Keep W5 and later waves closed.

Because this repair adds no dependency, the already-passed W4 manifest/lock
barrier does not repeat. The kernel step runs first while all W4 source remains
quiescent; storage and control may resume only after its scoped checks and
focused review pass. E0002-07 remains read-only throughout the shared-kernel
repair.

Until a product-owner decision records this or an equivalent exact boundary,
E0002-05 stays done, E0002-06/E0002-11 stay blocked, E0002-07 stays in review,
and no source edit or Cargo command is authorized. W5/later dispatch, Gate-B
drift, external effects, E0006 reuse, commit, push, Gate C, and release remain
unauthorized.

## D-E0002-024 — Product owner authorizes consolidated W4 continuation

Status: adopted / narrowly dispatchable · Date: 2026-09-02 · Decision owner:
product owner

After receiving D-E0002-023, the product owner directed:

> Authorize the consolidated D-E0002-023 W4 continuation. First reopen
> E0002-05 only for the three D-E0002-022 nonsecret kernel-derived
> `ControlDigest` accessors and exact tests. Require kernel format/tests and
> focused review. Then resume E0002-06 to consume those accessors and finish
> storage, and E0002-11 only for its four recorded compile corrections and
> remaining checks. Keep E0002-07 source unchanged. After local quiescence, run
> serial reverse-impact checks and fresh reviews. Stop the affected
> continuation on any failure. Keep W5 closed.

All W4 writers were source-quiescent before this decision. E0002-05 may add
only `LeaseClaimRequest::lease_token_digest`,
`ReclaimRequest::new_lease_token_digest`, and
`BeginDispatchRequest::dispatch_token_digest`, backed by private infallible
proof digest helpers using the two already-frozen token domains, while
retaining every constant-work verifier. It adds only the exact known-token,
verifier-agreement, and wrong-domain tests recorded in D-E0002-022. It changes
no serialized shape, Gate-B artifact/digest, manifest, lockfile, or dependency.

E0002-05 must pass scoped format/tests and a focused independent review before
the orchestrator emits `W4 kernel digest API accepted`. Only after that signal
may E0002-06 resume its frozen task and consume the three accessors, or E0002-11
make its four recorded fixed-array/strict-JSON/base64/UUID compile corrections
and resume its frozen local checks. E0002-07 remains source-read-only.

After all three W4 lanes report local quiescence, the orchestrator runs
reverse-impact checks serially for the shared kernel and each W4 crate, then
obtains fresh independent W4 reviews. An edit-scope, format, compile, test,
impact, or review failure stops the affected continuation for another owner
decision.

No manifest/lock barrier repeat is needed because no dependency changes. This
decision preserves Gate B and does not dispatch W5 or later, authorize external
effects, E0006 reuse, commit, push, Gate C, or release.

## D-E0002-025 — Proposed exhaustive W4 acceptance repair packet

Status: proposed / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: product owner

D-E0002-024 completed and independently accepted the exact three-accessor
custody-digest kernel seam. All eleven frozen artifact hashes and the Gate-B
packet digest still reproduce, and no manifest/lock/dependency change occurred.
The three W4 lanes then reached local quiescence, but exhaustive independent
review rejected each implementation. Green regression counts are not treated
as product acceptance.

### Kernel prerequisite for runtime

A read-only cross-owner audit found no existing typed end-to-end path for the
frozen `result_invalid` outcome and confirmed that runtime's local
`AggregateBudgetStore` duplicates a helper owned by kernel. Before any W4 lane
resumes, E0002-05 requires this exact additive repair:

1. Add `governed_runtime_failure_code(&ExecutionError) -> RuntimeFailureCode`.
   Normalize kernel-created missing usage, wrong reporter branch, invalid
   governed output, and over-ceiling usage to existing typed
   `ExecutionError::EvidenceFailed`; map that governed-result variant to
   `ResultInvalid`, while every ordinary handler failure—including misleading
   text containing “report” or “result”—maps to `HandlerFailed`.
2. Add capability-owned `GovernedAdapterReporter::missing_usage_error()` so an
   adapter can represent absent usage without string classification.
3. Enhance the kernel-owned `RecordingOperatorControlStore` with
   nonserializing `RecordingOperatorRequest` projections, `requests()`, and a
   boundary-keyed thread-safe responder hook. Capture command, claim/reclaim,
   reserve, begin, commit, and failure requests using only IDs, workspace/run/
   lease/process/fence/control bindings, derived token digests, reservation/
   permit IDs, budget amounts, prepared/dispatch match, failure code, error
   digest, and the custody-sealed five-dimensional intent ceiling. Never expose
   raw tokens or secret-bearing request bodies.
4. Add direct classification, projection accuracy, raw-token-absence,
   responder-inspection, and concurrent-responder tests. No store trait
   signature, global `ExecutionError` variant, serialized shape, dependency,
   manifest, lockfile, schema, evaluator, or Gate-B artifact may change.

Kernel format/tests and a focused independent review must pass before the
orchestrator signals `W4 kernel runtime seams accepted` and reopens any W4
source.

### E0002-06 storage repair boundary

E0002-06 format and 135 scoped tests pass, but the new operator behavior is
mostly unimplemented or untested. Its exact remaining boundary is:

1. Remove or close the public unguarded migration-14 path so schema 14 can be
   applied only by the held-lock guarded upgrader. Add fresh, 13-to-14,
   reopen, public rollback rejection, and populated child-before-parent down-
   SQL tests without changing migrations 1-13 or the frozen SQL.
2. Use one unduplicated retained lock descriptor; enforce exact sidecar mode
   and final sidecar safety; fail existing-only open without a valid policy;
   descriptor-load and verify Human/Agent public tuples; cover exclusive
   creation, exact retry, contention, interrupted 14-without-policy recovery,
   mismatch, movement, and sidecar failures.
3. Strictly reconstruct and validate the full singleton policy, Human
   enrollment/capability digest, five-dimensional budget account, duplicate
   SQL/JSON columns, stable provision binding, and audit head.
4. Validate exact-existing run control/projection and guard legacy governed-run
   writes. Maintain append-only projections and authority audits on every
   command/runtime transition.
5. Implement every auth-filtered attention/detail/approval/command/audit read,
   redaction, latest snapshot, exact ordering, filter digest, keyset boundary,
   scope binding, and cursor open/seal behavior under concurrent inserts.
6. Implement durable command digest/scope validation, atomic claim, exact
   replay/conflict, approval signing and approve/deny, cancel, explicit resume,
   session revoke, receipts/proofs/projections, and frozen audit sequences.
7. Implement completed replay joins, strict result decode/schema validation,
   same-transaction actor loading and full Proof verification, including
   deadline/exhaustion success and malformed/catalog/actor/proof failures.
8. Complete all reclaim branches, expiry/checkpoint/actionability checks,
   reservation release or full forfeit, directive create/reuse, replay failure,
   projections, and audit ordering.
9. Use only the frozen budget-reservation digest domain; enforce normal versus
   recovery rules, consumed directive/source reservation, existing-open
   reservation conflicts, required rejection audits, and full begin-dispatch
   catalog/run/step/checkpoint/cancel/replay checks.
10. Implement valid runtime commit and failure settlement: dispatch-token/CAS,
    sealed usage and trusted duration, five-dimensional commit/release or full
    forfeit, replay state, result/proof/run/step/checkpoint/event/evaluation/
    approval persistence, projection/audit updates, and active-dispatch clear.
11. Add reopen round trips for every new public function and deterministic
    wrong-token, idempotency, race, cancel/resume, lease/reclaim, stale-fence,
    restart, five-dimensional contention, replay-substitution, pagination, and
    lifecycle tests. Existing exact migration SQL and accepted digest-accessor
    use remain unchanged.

### E0002-07 runtime repair boundary

E0002-07 format and 133 scoped tests pass, but review rejected its acceptance
evidence and one exactness defect. Its exact remaining boundary is:

1. Use existing command-store APIs to prove cancel before provider, governed
   tool, and result commit; explicit resume of the exact checkpoint; concurrent
   cancel/resume and duplicate-resume one-winner receipts/idempotency.
2. Remove the runtime-local trait fake. Use the accepted kernel recorder and
   responder projections to prove same-run claim/reclaim, every recovery
   branch, raw custody/fence/control revision, and stale reserve/begin/commit/
   write rejection.
3. Exercise all five aggregate dimensions across concurrent runs and prove
   reserved/committed/full-forfeit/failure/recovery accounting.
4. Replace all error-string matching with the typed kernel classifier. Cover
   missing, wrong-kind, and over-ceiling provider/tool reports, exact
   `result_invalid` failure body/error digest, custody-bound full-ceiling
   targeting, successful tool reporting, and independent provider/tool/
   external-effect counters.
5. Add deadline-equality and exhausted-capacity replay bypass, exact second-
   lookup race/miss, genuinely verified prior proof and corruption failures.
   Adopt the store-returned control revision on every valid non-authorized
   begin outcome before releasing the reservation.
6. Keep all product work in runtime source/tests/handoff, with no storage,
   transport, manifest, provider, tool, or external-effect change.

### E0002-11 control repair boundary

E0002-11's four D-E0002-024 compile corrections and format pass; eight of nine
tests pass. The listener bind failure is most likely restricted-network
sandbox infrastructure and should be rerun unchanged with socket permission,
but that cannot substitute for these independently found source blockers:

1. Add one fail-closed startup/preflight composition that orders workspace,
   static integrity, TTY/Human, randomness/auth, store opener, loopback bind,
   and authority publication, passing one OS environment throughout.
2. Enforce actual peer, exact Host, target length, auth-first session and least
   capability, rate limit, mutation Origin/media/body bounds, and generic error
   ordering before injected business handlers. Keep the frozen 15-route
   inventory and exclude all legacy fallbacks.
3. Assemble challenge/exchange/protected/self-revoke/static delivery on the
   exact OS-selected origin; bind session creation and restart invalidation to
   that instance without E0006 authority.
4. Accept independently frozen manifest digests and validate static bytes,
   digest-bearing names, media, and closed inventory before bind; never
   self-certify expected hashes from the same supplied bytes.
5. Make the forbidden repository `.proof` build anchor non-user-configurable
   and require its exact descriptor boundary; expose no caller-selected trust
   implementation or fallback.
6. Add TTY timeout, consume-on-every-failure, restoration-error handling,
   fail-closed signal behavior, and production descriptor-backed Human signer
   tuple revalidation/zeroization.
7. Couple listener stop/drain to session/cursor/lease/dispatch/signer cleanup;
   guarantee store close and lock release on intermediate errors; prove control
   restart changes instance/cursor/auth and rejects old sessions.
8. Add deterministic Tower/process tests for bind/port failure, peer/Host/
   Origin/media/body/rate limits, challenge concurrency/replay/malformed/
   expiry/lost response, revoke/expiry/reload/restart, SIGINT/SIGTERM, complete
   security headers, and zero callback/provider/tool/external effects.
9. Strip forbidden response headers from injected and framework/parser errors,
   apply exact CSP/security headers on every response, and prove absence of
   cookie/permissive-CORS/cache leakage.

Control remains confined to `crates/proof-operator-control/**` and its handoff;
it may not add dependencies, change root/lock, import storage/runtime/general
HTTP/UI, implement downstream business handlers, or weaken the real socket
test.

### Proposed execution order

The product owner may authorize exactly:

> Authorize D-E0002-025 exactly as bounded. First reopen E0002-05 only for the
> typed governed-failure classifier, capability-owned missing-usage error, and
> nonsecret recording-store request/responder seam plus direct tests. Require
> kernel format/tests and focused independent review; stop on any failure.
> After the exact kernel acceptance signal, resume E0002-06, E0002-07, and
> E0002-11 in parallel only for their enumerated blocker sets. Preserve Gate B,
> manifests, lockfile, task paths, and zero external effects. Require local
> scoped checks, then serial quiescent reverse-impact checks and fresh
> independent reviews; stop each affected lane on any failure. Keep W5 and
> later waves closed.

All source remains quiescent until such a decision. No manifest/lock barrier
repeat is required because the packet permits no dependency change. This
proposal does not alter Gate B, authorize live provider/tool/browser effects,
permit E0006 reuse, commit, push, Gate C, or release.

## D-E0002-026 — Product owner authorizes exhaustive W4 repair sequence

Status: adopted / kernel prerequisite dispatch · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-025, the product owner directed:

> Authorize D-E0002-025 exactly as bounded. First reopen E0002-05 only for the
> typed governed-failure classifier, capability-owned missing-usage error, and
> nonsecret recording-store request/responder seam plus direct tests. Require
> kernel format/tests and focused independent review; stop on any failure.
> After the kernel acceptance signal, resume E0002-06, E0002-07, and E0002-11
> in parallel only for their enumerated blocker sets. Preserve Gate B,
> manifests, lockfile, task paths, and zero external effects. Require local
> scoped checks, serial quiescent reverse-impact checks, and fresh independent
> reviews. Stop each affected lane on failure. Keep W5 closed.

This activates E0002-05 alone for the exact D-E0002-025 kernel prerequisite.
It may add only the typed governed failure classifier, the capability-owned
missing-usage constructor, nonserializing/nonsecret recording request
projections, request inspection, the boundary-keyed thread-safe responder, and
their direct classification/projection/secrecy/concurrency tests. Existing
store trait signatures and global `ExecutionError` variants remain unchanged.
No raw token or secret-bearing request may be exposed.

E0002-05 must pass scoped format/tests and focused independent review before
the orchestrator emits `W4 kernel runtime seams accepted`. E0002-06,
E0002-07, and E0002-11 remain source-read-only until that signal. Only then may
they resume, in parallel, within every enumerated storage/runtime/control
blocker in D-E0002-025. Each lane stops independently on its first edit-scope,
format, compile, test, impact, or review failure.

When all three W4 writers are locally green and quiescent, the orchestrator
runs shared-kernel and W4 reverse-impact checks serially and obtains fresh
independent reviews. No manifest/lock barrier repeats because no dependency
change is authorized.

This decision preserves every Gate-B artifact and digest, W4 path ownership,
and zero external effects. It does not dispatch W5 or later, authorize E0006
reuse, live provider/tool/browser effects, commit, push, Gate C, or release.

## D-E0002-027 — Exhaustive W4 repair stops on kernel format failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

E0002-05 drafted only the D-E0002-026 typed governed-failure classifier,
capability-owned missing-usage error, nonserializing/nonsecret recording-store
request projections and responder seam, plus their direct tests. Its first
mandated acceptance command, `rtk cargo fmt --check -p proof-kernel`, exited 1
with exactly two rustfmt-only diffs:

1. add a trailing comma after `Uuid::now_v7()` near
   `crates/proof-kernel/src/executor/engine.rs:1709`; and
2. collapse the `ReserveAggregateBudget` `request_response(...)` call near
   `crates/proof-kernel/src/operator/store.rs:2623`.

The kernel owner made no post-failure edit, ran no scoped unit or reverse-
impact test, did not request focused independent review, and did not emit
`W4 kernel runtime seams accepted`.

D-E0002-026 requires a stop on any format failure. E0002-05 therefore returns
to `blocked`; its current changes are candidate work without acceptance
evidence. E0002-06, E0002-07, and E0002-11 never received the kernel acceptance
signal, made no D-E0002-026 continuation edit, and remain source-frozen and
blocked. No W4 reverse-impact check or fresh lane review is authorized or
meaningful in this state. W5 and all later waves remain pending and closed.

This record grants no formatting repair, Cargo command, source edit, Gate-B
change, manifest or lockfile change, W4 or later-wave dispatch, external
effect, E0006 reuse, commit, push, Gate C, or release. Continuing requires a
new product-owner decision that explicitly authorizes only the two recorded
rustfmt changes followed by kernel format/tests and focused independent
review. The D-E0002-025 storage/runtime/control continuations may begin only
after that exact kernel acceptance signal, with the same local checks, serial
quiescent reverse-impact checks, fresh independent reviews, and stop-on-failure
rules.

## D-E0002-028 — Product owner authorizes exact kernel formatting continuation

Status: adopted / narrowly dispatchable · Date: 2026-09-02 · Decision owner:
product owner

After receiving D-E0002-027, the product owner directed:

> Approve and proceed.

In the immediately presented D-E0002-027 context, this authorizes E0002-05 to
apply only the two recorded rustfmt changes: add the trailing comma after
`Uuid::now_v7()` near `executor/engine.rs:1709`, and apply rustfmt's exact
reflow of the `ReserveAggregateBudget` `request_response(...)` call near
`operator/store.rs:2623`. No other kernel source or test change is authorized.

After those two mechanical edits, E0002-05 must run
`rtk cargo fmt --check -p proof-kernel` and, only if it passes,
`rtk cargo test -p proof-kernel`. Only if both pass may the orchestrator obtain
a fresh focused independent review of the entire D-E0002-025/D-E0002-026
kernel prerequisite. Any edit-scope, format, compile, unit-test, or review
failure stops the sequence immediately for another owner decision.

Only a focused-review PASS permits the orchestrator to emit
`W4 kernel runtime seams accepted` and resume E0002-06, E0002-07, and E0002-11
in parallel under their exact D-E0002-025 blocker sets and D-E0002-026 stop
rules. Until then those lanes remain source-frozen and blocked. This decision
preserves Gate B, manifests, lockfile, task paths, zero external effects, and
the later serial quiescent reverse-impact/fresh-review sequence. W5 and all
later waves remain closed. It authorizes no E0006 reuse, live provider/tool/
browser effect, commit, push, Gate C, or release.

## D-E0002-029 — Exact kernel formatting continuation stops again

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

E0002-05 applied the D-E0002-028 trailing comma correctly and attempted only
the authorized `ReserveAggregateBudget` reflow. Its first mandated command,
`rtk cargo fmt --check -p proof-kernel`, nevertheless exited 1 because the
recorded description was under-specified: rustfmt requires the entire
`match self.request_response(OperatorStoreBoundary::ReserveAggregateBudget,
projection)? {` expression on one line.

The kernel owner made no post-failure edit, ran no scoped unit or reverse-
impact test, did not update its handoff, did not request focused independent
review, and did not emit `W4 kernel runtime seams locally green` or
`W4 kernel runtime seams accepted`.

D-E0002-028 requires a stop on any format failure. E0002-05 therefore returns
to `blocked`, and its candidate source remains unaccepted. E0002-06, E0002-07,
and E0002-11 never received the kernel acceptance signal, made no continuation
edit, and remain source-frozen and blocked. No reverse-impact check or fresh
lane review is authorized in this state. W5 and all later waves remain pending
and closed.

This record grants no further reflow, Cargo command, source edit, Gate-B
change, manifest or lockfile change, W4 or later-wave dispatch, external
effect, E0006 reuse, commit, push, Gate C, or release. Continuing requires a
new product-owner decision authorizing only rustfmt's now-exact one-line
`match self.request_response(...) {` reflow, followed by kernel format/tests
and focused independent review. Only the exact kernel acceptance signal may
then release the D-E0002-025 storage/runtime/control continuations under their
existing stop-on-failure and later serial-quiescence rules.

## D-E0002-030 — Product owner authorizes exact one-line kernel reflow

Status: adopted / narrowly dispatchable · Date: 2026-09-02 · Decision owner:
product owner

After receiving D-E0002-029, the product owner directed:

> Authorize one D-E0002-029 continuation limited to rustfmt's exact one-line
> reserve-request reflow, then kernel format/tests and focused independent
> review. Preserve all existing gates and stop on any failure.

This authorizes E0002-05 to change only the current reserve-budget recording
expression so rustfmt places the complete
`match self.request_response(OperatorStoreBoundary::ReserveAggregateBudget,
projection)? {` expression on one line. No other source, test, dependency,
manifest, lockfile, schema, evaluator, contract, or Gate-B artifact change is
authorized.

E0002-05 then runs `rtk cargo fmt --check -p proof-kernel` and, only on PASS,
`rtk cargo test -p proof-kernel`. Only if both pass may the orchestrator obtain
a fresh focused independent review of the full D-E0002-025/D-E0002-026 kernel
prerequisite. Any edit-scope, format, compile, unit-test, or review failure
stops the sequence immediately for another owner decision.

Only focused-review PASS permits the orchestrator to emit
`W4 kernel runtime seams accepted` and resume E0002-06, E0002-07, and E0002-11
under the exact D-E0002-025/D-E0002-026 boundaries. Until then all three lanes
remain source-frozen and blocked. Gate B, manifests, lockfile, task paths, zero
external effects, and the later serial quiescent reverse-impact/fresh-review
sequence remain unchanged. W5 and every later wave stay closed. This decision
authorizes no E0006 reuse, provider/tool/browser effect, commit, push, Gate C,
or release.

## D-E0002-031 — Exact kernel reflow passes format and stops on one test

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

E0002-05 applied only rustfmt's exact one-line reserve-request reflow under
D-E0002-030. `rtk cargo fmt --check -p proof-kernel` passed. The next mandated
command, `rtk cargo test -p proof-kernel`, exited 101 with 125 tests passed and
one failed:
`operator::prepared::tests::custody_binds_exact_lease_fence_and_permit_then_consumes_failure_path`.

The failing `assert!(!debug.contains("token:"))` near
`crates/proof-kernel/src/operator/prepared.rs:1996` is overbroad: it matches the
safe, nonsecret `replay_claim_token: None` field in the Begin request
projection. Read-only inspection confirms that the projection still contains
no raw lease, new-lease, or dispatch token field, and the adjacent assertion
continues to reject the raw `[9; 32]` custody bytes.

The kernel owner made no post-failure correction, ran no reverse-impact check,
did not update its handoff, did not request focused independent review, and
did not emit either kernel green/acceptance signal.

D-E0002-030 requires a stop on any test failure. E0002-05 therefore returns to
`blocked`, and its candidate source remains unaccepted. E0002-06, E0002-07,
and E0002-11 never received the kernel acceptance signal, made no continuation
edit, and remain source-frozen and blocked. W5 and every later wave remain
pending and closed.

This record grants no assertion rewrite, Cargo command, source edit, Gate-B
change, manifest or lockfile change, W4 or later-wave dispatch, external
effect, E0006 reuse, commit, push, Gate C, or release. A minimal continuation
would replace only the broad `"token:"` predicate with exact absence checks
for the raw field labels `"lease_token:"`, `"new_lease_token:"`, and
`"dispatch_token:"`, preserving the raw-byte assertion, then rerun kernel
format/tests and obtain focused independent review. Only the exact kernel
acceptance signal may release the D-E0002-025 storage/runtime/control
continuations under their existing stop-on-failure and serial-quiescence rules.

## D-E0002-032 — Product owner authorizes exact secrecy-assertion continuation

Status: adopted / narrowly dispatchable · Date: 2026-09-02 · Decision owner:
product owner

After receiving D-E0002-031, the product owner directed:

> Authorize one D-E0002-031 continuation limited to replacing the broad
> `"token:"` predicate with exact absence checks for raw lease, new-lease, and
> dispatch token field labels, preserving the raw-byte assertion; then rerun
> kernel format/tests and focused independent review. Preserve all gates and
> stop on any failure.

This authorizes E0002-05 to replace only the broad
`assert!(!debug.contains("token:"))` in the named custody/projection test with
exact absence checks for `"lease_token:"`, `"new_lease_token:"`, and
`"dispatch_token:"`. The adjacent raw `[9; 32]` byte-absence assertion must
remain unchanged. No production source, other test, dependency, manifest,
lockfile, schema, evaluator, contract, or Gate-B artifact change is authorized.

E0002-05 then runs `rtk cargo fmt --check -p proof-kernel` and, only on PASS,
`rtk cargo test -p proof-kernel`. Only if both pass may the orchestrator obtain
a fresh focused independent review of the full D-E0002-025/D-E0002-026 kernel
prerequisite, including the typed classification semantics and nonsecret,
thread-safe recording/responder seam. Any edit-scope, format, compile,
unit-test, or review failure stops the sequence immediately for another owner
decision.

Only focused-review PASS permits the orchestrator to emit
`W4 kernel runtime seams accepted` and resume E0002-06, E0002-07, and E0002-11
under the exact D-E0002-025/D-E0002-026 boundaries. Until then all three lanes
remain source-frozen and blocked. Gate B, manifests, lockfile, task paths, zero
external effects, and the later serial quiescent reverse-impact/fresh-review
sequence remain unchanged. W5 and every later wave stay closed. This decision
authorizes no E0006 reuse, provider/tool/browser effect, commit, push, Gate C,
or release.

## D-E0002-033 — Kernel runtime seams accepted; exact W4 repairs resume

Status: adopted / W4 repair dispatch · Date: 2026-09-02 · Decision owner:
orchestrator under D-E0002-025, D-E0002-026, and D-E0002-032

E0002-05 made only the assertion correction authorized by D-E0002-032.
`rtk cargo fmt --check -p proof-kernel` passed, and
`rtk cargo test -p proof-kernel` passed 153 tests across six suites. The owner
then emitted `W4 kernel runtime seams locally green` and stopped without a
reverse-impact check.

A fresh independent focused review returned PASS. It confirmed typed,
message-independent governed-failure classification; the capability-owned
missing-usage error; complete nonserializing/nonsecret request projections;
responder invocation outside locks and meaningful concurrency coverage; no
store-trait or global `ExecutionError` drift; and the exact D-E0002-032 raw-
field/raw-byte secrecy checks. The reviewer independently reproduced all 11
Gate-B artifact SHA-256 values, all five semantic digests, and packet digest
`eaff3d4d78ca3e6e4fe521f53b12b9598765db50ffd38fde0d6bf3aeb4c42dd4`.

The orchestrator therefore emits the exact acceptance signal:

`W4 kernel runtime seams accepted`

E0002-05 becomes `done`. Under the already adopted D-E0002-026 execution
order, E0002-06, E0002-07, and E0002-11 now resume in parallel only for their
complete, enumerated storage/runtime/control blocker sets in D-E0002-025.
They may edit only their frozen task paths, may not change dependencies,
manifests, lockfile, Gate-B artifacts, or other owners' sources, and must
preserve zero external effects. Each lane runs only its local scoped format/
tests and stops independently on its first edit-scope, format, compile, or
unit-test failure.

No reverse-impact command may run until all three writers are locally green,
stopped, and quiescent. The orchestrator then runs the kernel and W4 impact
checks serially and obtains fresh independent lane reviews, stopping the
affected continuation on any failure. W5 and every later wave remain pending
and closed. This record authorizes no E0006 reuse, provider/tool/browser
effect, commit, push, Gate C, or release.

## D-E0002-034 — W4 runtime lane stops on formatting; peers continue

Status: recorded / runtime owner decision required · Date: 2026-09-02 ·
Decision owner: orchestrator

E0002-07 completed its source/type audit and candidate implementation for the
six enumerated D-E0002-025 runtime blocker areas. Its first mandated local
command, `rtk cargo fmt --check -p proof-agent-runtime`, exited 1 with
rustfmt-only differences in `crates/proof-agent-runtime/src/operator.rs`:
import packing and mechanical chain, assertion, and closure reflow.

The runtime owner made no post-failure edit, ran no scoped unit or reverse-
impact test, did not update its handoff, and did not emit
`W4 runtime locally green`. Under D-E0002-033's stop-on-first-failure rule,
E0002-07 returns to `blocked`; its candidate source is unaccepted and frozen.

D-E0002-033 makes each W4 stop lane-local. E0002-06 storage and E0002-11
control therefore remain active inside their exact D-E0002-025 blocker sets.
No reverse-impact check may run while either writer remains active or while
runtime lacks local acceptance. W5 and all later waves remain closed.

This record grants no runtime formatting repair, Cargo command, source edit,
review, Gate-B change, manifest or lockfile change, later-wave dispatch,
external effect, E0006 reuse, commit, push, Gate C, or release. Continuing
E0002-07 requires a new product-owner decision explicitly authorizing only
rustfmt's recorded `operator.rs` reflow followed by runtime format/tests, then
eventual quiescent reverse-impact and fresh independent review under the
existing D-E0002-025/D-E0002-033 sequence, stopping again on any failure.

## D-E0002-035 — W4 control lane stops on formatting; storage continues

Status: recorded / control owner decision required · Date: 2026-09-02 ·
Decision owner: orchestrator

E0002-11 completed its source/API audit and candidate implementation for the
nine enumerated D-E0002-025 control blocker areas. Its first mandated local
command, `rtk cargo fmt --check -p proof-operator-control`, exited 1 with
formatting-only differences in `environment.rs`, `lifecycle.rs`, `listener.rs`,
`routing.rs`, `signer.rs`, `tests.rs`, and `workspace.rs`. The differences
include rustfmt line reflow and reversal of a manual short-import layout.

The control owner made no post-failure edit, ran no scoped unit or reverse-
impact test, did not update its handoff, and did not emit
`W4 control locally green`. Under D-E0002-033's stop-on-first-failure rule,
E0002-11 returns to `blocked`; its candidate source is unaccepted and frozen.

D-E0002-033 makes each W4 stop lane-local. E0002-06 storage therefore remains
active inside its exact D-E0002-025 blocker set. E0002-07 runtime remains
separately blocked under D-E0002-034. No reverse-impact check may run while
storage is active or while runtime/control lack local acceptance. W5 and all
later waves remain closed.

This record grants no control formatting repair, Cargo command, source edit,
review, Gate-B change, dependency, manifest or lockfile change, later-wave
dispatch, external effect, E0006 reuse, commit, push, Gate C, or release.
Continuing E0002-11 requires a new product-owner decision explicitly
authorizing only rustfmt's recorded seven-file control reflow followed by
control format/tests (with the real loopback test retaining host socket
permission), then eventual quiescent reverse-impact and fresh independent
review under the existing D-E0002-025/D-E0002-033 sequence, stopping again on
any failure.

## D-E0002-036 — W4 storage stops on frozen stale-fence profile contradiction

Status: recorded / Gate-B owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

During its required source/type audit, E0002-06 found that the frozen product
artifacts cannot represent one mandatory rejection event coherently:

1. `contracts/operator-control-plane.md:1047-1051`, `:1715-1717`, and
   `:1741-1743` require an authenticated operator-command fence mismatch to
   append exactly `stale_fence_rejected`; this is a command whose safely known
   target/fence is the audit authority.
2. The frozen kernel accepts `StaleFenceRejected` only with presence mask
   `0x16c30` at `crates/proof-kernel/src/operator/durable.rs:1529-1530`.
   That mask requires `server_instance_id`, `run_id`, `reservation_id`,
   `lease_id`, `process_epoch_id`, `permit_id`, and `fence_epoch`, while
   forbidding `human_id`, `session_id`, `command_id`, and `command_kind`.
3. The frozen reads schema branch at
   `schemas/operator-control/reads-v1.schema.json:2888` likewise requires the
   server/run/lease/process/fence profile and explicitly requires Human,
   session, command ID, and command kind to be null. Its reservation and permit
   fields remain only generically nullable, which also fails to encode the
   kernel's exact non-null requirement.
4. Approval, cancel, and resume commands carry an expected fence but need not
   own any reservation or dispatch permit. Storage therefore cannot append the
   contract-required event for every authenticated command fence mismatch
   without fabricating unrelated runtime authority, weakening the event policy,
   or changing frozen contract/schema/kernel artifacts.

AGENTS and D-E0002-033 require storage to stop on a needed cross-owner or
frozen-contract change. The owner made no edit after identifying the conflict,
ran no format, unit, or reverse-impact command, did not update its handoff, and
did not emit `W4 storage locally green`. E0002-06 returns to `blocked`; its
candidate source is unaccepted and frozen.

All W4 writers are now stopped. E0002-07 remains blocked under D-E0002-034 and
E0002-11 under D-E0002-035. No quiescent reverse-impact or fresh lane review is
authorized because none of the three lanes has local acceptance. W5 and every
later wave remain pending and closed.

This record does not choose a replacement audit profile and grants no Gate-B,
contract, schema, evaluator, kernel, storage, formatting, manifest, lockfile,
or later-wave change. It authorizes no tests, external effect, E0006 reuse,
commit, push, Gate C, or release. Continuing requires a product-owner decision
that first authorizes a narrowly scoped E0002-13/kernel/schema consistency
repair and refreshed Gate-B digest packet with independent review. Storage can
resume only after that packet is accepted and after a separate exact
continuation decision. Runtime and control formatting repairs remain separate
stopped lanes.

## D-E0002-037 — Proposed planning-only Gate-B stale-fence consistency repair

Status: proposed / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator proposal

Two fresh read-only reviews independently confirmed D-E0002-036. Reachability
counterexamples include waiting/idle runs with no reservation, cancel with no
reserved row, and a pre-dispatch `reserved` row whose migration-14 invariant
requires `permit_id = NULL`; none can satisfy kernel mask `0x16c30` without
fabrication. A third fresh adjudicator compared the proposed repairs against
the full contract, evaluator, five runtime stale-fence fixtures, schema,
kernel, and migration and selected the following narrow resolution with 0.92
confidence.

### Exact semantic correction

`stale_fence_rejected` remains a distinct durable event only for an
authenticated operator-command fence mismatch. It accepts exactly the existing
command-rejection profiles plus the submitted `expected_fence_epoch`:

| command branch | exact presence mask |
|---|---:|
| approval decision | `0x101e3` |
| run cancel | `0x101a3` |
| approval-branch run resume | `0x2101e3` |
| recovery-branch run resume | `0x4181a3` |

These profiles retain the safely known Human, session, run, proposed command
ID/kind, and approval/recovery branch attribution already frozen for
`command_rejected`; the session rule also requires the matching authority
digest. They add only the rejected command's expected fence. Reservation,
lease, process, permit, budget, proof, and every unrelated optional field are
null. Session revoke has no fence and gains no profile.

The obsolete runtime-shaped mask `0x16c30` is rejected. The contract's
unbackticked statement that a late authorized response is recorded as a
redacted stale-fence rejection is clarified as the typed store/runtime failure
observation, not an `AuditEvent`: section 10 makes an empty expected
`new_events` list prohibitive, section 13 limits the durable event to
authenticated commands, and all five materialized runtime stale-fence recipes
have `new_events: []`. Those five recipes and their case semantics remain
unchanged.

Because a fence-rejected command is not inserted, the contract's exhaustive
proposed-command-ID referential exception adds `stale_fence_rejected` beside
`command_rejected`, `command_conflict`, and command-scoped `control_failure`.
Migration 14 already permits this because `operator_audit_events.command_id`
is deliberately not a foreign key; neither migration SQL file changes.

### Planning-only artifact and digest closure

If authorized, E0002-13 may change only:

1. `contracts/operator-control-plane.md` for the exact distinction, four
   profiles, and proposed-command-ID exception;
2. `schemas/operator-control/reads-v1.schema.json` to replace its current
   runtime-shaped branch with four exact command branches;
3. `schemas/operator-control/manifest-v1.json` to refresh the reads hash;
4. `evals/operator-control-v1.json` to refresh its artifact ledger, affected
   assembly-bound fixture blueprints/case hashes, semantic digests, and raw
   policy hash; and
5. E0002 edition administration, handoff, validation evidence, and a new exact
   superseding Gate-B packet. D-E0002-012 and D-E0002-013 remain immutable
   historical decisions.

The Gate-B packet changes artifact ordinals 1, 4, 10, and 11 and receives a
new packet digest. `valid_scenarios` and `rejection_vectors` semantic digests
change because the new schema-manifest hash propagates through 28 embedded
assembly-bound setup blueprints. `checks`, `backend_subset`, and
`store_error_matrix` semantic digests remain byte-identical.

Only after E0002-13 freezes those source artifacts may E0002-12 mechanically
refresh the corresponding 28 setup recipe documents (two valid and 26
rejection cases), their setup/envelope/case hash links, all 121 wrapper
`policy_sha256` values, and `index-v1.json`. The five runtime stale-fence
wrappers change only their evaluator policy hash; their setup/action/mutation/
expected documents, case semantics, and `new_events: []` remain byte-identical.
No fixture ID, ordinal, count, seed, policy scenario, expected outcome, or data
recipe outside that derived closure may change.

Common, auth, mutations, durable, store, evaluator, and manifest-schema bytes;
migration 14 up/down; Cargo manifests; `Cargo.lock`; source code; dependencies;
and every other Gate-B artifact remain unchanged. The repair adds no new event
kind, HTTP behavior, live effect, or material product choice.

E0002-13 must run strict duplicate-aware schema/meta-schema/reference/manifest,
artifact, semantic-digest, and packet-digest validation. E0002-12 must run the
frozen corpus schema/order/cardinality/blueprint/digest-linkage/seed/secret-
sentinel validation. After all writers stop, three fresh independent read-only
reviews cover command/audit semantics, schema/manifest/packet closure, and
evaluator/fixture propagation. Any edit-scope, validation, or review failure
stops the repair. PASS produces an exact superseding Gate-B digest packet and
then stops for explicit product-owner acceptance.

This proposal authorizes no change. E0002-13 and E0002-12 remain done and
closed; E0002-05, E0002-06, E0002-07, E0002-11, W5, and every later wave remain
non-dispatchable. It grants no kernel/storage/runtime/control formatting or
source work, Cargo command, external effect, E0006 reuse, commit, push, Gate C,
or release.

The product owner may authorize exactly:

> Authorize D-E0002-037 as a planning-only Gate-B consistency repair. Reopen
> E0002-13 only for the exact four command-attributed
> `stale_fence_rejected` profiles and contract/reads/manifest/evaluator/packet
> digest closure, then E0002-12 only for the derived mechanical fixture
> propagation. Preserve migration 14, all other schemas and runtime fixture
> recipes, Cargo manifests, lockfile, and implementation source. Require exact
> validations and three fresh independent reviews; stop on any failure and
> return the superseding Gate-B digest packet for explicit acceptance. Do not
> dispatch kernel, storage, runtime, control, W5, or later work.

## D-E0002-038 — Product owner authorizes planning-only Gate-B repair

Status: adopted / planning-only serialized dispatch · Date: 2026-09-02 ·
Decision owner: product owner

After receiving D-E0002-037, the product owner directed:

> Authorize D-E0002-037 as a planning-only Gate-B consistency repair. Reopen
> E0002-13 only for the exact four command-attributed `stale_fence_rejected`
> profiles and contract/reads/manifest/evaluator/packet digest closure, then
> E0002-12 only for derived mechanical fixture propagation. Require exact
> validations and three fresh independent reviews; stop on any failure. Do not
> dispatch implementation or W5.

This reopens E0002-13 alone for exactly the D-E0002-037 semantic and digest
closure. It may edit only the operator-control contract, reads schema, schema
manifest, evaluator policy, its unique handoff, and orchestrator-owned E0002
administration. It freezes exactly four command-attributed
`stale_fence_rejected` profiles (`0x101e3`, `0x101a3`, `0x2101e3`, and
`0x4181a3`), rejects the obsolete `0x16c30` runtime audit profile, clarifies
that runtime stale-fence observations append no audit event, and adds this
event to the proposed-command-ID referential exception.

E0002-13 must preserve every non-enumerated artifact and recompute the exact
artifact, embedded case, semantic, evaluator, and packet digests described by
D-E0002-037. It performs strict duplicate-aware schema/meta-schema/reference/
manifest and digest validation without any Cargo command. On any edit-scope or
validation failure it stops. Only on PASS may it emit
`Gate B stale-fence repair artifacts frozen` and enter review.

E0002-12 remains closed until that exact signal. It may then update only the
derived mechanical fixture closure: the 28 assembly-bound setup documents and
their hash links, all 121 wrapper policy hashes, and the fixture index. It may
make no semantic recipe, seed, ID, order, count, expected-event, or policy
choice. Its frozen schema/order/cardinality/blueprint/digest/seed/secret scan
must pass; any failure stops the sequence.

After both writers stop and the artifacts are quiescent, three fresh
independent read-only reviews cover: command/audit semantics, schema/manifest/
packet closure, and evaluator/fixture propagation. Any review failure stops
for another owner decision. Three PASS results permit only an exact
superseding Gate-B packet to be returned for explicit product-owner acceptance;
they do not accept Gate B automatically.

This decision preserves migration 14, every other schema and runtime fixture
recipe, Cargo manifests, `Cargo.lock`, dependencies, and all implementation
source. It does not dispatch kernel, storage, runtime, control, W5, or later
work and authorizes no Cargo command, external effect, E0006 reuse, commit,
push, Gate C, or release.

## D-E0002-039 — Planning-only Gate-B repair stops at dependency validation

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

The orchestrator applied only the administrative activation needed to begin
D-E0002-038: E0002-13 changed from `done` to `active`, and E0002-12 from `done`
to `blocked`. Before any contract, schema, manifest, evaluator, fixture,
handoff, or implementation edit and before any worker dispatch, the mandated
edition validation command `rtk scripts/swarm.sh validate AXP-E0002` failed:

`editions/AXP-E0002/assignments.tsv: E0002-05 is done but dependency E0002-13 is not done`

The validator correctly forbids reopening a completed prerequisite in place
while its completed dependents remain done. D-E0002-038 requires a stop on any
validation failure. The planning repair therefore stopped immediately; no
artifact digest changed, no E0002-12 barrier signal existed, and none of the
three independent repair reviews was dispatched.

The orchestrator restored E0002-13 and E0002-12 to their historical `done`
assignment states so the edition graph again represents the last valid closed
state. This restoration does not resolve D-E0002-036 or make the historical
Gate-B packet suitable for resumed implementation. E0002-06, E0002-07, and
E0002-11 remain blocked; W5 and every later wave remain closed.

This record grants no graph repair, new task, status cascade, Gate-B artifact,
fixture, kernel, storage, runtime, control, Cargo, manifest, lockfile, external
effect, E0006 reuse, commit, push, Gate C, or release change. Continuing
requires a new product-owner decision authorizing an additive planning-only
repair task after historical E0002-13, plus a distinct derived-fixture refresh
task after it, instead of reopening completed prerequisites in place. The
exact D-E0002-037 semantic boundary, validation/review gates, and prohibition
on implementation remain unchanged.

## D-E0002-040 — Proposed additive planning-only Gate-B repair workgraph

Status: proposed / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator proposal

D-E0002-039 proved that a completed prerequisite cannot be reopened while its
completed dependents remain done. The validator-safe repair is additive and
preserves E0002-13, E0002-05, E0002-08, and E0002-12 as immutable completed
history.

If authorized, the orchestrator first performs one administrative-only graph
edit:

1. Add E0002-16 in W4, owned by the orchestrator at `gpt-5.6-sol` / high,
   dependent on historical E0002-13 and E0002-12. Its exclusive product paths
   are the operator-control contract, reads schema, schema manifest, evaluator,
   and its unique handoff; its edition-administration paths cover only the new
   packets, assignments, graph, status, evidence, and decisions required for
   this repair. Its substantive scope is exactly the D-E0002-037 four-profile
   contract/reads/manifest/evaluator/packet digest closure.
2. Add E0002-17 in W5, owned by e0002-fixtures at `gpt-5.6-luna` / medium,
   dependent on E0002-16 and historical E0002-12. Its only product path is
   `evals/fixtures/operator-control/**` plus its unique handoff. Its scope is
   exactly the D-E0002-037 derived mechanical fixture propagation.
3. Shift every not-done implementation/release wave forward by two without
   changing any existing task ID, owner, model, effort, writable path, or
   acceptance contract: current W4 becomes W6, W5 becomes W7, W6 becomes W8,
   W7 becomes W9, W8 becomes W10, W9 becomes W11, and W10 becomes W12. Add
   E0002-17 as a dependency of E0002-06, E0002-07, and E0002-11, and include
   E0002-16 and E0002-17 in E0002-10's all-prior integration dependency set.

The edition validator and `rtk git diff --check` must pass immediately after
that administrative edit. Any failure stops before a repair worker or artifact
edit. On PASS, dispatch only E0002-16. Its exact duplicate-aware schema,
meta-schema, reference, manifest, artifact, semantic-digest, and packet-digest
validations must pass before the orchestrator records `Gate B stale-fence
repair artifacts frozen` and marks that drafting barrier done. Only then may
E0002-17 run the exact 28-setup/121-wrapper/index mechanical refresh and its
frozen corpus schema, order, cardinality, blueprint, digest-linkage, seed, and
secret-sentinel validation. Any failure stops the affected sequence.

After both writers are quiescent, three fresh independent read-only reviews
must separately cover command/audit semantics, schema/manifest/packet closure,
and evaluator/fixture propagation. Three PASS results leave E0002-17 in review
and return the exact superseding Gate-B packet for explicit product-owner
acceptance. Only that dated digest-bound acceptance may mark E0002-17 done;
even then, no implementation task is dispatched without a separate owner
continuation.

This proposal itself changes no assignment, task packet, workgraph, contract,
schema, evaluator, fixture, digest, source, migration, Cargo manifest,
`Cargo.lock`, dependency, or Gate-B state. All current W4/W5 implementation
tasks and later work remain closed. It grants no Cargo command, external
effect, E0006 reuse, commit, push, Gate C, or release.

The product owner may authorize exactly:

> Authorize D-E0002-040 exactly as bounded. Add E0002-16 after historical
> E0002-13/E0002-12 for the D-E0002-037 Gate-B artifact and digest closure,
> then E0002-17 for derived fixture propagation; shift the current unstarted
> W4-W10 waves forward by two and add the recorded dependencies without
> changing their scope. Require validation before dispatch, serialized freeze
> barriers, exact validations, quiescence, and three fresh independent reviews;
> stop on any failure and return the superseding Gate-B packet for explicit
> acceptance. Do not dispatch implementation or any shifted W5-or-later work.

## D-E0002-041 — Product owner authorizes additive Gate-B repair workgraph

Status: adopted / administrative graph gate only · Date: 2026-09-02 ·
Decision owner: product owner

After receiving D-E0002-040, the product owner directed:

> Authorize D-E0002-040 exactly as bounded. Add E0002-16 after historical
> E0002-13/E0002-12 for the D-E0002-037 Gate-B artifact and digest closure,
> then E0002-17 for derived fixture propagation; shift the current unstarted
> W4-W10 waves forward by two and add the recorded dependencies without
> changing their scope. Require validation before dispatch, serialized freeze
> barriers, exact validations, quiescence, and three fresh independent reviews;
> stop on any failure and return the superseding Gate-B packet for explicit
> acceptance. Do not dispatch implementation or any shifted W5-or-later work.

This authorizes only the additive administrative graph edit described in
D-E0002-040, initially with E0002-16 ready and E0002-17 pending. Historical
E0002-13/E0002-12 and every completed task remain done. The existing blocked
storage/runtime/control tasks move to W6 and gain E0002-17 as a dependency;
all other not-done implementation and release waves shift forward exactly two.
Their task IDs, ownership, models, paths, scope, candidate source, historical
signals, and acceptance obligations do not otherwise change.

The newly numbered W5 is exclusively the authorized planning-only E0002-17
fixture propagation task. The prohibition on shifted W5-or-later dispatch
applies to every existing implementation/release task, now W6-W12; it does not
contradict the expressly authorized serialized E0002-17 planning step.

Before any Gate-B artifact edit or worker dispatch, the orchestrator must run
the edition validator and diff check. Any failure stops. PASS permits E0002-16
alone to become active under its exact packet. E0002-17 remains closed until
the exact freeze signal, and implementation remains closed through explicit
acceptance of a superseding digest packet and a separate continuation decision.

## D-E0002-042 — Additive graph gate passes; E0002-16 alone is dispatched

Status: adopted / planning-only artifact dispatch · Date: 2026-09-02 ·
Decision owner: orchestrator under D-E0002-041

The complete additive administrative graph was staged with E0002-16 ready and
E0002-17 pending. Before any Gate-B artifact or fixture edit and with every
implementation writer stopped, `rtk scripts/swarm.sh validate AXP-E0002` and
`rtk git diff --check` both passed. All eleven historical D-E0002-012 raw
artifact hashes were then reproduced exactly, including contract
`4f51a81e6c75a1c09bbc4362087ab2d574c37e4b3a98a98ec3f2ca9998be85e8`,
reads schema `5072e5c3586267c1dc4de4cdb7944d617f1534a2e4ebbb3da6cb1c094636598f`,
manifest `3a2a100651a55ba39d667b4d12f8342c279486b9d499a6aa02f25eb35634da09`,
and evaluator `d1a10306ece122cb771fc9aa3b51437887040649c117a4716b1b0b9795f4a339`.

The administrative gate therefore passes and only E0002-16 becomes active.
It may make exactly the D-E0002-037 four-profile artifact/digest repair under
its packet. E0002-17 remains pending until the exact freeze signal. Shifted W6
and every implementation/release task remain blocked or pending and receive no
dispatch. This decision authorizes no Cargo command, migration, source,
fixture, external effect, E0006 reuse, commit, push, Gate C, or release.

## D-E0002-043 — E0002-16 stops on evaluator baseline-assumption failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

E0002-16 applied only its exact contract clarification, four reads-schema
command profiles, and the corresponding reads hash in the schema manifest. It
then generated a repository-external evaluator candidate under an explicit
JSON-path whitelist before any evaluator or fixture edit. The first required
candidate-generation command, `rtk python3 /tmp/e16_update_evaluator.py`,
exited 1 with:

`unexpected assembly baseline: reject_old_control_authority`

The helper correctly stopped because it assumed all 26 assembly-bound
rejection cases inherited `valid_real_product_assembly`; the named case has a
different recorded valid baseline. No candidate output was accepted, the
repository evaluator remains at its historical D-E0002-012 bytes, and every
fixture remains untouched. No freeze signal exists. The contract, reads
schema, and manifest changes are an unaccepted partial candidate whose current
raw hashes are respectively
`83aa0eec2e1ae7d063776b9febf29009f92316c910728248db5bd40b74e405f0`,
`acbf50d28c57129794e8becb92236706794468f3ace63afc0a80fcfed1ce29c5`,
and `a6015dae7f20d379783e0ac0aa22a4125b4b6167625a1beeaadc3f0d00d02ad8`.

D-E0002-040 requires a stop on any failure. E0002-16 is therefore blocked;
E0002-17 was not dispatched; the three final independent reviews were not
started; shifted W6 and all later implementation/release work remain closed.
No Cargo command, migration, source, manifest, lockfile, dependency, external
effect, E0006 reuse, commit, push, Gate C, or release change occurred.

Continuing requires a new product-owner decision limited to replacing the
rejected common-baseline assumption with per-case lookup of each affected
rejection's already recorded `baseline_scenario_id`, then completing the
unchanged evaluator candidate whitelist, validations, and E0002-16 freeze
sequence. It grants no semantic change, E0002-17 dispatch, or implementation.

## D-E0002-044 — Product owner authorizes one bounded E0002-16 continuation

Status: adopted / planning-only continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-043, the product owner directed:

> Authorize one D-E0002-044 continuation limited to replacing the evaluator
> candidate helper’s common-baseline assumption with exact per-case lookup of
> each affected rejection’s recorded baseline_scenario_id, then complete the
> unchanged E0002-16 evaluator application, validations, digest closure, and
> freeze sequence. Preserve the current contract/reads/manifest candidate,
> historical evaluator and fixture bytes until their authorized steps, and
> every existing gate. Stop on any failure. Do not dispatch E0002-17 before the
> E0002-16 freeze signal, and do not dispatch implementation.

This reactivates E0002-16 only at the recorded blocker. The candidate helper
may replace only its rejected common-baseline assertion with a lookup of each
of the 26 already enumerated assembly-bound rejection cases' existing
`baseline_scenario_id` and that valid case's freshly recomputed semantic
digest. Every JSON-path whitelist, artifact boundary, validation, digest rule,
and stop condition remains unchanged. E0002-17 stays pending until the exact
freeze signal; shifted W6 and all later implementation/release work stay
closed. No Cargo command, source, migration, dependency, external effect,
E0006 reuse, commit, push, Gate C, or release is authorized.

## D-E0002-045 — E0002-16 stops on evaluator-promotion patch failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

Under D-E0002-044, the edition validator and diff check passed before work.
The temporary evaluator helper then replaced only its common-baseline
assumption with exact lookup of each affected rejection's recorded
`baseline_scenario_id`. Its exact-path guard passed with 87 permitted leaf
changes across exactly 28 assembly-bound cases, and the independent semantic
audit passed. The resulting repository-external evaluator candidate has raw
SHA-256
`4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`.

The first attempt to apply that byte-exact candidate through the required
patch interface failed before any repository evaluator edit because the
generated unified-diff range header was not accepted:

`Failed to find context '-16,7 +16,7 @@'`

The repository evaluator therefore remains at historical SHA-256
`d1a10306ece122cb771fc9aa3b51437887040649c117a4716b1b0b9795f4a339`;
all fixtures remain historical; and the current contract, reads, and manifest
candidate remains unchanged from D-E0002-043. No freeze signal exists.

D-E0002-044 requires a stop on any failure. E0002-16 is blocked, E0002-17 was
not dispatched, and no independent final review or implementation work was
started. No Cargo command, source, migration, dependency, external effect,
E0006 reuse, commit, push, Gate C, or release change occurred.

Continuing requires a new product-owner decision limited to applying the
already validated external evaluator candidate through a patch form accepted
by the repository interface, then completing the unchanged E0002-16
validations, digest closure, and freeze sequence. The candidate content may
not be regenerated or semantically changed. E0002-17 remains closed until the
exact freeze signal, and implementation remains closed.

## D-E0002-046 — Product owner authorizes one pinned evaluator continuation

Status: adopted / planning-only continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-045, the product owner directed:

> Authorize one D-E0002-046 continuation limited to applying the already
> validated evaluator candidate at SHA-256
> `4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`
> through a patch form accepted by the repository interface, without
> regenerating or changing candidate content, then complete the unchanged
> E0002-16 validations, digest closure, and freeze sequence. Preserve the
> current contract/reads/manifest candidate, historical fixtures, and every
> existing gate. Stop on any failure. Do not dispatch E0002-17 before the
> E0002-16 freeze signal, and do not dispatch implementation.

This reactivates E0002-16 only at the recorded evaluator-promotion blocker.
Before application, the orchestrator must reproduce the pinned candidate,
historical evaluator, and historical fixture-index hashes plus edition and
diff-check gates. It may apply exactly the existing candidate bytes through an
accepted patch form, without rerunning the generator or changing candidate
content, then execute only the unchanged E0002-16 validations, digest closure,
packet closure, and freeze sequence. Any failure stops without retry.

E0002-17 remains pending until the exact freeze signal; shifted W6 and every
implementation/release task remain closed. No Cargo command, source,
migration, fixture, dependency, external effect, E0006 reuse, commit, push,
Gate C, or release is authorized.

## D-E0002-048 — Product owner authorizes final E0002-16 closure continuation

Status: adopted / planning-only continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-047, the product owner directed:

> Authorize one D-E0002-048 continuation limited to correcting the read-only
> artifact-ledger inventory command’s nested-key quoting, then completing the
> unchanged E0002-16 custody, digest, canonical packet, validation, and freeze
> sequence. Preserve the four current Gate-B artifact bytes, including
> evaluator SHA-256
> `4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`;
> do not regenerate or semantically change them. Stop on any failure. Do not
> dispatch E0002-17 before the exact freeze signal, and do not dispatch
> implementation.

This reactivates E0002-16 only at the recorded read-only inventory blocker.
Before continuing, the orchestrator must reproduce the edition/diff gate and
the exact four candidate artifact hashes, evaluator/candidate byte identity,
and historical fixture-index custody. It may then correct only the command's
nested-key quoting and execute the unchanged raw/semantic custody, canonical
packet digest, validation, and freeze-state sequence. The four Gate-B artifact
bytes may not change. Any failure stops without retry.

E0002-17 remains pending until the exact freeze signal; shifted W6 and every
implementation/release task remain closed. No Cargo command, source,
migration, fixture, dependency, external effect, E0006 reuse, commit, push,
Gate C, or release is authorized.

## D-E0002-049 — Superseding Gate-B packet staged for exact validation

Status: proposed / packet valid but ordering repair required / non-dispatchable · Date: 2026-09-02 ·
Decision owner: orchestrator under D-E0002-048

The corrected read-only artifact-ledger inventory passed. Direct raw custody
reproduced all eleven packet artifacts: only ordinals 1, 4, 10, and 11 differ
from historical D-E0002-012, while the other seven remain byte-identical.
The packet below was derived mechanically from D-E0002-012 with exactly eight
changed JSON paths: task, date, four authorized artifact hashes, and the
`valid_scenarios` and `rejection_vectors` semantic digests.

<!-- gate-b-packet-v2:start -->
```json
{
  "schema": "proof.operator.gate-b-packet/v1",
  "edition": "AXP-E0002",
  "task": "E0002-16",
  "date": "2026-09-02",
  "status": "pending_owner",
  "implementation_dispatch": false,
  "artifacts": [
    {
      "ordinal": 1,
      "role": "contract",
      "path": "contracts/operator-control-plane.md",
      "sha256": "sha256:83aa0eec2e1ae7d063776b9febf29009f92316c910728248db5bd40b74e405f0"
    },
    {
      "ordinal": 2,
      "role": "schema",
      "path": "schemas/operator-control/common-v1.schema.json",
      "sha256": "sha256:e64b2278a81db61ccf333005274990366047e133c9a507915c4123e641c3412b"
    },
    {
      "ordinal": 3,
      "role": "schema",
      "path": "schemas/operator-control/auth-v1.schema.json",
      "sha256": "sha256:8ec5777fbb9c7a36484f3503a04f36dd297a880f6cf9c0ba7737384461c5c37d"
    },
    {
      "ordinal": 4,
      "role": "schema",
      "path": "schemas/operator-control/reads-v1.schema.json",
      "sha256": "sha256:acbf50d28c57129794e8becb92236706794468f3ace63afc0a80fcfed1ce29c5"
    },
    {
      "ordinal": 5,
      "role": "schema",
      "path": "schemas/operator-control/mutations-v1.schema.json",
      "sha256": "sha256:0d0ea379971e673fc45cd47d38896b18ddf06712f872ca5a6363d6b07ecf940c"
    },
    {
      "ordinal": 6,
      "role": "schema",
      "path": "schemas/operator-control/durable-v1.schema.json",
      "sha256": "sha256:20c7380cad0692f7459bbb6b4d06d2a9b18810f99e45963fa1941d4f5733f719"
    },
    {
      "ordinal": 7,
      "role": "schema",
      "path": "schemas/operator-control/store-v1.schema.json",
      "sha256": "sha256:afb321e63ae8884ace0be83c7cad08f1a22e60876a082cf092dc7081d66ef4ab"
    },
    {
      "ordinal": 8,
      "role": "schema",
      "path": "schemas/operator-control/evaluator-v1.schema.json",
      "sha256": "sha256:1fae78f5c452ae35a4ab536d1e6e12cf8ae9ec51de4915cd8ff4180a36c9d2c6"
    },
    {
      "ordinal": 9,
      "role": "schema",
      "path": "schemas/operator-control/manifest-v1.schema.json",
      "sha256": "sha256:685f683910f0956987db08a1646aa2aee5f5d884fc13634fb80fa4bbf08467d6"
    },
    {
      "ordinal": 10,
      "role": "schema_manifest",
      "path": "schemas/operator-control/manifest-v1.json",
      "sha256": "sha256:a6015dae7f20d379783e0ac0aa22a4125b4b6167625a1beeaadc3f0d00d02ad8"
    },
    {
      "ordinal": 11,
      "role": "evaluator",
      "path": "evals/operator-control-v1.json",
      "sha256": "sha256:4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d"
    }
  ],
  "semantic_digests": {
    "valid_scenarios": "sha256:f8b623b06f84d6f2e2b8f018865b662daf0dee5a59c7e3bbb40d4bd520bce95d",
    "checks": "sha256:43b5e0daa4fd2f810c04a8ecaf61b295e6069bf8b43a92d111accc4201ac958d",
    "rejection_vectors": "sha256:094d2bc6109752e44bd6ee2a1c3f8f7f6c1cc97f3fa92fbbdeeb6170dc17afe1",
    "backend_subset": "sha256:5298e900fec2d3314cde366a2ec90ca11ea6dd812459bf8e6af0ffb6dd980c3b",
    "store_error_matrix": "sha256:cd99bd03809a467a9478b77ebe2e73fd2959db98cafa038f98744d7491beaa6c"
  },
  "evaluation": {
    "required_score_basis_points": 10000,
    "replay_count": 2,
    "valid_scenarios": 16,
    "ordered_checks": 20,
    "rejection_vectors": 105,
    "backend_subset_scenarios": 4,
    "backend_subset_vectors": 16,
    "store_boundaries": 21,
    "store_error_variants": 9,
    "store_matrix_cells": 189,
    "typed_absence_cases": 4,
    "manifest_logical_shapes": 206,
    "protected_and_public_routes": 15,
    "schema_self_tests": 4
  },
  "migration_14": {
    "contract_section": 14,
    "description": "create governed operator control, projection, fence, budget, command, and audit schema",
    "prior_versions_unchanged": "1-13",
    "validated_up_down": true,
    "operator_tables": 14,
    "immutable_triggers": 20,
    "indexes": 19,
    "pre_14_objects_preserved": true
  },
  "material_choices": [
    {
      "ordinal": 1,
      "id": "independent_terminal_signed_human_challenge_and_volatile_session"
    },
    {
      "ordinal": 2,
      "id": "six_capability_intersection_exact_human_no_delegation"
    },
    {
      "ordinal": 3,
      "id": "loopback_request_error_secret_boundary_with_same_uid_root_limits"
    },
    {
      "ordinal": 4,
      "id": "disposable_workspace_forbidden_repository_root_identity_single_schema14_database_trusted_open_persisted_signers"
    },
    {
      "ordinal": 5,
      "id": "dedicated_router_exact_inventory_and_legacy_route_exclusion"
    },
    {
      "ordinal": 6,
      "id": "migration14_immutable_provisioning_atomic_store_and_legacy_write_rejection"
    },
    {
      "ordinal": 7,
      "id": "approval_explicit_resume_cancel_dispatch_idempotency_revoke_and_signer_order"
    },
    {
      "ordinal": 8,
      "id": "lease30s_renew10s_fenced_recovery_and_distinct_restart_semantics"
    },
    {
      "ordinal": 9,
      "id": "five_dimension_aggregate_reservation_and_forfeit_rules"
    },
    {
      "ordinal": 10,
      "id": "append_only_projection_cursor_mac_audit_redaction_and_static_constraints"
    },
    {
      "ordinal": 11,
      "id": "exact_root_member_dependency_and_lock_deltas"
    },
    {
      "ordinal": 12,
      "id": "no_new_global_execution_error_and_all_required_zero_effect_evaluation"
    }
  ]
}
```
<!-- gate-b-packet-v2:end -->

Packet SHA-256:
`sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`

This is a staged validation candidate, not Gate-B acceptance or an E0002-16
freeze signal. E0002-16 remains active only for exact packet reproduction and
the remaining unchanged validation/freeze sequence. E0002-17 and all
implementation remain closed.

## D-E0002-047 — E0002-16 stops on artifact-inventory command failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

Under D-E0002-046, the edition validator, diff check, and pre-application
custody hashes all passed. The contract, reads schema, manifest, historical
evaluator, pinned external evaluator candidate, and fixture index reproduced
their exact recorded hashes. The repository patch interface then applied only
the pinned evaluator candidate. `rtk sha256sum` and `rtk cmp` proved exact byte
equality at SHA-256
`4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`.

The strict freeze validator passed duplicate-aware parsing, eight Draft 2020-12
schema checks, manifest/reference/hash closure, 206 logical shapes, 15 routes,
four self-tests, the policy ledger, and all five semantic digests. The semantic
closure audit also passed. The next required read-only artifact-ledger
inventory command exited 1 before producing data because its nested Python key
quoting was malformed:

`SyntaxError: unexpected character after line continuation character`

No artifact was changed by that command. The evaluator remains at the exact
pinned candidate bytes, and every fixture remains historical. The remaining
raw-artifact custody, canonical packet digest, and freeze-state transition were
not run. No freeze signal exists.

D-E0002-046 requires a stop on any failure. E0002-16 is blocked, E0002-17 was
not dispatched, and no final review or implementation work was started. No
Cargo command, source, migration, fixture, dependency, external effect, E0006
reuse, commit, push, Gate C, or release change occurred.

Continuing requires a new product-owner decision limited to correcting the
read-only artifact-ledger inventory command and completing the otherwise
unchanged E0002-16 custody, digest, packet, validation, and freeze sequence.
The four Gate-B artifact bytes may not change. E0002-17 remains closed until
the exact freeze signal, and implementation remains closed.

## D-E0002-050 — E0002-16 stops on decision-log ordering defect

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

Under D-E0002-048, the corrected read-only artifact inventory passed. The
strict freeze validator, semantic audit, eleven-artifact raw custody,
historical fixture-index custody, and the independently parsed canonical
packet all passed. The proposed superseding packet changes exactly eight JSON
paths from D-E0002-012 and reproduces at
`sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`.

During the freeze transition, inspection found that the D-E0002-049 packet
block had been inserted physically before its authorizing D-E0002-048 because
the patch operation matched an earlier identical end-of-decision anchor. The
packet content and digest are valid, but the append-only decision chronology is
not. The attempted freeze transition is therefore invalid and no freeze signal
exists.

D-E0002-048 requires a stop on any failure. E0002-16 is blocked, E0002-17 was
not dispatched, and no final review or implementation work was started. The
four Gate-B artifact bytes and historical fixture bytes remain unchanged from
the validated pre-transition boundary. No Cargo command, source, migration,
fixture, dependency, external effect, E0006 reuse, commit, push, Gate C, or
release change occurred.

Continuing requires a new product-owner decision limited to relocating the
unchanged D-E0002-049 block after D-E0002-048 and before this stop record, then
rerunning the unchanged decision-order, packet, edition, diff, and freeze
transition checks. The packet object and digest and all four Gate-B artifact
bytes may not change. E0002-17 remains closed until the exact freeze signal,
and implementation remains closed.

## D-E0002-051 — Product owner authorizes exact decision-block relocation

Status: adopted / planning-only continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-050, the product owner directed:

> Authorize one D-E0002-051 continuation limited to relocating the unchanged
> D-E0002-049 block after D-E0002-048 and before D-E0002-050, preserving its
> exact packet object and digest
> `sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`,
> then rerunning the unchanged decision-order, packet, edition, diff, and
> E0002-16 freeze-transition checks. Preserve all four Gate-B artifact bytes
> and historical fixtures. Stop on any failure. Do not dispatch E0002-17
> before the valid exact freeze signal, and do not dispatch implementation.

This reactivates E0002-16 only at the recorded decision-ordering blocker. The
orchestrator must first reproduce the current packet, four-artifact, fixture,
edition, and diff boundary. It may then relocate the complete D-E0002-049 block
byte-for-byte from before D-E0002-048 to immediately after D-E0002-048 and
before D-E0002-050. The packet object, recorded digest, and all artifact bytes
must remain unchanged. Only exact decision-order, packet, edition, diff, and
freeze-transition checks may follow. Any failure stops without retry.

E0002-17 remains pending until a valid exact freeze signal; shifted W6 and
every implementation/release task remain closed. No Cargo command, source,
migration, fixture, dependency, external effect, E0006 reuse, commit, push,
Gate C, or release is authorized.

## D-E0002-052 — E0002-16 artifact and packet freeze passes

Status: adopted / drafting barrier complete / Gate B pending · Date:
2026-09-02 · Decision owner: orchestrator under D-E0002-051

The complete D-E0002-049 block was relocated byte-for-byte after D-E0002-048
and before D-E0002-050. Its 8,401 bytes retain SHA-256
`1203a6d190c64695f023f80b72539d1019ebcbad2b89d39693d008c8ffad6f6d`,
and the resulting heading order is exactly D-E0002-048, D-E0002-049,
D-E0002-050, then D-E0002-051. No packet or artifact byte changed.

The post-relocation checks all passed: the superseding packet strictly parses
and reproduces at
`sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`;
its delta from D-E0002-012 is exactly task, date, artifact ordinals 1, 4, 10,
and 11, plus `valid_scenarios` and `rejection_vectors`. Evaluation counts,
migration 14, material choices, seven other raw artifacts, three other
semantic digests, and historical fixtures retain exact custody.

Duplicate-aware parsing, eight Draft 2020-12 schema validations,
manifest/reference/hash closure, 206 logical shapes, 15 routes, four
self-tests, evaluator closure, the 189+4 store matrix, direct artifact hashes,
edition validation, diff hygiene, and writer quiescence all passed.

The orchestrator therefore emits the valid exact signal:

`Gate B stale-fence repair artifacts frozen`

E0002-16 becomes done as a drafting barrier only. The superseding Gate B is
not accepted. E0002-17 remains pending until the done transition itself
validates; only then may its already-authorized planning-only fixture lane be
dispatched. Shifted W6 and all implementation/release work remain closed. This
decision grants no Cargo command, source, migration, external effect, E0006
reuse, commit, push, Gate C, release, or implementation dispatch.

## D-E0002-053 — E0002-17 alone is dispatched after the freeze barrier

Status: adopted / planning-only fixture dispatch · Date: 2026-09-02 · Decision
owner: orchestrator under D-E0002-041 and D-E0002-052

The E0002-16 done transition passed exact decision-order and D-E0002-049 block
custody, superseding packet reproduction, edition validation, and diff hygiene.
The valid exact `Gate B stale-fence repair artifacts frozen` signal therefore
precedes this dispatch.

Only E0002-17 becomes active under its frozen W5 packet. It may mechanically
refresh exactly the 28 enumerated assembly-bound setup documents and their
derived hash links, all 121 wrapper `policy_sha256` values and resulting index
links, and its unique handoff. It may make no policy judgment or change any
fixture ID, ordinal, count, seed, action, mutation, expected result, evaluator,
contract, schema, manifest, source, migration, Cargo file, or dependency. The
five runtime stale-fence recipes retain their setup/action/mutation/expected
bytes, case semantics, and empty `new_events`.

Any scope, custody, schema, order, count, blueprint, digest, seed,
secret-sentinel, edition, or diff failure stops E0002-17 without retry. On
complete local PASS it enters review rather than done. Shifted W6 and every
implementation/release task remain closed. This dispatch grants no Cargo
command, provider/browser action, external effect, E0006 reuse, commit, push,
Gate B acceptance, Gate C, release, or implementation dispatch.

## D-E0002-054 — E0002-17 stops on pre-action assignment-path failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

After D-E0002-053 dispatched E0002-17 alone, its worker began the mandatory
instruction and packet reads. The first repository command was
`rtk cat assignments.tsv`; it exited 1 because no assignment file exists at
the repository root:

`/usr/bin/cat: assignments.tsv: No such file or directory`

The required file is `editions/AXP-E0002/assignments.tsv`. The worker stopped
without retry as required. It changed no fixture or handoff byte, ran no
fixture generator or validator, and made no policy judgment. The E0002-16
artifacts, packet, and valid freeze signal remain unchanged.

E0002-17 is blocked. The three final independent reviews were not started;
shifted W6 and all implementation/release work remain closed. No Cargo command,
source, migration, fixture, dependency, external effect, E0006 reuse, commit,
push, Gate B acceptance, Gate C, release, or implementation dispatch occurred.

Continuing requires a new product-owner decision limited to correcting the
pre-action assignment read to `editions/AXP-E0002/assignments.tsv`, then
executing the otherwise unchanged E0002-17 mechanical fixture propagation and
validation sequence. Any failure stops without retry. Implementation remains
closed.

## D-E0002-055 — Product owner authorizes one E0002-17 path correction

Status: adopted / planning-only continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-054, the product owner directed:

> Authorize one D-E0002-055 continuation limited to correcting E0002-17’s
> pre-action assignment read to `editions/AXP-E0002/assignments.tsv`, then
> executing its unchanged mechanical fixture propagation and validation
> sequence. Preserve the frozen E0002-16 artifacts, packet digest
> `sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`,
> and valid freeze signal. Stop on any failure. On local PASS, place E0002-17
> in review for the already-required three fresh independent reviews; do not
> mark it done, accept Gate B, or dispatch implementation.

This reactivates only E0002-17 at its pre-action path blocker. The worker must
read the exact edition assignment path and all other required packets before
editing. It then retains its existing exclusive fixture/handoff paths,
mechanical propagation scope, custody rules, numeric budget, zero-spend rule,
and stop conditions. The E0002-16 artifacts, packet, and freeze signal are
immutable inputs.

On complete local PASS, E0002-17 enters review rather than done and all writers
quiesce before three fresh independent read-only reviews. Shifted W6 and every
implementation/release task remain closed. No Cargo command, policy judgment,
provider/browser action, external effect, E0006 reuse, commit, push, Gate B
acceptance, Gate C, release, or implementation dispatch is authorized.

## D-E0002-056 — E0002-17 stops on fixture raw-digest mismatch

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

Under D-E0002-055, the worker successfully read
`editions/AXP-E0002/assignments.tsv` and the remaining required packets, then
began the unchanged mechanical fixture propagation within its exclusive path.
It partially applied derived fixture changes. Its first required fixture
validator then failed with:

`document raw digest mismatch: valid_control_plane_restart setup`

The worker stopped immediately without retry or further validation. The
partial fixture candidate remains preserved and unaccepted. Its current
`evals/fixtures/operator-control/index-v1.json` SHA-256 is
`a71a42ffb0ac2e11ef5f68ea65863fba6a7492eba2759faab9033a662339cb60`.
No claim is made about later mismatches because the validator stopped at its
first failure.

The frozen E0002-16 artifacts, superseding packet digest
`sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`,
and valid freeze signal remain authoritative and unchanged. E0002-17 is
blocked. The three final reviews were not started; shifted W6 and every
implementation/release task remain closed. No Cargo command, source,
migration, dependency, policy decision, external effect, E0006 reuse, commit,
push, Gate B acceptance, Gate C, release, or implementation dispatch occurred.

Continuing first requires a new product-owner decision for a read-only
diagnostic limited to the preserved E0002-17 partial candidate: enumerate its
exact changed-file set and the complete derived raw/envelope/index digest-link
mismatch set without editing or rerunning propagation. Return an exact bounded
repair packet and keep reviews and implementation closed.

## D-E0002-057 — Product owner authorizes bounded fixture diagnostics

Status: adopted / read-only diagnostic continuation · Date: 2026-09-02 ·
Decision owner: product owner

The product owner directed:

> Authorize one D-E0002-057 diagnostic-only continuation limited to read-only
> inspection of the preserved E0002-17 partial fixture candidate at index
> SHA-256
> `a71a42ffb0ac2e11ef5f68ea65863fba6a7492eba2759faab9033a662339cb60`.
> Enumerate the exact changed-file set and complete raw-document, envelope,
> case, and index digest-link mismatch set, beginning with
> `valid_control_plane_restart` setup. Do not edit, regenerate, or resume
> propagation. Preserve the frozen E0002-16 artifacts, packet digest, and valid
> freeze signal. Stop on any diagnostic failure and return an exact bounded
> repair packet. Keep reviews and implementation closed.

This grants read-only fixture diagnosis only. It does not authorize repair,
review, Gate-B acceptance, or implementation.

## D-E0002-058 — Product owner narrows diagnostic inputs

Status: adopted / read-only diagnostic continuation · Date: 2026-09-02 ·
Decision owner: product owner

The product owner directed:

> Authorize one D-E0002-058 read-only diagnostic continuation limited to
> inspecting the explicit files `/tmp/propagate_e0002.py` and
> `/tmp/validate_e0002_fixtures.py` plus the preserved E0002-17 fixture tree and
> frozen evaluator. Do not recursively traverse `/tmp`. Complete the unchanged
> D-E0002-057 enumeration of exact changed files and all raw-document,
> envelope, case, and index digest-link mismatches. Do not edit, regenerate, or
> resume propagation. Preserve all frozen artifacts, packet digest, freeze
> signal, and partial fixture bytes. Stop on any failure and return an exact
> bounded repair packet. Keep reviews and implementation closed.

## D-E0002-059 — Product owner authorizes exact in-memory collector correction

Status: adopted / read-only diagnostic continuation · Date: 2026-09-02 ·
Decision owner: product owner

The product owner directed:

> Authorize one D-E0002-059 read-only diagnostic continuation limited to
> rerunning the in-memory D-E0002-058 collector with the seed delimiter
> expressed as `bytes([0])`, and explicitly classifying affected blueprint
> comparisons against both the whole phase object and its recorded `steps`
> projection without assuming either. Inspect only
> `/tmp/propagate_e0002.py`, `/tmp/validate_e0002_fixtures.py`, the frozen
> evaluator, and the preserved E0002-17 fixture tree. Complete the exact
> changed-file set and every raw-document, payload, envelope, case, and index
> digest-link mismatch. Do not edit, regenerate, or resume propagation.
> Preserve all frozen artifacts, packet digest, freeze signal, and partial
> fixture bytes. Stop on any failure and return the exact fixture-repair
> packet. Keep reviews and implementation closed.

## D-E0002-060 — Product owner authorizes final repair-byte simulation

Status: adopted / read-only diagnostic continuation · Date: 2026-09-02 ·
Decision owner: product owner

The product owner directed:

> Authorize one D-E0002-060 read-only diagnostic continuation limited to
> replacing the in-memory repair-byte serializer's literal backslash-n suffix
> with `bytes([10])`, then verifying the already-enumerated 28 setup/envelope
> pairs and calculating their exact canonical payload digests and final raw
> setup-document digests. Do not reopen discovery, inspect additional paths,
> edit files, regenerate fixtures, or resume propagation. Preserve all frozen
> artifacts, packet digest, freeze signal, and partial fixture bytes. Stop on
> any failure and return the final exact fixture-repair packet. Keep reviews
> and implementation closed.

## D-E0002-061 — Product owner authorizes the certified 28-pair repair

Status: adopted / bounded fixture repair · Date: 2026-09-02 · Decision owner:
product owner

The product owner directed:

> Authorize D-E0002-061 exactly as bounded. Reopen E0002-17 only to repair the
> preserved 28 setup/envelope pairs certified by D-E0002-060. Require every
> D-E0002-060 partial-state precondition before the first edit. In each setup
> document, preserve the payload and all other fields and replace only
> `payload_sha256` with its certified recursively sorted canonical digest;
> preserve compact UTF-8 serialization and exactly one terminal LF, and require
> the resulting raw digest to equal the D-E0002-060 matrix. Then replace only
> the corresponding envelope's `setup_sha256` with that certified final raw
> digest. Correct `/tmp/validate_e0002_fixtures.py` only so a phase object
> containing `steps` compares the document payload to that exact `steps`
> projection, while every other phase retains whole-phase comparison. Do not
> rerun propagation or modify the index, evaluator, schemas, other fixtures,
> frozen Gate-B artifacts, packet, manifests, lockfile, or implementation. Run
> the strict fixture validator and exact custody checks; require evaluator
> SHA-256
> `4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`,
> index SHA-256
> `a71a42ffb0ac2e11ef5f68ea65863fba6a7492eba2759faab9033a662339cb60`,
> and packet digest
> `sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`.
> Stop on any precondition, edit, digest, or validation failure. On PASS, stop
> and return E0002-17 for the separately authorized three fresh independent
> reviews; keep implementation closed.

## D-E0002-062 — Product owner narrows the temporary validator rule

Status: adopted / validator-only continuation · Date: 2026-09-02 · Decision
owner: product owner

The product owner directed:

> Authorize one D-E0002-062 continuation limited to replacing
> `/tmp/validate_e0002_fixtures.py`'s broad `phase_blueprint contains steps`
> selection with the exact E0002-17 affected-set rule: use the `steps`
> projection only for the 28 D-E0002-060 case IDs when `phase == "setup"`;
> require the whole phase object for every other case and phase. Do not modify
> any fixture, index, evaluator, schema, frozen Gate-B artifact, packet,
> manifest, lockfile, or implementation. Then rerun the strict fixture
> validator and exact evaluator, index, packet, and 56 repaired-file custody
> checks. Stop on any failure. On PASS, stop and return E0002-17 for the
> separately authorized three fresh independent reviews; keep implementation
> closed.

## D-E0002-063 — Product owner authorizes whole-phase repair simulation

Status: adopted / read-only diagnostic continuation · Date: 2026-09-02 ·
Decision owner: product owner

The product owner directed:

> Authorize one D-E0002-063 read-only diagnostic continuation limited to the
> frozen evaluator, the `FixtureRecipeDocument` schema used by
> `/tmp/validate_e0002_fixtures.py`, the validator itself, and the preserved 28
> E0002-17 setup/envelope pairs. For each pair, simulate in memory replacing
> the setup document's list payload with the complete frozen
> `fixture_blueprint.setup` object, recompute its recursively sorted canonical
> `payload_sha256`, serialize with the existing compact UTF-8 form and one
> terminal LF, compute the resulting raw setup digest, and validate the
> simulated document against the exact schema. Also determine the exact
> validator rollback required to restore whole-phase comparison. Do not edit
> files, regenerate fixtures, or resume propagation. Preserve every current
> byte and all frozen artifacts, packet, index, manifests, lockfile, and
> implementation. Stop on any failure and return the exact expanded
> fixture-repair matrix and bounded repair packet. Keep reviews and
> implementation closed.

## D-E0002-064 — Product owner authorizes the expanded whole-phase repair

Status: adopted / bounded fixture repair · Date: 2026-09-02 · Decision owner:
product owner

The product owner directed:

> Authorize D-E0002-064 exactly as bounded. Reopen E0002-17 only for the
> D-E0002-063 expanded repair. Require the exact D-E0002-061 current setup and
> envelope digest state before editing. For the 28 certified setup documents,
> replace only the list-valued payload with the corresponding complete frozen
> `fixture_blueprint.setup` object and replace `payload_sha256` with the
> D-E0002-063 canonical whole-phase digest. Preserve every other field, compact
> UTF-8 serialization, key order, and exactly one terminal LF; require every
> resulting raw digest to equal the D-E0002-063 matrix. Then replace only each
> matching envelope's `setup_sha256` with that final raw digest. Restore
> `/tmp/validate_e0002_fixtures.py` to direct whole-phase comparison and require
> SHA-256
> `9437ad3a615d0fc302fd0b2d3e3717ed29d46451292349aa867453ecc0b9f156`.
> Do not modify the index, evaluator, schemas, other fixtures, frozen Gate-B
> artifacts, packet, manifests, lockfile, or implementation. Run the strict
> fixture validator and exact evaluator, index, packet, validator, and 56-file
> custody checks. Stop on any failure. On PASS, stop and return E0002-17 for
> the separately authorized three fresh independent reviews; keep
> implementation closed.

## D-E0002-065 — E0002-17 expanded fixture repair passes locally

Status: recorded / local PASS / review required · Date: 2026-09-02 · Decision
owner: orchestrator under D-E0002-064

Every D-E0002-061 intermediate setup/envelope digest precondition matched
before the first edit. The D-E0002-064 repair changed exactly the certified 28
setup documents and 28 matching envelopes. Each setup now carries the complete
frozen `fixture_blueprint.setup` object and its certified recursively sorted
canonical payload digest; compact UTF-8 serialization, key order, every other
field, and exactly one terminal LF were preserved. Each envelope changed only
its `setup_sha256` link.

The temporary validator was restored to direct whole-phase comparison and
reproduces SHA-256
`9437ad3a615d0fc302fd0b2d3e3717ed29d46451292349aa867453ecc0b9f156`.
The strict fixture validator passes 16 valid scenarios, 105 rejection
envelopes, and 468 recipe documents with exact order, blueprints, seeds, and
digests. Exact custody also reproduces evaluator SHA-256
`4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`,
index SHA-256
`a71a42ffb0ac2e11ef5f68ea65863fba6a7492eba2759faab9033a662339cb60`,
and superseding packet digest
`sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`.

E0002-17 enters review and is not done. No independent review is claimed by
this repair, the superseding Gate B remains unaccepted, and shifted W6 and all
later implementation/release work remain closed.

## D-E0002-066 — Product owner authorizes exact wrapper/index byte-custody repair

Status: adopted / bounded E0002-17 repair and re-review · Date: 2026-09-02 ·
Decision owner: product owner

The first quiescent review sequence stopped when the independent evaluator/
fixture reviewer found that the propagation helper had appended one terminal
LF to all 121 fixture wrappers. The orchestrator reproduced `121/121` wrapper
LFs and additionally found one terminal LF on `index-v1.json`. Historical
E0002-12 custody requires compact UTF-8 JSON with no trailing newline. The
five runtime stale-fence wrapper cases are especially dispositive because
D-E0002-037 permits only their wrapper `policy_sha256` field to change. The
schema/manifest/packet review passed, but it is not sufficient to advance the
three-review gate; the command/audit review was stopped after the failure.

The product owner authorizes exactly this repair:

> Reopen E0002-17 only to remove exactly one final `0x0a` byte from each of
> the 16 valid wrappers, 105 rejection wrappers, and
> `evals/fixtures/operator-control/index-v1.json`. Require current evaluator
> SHA-256 `4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`,
> current index SHA-256
> `a71a42ffb0ac2e11ef5f68ea65863fba6a7492eba2759faab9033a662339cb60`,
> and validator SHA-256
> `9437ad3a615d0fc302fd0b2d3e3717ed29d46451292349aa867453ecc0b9f156`
> before the first fixture edit. Preserve every JSON value and every byte
> preceding those 122 final LF bytes. In particular, preserve the 28 setup
> documents and their D-E0002-064-authorized terminal LFs, all other recipe
> documents, all seeds, IDs, ordinals, counts, paths, hashes, semantics, frozen
> Gate-B artifacts, packet, schemas, manifests, Cargo files, and implementation
> source. Require resulting index SHA-256
> `fde90bde1b2983a4efe8b4eb224da4ce461ccf69037a77ba6a9c4bb58882740d`,
> strict fixture validation, an independent exact no-final-newline scan of all
> wrappers and the index, evaluator/validator custody, edition validation, and
> `rtk git diff --check`. Stop on any failure. On PASS, return E0002-17 to
> review and run three fresh independent read-only reviews of command/audit
> semantics, schema/manifest/packet closure, and evaluator/fixture propagation.
> Do not accept Gate B or dispatch implementation.

This decision authorizes no JSON-semantic change, recipe-document change,
validator edit, Cargo command, provider/browser/external effect, E0006 reuse,
commit, push, Gate B acceptance, Gate C, release, or shifted W6-and-later work.

## D-E0002-067 — Exact wrapper/index byte-custody repair passes locally

Status: recorded / local PASS / three fresh reviews required · Date:
2026-09-02 · Decision owner: orchestrator under D-E0002-066

Every D-E0002-066 precondition matched before the first fixture edit. The
bounded mechanical rewrite removed exactly one final `0x0a` byte from each of
the 16 valid wrappers, 105 rejection wrappers, and `index-v1.json`. An exact
post-edit comparison proved that every current target plus one LF equals its
`HEAD` predecessor byte-for-byte, all parsed JSON values are identical, and no
other fixture path changed. The 28 D-E0002-064 setup documents retain their
authorized terminal LFs and all recipe documents remain byte-identical to
`HEAD`.

The repaired index SHA-256 is
`fde90bde1b2983a4efe8b4eb224da4ce461ccf69037a77ba6a9c4bb58882740d`.
Evaluator SHA-256 remains
`4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d`
and validator SHA-256 remains
`9437ad3a615d0fc302fd0b2d3e3717ed29d46451292349aa867453ecc0b9f156`.
The strict fixture validator passes 16 valid scenarios, 105 rejection
envelopes, and 468 recipe documents with exact order, blueprints, seeds, and
digests. Edition validation and `rtk git diff --check` also pass.

E0002-17 returns to review and is not done. The prior partial review set is not
reused. Only three fresh independent read-only reviews may now cover
command/audit semantics, schema/manifest/packet closure, and evaluator/fixture
propagation. The superseding Gate B remains unaccepted, and shifted W6 plus all
later implementation/release work remains closed.

## D-E0002-068 — Fresh review sequence stops on command/audit semantics

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

All three D-E0002-067 reviewers were fresh, independent, read-only, and
non-authoring. The schema/manifest/packet reviewer passed all eleven artifact
hashes, five semantic digests, the exact eight-path packet delta, canonical
superseding digest
`sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8`,
and its `pending_owner` / `implementation_dispatch: false` state. The
evaluator/fixture reviewer passed the exact 16 valid, 105 rejection, and 468
recipe-document corpus; 28 whole-phase setup correspondences; all hashes,
seeds, links, and secret/filesystem custody; the 122-byte terminal-LF repair;
and index SHA-256
`fde90bde1b2983a4efe8b4eb224da4ce461ccf69037a77ba6a9c4bb58882740d`.

The command/audit reviewer failed the sequence on three pre-existing semantic
inconsistencies:

1. Each of the four D-E0002-037 `stale_fence_rejected` reads-schema branches
   references `SafeInteger` for `fence_epoch`, accepting zero, while every
   originating command references `PositiveSafeInteger` for submitted
   `expected_fence_epoch` and kernel command validation rejects zero.
2. `AuditEvent::validate_profile` and its profile test still accept only the
   obsolete runtime-shaped mask `0x16c30`; all four newly frozen command masks
   are rejected. Storage validates through that kernel method, but E0002-05 is
   done and the current post-Gate-B graph assigns no kernel writer before W6.
3. Runtime rejection ordinals 067, 103, and 105 call `claim_run_lease` with a
   `LeaseClaimRequest` but expect observation stage `reclaim`. Ordinal 103 also
   describes possession of an existing raw lease token even though claim and
   reclaim use fresh unbound custody, so the current action cannot exercise the
   declared failure.

The same reviewer confirmed the four intended presence masks, rejection of
`0x16c30` by the reads schema, runtime no-audit semantics and empty
`new_events`, the proposed-command referential exception, exact D-E0002-066
byte-only change, strict fixture validation, edition validation, and diff
check. No reviewer edited a file or ran Cargo.

The required stop is now in force. E0002-17 is blocked and not done; neither
passing review may substitute for the failed complete set. The superseding
Gate B remains unaccepted, and all shifted W6-and-later work remains closed.
No semantic diagnosis, contract/schema/evaluator/fixture/kernel/workgraph
repair, Cargo command, review retry, provider/browser/external effect, E0006
reuse, commit, push, Gate B acceptance, Gate C, or release is authorized.
Continuing requires a new explicit product-owner decision.

## D-E0002-069 — Product owner authorizes planning-only semantic diagnostic

Status: adopted / bounded read-only diagnostic · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-068, the product owner directed the orchestrator to
proceed with the proposed planning-only diagnostic. Authority is limited to:

1. identify every exact reads-schema path that admits zero for a submitted
   command fence and the complete derived manifest/evaluator/fixture/digest
   closure needed to make those fields positive-safe-integer exact;
2. resolve the intended action, subject schema, observation stage, mutation,
   and expected result for runtime rejection ordinals 067, 103, and 105 against
   the frozen lease/fence contract and actual candidate APIs;
3. identify the exact kernel validation/test delta required to replace obsolete
   audit mask `0x16c30` with the four command-attributed profiles; and
4. design a validator-safe additive task/wave/ownership graph that performs the
   product-artifact repair before a new superseding Gate-B packet and performs
   kernel source alignment only after explicit Gate-B acceptance and separate
   implementation dispatch.

The diagnostic may read repository artifacts and exact named `/tmp` validators,
run read-only parsing/hash/schema/diff/edition commands, and update only
orchestrator-owned edition decision/status/evidence records with its findings.
It must return an exact bounded repair proposal and predicted digest closure
for a new product-owner decision. It may not edit any contract, schema,
manifest, evaluator, fixture, kernel, migration, Cargo file, implementation
source, task/assignment/workgraph, or temporary validator; run Cargo; retry a
review; accept Gate B; dispatch any task; invoke a provider/browser/external
effect; reuse E0006 authority; commit; push; approve Gate C; or claim release.
Any ambiguity that requires a product choice stops the diagnostic and is
returned explicitly rather than silently resolved.

## D-E0002-070 — Semantic diagnostic closes with an additive repair proposal

Status: proposed / product-owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator proposal

The D-E0002-069 read-only diagnostic reproduced all three D-E0002-068 review
findings against the frozen contract and current candidate APIs. It also found
one additional operation/stage mismatch, rejection ordinal 078, while checking
the complete bounded set of single-step runtime rejections. No product,
fixture, kernel, task, assignment, workgraph, validator, Cargo, migration, or
implementation file was changed.

### Exact planning-artifact correction

The only submitted-command zero-fence paths are these four reads-schema JSON
pointers:

- `#/$defs/AuditEvent/allOf/1/oneOf/16/properties/fence_epoch` — approval
  decision;
- `#/$defs/AuditEvent/allOf/1/oneOf/17/properties/fence_epoch` — run cancel;
- `#/$defs/AuditEvent/allOf/1/oneOf/18/properties/fence_epoch` —
  approval-branch run resume; and
- `#/$defs/AuditEvent/allOf/1/oneOf/19/properties/fence_epoch` —
  recovery-branch run resume.

Each must change only from `SafeInteger` to `PositiveSafeInteger`. The contract
already requires the submitted positive fence and needs no edit. The manifest
then refreshes only the reads-schema hash, and the evaluator refreshes only the
reads and manifest artifact entries, the existing 28 assembly-bound manifest
links and derived case/baseline hashes, the four runtime cases below, and the
two affected semantic digests.

| Ordinal | Exact corrected action and subject | Exact mutation | Exact observation | Audit |
|---:|---|---|---|---|
| 067 | `claim_run_lease` / `LeaseClaimRequest` | retain the active-second-claim barrier | `claim_lease`, `not_actionable` | retain `new_events: []` |
| 078 | `settle_budget_reservation` / `BudgetReservation` | retain `ReleasePreDispatch` against the dispatching row | `settle_budget`, `not_actionable` | retain the contract-required `budget_rejected` |
| 103 | `begin_dispatch` / `BeginDispatchRequest` | target `BeginDispatchRequest` at `/authority/lease_token`, replacing it with the symbolic different raw token | `begin_dispatch`, `stale_fence` | retain `new_events: []` |
| 105 | `begin_dispatch` / `BeginDispatchRequest` | target `BeginDispatchRequest` at `/authority/owner_instance_id`, replacing it with the symbolic revoked instance | `begin_dispatch`, `stale_fence` | retain `new_events: []` |

Ordinal 067's setup already contains an active lease and a current-revision
second claim; production storage checks that singleton and returns
`NotActionable`. Ordinals 103 and 105 must use a later authority-bearing call:
claim and reclaim deliberately establish fresh unbound token custody and
cannot prove possession of an existing token. Their existing live lease plus
reserved-pre-dispatch setup makes `begin_dispatch` the exact no-effect
boundary; `validate_authority` checks the token and then the owner instance
before any reservation mutation. The full single-step runtime-rejection audit
found no other operation/stage mismatch. Ordinal 078 was not named in
D-E0002-069 but cannot remain at `reclaim`: the runtime reports
`SettleBudget`. Its existing audit expectation follows the contract's
authority-proven settlement-rejection row; E0002-06 must later make the current
early `NotActionable` candidate path append that event atomically rather than
weakening the evaluator.

### Exact post-Gate-B kernel correction

A new kernel task must change `AuditEvent::validate_chain_link` so
`StaleFenceRejected` accepts only a rejected event with a nonzero
`fence_epoch` and one of these correlated `(command_kind, presence_mask)`
pairs:

- `(approval_decide, 0x101e3)`;
- `(run_cancel, 0x101a3)`;
- `(run_resume, 0x2101e3)`; or
- `(run_resume, 0x4181a3)`.

It must reject `0x16c30`, a zero fence, a mismatched command kind, and every
other mask. The existing frozen-profile test replaces its obsolete tuple with
the four accepted tuples and adds explicit obsolete-mask, zero-fence, and
wrong-command-kind rejection assertions. No public type, event kind, error,
Cargo file, or downstream source changes in that task. Kernel formatting,
scoped tests, quiescent reverse-impact tests, and independent review are
required, but only after a new Gate-B acceptance and separate dispatch.

### Predicted digest and fixture closure

An exact in-memory projection using the repository's canonical digest rules
produced:

| Value | Current | Predicted after the exact correction |
|---|---|---|
| reads schema raw SHA-256 | `sha256:acbf50d28c57129794e8becb92236706794468f3ace63afc0a80fcfed1ce29c5` | `sha256:1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542` |
| schema manifest raw SHA-256 | `sha256:a6015dae7f20d379783e0ac0aa22a4125b4b6167625a1beeaadc3f0d00d02ad8` | `sha256:1ffc871f4478732ba4511e6ae413c30cdfb3cdf588a60bc03de5479dcdfd176c` |
| valid-scenarios semantic SHA-256 | `sha256:f8b623b06f84d6f2e2b8f018865b662daf0dee5a59c7e3bbb40d4bd520bce95d` | `sha256:240d33818aaeb9fe6bf89bba0ba7393bca71cb0ec41174a95040fe73916982fd` |
| rejection-vectors semantic SHA-256 | `sha256:094d2bc6109752e44bd6ee2a1c3f8f7f6c1cc97f3fa92fbbdeeb6170dc17afe1` | `sha256:388ddb7eefa13f831fbf149e77438e73715a8e00aa4f1ddf297b4486a694d55a` |
| evaluator raw SHA-256 | `sha256:4e74345485f576a271bf05bae762bee8a11ae821267cdd7573c5d9cd9fc2e36d` | `sha256:fec4fa13f723ee47a2a28d6048ac1e8d1a80dab1f0f599040da08348d4b3830e` |
| fixture index raw SHA-256 | `sha256:fde90bde1b2983a4efe8b4eb224da4ce461ccf69037a77ba6a9c4bb58882740d` | `sha256:0380cbb60d84b44765b19ee86e4559bc2b56b84494d1f23bd8b5661b2d09b991` |
| Gate-B packet canonical SHA-256 | `sha256:c1772ffb53a13f66e796b6399f1b70994ac8e80710e6c46fc0a8e434df4ceca8` | `sha256:25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2` |

The four directly corrected case-semantic hashes are predicted as ordinal 067
`sha256:cfe7cb112ec6a0dcf078e1b58f7ded2662a3e7b1a986faf6bfe0411a896cd1f7`,
078 `sha256:40a43f4ba59ce201b24a61098b26af16f7b4c9b4f6c862d27a90e5dfb19b7136`,
103 `sha256:0bbdb382be855c671e9d76dc50e29edf8527892e4626db8bf8fe9e0d3265cf0e`,
and 105
`sha256:b898ef56937c4a0733da8f81740cd524d4c1fef602ac0b2747f384757778dd95`.
The evaluator projection changes 107 exact JSON leaves: 28 existing
assembly-bound cases plus the four direct runtime cases produce 32 changed case
hashes. The mechanical fixture closure is exactly 36 recipe documents (the 28
existing assembly setup documents plus eight direct mutation/action/expected
documents), all 121 wrappers, and the index: 158 fixture files total. It
preserves every seed, ID, ordinal, count, baseline, unrelated recipe, and the
current terminal-byte profile: the 28 already-authorized setup LFs remain;
the eight direct documents, all wrappers, and the index remain without a final
LF.

The projected Gate-B v3 packet names E0002-18, stays `pending_owner`, keeps
`implementation_dispatch: false`, and differs from v2 at exactly six JSON
paths: task; artifact ordinals 4, 10, and 11; and the valid/rejection semantic
digests. Checks, backend subset, store-error matrix, evaluation counts,
migration 14, material choices, contract, all other schemas, Cargo files, and
implementation bytes remain unchanged. The in-memory projection validates all
eight Draft 2020-12 schemas, the manifest, evaluator, 121 envelopes, and 468
recipe documents. These hashes are predictions, not a freeze or acceptance;
the authorized writers must reproduce them from actual bytes and stop on any
disagreement.

### Validator-safe additive graph proposal

Preserve E0002-16 as done and E0002-17 as blocked historical evidence. Add:

1. E0002-18 in W6, one Sol/high orchestrator planning writer for only the reads
   schema, manifest, evaluator, v3 packet, and bounded edition records;
2. E0002-19 in W7, one Luna/medium fixture writer for only the exact 158-file
   derived closure, followed by three fresh independent read-only reviews and
   an explicit digest-bound owner Gate-B decision; and
3. E0002-20 in W8, one Sol/high kernel writer, pending and non-dispatchable
   until Gate B is accepted and the owner separately dispatches it.

Move the currently blocked storage/runtime/control lanes together to W9, the
two current W7 lanes to W10, and the remaining waves successively through W15.
Replace downstream E0002-17 dependencies with E0002-19/E0002-20 as applicable;
the final integration task depends on E0002-18/19/20 and excludes the blocked,
superseded E0002-17. E0002-06 retains the ordinal-078 audit obligation, and
E0002-07 plus the evaluator harness must align active-second-claim evidence to
`ClaimLease`/`NotActionable` without exposing a raw token. A read-only
projection passes the assignment validator's task-ID, earlier-wave dependency,
three-worker, status/dependency, and same-wave ownership rules with 20 tasks,
only E0002-18 active, and wave cardinalities
`1,1,3,1,1,1,1,1,3,2,1,1,1,1,1` for W1 through W15. Actual task/handoff and
graph files do not yet exist and no projected status is dispatch authority.

This proposal grants no edit or dispatch authority. E0002-17 and all
implementation remain blocked; the current v2 packet is unaccepted. The
product owner may authorize exactly:

> Authorize D-E0002-070's additive E0002-18/E0002-19/E0002-20 graph. Activate
> E0002-18 only for the exact four positive-fence refs, runtime ordinals
> 067/078/103/105, derived manifest/evaluator hashes, and pending Gate-B v3
> packet; then activate E0002-19 only for the exact derived fixture closure.
> Require exact validations and three fresh independent read-only reviews and
> stop for explicit Gate-B acceptance. Keep E0002-20, kernel/source work, the
> shifted implementation lanes, Cargo, external effects, Gate C, commit, push,
> and release closed pending later explicit decisions.

## D-E0002-071 — Product owner authorizes D-E0002-070 additive repair graph

Status: adopted / administrative graph and serialized planning repair · Date:
2026-09-02 · Decision owner: product owner

After receiving D-E0002-070, the product owner directed:

> i authorize D-E0002-070

This adopts D-E0002-070 exactly as written. The orchestrator may install the
validator-safe 20-task W1-W15 graph with E0002-18 ready, E0002-19 and E0002-20
pending, E0002-17 preserved as blocked historical evidence, and every existing
implementation task shifted without changing its candidate source or scope.

Before any product-artifact edit, the complete administrative graph and diff
must validate. PASS permits E0002-18 alone to become active for exactly the
four positive-fence schema references, runtime ordinals 067/078/103/105, the
derived manifest/evaluator hashes, and the pending-owner Gate-B v3 packet.
After its exact freeze signal, E0002-19 alone may activate for the exact
158-file mechanical fixture closure. Its locally passing candidate must then
stop for three fresh independent read-only reviews and an explicit
digest-bound product-owner Gate-B decision.

This decision does not accept Gate B or dispatch E0002-20. Kernel or other
source work, migration/Cargo commands, the shifted implementation lanes,
provider/browser/external effects, E0006 reuse, Gate C, commit, push, and
release remain closed. Any validation, custody, semantic, or predicted-digest
mismatch stops without automatic retry.

## D-E0002-072 — Additive graph gate passes; E0002-18 alone is dispatched

Status: adopted / planning-only artifact dispatch · Date: 2026-09-02 ·
Decision owner: orchestrator under D-E0002-071

The complete D-E0002-070 administrative graph was staged with twenty tasks
across W1-W15, E0002-18 ready, E0002-19/E0002-20 pending, E0002-17 preserved
blocked, the three unfinished implementation lanes shifted together to W9,
and every later dependency updated. Before any artifact edit:

- `rtk scripts/swarm.sh validate AXP-E0002` passed;
- `rtk scripts/swarm.sh validate-assignments
  editions/AXP-E0002/assignments.tsv` passed;
- `rtk git diff --check` passed; and
- an exact assertion passed for all twenty tasks, the sole ready task, shifted
  W9 cardinality, and W15 integration placement.

The administrative gate therefore passes and E0002-18 alone becomes active
for D-E0002-070's exact four schema references, runtime ordinals
067/078/103/105, derived manifest/evaluator hashes, and pending-owner Gate-B v3
packet. E0002-19 remains pending until the exact artifact-freeze signal.
E0002-20, kernel and all other source, fixtures, migrations, Cargo, provider/
browser/external effects, Gate B acceptance, Gate C, commit, push, and release
remain closed.

## D-E0002-073 — E0002-18 stops on reads-schema render-assumption failure

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

Under D-E0002-072, the additive graph and diff gates passed and E0002-18 began
with exact input custody. Reads, manifest, evaluator, and fixture-index raw
SHA-256 values reproduced respectively at `acbf50d2...29c5`,
`a6015dae...2ad8`, `4e743454...e36d`, and `fde90bde...2740d`. Parsed reads
AuditEvent branches 16 through 19 contain exactly the four expected
`SafeInteger` references.

The first candidate command, `rtk python3 -B
/tmp/e0002_e18_transform.py`, stopped before writing any candidate or
repository artifact with:

`noncanonical tracked formatting: .../reads-v1.schema.json`

Read-only diagnosis proved that the schema is valid mixed-format JSON: its
first intentional compact inline object begins at byte 3602, so generic
`json.dumps(indent=2)` expands unrelated bytes. The manifest and evaluator do
round-trip byte-for-byte through that renderer. The reads schema contains 43
total `SafeInteger` references, so a global textual replacement is also
invalid. No schema, manifest, evaluator, packet, fixture, kernel, source,
migration, or Cargo byte changed.

D-E0002-071 requires a stop without automatic retry. E0002-18 is blocked;
E0002-19 was not dispatched; Gate B and all source remain closed. A safe
continuation is bounded to replacing only the rejected whole-file-render
assumption with a surgical edit of the four already identified parsed JSON
paths, verifying that the semantic diff is exactly those four paths and the
raw reads hash is exactly
`sha256:1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542`,
then resuming the otherwise unchanged E0002-18 candidate, custody, validation,
packet, and freeze sequence. Any mismatch still stops. E0002-19 remains closed
until the exact freeze and then only D-E0002-071's existing mechanical authority
applies. No Gate-B acceptance, E0002-20, Cargo, source, external effect, commit,
push, Gate C, or release authority follows automatically.

## D-E0002-074 — Product owner authorizes surgical reads-render continuation

Status: adopted / bounded E0002-18 continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-073, the product owner directed:

> i authorize. proceed

This reactivates only E0002-18 at the recorded candidate-render blocker. The
temporary candidate helper may replace its rejected generic whole-file reads
render with an exact line-preserving transformation of only AuditEvent oneOf
branches 16 through 19 from `SafeInteger` to `PositiveSafeInteger`. It must
prove every other tracked byte is preserved, the parsed semantic diff contains
exactly those four paths, and reads raw SHA-256 equals
`sha256:1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542`
before resuming the otherwise unchanged E0002-18 manifest/evaluator/v3 packet,
custody, validation, and freeze sequence.

Any mismatch stops without retry. E0002-19 remains closed until the exact
freeze, after which D-E0002-071's existing mechanical-fixture authority
applies. Gate-B acceptance, E0002-20, kernel or other source, migrations,
Cargo, provider/browser/external effects, E0006 reuse, commit, push, Gate C,
and release remain closed.

## D-E0002-075 — E0002-18 stops on reads-candidate promotion mismatch

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

Under D-E0002-074, the revised repository-external candidate transformation
replaced only the four certified physical lines and passed every predicted
pre-promotion assertion:

- reads: four parsed paths, raw SHA-256
  `1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542`;
- manifest: one parsed path, raw SHA-256
  `1ffc871f4478732ba4511e6ae413c30cdfb3cdf588a60bc03de5479dcdfd176c`;
- evaluator: 107 parsed leaves, 28 assembly cases, 32 changed case hashes,
  raw SHA-256
  `fec4fa13f723ee47a2a28d6048ac1e8d1a80dab1f0f599040da08348d4b3830e`;
  and
- Gate-B v3: six paths from v2 and canonical SHA-256
  `25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`.

The reads/manifest promotion patch was then applied. Its split context chunks
allowed the first changed hunk to match the earlier structurally similar
branch 11 `lease_acquired` fence instead of branch 16 `approval_decide`. The
first required post-write comparison caught the mismatch and stopped before
evaluator promotion. Current reads raw SHA-256 is
`35bb9a16b1f29dcbec8b7ef592d3e1e548a560c9ec055fa90f34c60065cb7bbf`;
its positive AuditEvent branches are exactly 11, 17, 18, and 19. The pinned
candidate differs from current bytes at exactly two lines: restore branch 11
to `SafeInteger` and change branch 16 to `PositiveSafeInteger`. The manifest
already equals its certified candidate at raw SHA-256
`1ffc871f4478732ba4511e6ae413c30cdfb3cdf588a60bc03de5479dcdfd176c`.
The evaluator remains at `4e743454...e36d`; no packet, fixture, kernel, source,
migration, or Cargo byte changed.

D-E0002-074 requires a stop on mismatch, so E0002-18 is blocked and no writer
is active. A safe continuation is limited to re-verifying the pinned
`/tmp/e0002_e18_reads.json` hash and its exact four-path delta from the
pre-E0002-18 value, proving its current-file delta is exactly the two lines
above, promoting those pinned bytes without a context-search patch, and
requiring exact byte equality before resuming the otherwise unchanged
E0002-18 evaluator, packet, validation, and freeze sequence. E0002-19 remains
closed until the freeze. Gate-B acceptance, E0002-20, Cargo, source, external
effects, commit, push, Gate C, and release remain closed.

## D-E0002-076 — Product owner authorizes pinned reads-candidate promotion

Status: adopted / bounded E0002-18 continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-075, the product owner directed:

> i authorize

This reactivates only E0002-18 at the recorded promotion mismatch. Before any
write, the orchestrator must reproduce the pinned reads candidate raw SHA-256
`1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542`,
reconstruct and hash the pre-E0002-18 reads bytes at
`acbf50d28c57129794e8becb92236706794468f3ace63afc0a80fcfed1ce29c5`,
confirm their parsed delta is exactly AuditEvent branches 16-19, and confirm
the current schema differs from the candidate only by restoring branch 11 and
changing branch 16. It may then promote the pinned candidate bytes without a
context-search patch and must prove exact byte equality immediately.

Only after that PASS may the otherwise unchanged E0002-18 evaluator, v3 packet,
validation, and freeze sequence resume. Any mismatch stops without retry.
E0002-19 remains closed until the exact freeze and then only D-E0002-071's
mechanical authority applies. Gate-B acceptance, E0002-20, Cargo, source,
external effects, commit, push, Gate C, and release remain closed.

## D-E0002-077 — E0002-18 Gate-B v3 candidate is staged

Status: recorded / pending-owner packet candidate / no dispatch · Date:
2026-09-02 · Decision owner: E0002-18 orchestrator

The D-E0002-076 pinned reads promotion and immediate byte comparison passed.
The reads, manifest, and evaluator artifacts now reproduce D-E0002-070's
predicted raw SHA-256 values, and the evaluator passes its schema and semantic
closure audits. The E0002-19 fixture projection independently closes at 36
recipe documents, 121 wrappers, 158 changed fixture files, 16 valid plus 105
rejection envelopes, 468 recipe documents, and predicted index SHA-256
`0380cbb60d84b44765b19ee86e4559bc2b56b84494d1f23bd8b5661b2d09b991`.

The exact pending-owner v3 packet candidate is:

<!-- gate-b-packet-v3:start -->
```json
{
  "schema": "proof.operator.gate-b-packet/v1",
  "edition": "AXP-E0002",
  "task": "E0002-18",
  "date": "2026-09-02",
  "status": "pending_owner",
  "implementation_dispatch": false,
  "artifacts": [
    {
      "ordinal": 1,
      "role": "contract",
      "path": "contracts/operator-control-plane.md",
      "sha256": "sha256:83aa0eec2e1ae7d063776b9febf29009f92316c910728248db5bd40b74e405f0"
    },
    {
      "ordinal": 2,
      "role": "schema",
      "path": "schemas/operator-control/common-v1.schema.json",
      "sha256": "sha256:e64b2278a81db61ccf333005274990366047e133c9a507915c4123e641c3412b"
    },
    {
      "ordinal": 3,
      "role": "schema",
      "path": "schemas/operator-control/auth-v1.schema.json",
      "sha256": "sha256:8ec5777fbb9c7a36484f3503a04f36dd297a880f6cf9c0ba7737384461c5c37d"
    },
    {
      "ordinal": 4,
      "role": "schema",
      "path": "schemas/operator-control/reads-v1.schema.json",
      "sha256": "sha256:1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542"
    },
    {
      "ordinal": 5,
      "role": "schema",
      "path": "schemas/operator-control/mutations-v1.schema.json",
      "sha256": "sha256:0d0ea379971e673fc45cd47d38896b18ddf06712f872ca5a6363d6b07ecf940c"
    },
    {
      "ordinal": 6,
      "role": "schema",
      "path": "schemas/operator-control/durable-v1.schema.json",
      "sha256": "sha256:20c7380cad0692f7459bbb6b4d06d2a9b18810f99e45963fa1941d4f5733f719"
    },
    {
      "ordinal": 7,
      "role": "schema",
      "path": "schemas/operator-control/store-v1.schema.json",
      "sha256": "sha256:afb321e63ae8884ace0be83c7cad08f1a22e60876a082cf092dc7081d66ef4ab"
    },
    {
      "ordinal": 8,
      "role": "schema",
      "path": "schemas/operator-control/evaluator-v1.schema.json",
      "sha256": "sha256:1fae78f5c452ae35a4ab536d1e6e12cf8ae9ec51de4915cd8ff4180a36c9d2c6"
    },
    {
      "ordinal": 9,
      "role": "schema",
      "path": "schemas/operator-control/manifest-v1.schema.json",
      "sha256": "sha256:685f683910f0956987db08a1646aa2aee5f5d884fc13634fb80fa4bbf08467d6"
    },
    {
      "ordinal": 10,
      "role": "schema_manifest",
      "path": "schemas/operator-control/manifest-v1.json",
      "sha256": "sha256:1ffc871f4478732ba4511e6ae413c30cdfb3cdf588a60bc03de5479dcdfd176c"
    },
    {
      "ordinal": 11,
      "role": "evaluator",
      "path": "evals/operator-control-v1.json",
      "sha256": "sha256:fec4fa13f723ee47a2a28d6048ac1e8d1a80dab1f0f599040da08348d4b3830e"
    }
  ],
  "semantic_digests": {
    "valid_scenarios": "sha256:240d33818aaeb9fe6bf89bba0ba7393bca71cb0ec41174a95040fe73916982fd",
    "checks": "sha256:43b5e0daa4fd2f810c04a8ecaf61b295e6069bf8b43a92d111accc4201ac958d",
    "rejection_vectors": "sha256:388ddb7eefa13f831fbf149e77438e73715a8e00aa4f1ddf297b4486a694d55a",
    "backend_subset": "sha256:5298e900fec2d3314cde366a2ec90ca11ea6dd812459bf8e6af0ffb6dd980c3b",
    "store_error_matrix": "sha256:cd99bd03809a467a9478b77ebe2e73fd2959db98cafa038f98744d7491beaa6c"
  },
  "evaluation": {
    "required_score_basis_points": 10000,
    "replay_count": 2,
    "valid_scenarios": 16,
    "ordered_checks": 20,
    "rejection_vectors": 105,
    "backend_subset_scenarios": 4,
    "backend_subset_vectors": 16,
    "store_boundaries": 21,
    "store_error_variants": 9,
    "store_matrix_cells": 189,
    "typed_absence_cases": 4,
    "manifest_logical_shapes": 206,
    "protected_and_public_routes": 15,
    "schema_self_tests": 4
  },
  "migration_14": {
    "contract_section": 14,
    "description": "create governed operator control, projection, fence, budget, command, and audit schema",
    "prior_versions_unchanged": "1-13",
    "validated_up_down": true,
    "operator_tables": 14,
    "immutable_triggers": 20,
    "indexes": 19,
    "pre_14_objects_preserved": true
  },
  "material_choices": [
    {
      "ordinal": 1,
      "id": "independent_terminal_signed_human_challenge_and_volatile_session"
    },
    {
      "ordinal": 2,
      "id": "six_capability_intersection_exact_human_no_delegation"
    },
    {
      "ordinal": 3,
      "id": "loopback_request_error_secret_boundary_with_same_uid_root_limits"
    },
    {
      "ordinal": 4,
      "id": "disposable_workspace_forbidden_repository_root_identity_single_schema14_database_trusted_open_persisted_signers"
    },
    {
      "ordinal": 5,
      "id": "dedicated_router_exact_inventory_and_legacy_route_exclusion"
    },
    {
      "ordinal": 6,
      "id": "migration14_immutable_provisioning_atomic_store_and_legacy_write_rejection"
    },
    {
      "ordinal": 7,
      "id": "approval_explicit_resume_cancel_dispatch_idempotency_revoke_and_signer_order"
    },
    {
      "ordinal": 8,
      "id": "lease30s_renew10s_fenced_recovery_and_distinct_restart_semantics"
    },
    {
      "ordinal": 9,
      "id": "five_dimension_aggregate_reservation_and_forfeit_rules"
    },
    {
      "ordinal": 10,
      "id": "append_only_projection_cursor_mac_audit_redaction_and_static_constraints"
    },
    {
      "ordinal": 11,
      "id": "exact_root_member_dependency_and_lock_deltas"
    },
    {
      "ordinal": 12,
      "id": "no_new_global_execution_error_and_all_required_zero_effect_evaluation"
    }
  ]
}
```
<!-- gate-b-packet-v3:end -->

Packet SHA-256:
`sha256:25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`

This is a staged candidate, not Gate-B acceptance or an implementation
dispatch. E0002-18 remains active only for exact packet reproduction, custody,
validation, and its freeze transition. E0002-19, E0002-20, and all source
remain closed.

## D-E0002-078 — E0002-18 stops on temporary root-pointer assertion

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

After the D-E0002-076 pinned reads promotion passed byte equality, the
manifest and evaluator were promoted from their certified candidates. Their
focused validations passed, and the live raw SHA-256 values reproduce
D-E0002-070 exactly: reads
`1d02cf888c00dcc024fae2ed930e227aa11fdb99b32ec1d498457f7317647542`,
manifest
`1ffc871f4478732ba4511e6ae413c30cdfb3cdf588a60bc03de5479dcdfd176c`,
and evaluator
`fec4fa13f723ee47a2a28d6048ac1e8d1a80dab1f0f599040da08348d4b3830e`.
D-E0002-077 stages the exact pending-owner v3 packet at canonical digest
`25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`.

The next mandated comprehensive command,
`rtk python3 -B /tmp/validate_e0002_e18.py`, exited with
`manifest logical shape mismatch: SchemaManifest`. Read-only diagnosis shows
that the temporary validator accepts only logical-shape pointers beginning
`#/$defs/`. The frozen manifest intentionally maps `SchemaManifest` to the
whole `manifest-v1.schema.json` document with pointer `#`, and that schema's
`LogicalShape.json_pointer` pattern explicitly permits both root `#` and
`#/$defs/...`. The manifest remains byte-identical to its pinned candidate;
the failed check made no repository write. Unchanged pre-E0002-19 fixture-index
custody remains
`fde90bde1b2983a4efe8b4eb224da4ce461ccf69037a77ba6a9c4bb58882740d`,
and `rtk git diff --check` passes.

E0002-18 requires a stop on any reference-check mismatch without retry. It is
therefore `blocked`; no freeze signal was emitted and E0002-19 remains
`pending`. No harness correction, validation retry, fixture propagation,
review, Gate-B acceptance, E0002-20 dispatch, Cargo command, source edit,
external effect, commit, push, Gate C, or release is authorized by this record.

A bounded continuation would change only the temporary validator assertion so
root pointer `#` resolves to the complete schema document while existing
`#/$defs/...` resolution remains exact, then rerun the otherwise unchanged
E0002-18 validation sequence. Any actual parse, schema, reference, semantic,
digest, custody, projection, edition, or diff-check mismatch would stop again
without repair or retry.

## D-E0002-079 — Product owner authorizes root-pointer harness continuation

Status: adopted / bounded E0002-18 continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-078, the product owner directed:

> i authorize a D-E0002-078 continuation

This reactivates only E0002-18. The orchestrator may correct only the temporary
comprehensive validator's logical-shape assertion so pointer `#` resolves to
the complete referenced schema document while `#/$defs/...` resolution remains
exact. It must then rerun the otherwise unchanged complete E0002-18 validation
sequence.

Only an exact PASS permits the `Gate B semantic repair artifacts frozen`
signal, completion of E0002-18, and activation of E0002-19 under the already
adopted D-E0002-071 authority. Any actual parse, schema, reference, semantic,
digest, custody, fixture-projection, edition, or diff-check mismatch stops the
sequence again without repair or retry.

This decision authorizes no artifact or fixture rewrite, Gate-B acceptance,
E0002-20 dispatch, Cargo command, product-source edit, migration, provider or
browser effect, commit, push, Gate C, or release.

## D-E0002-080 — Gate B semantic repair artifacts frozen

Status: adopted / E0002-18 done / E0002-19 mechanical dispatch · Date:
2026-09-02 · Decision owner: orchestrator under D-E0002-071 and D-E0002-079

The D-E0002-079 correction changed only the temporary comprehensive
validator's handling of logical-shape root pointer `#`. The complete rerun
then passed: all eight schemas, exact reads/manifest/evaluator raw hashes, 107
evaluator JSON leaves, 32 changed case hashes, ordinals 067/078/103/105, 28
assembly-bound links, five semantic digests, the exact six-path v2-to-v3 packet
delta, and canonical packet digest
`25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`.

Independent JSON/schema and semantic audits passed. The pinned E0002-19
projection passed strict validation at 36 changed recipe documents, 121
wrappers, 158 changed fixture files, 16 valid plus 105 rejection envelopes,
and 468 recipe documents, with predicted index SHA-256
`0380cbb60d84b44765b19ee86e4559bc2b56b84494d1f23bd8b5661b2d09b991`.
Live artifacts compare byte-for-byte with their certified candidates; the
unchanged pre-E0002-19 index remains
`fde90bde1b2983a4efe8b4eb224da4ce461ccf69037a77ba6a9c4bb58882740d`.
Edition validation and `rtk git diff --check` pass without Cargo.

The orchestrator emits the exact required signal:

`Gate B semantic repair artifacts frozen`

E0002-18 is `done`. Under D-E0002-071, only E0002-19 now becomes `active` for
byte-exact promotion and validation of the already-certified 158-file fixture
projection. It must enter review after local PASS, and three fresh independent
read-only reviews must pass before the product owner receives the exact v3
Gate-B acceptance request.

This freeze is not Gate-B acceptance and is not an implementation dispatch.
E0002-20, shifted W9, Cargo, product source, migrations, provider/browser or
external effects, commit, push, Gate C, and release remain closed.

## D-E0002-081 — E0002-19 fixture closure passes locally and enters review

Status: recorded / local PASS / three-review hold · Date: 2026-09-02 ·
Decision owner: orchestrator under D-E0002-071 and D-E0002-080

E0002-19 preflighted and promoted only the certified projection delta: 36
recipe documents, all 121 wrappers, and `index-v1.json`, exactly 158 files.
The 28 assembly-bound setup documents retain terminal LF; the eight direct
documents, all wrappers, and the index retain no final LF. The full 590-file
live corpus is byte-identical to the pinned projection, and the durable handoff
enumerates the exact changed recipe set plus the complete wrapper/index rule.

Strict live validation passes 16 valid and 105 rejection envelopes, 468 recipe
documents, exact schema/order/cardinality/blueprints, all deterministic seeds,
every document/envelope/index digest link, four directly corrected case hashes,
the terminal-byte profile, and the secret-sentinel scan. JSON/schema, evaluator
semantic, and independent projection validators pass. Final raw SHA-256 values
remain reads `1d02cf88...7542`, manifest `1ffc871f...176c`, evaluator
`fec4fa13...3830e`, and fixture index `0380cbb6...b991`. Edition validation
and `rtk git diff --check` pass without Cargo.

The fixture writer is stopped and E0002-19 enters `review`. Three fresh
independent read-only reviewers must separately cover:

1. schema, manifest, and exact pending-owner v3 packet closure;
2. evaluator, fixture, terminal-byte, and changed-file custody; and
3. command/audit semantics and the planned kernel/source alignment, including
   ordinal 078's storage audit obligation.

Any review failure blocks E0002-19. Three PASS results still do not accept Gate
B or mark the task done; they permit only presentation of exact packet digest
`25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`
for an explicit product-owner decision. E0002-20 requires a separate later
dispatch even after Gate B.

This record authorizes no writer, repair, Gate-B acceptance, E0002-20 dispatch,
Cargo command, product-source edit, migration, provider/browser or external
effect, commit, push, Gate C, or release.

## D-E0002-082 — E0002-19 review stops on missing downstream ownership

Status: recorded / owner decision required / non-dispatchable · Date:
2026-09-02 · Decision owner: orchestrator

After D-E0002-081, three fresh independent read-only reviews began. The
schema/manifest/packet reviewer returned PASS. It independently reproduced all
11 live artifact hashes, eight meta-valid schemas and 1,326 resolved
references, the manifest's 8 files/206 logical shapes/15 routes/4 self-tests,
the exact four-branch reads delta, all five semantic digests, and the exact
six-path v2-to-v3 packet delta. Packet digest
`25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`
remains `pending_owner` with `implementation_dispatch: false`.

The command/audit/source-alignment reviewer returned FAIL despite confirming
that the repaired schema/evaluator/fixture semantics themselves are coherent
and that pending E0002-20 exactly owns the later four-profile kernel alignment:

1. D-E0002-070 requires the ordinal-078 `settle_budget_reservation` rejection
   to append `budget_rejected` atomically before returning `NotActionable`.
   Current storage returns at that state check without the audit append, while
   E0002-06's task and handoff contain no reference to ordinal 078,
   settlement, or this audit obligation.
2. D-E0002-070 requires ordinal-067 runtime evidence to align the active second
   lease claim with `ClaimLease` / `NotActionable`. The current runtime
   contention responder returns `Conflict`, while E0002-07's task and handoff
   do not bind the correction. Their only matching `not_actionable` handoff
   text concerns a different reserve-replay path.

E0002-14 may detect these evaluator vectors later but cannot repair shifted-W9
storage or runtime source. The assignment graph therefore does not durably
close both obligations before implementation completion. Read-only
orchestrator inspection reproduced both omissions. The third, fixture-custody
reviewer was interrupted without a verdict after the required stop; its work
is not counted or reusable.

E0002-19 is `blocked`. All locally validated schema, evaluator, packet, and
fixture bytes remain preserved but unaccepted. No review edited a file or ran
Cargo, and this record authorizes no packet/task repair, artifact or fixture
rewrite, Gate-B acceptance, E0002-20 dispatch, source/Cargo/migration work,
provider/browser or external effect, commit, push, Gate C, or release.

A bounded planning-only continuation would add the exact ordinal-078 atomic
audit obligation to E0002-06's task/handoff and the exact ordinal-067
`ClaimLease`/`NotActionable` alignment obligation to E0002-07's task/handoff,
without changing any source, artifact, fixture, packet digest, dependency, or
other task scope. Edition validation and diff hygiene must pass, followed by a
wholly fresh three-review set. Any mismatch or review failure stops again.

## D-E0002-083 — Product owner authorizes exact ownership-packet repair

Status: adopted / planning-only continuation · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-082, the product owner directed:

> proceed

This authorizes only the bounded continuation proposed in D-E0002-082. The
orchestrator may add evaluator ordinal 078's authority-proven, atomic
`budget_rejected` settlement-rejection obligation to the E0002-06 task and
handoff, and evaluator ordinal 067's active-second-claim
`ClaimLease`/`NotActionable` recording-store obligation to the E0002-07 task
and handoff. Each addition must bind its exact durable/audit/effect outcome and
future focused test without changing source now.

The orchestrator must then reproduce artifact and fixture hashes, validate the
edition and diff, and confirm no other task scope or dependency changed. Exact
PASS permits E0002-19 to return to `review` for three wholly fresh independent
read-only reviews. No result from the D-E0002-081 review set may be reused. Any
validation or review failure stops again without repair or retry.

This decision authorizes no source, artifact, fixture, packet-digest,
dependency, migration, Cargo, provider/browser or external-effect change; no
Gate-B acceptance, E0002-20 dispatch, implementation activation, commit, push,
Gate C, or release.

## D-E0002-084 — Ownership repair validates; wholly fresh reviews begin

Status: adopted / E0002-19 review-only dispatch · Date: 2026-09-02 · Decision
owner: orchestrator under D-E0002-083

The planning-only continuation added exactly two acceptance obligations to
their existing task packets and handoffs. E0002-06 now owns evaluator ordinal
078: after authority is proven, a `ReleasePreDispatch` settlement against a
`dispatching` reservation must preserve reservation/budget/control state,
commit exactly one chained `budget_rejected` audit event atomically, then
return `NotActionable` at runtime stage `settle_budget`, with an exact future
round-trip test. E0002-07 now owns evaluator ordinal 067: its active-second-
claim recording-store test must inject `NotActionable`, surface
`ClaimLease`/`NotActionable`, preserve state and audit, produce zero effects,
and retain digest-only custody without raw token exposure.

No source or test implementation changed and no Cargo command ran. The four
task/handoff additions preserve every dependency and other task scope. Exact
artifact SHA-256 values remain reads `1d02cf88...7542`, manifest
`1ffc871f...176c`, evaluator `fec4fa13...3830e`, and fixture index
`0380cbb6...b991`. Strict 16-valid/105-rejection/468-recipe validation and the
exact 590-file live/projection, 158-file promotion, terminal-byte, direct-case,
and secret-sentinel custody validation pass. Edition validation and
`rtk git diff --check` pass.

E0002-19 therefore returns to `review` for three wholly fresh independent
read-only verdicts covering schema/manifest/packet closure, evaluator/fixture/
byte custody, and command/audit/planned-source alignment. No D-E0002-081 review
result may be reused. Any failure stops E0002-19 again.

This record is not Gate-B acceptance and authorizes no E0002-20 dispatch,
source, fixture, artifact, migration, Cargo, provider/browser or external
effect, commit, push, Gate C, or release.

## D-E0002-085 — Three wholly fresh reviews pass; Gate B awaits owner

Status: recorded / product-owner Gate-B decision required / non-dispatchable ·
Date: 2026-09-02 · Decision owner: orchestrator

The three reviewers created after D-E0002-084 each returned an independent
PASS without editing a file or running Cargo:

1. Schema/manifest/packet review reproduced all 11 artifact hashes, eight
   meta-valid schemas and 1,326 references, manifest closure at 8 files/206
   logical shapes/15 routes/4 self-tests, the exact reads branches 16-19
   delta, all five semantic digests, v2 digest `c1772ffb...eca8`, v3 digest
   `25f64ff5...6eb2`, and exactly the six documented packet paths. V3 remains
   `pending_owner` with `implementation_dispatch: false`.
2. Evaluator/fixture/byte review reproduced evaluator `fec4fa13...3830e`, 107
   changed JSON leaves, 32 changed case hashes, exact ordinals 067/078/103/105,
   the strict 16-valid/105-rejection/468-recipe/590-file corpus, all seeds and
   links, the exact 36-document/121-wrapper/index 158-file delta and every
   before/after hash, 432 unchanged files, exactly 28 assembly setup terminal
   LFs, index `0380cbb6...b991`, and no private-key/JWT/provider-token match.
3. Command/audit/planned-source review confirmed the four positive-fence
   profiles and all four corrected runtime cases; current kernel/storage/
   runtime gaps remain intentionally frozen for later work; E0002-20 exactly
   owns the kernel alignment; E0002-06 and E0002-07 now exactly own ordinals
   078 and 067; E0002-14 remains verification-only; and dependencies keep all
   implementation closed.

Post-review edition validation, `rtk git diff --check`, strict fixture
validation, full live/projection byte custody, and exact reads/manifest/
evaluator/index hashes pass again. E0002-19 remains `review`, not `done`.

The exact pending-owner Gate-B v3 packet is the D-E0002-077 JSON at canonical
SHA-256
`sha256:25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`.
The product owner may now accept, revise, or reject that exact digest. An
acceptance may complete E0002-19 but does not dispatch E0002-20; that kernel
task requires a separate later product-owner decision.

This record grants no Gate-B acceptance, E0002-20 dispatch, source, fixture,
artifact, migration, Cargo, provider/browser or external effect, commit, push,
Gate C, or release.

## D-E0002-086 — Product owner accepts repaired Gate B; no source dispatch

Status: adopted / Gate B v3 accepted / E0002-19 done · Date: 2026-09-02 ·
Decision owner: product owner

After receiving D-E0002-085 and the immediately presented exact packet digest,
with the explicit statement that acceptance completes E0002-19 only and does
not dispatch E0002-20, the product owner directed:

> proceed

In that immediate decision context, this accepts AXP-E0002 repaired Gate B
exactly as staged in D-E0002-077 at canonical packet SHA-256
`sha256:25f64ff58615d495abbe4b1058bf670b11ec6d527485658d8d5863950dc16eb2`,
including all 11 constituent artifact hashes, all five semantic digests, the
evaluation and migration-14 values, and material choices 1-12. The immutable
packet object retains its recorded `pending_owner` proposal state; this dated
digest-bound decision supplies the acceptance.

E0002-19 becomes `done`. This acceptance does not dispatch E0002-20: that task
remains `pending` and non-dispatchable until a separate explicit product-owner
decision. Shifted W9 and every later implementation task remain blocked or
pending. No source, Cargo, migration execution, provider/browser or external
effect, commit, push, Gate C, or release is authorized.

## D-E0002-087 — Product owner dispatches exact E0002-20 kernel alignment

Status: adopted / E0002-20-only source dispatch · Date: 2026-09-02 · Decision
owner: product owner

After receiving D-E0002-086 and reading the exact E0002-20 task packet, the
product owner directed:

> i authorize

All prerequisites are satisfied: E0002-05/E0002-18/E0002-19 are done, repaired
Gate B v3 is accepted at packet digest `25f64ff5...6eb2`, no writer is active,
and `crates/proof-kernel` has no pre-dispatch worktree delta. This separately
dispatches only E0002-20.

E0002-20 may change `AuditEvent::validate_chain_link` so
`StaleFenceRejected` accepts only a rejected event with nonzero `fence_epoch`
and one correlated `(command_kind, presence_mask)` tuple:
`(approval_decide, 0x101e3)`, `(run_cancel, 0x101a3)`,
`(run_resume, 0x2101e3)`, or `(run_resume, 0x4181a3)`. Its existing focused
test must replace the obsolete tuple with all four accepted tuples and
explicitly reject `0x16c30`, zero fence, and wrong command kind. Every other
mask remains rejected. No public type, event kind, error, serialized shape,
Cargo file, artifact, fixture, or downstream source may change.

The owner must run `rtk cargo fmt --check -p proof-kernel`, then
`rtk cargo test -p proof-kernel`. After the writer stops and all affected
writers are quiescent, it must inspect the reverse-impact set and run
`rtk scripts/test-scoped.sh proof-kernel`, followed by exact custody/edition/
diff checks and one fresh independent read-only review. Any failure stops the
task without automatic retry or repair.

This decision does not dispatch shifted W9 or any later task and authorizes no
storage/runtime/control/transport/UI source, migration, dependency, provider/
browser or external effect, commit, push, Gate C, or release.

## D-E0002-088 — E0002-20 exact kernel alignment passes acceptance

Status: recorded / E0002-20 done / shifted W9 still closed · Date: 2026-09-02 ·
Decision owner: orchestrator

The D-E0002-087 writer changed only
`crates/proof-kernel/src/operator/durable.rs`, from raw SHA-256
`e31b7112d32700e6c9a7acd621901dab0fd604f07206dc6a910b3859e55099ce` to
`24c5245df008545b8d3aa709451ac3aee3cc493c550fc4a1b7a7db16732f972a`.
`StaleFenceRejected` now requires `Rejected`, a present nonzero fence, and
exactly the four accepted command/mask correlations. The existing focused
profile test covers all four positives and explicitly rejects the obsolete
mask, zero fence, and a wrong command correlation. No public type, event,
error, serialized shape, Cargo file, artifact, fixture, or downstream source
changed.

`rtk cargo fmt --check -p proof-kernel` passes, and
`rtk cargo test -p proof-kernel` passes 153 tests across six suites. The
reverse-impact list contains `proof-kernel` and 13 workspace dependents. Its
restricted-sandbox run could not bind loopback listeners in two
`proof-operator-control` tests (`EPERM` / `ListenerUnavailable`); the identical
command, repeated with explicit loopback-socket permission and no intervening
source edit, passes 762 tests across 55 suites.

Final kernel custody, `rtk scripts/swarm.sh validate AXP-E0002`, global
`rtk git diff --check`, and the frozen reads `1d02cf88...7542`, manifest
`1ffc871f...176c`, evaluator `fec4fa13...3830e`, and fixture-index
`0380cbb6...b991` hashes all pass. One fresh independent read-only reviewer
reproduced the source transition, exact positive and negative validation,
unchanged public/serialized surface, test evidence, edition/diff checks, the
four frozen hashes, and accepted contract hash `83aa0eec...05f0`; it found no
issue, edited no file, and ran no Cargo.

E0002-20 is therefore `done`. This acceptance does not dispatch E0002-06,
E0002-07, E0002-11, or any later task. Shifted W9 remains blocked pending a
separate explicit product-owner decision. No migration, provider/browser or
external effect, commit, push, Gate C, or release is authorized.

## D-E0002-089 — Product owner dispatches shifted W9 continuation

Status: adopted / E0002-06, E0002-07, and E0002-11 active / W10 closed · Date:
2026-09-02 · Decision owner: product owner

After receiving D-E0002-088 and the exact next-wave boundary, the product owner
directed:

> I authorize shifted W9 continuation for E0002-06, E0002-07, and E0002-11.

This activates only the three named, disjoint task packets. All prerequisites
are satisfied: repaired Gate B v3 is accepted at `25f64ff5...6eb2`;
E0002-05/E0002-08/E0002-19/E0002-20 are done as applicable; all writers are
quiescent; the three writable source scopes plus root manifest/lock have no
pre-dispatch worktree delta; edition validation and global diff hygiene pass;
and the accepted kernel, reads, manifest, evaluator, and fixture-index hashes
remain exact.

E0002-06 owns only its storage scope and exact evaluator ordinal-078 repair:
after authority proof, an invalid pre-dispatch release against a dispatching
reservation must atomically append one chained `budget_rejected` / `rejected`
audit while preserving reservation, budget, and control state, then return
`NotActionable`; invalid authority appends nothing. E0002-07 owns only its
runtime scope and exact ordinal-067 recording-store repair: active second lease
claim must surface `ClaimLease` / `NotActionable` with unchanged state/audit,
zero effects, digest-only custody, and no raw token exposure. E0002-11 owns the
root manifest/lock and control crate, but must first verify the current clean
candidate and must not introduce a speculative source or dependency change if
it already satisfies the frozen contract.

Each owner runs only its local format and scoped crate tests while writers are
active, records durable evidence, and stops. Reverse-impact commands wait until
all three writers are quiescent, then run per changed/accepted crate. Each lane
requires exact custody and a fresh independent non-author review before it can
be marked done. Historical `W4 root ready`, `W4 storage manifest frozen`, and
`W4 lock stable` signals remain immutable evidence and are not rerun or
reinterpreted.

Any cross-owner need, frozen-contract mismatch, unapproved dependency, secret
exposure, real provider/tool/external effect, or exhausted task-local retry
stops only the affected lane with its evidence preserved. This decision does
not dispatch E0002-14, E0002-02, W10, or any later task and authorizes no live
provider/browser work, product database migration, destructive action, commit,
push, Gate C, or release.

## D-E0002-090 — E0002-06 stops at the task-local retry ceiling

Status: recorded / E0002-06 blocked / E0002-07 and E0002-11 continue · Date:
2026-09-02 · Decision owner: orchestrator

The E0002-06 writer implemented the bounded ordinal-078 storage path in only
`crates/proof-storage/src/sqlite/operator_store.rs` and a new focused
`operator_store_tests.rs`. The candidate validates authority first; for an
exact dispatching reservation it commits one chained `budget_rejected` /
`rejected` audit event while preserving reservation, budget, and control
state, then returns `NotActionable`. Invalid authority still appends nothing.

The first combined format/scoped gate passed formatting but failed the storage
suite at 103 passed / one failed because schema-14 indexed `occurred_at` used
`+00:00` while strict event JSON used canonical `Z`. The bounded focused retry
failed on the same unavailable result. Before the orchestrator's stop reached
the worker, additional focused diagnostics exposed that timestamp mismatch and
then a legacy-versus-strict test-baseline JSON mismatch; the worker corrected
both in its owned scope, and its last focused observation passed one test with
103 filtered. Those additional commands and corrections are retained in the
handoff but do not reset or extend the packet's one-retry ceiling.

The source is frozen at `operator_store.rs` raw SHA-256
`5fc1dc130c2f7e0dbbaa3653d846e2d7b84929638964fc20c14a991471c20093`
and `operator_store_tests.rs` raw SHA-256
`c113d8b68dc71ec60e59b031c85b0bded7d92bac3b63502dd0b8e5bede90fcd2`.
There is no known remaining ordinal-078 behavioral failure, but no authorized
post-correction full format/scoped-test result exists. E0002-06 is therefore
blocked and may run no further command or edit without a new product-owner
decision.

This is lane-local under D-E0002-089. E0002-07 and E0002-11 writers are
stopped and locally green, so their quiescent impact, custody, and independent
review gates may continue. E0002-06 reverse impact and review remain closed.
W10 and every later task remain closed. No live provider/tool/browser effect,
real product database, destructive action, commit, push, Gate C, or release
occurred.

## D-E0002-091 — E0002-11 passes; E0002-07 stops on review gap

Status: recorded / E0002-11 done / E0002-07 blocked / W10 closed · Date:
2026-09-02 · Decision owner: orchestrator

After all shifted-W9 source writers stopped, the E0002-07 reverse-impact list
contained only `proof-agent-runtime` and `proof-transport-cli`. The restricted
run passed all 143 runtime tests but blocked 15 CLI tests on sandbox-only
`EPERM` and synthetic unsafe-root ownership. The identical command with host
UID/filesystem/process permission and no intervening source edit passed 267
tests across five suites. Runtime source custody is one test-only delta in
`src/operator.rs`, raw SHA-256 `027a17c6...ab4b`; no public or production
runtime behavior changed.

The fresh runtime reviewer confirmed that ordinal 067 itself is exact:
`ClaimLease` / `NotActionable`, unchanged first lease and durable audit/
mutation state, no added event, zero provider/tool/governed-write/external
effect, digest-only request custody, and no raw-token diagnostic exposure.
It also found one acceptance regression: the change replaced the sole
barrier-synchronized two-thread E0002 lease-claim race and exactly-one-success
assertion with the sequential ordinal-067 test. Repository-wide inspection
found no equivalent concurrent `OperatorRuntime::claim_lease` one-winner
coverage. Because E0002-07 consumed its single bounded retry on a test-only
compile assertion, the reviewer made no repair and E0002-07 is blocked. A safe
later continuation is test-only: preserve the ordinal-067 test exactly and add
separate barrier-synchronized concurrent claim one-winner coverage, then rerun
the full local/impact/custody/review sequence.

E0002-11 required no source, dependency, root-manifest, or lockfile change.
Its local format check passed; the restricted local test reported 15 passes
and two loopback-only environment failures, while the identical permitted run
passed 17 tests across two suites. Its quiescent reverse-impact set was only
`proof-operator-control` and passed the same 17 tests. A fresh independent
security reviewer then passed the complete frozen synthetic-shell boundary:
exact loopback and clean URL, nonconfigurable OS environment, build-anchored
authoritative opener, one-use signed Human challenge/session, same-origin and
security headers, closed 15-route/static surface, volatile restart/revoke,
ordered shutdown, secret custody, exact approved dependencies/rustix, no
legacy route, and no E0006 reuse. Root manifest `f739ab46...f382`, lockfile
`b85b5d84...8aed`, and control `lib.rs` `e9c8e559...f646` remain exact.
E0002-11 is therefore done; real SQLite/runtime/router/static composition
remains assigned to E0002-14/E0002-15.

The accepted kernel/contract/artifact/semantic/packet/fixture hashes, edition
validation, and diff hygiene remain exact. No file changed during either fresh
review. E0002-06 remains blocked under D-E0002-090, E0002-07 is blocked under
this decision, no task is active, and W10 remains closed. No provider/tool/
browser effect, real product database, commit, push, Gate C, or release is
authorized or claimed.

## D-E0002-092 — Product owner authorizes exact storage/runtime continuations

Status: adopted / E0002-06 verification-only and E0002-07 test-only active /
no automatic retry / W10 closed · Date: 2026-09-02 · Decision owner: product
owner

After receiving the exact D-E0002-090 and D-E0002-091 continuation boundary,
the product owner directed:

> i authorize

In that immediate context, this reactivates only E0002-06 and E0002-07 for the
two remedies presented. E0002-11 remains done. Pre-dispatch custody reproduces
storage `operator_store.rs` `5fc1dc13...0093`, storage
`operator_store_tests.rs` `c113d8b6...fcd2`, runtime `operator.rs`
`027a17c6...ab4b`, unchanged control/root/lock hashes, all accepted frozen
hashes, edition validation, and tracked diff hygiene.

E0002-06 is verification-only. It may make no source or test edit. It runs
`rtk cargo fmt --check -p proof-storage` and, only on PASS,
`rtk cargo test -p proof-storage` against the exact pinned corrected bytes.
Any failure stops without retry. On both passes the writer stops; only then may
the orchestrator run the quiescent storage reverse-impact, exact custody, and a
fresh independent non-author review.

E0002-07 is test-only. It must preserve the accepted ordinal-067 sequential
test and every production/API byte, and add one separate barrier-synchronized
two-thread lease-claim test that proves exactly one owner/success,
deterministic losing actionability, and zero boundary effect. It may
mechanically format only its owned test edit, then runs the local format check
and full scoped runtime suite once. Any failure stops without retry. On PASS
the writer stops before quiescent reverse impact, custody, and a wholly fresh
independent non-author review.

The two lanes remain disjoint. Neither may edit kernel, control, transport,
contract, schema, evaluator, fixture, root Cargo files, or another handoff.
This decision does not dispatch E0002-14, E0002-02, W10, or later work and
authorizes no live provider/tool/browser effect, product database operation,
destructive action, commit, push, Gate C, or release.

## D-E0002-093 — Exact no-retry continuations stop at local gates

Status: recorded / E0002-06 and E0002-07 blocked / owner decisions required ·
Date: 2026-09-02 · Decision owner: orchestrator

E0002-06 first reproduced its pinned `operator_store.rs` and
`operator_store_tests.rs` hashes exactly. Its authorized
`rtk cargo fmt --check -p proof-storage` then failed only on three mechanical
layouts in the new test file: the kernel import group and two long equality
assertions. Per D-E0002-092, it ran no scoped test, made no edit, and did not
retry. Storage remains at `5fc1dc13...0093` / `c113d8b6...fcd2`; ordinal 078's
last focused observation remains green but is not full-suite acceptance.

E0002-07 preserved ordinal 067 and every production/API byte, and added only
the separate barrier-synchronized two-thread test
`concurrent_lease_claim_has_one_owner_and_not_actionable_loser`. The new test
proves exactly one live owner, one `ClaimLease` / `NotActionable` loser, two
distinct claim identities, and zero boundary effects. Its format check passed.
The single authorized full suite then stopped at 143 passed / one failed. The
new test was among the passes; the sole failure was the pre-existing
`live_resume_accepts_authority_valid_through_original_deadline` setup, which
rejected its delegation before execution. Runtime source is frozen at
`operator.rs` `26ad1547...6048`; unchanged `runtime.rs` is
`99437aee...5f1`.

Read-only inspection shows that positive live-resume test samples
`Utc::now() + 301 seconds`, while runtime separately samples a 300-second
required deadline. Under scheduling delay the one-second margin can expire;
the distinct negative boundary test remains at 299 seconds. This is an
inference from the exact source and generic validation branch, not a rerun.
No runtime impact or new independent review ran after the failed local gate.

Both lanes are blocked and no task is active. The smallest safe next authority
is: (1) storage mechanical-format-only at exactly the three reported test-file
locations, followed once by format/full scoped gates; and (2) runtime
test-only stabilization of only the positive live-resume validity margin from
301 to 360 seconds while retaining the 299-second negative case, ordinal 067,
and the new concurrent test, followed once by format/full scoped gates. Any
failure would stop without retry. Only after both local passes may quiescent
impact, custody, and wholly fresh independent reviews resume.

E0002-11 remains done. W10 and all later work remain closed. No provider/tool/
browser effect, product database operation, destructive action, commit, push,
Gate C, or release occurred or is authorized.

## D-E0002-094 — Product owner authorizes exact mechanical/test continuations

Status: adopted / E0002-06 mechanical-only and E0002-07 positive-test-only
active / no retry / W10 closed · Date: 2026-09-02 · Decision owner: product
owner

The product owner repeated the exact proposed D-E0002-093 authority:

> I authorize the exact D-E0002-093 storage mechanical-format-only and runtime
> positive-deadline test-stabilization continuations.

E0002-06 may apply only rustfmt's three already reported layout changes in
`operator_store_tests.rs`: the kernel import group and two long equality
assertions. No behavior, migration, production storage, or other test change is
authorized. Starting hashes are `operator_store.rs` `5fc1dc13...0093` and test
file `c113d8b6...fcd2`. It must inspect the mechanical delta, run one storage
format check and, only on PASS, one full scoped storage suite. Any mismatch or
failure stops without retry.

E0002-07 may change only the positive test
`live_resume_accepts_authority_valid_through_original_deadline` margin from
301 to 360 seconds. The separate 299-second rejection test, ordinal-067 test,
new synchronized concurrent-claim test, and every production/API byte must
remain exact. Starting hashes are `operator.rs` `26ad1547...6048` and
`runtime.rs` `99437aee...5f1`. It runs one runtime format check and one full
scoped runtime suite. Any mismatch or failure stops without retry.

The two lanes remain disjoint and stop before reverse impact. Only after both
writers quiesce on local PASS may the orchestrator run their scoped impact,
exact custody, and wholly fresh independent reviews. E0002-11 remains done.
This decision does not dispatch W10 or later work and authorizes no provider/
tool/browser effect, product database operation, destructive action, commit,
push, Gate C, or release.

## D-E0002-095 — Shifted W9 passes final acceptance

Status: recorded / E0002-06, E0002-07, and E0002-11 done / no active task /
W10 dependency-ready but not dispatched · Date: 2026-09-02 · Decision owner:
orchestrator

Both D-E0002-094 writers stopped after their exact local sequences passed.
Storage applied only the three authorized rustfmt layouts, then passed
`rtk cargo fmt --check -p proof-storage` and 141 tests across 8 suites. Runtime
changed only the positive live-resume test margin from 301 to 360 seconds,
preserved both 299-second negatives plus the ordinal-067 and synchronized
concurrent-claim tests, then passed its format check and 144 tests across 2
suites. No production runtime/API byte changed.

After both writers quiesced, `rtk scripts/test-scoped.sh proof-storage` passed
332 tests across 23 suites for the five-crate storage impact set, and
`rtk scripts/test-scoped.sh proof-agent-runtime` passed 268 tests across 5
suites for runtime plus CLI. No source edit occurred between local and impact
gates. Global diff hygiene and `rtk scripts/swarm.sh validate AXP-E0002`
passed.

Post-impact storage custody is `operator_store.rs`
`5fc1dc130c2f7e0dbbaa3653d846e2d7b84929638964fc20c14a991471c20093`
and `operator_store_tests.rs`
`31ec8f21cdb2846f5cf76d4b80ea0e16136e48b72ede7e2d70698a22139e180b`.
Migration custody remains `migrations.rs`
`69b219a3dc6a0a757af68685de055221f2a5a62b64b84c5b70244f5edb83c800`,
up SQL `f4f4fcd6c0a15702e1bf8cc04f64d86b237a7f95e36d48f351fc6e606e06e1ef`,
and down SQL
`e4dd549e5a160840f6c2b70b4cb7d534f5c799984ed79ea8eb47736a21c9b10b`.
Runtime custody is `operator.rs`
`26ad1547f548fb71a68b3d474474f82f22d1c93b0155527169de03310b696048`
and `runtime.rs`
`c650e9e539fe6313ec33edb26fedb2a7acb4727f79625061b45ae076563a4064`.
Root/control custody remains `Cargo.toml` `f739ab46...f382`, `Cargo.lock`
`b85b5d84...8aed`, and control `lib.rs` `e9c8e559...f646`.

Two wholly fresh independent read-only reviewers returned PASS. Storage review
confirmed authority-first ordering, canonical audit timestamp encoding, exact
dispatch-pointer validation, one committed `BudgetRejected` / `Rejected`
event followed by `NotActionable`, unchanged reservation/budget/control/effect
rows across schema-14 reopen, and later explicit full forfeiture. Runtime
review confirmed ordinal 067's preserved durable/audit state and digest-only
custody, a non-false-passing synchronized one-winner contention test with zero
effects, and that the 301-to-360 adjustment is positive-fixture slack only
while the 299-second negatives and 300-second production boundary remain
exact. Reviewers made no edit and ran no Cargo/test command or external effect.

E0002-06 and E0002-07 therefore join already accepted E0002-11 as done, and
shifted W9 is complete. E0002-14 and E0002-02 have satisfied dependencies and
are now W10 dependency-ready, but both remain `pending`. A separate explicit
product-owner decision is required to dispatch W10. This acceptance performs
no W10 source/manifest/lock work and authorizes no provider/tool/browser
effect, product database operation, destructive action, commit, push, Gate C,
or release.
