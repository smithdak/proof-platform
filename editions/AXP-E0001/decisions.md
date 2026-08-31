# E0001 Decisions and Exceptions

## D-E0001-001 — Gate A boundary

Status: Gate A approved · Proposed: 2026-08-29 · Approved: 2026-08-30 · Decision owner: product owner

Recommend defining E0001 as one live OpenAI Responses-backed Release Manager
journey that pauses for signed human approval, resumes the same run and step
after a process boundary, and produces an immutable, independently verifiable
local preview artifact. External deployment and production publication remain
out of scope.

Gate A authorizes only the charter and W1 contract/evaluation design. It does
not authorize public contract changes, implementation, credentials, paid API
use, or a live attempt.

The product owner approved this exact boundary and directed the orchestrator to
proceed. E0001-01 may run; every downstream E0001 task remains locked behind
its recorded dependencies and owner gates.

## D-E0001-002 — Gate B contract questions

Status: Gate B approved · Proposed: 2026-08-30 · Approved: 2026-08-30 · Decision owner: product owner

E0001-01 returned one bounded decision packet covering:

1. Prefer a backward-compatible `release.publish` new version over changing v1
   in place. Bind UUIDv7 idempotency, existing edition ID, environment, version
   label, preview artifact digest, canonical output, and original engine proof.
2. Define durable model-attempt states and distinguish a safely retryable
   pre-response failure from an ambiguous provider completion. Ambiguous states
   stop for reconciliation; they never automatically dispatch or replay a tool.
3. Version the sealed live evaluation policy and bind journey, provider/model,
   exact call, approval expiry/chronology, budgets, recovery limits, artifact,
   terminal output references, policy digest, and trace digest.
4. Define how cost is bounded and reported when the provider response supplies
   token usage but not authoritative micro-USD cost.

The first independent audit rejected the initial packet because the live SQLite
path cannot load or preserve operation-scoped delegations and the CLI reads the
credential before the proposed preflight. The revised Gate B packet must own
those needs explicitly: E0001-06 kernel versioning, E0001-07 storage migration
and delegation loading, and E0001-08 CLI scope/preflight behavior. It must also
freeze complete attempt, binding, tool-schema, exact-check-set, dispatch-before-
I/O, external-effect, and RegistryEntry contracts before implementation.

No W2 task is authorized until the repaired packet passes independent re-audit
and the product owner approves the resulting public-contract scope.

The 2026-08-30 re-audit also rejected the first repair: digest-bearing prompt,
goal, continuation, and schema literals differed between prose and policy; the
prose retained a runtime/CLI dependency cycle; the 20-vector set was not exact;
the deferred provider-construction API was not frozen across runtime/CLI
ownership; and scope JSON did not explicitly reject unknown keys. A second
bounded repair corrected the graph to W2 kernel +
storage, W3 runtime + Content, W4 CLI, W5 live verification, and W6 integration.

The second repair passed final independent re-audit. The owner-ready Gate B
decision is now exactly:

1. **B1 — operation:** add active `release.publish::v2` after implementation;
   preserve active v1 unchanged.
2. **B2 — kernel/authority:** add default-compatible version-aware handler
   hooks and require the live journey's explicit loaded delegation ID/scope.
3. **B3 — runtime/CLI:** add durable provider-attempt recovery, the additive
   lazy gateway-factory seam, scoped live start/resume, and preflight-before-
   credential ordering.
4. **B4 — evaluator:** adopt the strict versioned live policy with exact 17
   checks, exact ordered 20 tamper vectors, and all declared digests.
5. **B5 — paid provider, conditional:** authorize one direct
   `gpt-5.6-sol` Responses journey, maximum USD 0.15, only after deterministic
   preflight is independently 10/10 and every W2-W4 acceptance gate passes.
6. **B6 — migration/effects:** append SQLite v12, allow one immutable local
   preview artifact after signed approval, allow only B5's direct provider
   request as the remote exception, and preserve failure evidence/rollback.

Recommendation: approve B1-B4 and B6; conditionally approve B5. Approval starts
W2 E0001-06/E0001-07 only. It does not read a credential, run a migration on a
live workspace, call the provider, or authorize W5 before its prerequisites.

The product owner approved this recommendation and directed the orchestrator
to proceed. B1-B4 and B6 are approved. B5 is conditionally approved exactly as
written: one direct `gpt-5.6-sol` journey, maximum USD 0.15, only after W2-W4
pass and deterministic preflight is independently 10/10. This decision opens
only W2 E0001-06/E0001-07 now; it does not authorize either worker to read a
credential, call a provider, or execute a migration against a non-test/live
workspace.

## D-E0001-003 — Gate B live-use questions

Status: conditionally approved; activation pending prerequisites · Date: 2026-08-30 · Decision owner: product owner

Before E0001-04, the owner must explicitly approve the exact OpenAI model,
direct endpoint or reviewed proxy, maximum USD spend, maximum attempts, human
approver, synthetic data payload, evidence-retention/redaction policy, and
failure cleanup. No credential value may enter Git, an edition record, a handoff,
or chat evidence.

The owner conditionally approved the frozen direct endpoint, exact
`gpt-5.6-sol` model, one primary journey, and USD 0.15 ceiling through B5.
Credential availability, the distinct enrolled human approver, deterministic
10/10 evidence, W2-W4 acceptance, redaction/retention verification, and failure
cleanup remain runtime prerequisites. Conditional approval is not permission
to bypass any of them.

## D-E0001-004 — Durable SQLite terminal and approval ordering

Status: implemented clarification within approved B3/B4 · Date: 2026-08-30 · Decision owner: orchestrator

Real-SQLite W4 integration exposed three ordering seams that in-memory tests did
not reveal. The approved recovery and evaluator contract resolves them as
follows:

1. Persist and reread the signed approval request before inserting its
   approval-gated step, so the foreign key is valid and restart recovery binds
   the exact durable request.
2. On success, persist the `Succeeded` run before appending the exact
   `Completed` event. A matching sealed `Succeeded`, `Failed`, or
   `BudgetExceeded` replay is read-only; only a separately detected missing
   immutable evaluation may be recovered through its owning API.
3. Approval evaluation binds durable principal ID, kind, and public key.
   Storage-synthesized `created_at` is excluded because it is not persisted;
   any durable identity/key substitution remains visible and fails evaluation.

These are integration clarifications of the frozen semantics, not a contract,
schema, policy, or live-use expansion. The final runtime suite, dependent CLI
SQLite success filter, and independent recovery/security reviews all pass.

## D-E0001-005 — Public credential-free readiness prerequisite

Status: approved and active · Proposed: 2026-08-30 · Approved: 2026-08-30 · Decision owner: product owner

The post-W4 workflow audit found a hard operational blocker: supported public
CLI commands cannot create both a fresh terminal deterministic run with an
immutable exact 10/10 evaluation and the safe synthetic edition/manifest/
UUIDv7 bindings required by `agent live-start`. The legacy start command needs
a provider credential, generic run commands create evidence the live verifier
correctly rejects, and the only equivalent materializer is test-only.

Add E0001-09 as a CLI-only W5 prerequisite. It must drive the actual runtime,
existing signed approval command, evaluator, storage APIs, safe filesystem
creation, and existing live setup validation across a process boundary. It may
not fabricate ledger rows, read provider environment variables, construct a
live gateway, send a request, create the live provider-backed run, or change a
shared contract.

The prior live and integration tasks move to W6 and W7. Conditional B5 remains
inactive until E0001-09 passes implementation and independent security audits
and its public workflow produces fresh independently verified 10/10 readiness
evidence. This amendment narrows the live boundary; it does not increase model,
spend, attempt, data, retention, or external-effect authority.

## D-E0001-006 — Runtime-owned check-only validation seam

Status: required corrective split within approved B3/B4 · Date: 2026-08-30 · Decision owner: orchestrator

Independent E0001-09 contract review rejected the assumption that CLI
`start_setup` alone is the same check-only validation used by the runtime.
The runtime's authoritative `validate_live_setup` and target-agent validator
are private and currently run together only on `run_live`, which creates a
provider-backed AgentRun. Creating a sacrificial run or relying on a factory
failure would violate the credential and no-live-run boundary.

Add E0001-10 as a runtime-only W5 task. It exposes one public read-only
`check_live_start_setup` method that reuses the existing authoritative setup
and target-agent validators, accepts only start intent, performs no mutation,
and never invokes a gateway factory. The existing live-start path calls that
same method and retains its final timestamp-selected revalidation immediately
before the first run write.

E0001-09 moves to W6 and must call this API after constructing the normal
`LiveRunSetup`; CLI validation remains defense in depth but is not a
substitute. The live and integration tasks move to W7 and W8. This additive API
does not change the frozen validator semantics or expand provider, credential,
spend, data, attempt, retention, or external-effect authority. Conditional B5
remains inactive.

## D-E0001-007 — Generic runtime bootstrap reconciliation

Status: required corrective split within approved B3/B4 · Date: 2026-08-30 · Decision owner: orchestrator

Fault-oriented E0001-09 review proved that generic `AgentRuntime::start`
persists a new run before its initial checkpoint and Started event. A process
death after the Queued/Running save leaves the unique preparation-bound run
visible, but public `resume` currently fails on the missing checkpoint.
Creating a replacement run or letting CLI fabricate the missing ledger would
violate exact-once recovery.

Add E0001-11 as a runtime-only W6 task. Runtime resume may reconcile only the
exact pristine bootstrap states created by start: validated actor/agent/mode/
goal/revision chronology with no conflicting step, evaluation, approval,
checkpoint, or event evidence. It writes/rereads the initial state and exact
Started barrier once, then continues normal drive. Any evidence of later or
ambiguous work fails closed; no replacement run is allowed.

E0001-09 depends on E0001-11 and remains blocked until fault-injection and
independent recovery audit pass. This does not authorize ambiguous provider or
tool retries and does not expand live-use authority.

## D-E0001-008 — Pre-read pinned workspace and SQLite boundary

Status: required corrective split within approved B3/B6 · Date: 2026-08-30 · Decision owner: orchestrator

The E0001-09 secure-filesystem audit found that ordinary `Workspace::open`
and `SqliteStore::open(Path)` read the config/private key and open SQLite
before preparation's descriptor-relative checks. A static or swapped symlink
can therefore redirect sensitive reads or database mutation before readiness
fails. A check-then-open guard is not accepted.

Add E0001-12 as a storage-only W6 task. It accepts an already opened directory
descriptor plus a strict database leaf, opens SQLite through that pinned
directory with `SQLITE_OPEN_NOFOLLOW`, and retains the descriptor for the
connection lifetime so main/WAL/journal paths remain pinned. Existing storage
APIs stay compatible.

E0001-09 must descriptor-pin root and `.proof`, read exact config/key leaves
without following symlinks, and consume E0001-12 before any database access.
Static symlink and directory-replacement tests must prove failure before key
read or DB mutation. This narrows local authority and does not change schema,
migrations, provider use, or external effects.

## D-E0001-009 — Trusted fresh-workspace filesystem boundary

Status: approved superseding Gate B clarification · Date: 2026-08-30 · Decision owner: product owner under standing `approve and proceed` direction

E0001-12 stopped at its required escalation condition and independently
confirmed that D-E0001-008's strong SQLite requirement is impossible with the
stock Unix VFS. SQLite accepts a pathname rather than an open database or
directory descriptor. Its `SQLITE_OPEN_NOFOLLOW` processing rejects the
`/proc/self/fd/<dirfd>` magic link; without that flag SQLite canonicalizes the
descriptor alias to an ordinary absolute path used later for WAL, SHM,
journal, and delete operations. `openat2`, `fchdir`, inode rereads, advisory
locks, and private hard-link/copy staging do not preserve both the locking and
crash-durability contract. A true strong solution requires a descriptor-aware
VFS or a separate mount namespace, both outside this edition.

The owner has repeatedly directed the orchestrator to proceed with the
recommended bounded E0001 path. For this one fresh disposable synthetic
workspace, the filesystem threat model is therefore explicit:

- malformed/static filesystem content and concurrent unprivileged
  different-UID substitution are in scope;
- root and a hostile process with the same effective UID are out of scope,
  because either can already access the workspace private key, inspect the
  process, and reach any eventual provider credential;
- every ancestor must be no-symlink and protected against other-UID rename;
  a writable ancestor is allowed only when sticky and owned by root or the
  effective user;
- the fresh root, `.proof`, and storage directory must be owned by the
  effective user and private; config/key access remains descriptor-relative,
  regular-file, single-link, and no-follow;
- SQLite uses a verified ordinary local path, native `SQLITE_OPEN_NOFOLLOW`,
  guarded directory/database descriptors, pre/post inode checks, and
  `SQLITE_FCNTL_HAS_MOVED`. These are fail-closed defense in depth under the
  trust assumption, not descriptor-pinned sidecar semantics.

E0001-12 remains the immutable negative feasibility record. E0001-13 owns the
narrow storage seam; E0001-09 owns secure ancestry and config/key consumption.
Replace directory-rename-continuation acceptance with private-namespace,
symlink/hard-link, wrong-owner/mode, unsafe-ancestor, and injected initial-open
substitution tests. Conditional B5 remains inactive until E0001-11, E0001-13,
E0001-09, and independent fresh 10/10 verification all pass. This amendment
does not authorize a custom VFS, additional provider attempt/spend/data, a
historical workspace, or any production/external filesystem effect.

## D-E0001-010 — Conditional B5 prerequisites satisfied; credential boundary remains closed

Status: approved activation, execution pending credential · Date: 2026-08-30 · Decision owner: product owner under explicit `approve and proceed` direction

E0001-11 and E0001-13 are accepted, E0001-09 implementation and independent
security reviews pass, and the fresh public preparation is independently exact
10/10 with a stable immutable packet. Its exact live agent, distinct enrolled
Human approver, scoped delegation, synthetic edition/goal, manifest,
idempotency key, policy/check/tamper/tool/pricing digests, and next argv all
recompute. The final reverse-impact reruns pass. Conditional B5 may therefore
advance to its already approved credential boundary without changing the
contract.

The approved retention/redaction/cleanup interpretation is exactly the frozen
one: the direct Responses requests use `store=true`; outbound content is only
the sealed synthetic prompt/tool material; the bearer credential is read only
from `OPENAI_API_KEY` at provider construction and is never persisted or
reported; edition records retain only approved IDs, digests, chronology,
usage/cost, proof/evaluation, and redacted failures; the private disposable
workspace is not copied into Git or handoffs; ambiguous/failed attempts and
artifacts are preserved for reconciliation; and E0001 performs no destructive
cleanup or silent replacement run.

Both sandbox and host-context presence checks reported the credential absent
without printing a value. Consequently no factory, network request, charge,
live v2 run, or preview artifact was created. E0001-04 remains stopped. Once
the credential is securely injected into the agent environment, this decision
authorizes only the persisted exact primary journey: direct endpoint, exact
`gpt-5.6-sol`, maximum USD 0.15 owner spend, 120000 micro-USD runtime budget,
one signed-approval path, and the existing retry/ambiguity rules. Any different
model, endpoint, data, preparation, spend, or additional primary run requires
a new owner decision.

The product owner deferred E0001-04 until the morning of 2026-08-31 because a
credential cannot be provisioned before then. This records no waiver and no
new authority. The verified readiness packet remains the sole resume point,
and provider-attempt count remains zero.

## D-E0001-011 — Delegation grants preserve durable principal identity

Status: implemented protective correction · Date: 2026-08-30 · Decision owner: orchestrator

Final credential-free operator audit found that the CLI delegation grant path
hard-coded the workspace issuer as `Human`. When the recipient differed from
the workspace actor, the old conflict update could leave that Agent principal
reclassified as Human; it could likewise reclassify an already enrolled Human
recipient as Agent. This violates the canonical immutable principal ID, kind,
and public-key contract and can break later approval requester verification.

E0001-14 corrects the CLI-only binding before the paid boundary. The issuer is
saved from its actual workspace keypair principal. An absent recipient retains
the compatibility Agent placeholder, while an existing recipient is accepted
only with the same kind and, when supplied, public key; conflicts fail before
the delegation is saved. Focused tests prove a distinct-recipient grant leaves
the workspace Agent tuple unchanged and a Human recipient remains unchanged
with zero delegations persisted.

A narrow immutable read of the retained E0001 readiness database confirmed
that its requester is already `Agent`, its approver is `Human`, and its exact
delegation binds the intended actor. No key material was read and the packet
was not modified. E0001-14 therefore does not invalidate or replace the 10/10
packet. It adds W9, moves live dogfood to W10 and release integration to W11,
and requires a host-context full CLI plus immutable readiness replay before
provider construction. It authorizes no credential read, provider request,
additional run, schema change, or expanded spend/effect.

## D-E0001-012 — Workspace import preserves immutable authority

Status: implemented protective correction · Date: 2026-08-30 · Decision owner: orchestrator

Continued credential-free CLI audit found that workspace import bypassed the
canonical principal API with an `ON CONFLICT` update of kind and public key.
The same path could feed an existing delegation ID through a mutable upsert,
including replacing or un-revoking the stored grant, and it persisted proofs
one at a time before every signature had passed.

E0001-15 makes import preflight the authority boundary. Every exported
principal ID/key is decoded, duplicate IDs fail, and an enrolled principal is
accepted only with its exact durable kind/key. Delegations require unique IDs,
valid windows, enrolled/imported endpoints, and exact equality when their ID
already exists. Every proof signature is then checked against the enrolled or
preflighted principal set before any archive principal, proof, or delegation
is saved through the canonical APIs. The delegation write atomically permits
only insertion or an exact no-op, preventing a post-preflight change from
being overwritten. Tests prove collisions and unrevocation
leave durable state unchanged, invalid proof evidence does not partially
enroll its signer, and compatible round-trip/newer-proof behavior remains.

This correction intentionally does not change the global storage delegation
contract or claim that blob, registry, and workspace-file import is one
transaction. Signed monotonic operator revocation and broader import
transactionality require separately contracted work. The repair did not open
or change the retained readiness workspace, and no credential/provider/live
evidence was created. E0001-15 adds W10, moves live dogfood to W11 and release
integration to W12, and extends the existing host full-CLI plus immutable
readiness replay barrier to the new source revision. Both checks then passed in
host context: the locked CLI suite was 72/72 and the credential-free replay
returned the exact retained 10/10 packet and binding digest with both provider
variables unset. The paid boundary remains closed.

## D-E0001-013 — Workspace archive extraction is contained and manifest-bound

Status: implemented protective correction · Date: 2026-08-30 · Decision owner: orchestrator

Continued credential-free transfer audit found four connected trust failures.
Import opened the archive once for the first manifest and again after durable
writes for workspace data, permitting input substitution. It converted member
paths to strings and joined them without rejecting `..`, so a raw tar member
could escape `.proof/data` into private workspace controls. Final writes
followed target directory/leaf links. Workspace proof JSON also had no exact
binding to the signed and verified manifest proof set.

E0001-16 treats the archive as one immutable input snapshot. One stream must
contain exactly one regular manifest; every workspace-data entry is
preflighted for ordinary safe components, exact depth/type, UTF-8 JSON name,
and unique destination before archive-specific persistence. Manifest proof IDs
are unique, and any proof file must have the exact ID filename and exact JSON
value of its manifest proof.

Contained data writes now walk already-open directory descriptors. Exact bytes
are written and synced to a same-directory `0600` temporary file, atomically
renamed over the leaf, followed by directory sync and exact reread. This
replaces a link leaf itself without following its external target and rejects
a symlinked directory component. Adversarial coverage proves raw parent
traversal leaves the keypair/principal state unchanged, symlink targets receive
no write, absent/drifted proof files persist nothing, and safe link-leaf
replacement does not alter external files. Compatible imports remain green and
the host CLI suite passes 78/78.

This does not change archive format v1 or claim an archive-size ceiling or one
cross-database/CAS/filesystem transaction. Those require separate contracts.
No retained packet mutation, credential read, provider call, live v2 run,
artifact, failure, or charge is authorized. E0001-16 adds W11, moves live
dogfood to W12 and integration to W13, and requires the exact immutable
readiness replay once more after the final source tree. That replay passed with
both provider variables unset, returning the exact retained 10/10 packet,
`next_argv`, and binding digest; the paid boundary remains closed.

## D-E0001-014 — Workspace actor and signing key are one fail-closed identity

Status: implemented protective correction · Date: 2026-08-30 · Decision owner: orchestrator

Continued credential-free identity audit found that ordinary `Workspace::open`
trusted the config actor and signing key independently. It could construct
proofs naming one principal while signing with another key. Rotation loaded
only the key before archiving and replacing identity state. Initialization
also truncated existing config/key files, silently creating a new identity
over prior workspace proofs and delegations, while millisecond-only rotation
archive names could collide and truncate history.

E0001-17 makes the actor/key equality check one shared invariant for ordinary
and descriptor-relative open. Ordinary config must first be a private regular
file. Rotation enters through the invariant before any archive or identity
mutation. Initialization refuses when either identity leaf exists and creates
both with no-replace semantics; rotation archives add a UUID and are likewise
no-replace. Rejections preserve exact identity bytes and create no archive.

The repair deliberately makes a partial intentional rotation unavailable for
signing rather than adding automatic destructive recovery or claiming a
multi-file transaction. Recovery protocol design remains separate. E0001-17
adds W12, moves live dogfood to W13 and integration to W14, and repeats the
source-sensitive host/readiness barrier. The host CLI passes 81/81, and the
exact retained 10/10 packet, `next_argv`, and binding digest replay with both
provider variables unset. No historical workspace, credential, provider call,
live v2 run, artifact, failure, or charge was created or changed.

## D-E0001-015 — Canonical contracts reflect implemented Gate B state

Status: implemented documentation correction · Date: 2026-08-30 · Decision owner: orchestrator

Read-only preparation for the next edition found that the canonical kernel and
Content-domain contracts still labeled implemented E0001 prerequisites as
Gate B proposals. `kernel-api.md` named migration 11 current despite appended
and tested migration 12, and the frozen live contract still called conditional
B5 inactive after D-E0001-010 activated its prerequisites.

E0001-18 synchronizes status, not behavior. The kernel contract now records the
active default-compatible version-aware hooks, current schema version 12,
strict stored delegation scope, and `SqliteStore::load_delegation`. The Content
contract records active `release.publish::v2` alongside unchanged v1 and adds a
dated activation decision without rewriting the original proposal row. The
live contract records implemented B1-B4/B6 and satisfied B5 prerequisites,
while remaining explicit that execution is stopped at credential availability,
zero provider attempts, and pending Gate C. Its pre-implementation rationale
remains preserved as the immutable decision record.

No Rust, registry, schema, migration, policy, API, readiness packet, historical
workspace, credential, provider call, live v2 run, artifact, failure, charge,
or Gate C state changes. E0001-18 adds W13, moves live dogfood to W14 and
integration to W15, and repeats the source-sensitive host/readiness barrier.
The host CLI passes 81/81 and the exact retained 10/10 packet, `next_argv`, and
binding digest replay with both provider variables unset.

## D-E0001-016 — Live-v2 diagnostics and approval actionability are separate evidence thresholds

Status: implemented protective correction · Date: 2026-08-31 · Decision owner: orchestrator

Final credential-free operator review found that the live runtime persists
`agent_runtime_v2`, but the approval UI selected only v1 and therefore could
not render the exact pending live consequence. The first repair also exposed a
critical distinction: durable live checkpoints intentionally precede their
causal events at dispatch and committed-response barriers. Requiring those
events in `agent watch` would hide valid crash state, while omitting them from
approval actionability would permit a Human decision over evidence that cannot
resume safely.

E0001-19 establishes two thresholds. `RuntimeStateView` accepts only a complete
contiguous sequence-zero history with one supported version, exact envelope
shape and digest, and fully typed validated state; it intentionally requires no
event and is diagnostic only. `RuntimeApprovalContext` adds the exact
ModelRequested/ModelResponded ledger, committed-decision/call/arguments match,
sealed signed request, reconstructed exact waiting step, and policy-bound
Human. The UI exact-compares those sealed records with SQLite, offers only the
enrolled required Human, rechecks the identity on POST, and never recommends
generic `agent resume` for v2 because the sealed policy path is required.

Real-SQLite and adversarial tests prove the exact five arguments are actionable
only under the complete binding. Omitted prefixes, unknown recomputed envelope
fields, call/argument/request-signature/step-time substitutions, wrong Human,
and generic resume all fail closed before provider or governed effect. Valid
Dispatching and Committed pre-event crash barriers remain visible. Runtime
passes 116/116, the complete host CLI passes 88/88, two independent audits pass,
format/diff checks pass, and the credential-free readiness replay returns the
unchanged 10/10 packet and binding digest.

This correction changes no live policy, provider/model, spend, synthetic
payload, registry, schema, migration, authority, or Gate C state. The retained
workspace was replayed read-only with both provider variables removed. The
credential remains absent, so provider attempts, live runs, approval decisions,
publication effects, artifacts, failures, and charges remain zero. E0001-19 is
W14; live dogfood moves to W15 and integration/Gate C to W16.

## D-E0001-017 — Live start is one-shot and recovery is emitted, exact, and workspace-bound

Status: implemented protective correction · Date: 2026-08-31 · Decision owner: orchestrator

Final pre-provider operator audit found that the retained exact `live-start`
argv could create a second paid run, follow-up commands lost the selected
workspace, direct approval could persist a decision from the wrong enrolled
Human before runtime rejection, and the advertised path neither exposed the
five exact publication arguments nor supplied a secure live-v2 procedure.
Fault injection after the initial repair also proved an availability gap: a
crash immediately after the atomic claim preserved spend safety but stranded
the only run before its first dispatch.

E0001-20 makes start identity a storage-enforced fact. A canonical claim binds
the exact readiness and stable setup digests. SQLite migration 13 commits the
claim, initial Running run, checkpoint zero, and Started event zero under one
`BEGIN IMMEDIATE`; exact replay validates every stored record/digest and
returns the original run before gateway construction. Conflicting pairings
fail closed. Runtime permits first-dispatch recovery only from the exact
pristine prefix after a fresh durable epoch, with zero attempts, counters,
usage, cost, steps, approvals, evaluations, or later events. Prepared remains
certified pre-send; Dispatching/ResponseReceived restart remains ambiguous and
provider-free.

CLI results now emit array-form workspace-bound review, exact-Human decision,
policy-bearing live-resume, and watch argv. Native v2 history, signed request,
waiting step, input digest, committed call, five typed arguments, and required
Human are exact-validated before decision persistence. Generic resume and the
session-fragment-printing approval UI are not advertised. The new live runbook
requires the retained persisted start argv, emitted follow-ups, same-run
recovery, explicit stop rules, and credential-free terminal replay; the legacy
preview provider gate is marked superseded.

Final source passes Kernel 98/98, Storage 128/128, Runtime 119/119, host CLI
93/93, Runtime/CLI reverse impact 212/212, and full Kernel reverse impact
597/597 across 49 suites. Independent runtime/recovery and CLI/security audits
pass. The retained packet replays with both provider variables removed at
exact 10/10, unchanged binding digest and `next_argv`, ready-record SHA-256
`8bc2784f8ae20cc108fe50715ef31ec46289ed4bcf81da6a32b66554785cc40b`,
and private modes.

This correction authorizes no credential read, provider call, approval,
publication, live effect, charge, or Gate C decision. E0001-20 is W15; live
dogfood moves to W16 and integration/Gate C to W17.

## D-E0001-018 — Live execution requires both active-writer exclusion and expected-tail append

Status: implemented protective correction · Date: 2026-08-31 · Decision owner: orchestrator

A post-commit concurrency audit of E0001-20 proved that provider-send
cardinality was safe but immutable recovery was not. Two resumers could load
the same pristine tail, persist different epochs, and append incompatible
Prepared attempts through list-next-insert. The later checkpoint was itself
durable but made the complete history invalid. Expected-tail CAS alone closes
that stale append, but it cannot protect a process paused after the exact
Dispatching/event barrier: another resumer could terminalize the run before
the first process performs its already-authorized send, orphaning a paid
response.

E0001 therefore requires both controls. The frozen live runtime holds one
crash-released advisory workspace execution lease from before resume's first
durable read through return; an acquired start holds it from immediately after
the atomic claim through return, while exact Existing replay stays read-only.
Resume contention is `LiveRunBusy` and performs zero store writes, gateway
creation, provider sends, or governed effects. Start contention may retain only
the atomic four-record initial claim bundle; it performs no post-claim write,
gateway creation, provider send, or governed effect and must recover that same
run through exact persisted-start replay after lease release. Every post-claim
live checkpoint also prevalidates the full prospective history and atomically
compares the exact tail ID, sequence, and state digest. Resume epoch append
binds the exact prior state, and ordinary append rejects a tail owned by
another epoch.

The new `AgentRunStore` method is default-compatible (`Unsupported`). Recording
storage implements it under the checkpoint mutex and SQLite under
`BEGIN IMMEDIATE`; both reject malformed candidates and corrupt current or
retry-predecessor evidence, return `Stale` without insertion for a displaced
writer, and accept only an exact current retry with its exact predecessor.
Legacy checkpoint save remains unchanged for existing callers.

Final direct gates pass Kernel 106/106, Storage 133/133, Runtime 122/122, and
host CLI 93/93. Reverse impact passes Runtime/CLI 215 tests, Storage/transports
293 tests, and Kernel plus all 12 dependents 613 tests across 50 suites.
Independent final audit found no remaining material live concurrency race.
This correction changes no credential, provider/model, budget, synthetic
payload, authority, registry, schema migration, live-run evidence, or Gate C
state, and it authorizes no provider attempt.
