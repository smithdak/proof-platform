# AXP Edition Charter

- Edition ID: `AXP-E0002`
- Title: One Human Oversees Many Runs
- Owner: product owner
- Base revision: `b5646b50689d41837ed7fcdaca431b1421f693ab`
- Status: `blocked — Gate-B consistency repair authorized; implementation closed`
- Planning authority: D-E0006-025, E0002 Gate A scaffolding only
- External dependency disposition: AXP-E0006 Gate C was deferred/no-go;
  E0006 is quiescent, unreleased, and supplies no authority to this edition

## Outcome

User/problem: one Human operator cannot yet see or safely control several
durable agent runs as one governed workload. Existing APIs expose per-run state
without an independently authenticated, least-privilege operator boundary,
stable attention projection, complete command chronology, or race-safe control
semantics.

North-star journey:

1. The Human starts one loopback-only operator control plane and establishes an
   independent volatile session through an E0002-owned signed challenge rooted
   in a freshly enrolled Human identity inside a disposable trusted workspace.
   The challenge and session bind the exact Human, workspace, server instance,
   requested/granted capabilities, nonce, and expiry; no E0006 bootstrap,
   session, repository-root `.proof` identity, or historically exposed key
   participates.
2. An attention inbox shows a bounded, filterable, keyset-paginated projection
   of at least four governed runs: awaiting decision, running, recoverable, and
   terminal. Authentication and scope checks occur before enumeration.
3. The Human opens one run and sees its goal, current checkpoint, authority,
   budget, attempts, pending consequence, exact arguments, approval identity,
   and append-only audit chronology without raw secret or workspace-path
   leakage.
4. The Human denies one exact approval and approves another. Approval records a
   signed decision but never resumes execution automatically.
5. The Human explicitly resumes the approved run and cancels a different run
   before its next provider/tool dispatch. Cancel and resume each produce one
   durable idempotent command receipt and one race winner.
6. After a runtime-worker restart while the control plane remains live, fenced
   recovery has one owner, the still-valid scoped session can re-read durable
   state, projections and audit chronology are stable, aggregate budgets remain
   enforced, and stale worker epochs cannot write.
7. After a separate operator-control-plane restart, all volatile session
   authority is invalid. The durable runs remain intact, but the Human must
   establish a fresh signed challenge/session before reading or acting again.
8. The Human explicitly ends the new operator session. Session revoke, approval
   denial, and run cancellation remain distinct controls and audit events; page
   memory loss leaves no reusable browser credential.

Success metric and declared evaluation: proposed
`proof-operator-control/v1` is all-required. Before implementation fan-out,
E0002-13 must freeze a strict evaluator and rejection-vector digest covering
independent scoped authentication, auth-first disclosure control, projections
and pagination, exact approval decisions, explicit resume, zero-effect cancel,
command idempotency and uncertain responses, fenced recovery, aggregate
budgets, append-only audit, separate worker/control-plane restart semantics,
browser credential hygiene,
accessibility, and signed evidence linkage. The candidate passes only at
100% of required checks, with no skipped check, unauthorized read/write,
automatic resume, duplicate effect, stale-fence write, provider overrun, or
unexplained audit gap.

## Scope

In scope:

- An E0002-owned operator authentication design that is independent of E0006
  and binds exact `Human`, workspace, server instance, and capabilities.
- A disposable trusted evaluation workspace with freshly generated and
  enrolled Human and Agent identities. The repository-root `.proof` identity
  is classified as compromised and MUST NOT authenticate, sign, seed, or
  validate any E0002 fixture, browser journey, or acceptance run.
- Fresh operator-session issuance rooted in an enrolled Human signature over an
  instance/workspace/nonce/capability/expiry challenge. Granted capabilities
  are the intersection of the signed request, workspace policy, and server
  support; a client cannot self-grant authority.
- Proposed minimum capabilities: `run.read`, `approval.read`,
  `approval.decide`, `run.cancel`, `run.resume`, and `audit.read`.
- Loopback-only control-plane boundaries with exact Host, Origin, JSON media,
  no-CORS, no-cookie, no-Web-Storage, memory-only session behavior.
- One Gate-B-frozen authoritative workspace database and trusted-open policy.
  The control plane MUST NOT silently split native CLI/MCP runs from HTTP/WS
  state or inherit an ephemeral transport signer.
- A dedicated same-origin operator router that exposes only frozen protected
  operator and static-app surfaces. Existing unauthenticated audit, proof,
  domain-list, and generic operation routes are excluded unless Gate B binds
  an explicit authenticated composition and rejection policy.
- Auth-first, redacted attention/run/approval/audit projections with bounded
  keyset pagination and filter-bound cursors.
- Append-only operator command receipts and audit chronology for decisions,
  cancel, resume, session revoke, conflicts, expiry, and recovery.
- Durable fenced run ownership, restart recovery, and aggregate budget
  enforcement across concurrently governed runs.
- An accessible operator console, deterministic browser/API evaluation, and
  independent non-author verification.

Non-goals:

- Releasing, repairing, retrying, or running the AXP-E0006 standalone UI.
- Reusing, wrapping, relabeling, or claiming the E0006 bootstrap, session,
  evidence, release status, or authority.
- Remote access, wildcard binding, multi-tenant production auth, SSO, cookies,
  persistent browser credentials, permissive CORS, or production deployment.
- Native parent/child swarm scheduling, delegated worker topology, or task-tree
  coordination; those remain AXP-E0003 outcomes.
- Bulk approvals, automatic resume after approval, withdrawal/replacement of a
  durable signed decision, or conflating session revoke with run cancellation.
- New domain operations, external publication, destructive effects, or live
  model/provider work.
- Any public contract, schema, evaluator, migration, source, browser ceremony,
  product fixture, or root-manifest change during E0002-01 Gate A scaffolding.
- Repairing general HTTP/MCP/WebSocket/CLI domain-handler parity, legacy
  transport database layout, or unrelated public routes; those remain a
  separate tracked outcome unless required to isolate the E0002 control plane.

Budget (time, agent/model, tokens, live spend):

- Current state: the bounded `gpt-5.6-sol` E0002-13 contract-freeze attempt and
  read-only reviews completed within cap. D-E0002-013 accepted Gate B and
  dispatched only the three dependency-ready W3 lanes, with zero provider use,
  zero live spend, and no external effect.
- Proposed edition: seventeen tasks across W1-W12, one orchestrator plus at most
  three concurrent workers, one retry maximum per failed task before
  task-local escalation, and no full-edition model upgrade.
- Per-attempt ceilings are 45 minutes / 25,000 combined model tokens for Luna
  mechanical work, 90 minutes / 50,000 for Terra bounded implementation,
  120 minutes / 80,000 for Sol high-risk work, and 180 minutes / 120,000 for
  the Sol xhigh integration task. The current primary ceiling is 1,230,000
  combined model tokens and 31h30 agent time. Every task packet records its
  exact ceiling.
  The primary-plan ceiling is 1,125,000 combined model tokens and 28 hours 45
  minutes of agent time; the task-local one-retry policy is a hard maximum of
  twice those totals and requires preserved failure evidence.
- Routing: `gpt-5.6-luna` for mechanical frozen fixtures,
  `gpt-5.6-terra` for bounded HTTP read/UI work, and `gpt-5.6-sol` for the
  contract, auth, kernel, migration/storage, runtime races, control-plane
  assembly, mutations, independent security verification, and integration.
- External/live spend: zero. Any paid provider or external effect is a new
  Gate B decision and is not part of the proposed acceptance journey.

Material-risk triggers requiring Gate B:

- The independent operator authority/public contract, secrets and session
  issuance/handling/expiry/revocation, new auth crate, root manifest/lockfile
  changes, exact API/DTO/error policy, capability-grant policy, and separation
  of duties between authenticated operator and required approval Human.
- Every SQLite migration, append-only command/audit record, fenced lease,
  aggregate budget, authoritative workspace database/path, trusted-open rule,
  signer lifecycle, and recovery rule.
- Approval/cancel/resume mutation ordering, signing-key access, provider/tool
  dispatch barriers, uncertain-response recovery, and any remote boundary.
- Dedicated-router composition, exclusion or authentication of every legacy
  route, forwarded-client metadata policy, and the decision to avoid new
  global `ExecutionError` variants or map each one in HTTP in the same wave.
- Any added dependency, persistent credential, public exposure, destructive or
  external effect, paid provider use, or material scope expansion.

## Approval

- Gate A approver/date: product owner accepted 2026-09-01 in D-E0002-011
- Gate B decision/date: product owner accepted 2026-09-01 in D-E0002-013,
  binding D-E0002-012 packet digest
  `sha256:eaff3d4d78ca3e6e4fe521f53b12b9598765db50ffd38fde0d6bf3aeb4c42dd4`
- Gate C decision/date: pending; no candidate or release claim exists

Gate B completes E0002-13 and authorizes only E0002-05, E0002-08, and E0002-12
under their exact W3 paths, barriers, budgets, and acceptance tests. Every
later task remains pending and non-dispatchable.
