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

Status: proposed / owner decision required · Date: 2026-09-01 · Decision owner:
product owner

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
SQLite migration number and shapes, auth/session secret handling, exact
challenge/issuance/expiry/revoke flow, capability-grant and separation-of-duties
policy, failure order, DTO redaction, cursor bindings, append-only
commands/audit, approval/cancel/resume ordering, fenced recovery, aggregate
budgets, and rollback. Gate B must explicitly accept or reject those artifacts
before W3.

## D-E0002-004 — Proposed routing and retry policy

Status: adopted for planning only · Date: 2026-09-01 · Decision owner:
orchestrator

One mechanical frozen-fixture task starts on Luna; bounded HTTP reads and UI
start on Terra; contract, auth/security, kernel authority, migration/storage,
runtime races, loopback control-plane assembly, mutations, verification, and
integration use Sol. Each task has at most one bounded retry; a failed
evaluation escalates only that task and preserves its handoff. This routing
does not dispatch any task.

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

Record future scope changes, escalations, cross-owner requests, Gate B
decisions, and explicit exceptions here. Never silently broaden a task.
