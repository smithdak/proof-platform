# Operator Control Plane Contract

**Contract:** proof-operator-control/v1
**Status:** proposed for AXP-E0002 Gate B
**Owner:** product owner
**Last updated:** 2026-09-01

This contract is the complete implementation boundary for AXP-E0002. It
defines an independently authenticated, loopback-only control plane through
which one enrolled Human may inspect and govern several durable agent runs.
Nothing in this document imports authority, credentials, evidence, or release
status from AXP-E0006.

The words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, and
MAY are normative. An implementation is conformant only when every required
check in evals/operator-control-v1.json passes.

## 1. Outcome and non-goals

One Human can use one same-origin browser console to:

1. establish a volatile session by signing a fresh E0002 challenge;
2. inspect a bounded, redacted attention projection for at least four durable
   runs in different states;
3. inspect exact run, approval, command-receipt, and audit detail;
4. sign an approve or deny decision as the sealed required Human;
5. explicitly resume a waiting run after a decision;
6. cancel a run before provider, tool, or governed write dispatch;
7. observe safe worker recovery with a retained control session;
8. reauthenticate after a control-process restart; and
9. revoke the operator session without cancelling a run or withdrawing a
   decision.

The v1 control plane does not provide remote access, wildcard or proxy
binding, SSO, cookies, browser-persistent authority, bulk decisions,
auto-resume, decision withdrawal, delegation between Humans, parent/child
swarm scheduling, general domain operations, arbitrary audit/proof access, or
external/provider effects.

## 2. Authority and threat model

### 2.1 Authority roots

Authority is the conjunction of all of the following:

- a fresh disposable Linux workspace selected before listener bind;
- the existing database at <workspace>/.proof/storage/storage.db, opened
  through the trusted existing-only boundary in section 4;
- exactly one durable operator workspace policy at schema version 14;
- an enrolled principal of kind Human selected by that policy;
- an independently signed, unexpired challenge for the current server
  instance, workspace, policy epoch, origin, nonce, and capability set;
- a volatile session derived from the one-use challenge exchange;
- the capabilities required by the exact route; and
- for a mutation, fresh target actionability, revision, fence, command, budget,
  and signer checks under the ordering in sections 10 through 13.

Possession or readability of a private key is not by itself an authenticated
Human action. The launcher-owned terminal ceremony is REQUIRED before the
Human key may sign an authentication challenge.

### 2.2 In-scope attacks

Conformance covers malicious webpages, DNS rebinding, CSRF, CORS abuse,
lower-privilege other-UID local processes, forwarded-header spoofing, malformed or
duplicated fields and headers, guessed/replayed/cross-scope credentials,
concurrent exchange or mutation races, stale pages, lost responses, worker
and control-process crashes, stale worker writes, aggregate-budget races,
unsafe workspace paths/files/sidecars, signer substitution, legacy-router
exposure, and secret leakage through ordinary process/browser/evidence
surfaces.

### 2.3 Residual limitations

Root compromise, any malicious same-effective-UID process or filesystem
compromise (including reading/replacing owned 0600/0700 files, ignoring the
advisory lock, ptrace, or process-memory inspection), a
compromised browser or extension, a compromised enrolled Human private key,
and local denial of service are outside v1. Browser and server process memory
necessarily contain the active session secret. These limitations MUST appear
in release evidence and UI documentation.

## 3. Fixed encodings and constants

| Item | Exact v1 rule |
|---|---|
| UUIDs | lowercase canonical UUIDv7 |
| UTC time | RFC 3339 UTC with Z; deadline equality is expired |
| JSON | UTF-8, no BOM; strict DTO decode; unknown and duplicate names rejected recursively |
| Canonical JSON | proof_kernel::canonicalize over the strict decoded value |
| Safe integers | JSON and SQLite integers are 0 through 9007199254740991 |
| Kernel ContentDigest wire | exactly 64 lowercase hexadecimal BLAKE3 bytes, matching existing serde |
| Control digest wire | algorithm-qualified blake3-256:<64 lowercase hex> |
| Artifact digest wire | algorithm-qualified sha256:<64 lowercase hex> |
| Ed25519 signature wire | 64 bytes, unpadded base64url, exactly 86 characters |
| Random nonce/token | 32 OS-CSPRNG bytes, exactly 64 lowercase hexadecimal characters |
| Challenge lifetime | 120 seconds |
| Session absolute lifetime | 900 seconds |
| Session idle lifetime | 300 seconds |
| Cursor lifetime | 300 seconds and no later than session absolute expiry |
| Default / maximum page size | 25 / 100 |
| Request body limits | 4096 bytes for session routes; 8192 bytes for protected mutations |
| Query target limit | 2048 bytes after the request target is received, before decoding |

Arrays representing a set MUST be nonempty where required, contain no
duplicates, and be in ascending bytewise UTF-8 order. JSON Schema uniqueItems
is only a secondary check; runtime ordering and UUID version checks are
mandatory.

Every control digest uses BLAKE3 over exactly:

    ASCII domain label || 0x00 || payload bytes

For a JSON payload, payload bytes are `proof_kernel::canonicalize` UTF-8 after
strict typed decoding and after removing the named output digest field. For a
raw secret, payload bytes are the exact 32 decoded bytes. Removing a digest
field is not recursive or discretionary: only the output field named below is
absent, while every other required field remains. The exhaustive v1 map is:

| Field/use | Domain label | Exact payload |
|---|---|---|
| PrincipalBinding.public_key_fingerprint | Proof-Operator-Public-Key-v1 | decoded 32-byte Ed25519 public key |
| client_nonce_digest | Proof-Operator-Client-Nonce-v1 | decoded 32-byte client nonce |
| SessionAttestation.signed_bytes_digest | Proof-Operator-Session-Challenge-v1 | canonical SessionChallenge JSON |
| SessionClaims.token_digest | Proof-Operator-Session-Token-v1 | decoded 32-byte session token |
| SessionClaims.authority_digest / AuditEvent.session_authority_digest | Proof-Operator-Session-Authority-v1 | canonical SessionAuthorityBinding |
| OperatorWorkspace.workspace_fingerprint | Proof-Operator-Workspace-v1 | the exact workspace-fingerprint object in section 4.3 |
| OperatorWorkspace.schema_catalog_digest | Proof-Operator-Schema-Catalog-v1 | canonical SchemaCatalogBinding with entries sorted by operation then version |
| OperatorWorkspace.binding_digest / operator_workspaces.binding_digest | Proof-Operator-Workspace-Binding-v1 | canonical OperatorWorkspace without binding_digest |
| HumanEnrollment.capability_set_digest | Proof-Operator-Capability-Set-v1 | canonical CapabilitySet array |
| BudgetPolicy.limits_digest | Proof-Operator-Budget-Limits-v1 | canonical BudgetPolicy without limits_digest |
| RunControl.binding_digest | Proof-Operator-Run-Binding-v1 | canonical RunControl without binding_digest |
| RunLease.lease_token_digest | Proof-Operator-Lease-Token-v1 | decoded 32-byte lease token |
| BudgetReservation.dispatch_token_digest / DispatchPermit.dispatch_token_digest | Proof-Operator-Dispatch-Token-v1 | decoded 32-byte one-use dispatch token |
| RunLease.lease_digest | Proof-Operator-Lease-v1 | canonical RunLease without lease_digest |
| BudgetReservation.request_digest | Proof-Operator-Budget-Reservation-v1 | canonical BudgetReserveRequest with LeaseAuthority.lease_token removed |
| BudgetReserveRequest.intent_digest / BudgetReservation.intent_digest / DispatchPermit.intent_digest | Proof-Operator-Dispatch-Intent-v1 | canonical DispatchIntent |
| ReplayClaimBinding.binding_digest | Proof-Operator-Replay-Binding-v1 | canonical ReplayClaimBinding without binding_digest |
| ApprovalBinding.argument_digest | Proof-Operator-Approval-Argument-v1 | canonical ordered ReviewField array |
| PendingConsequence.consequence_digest / ApprovalBinding.consequence_digest | Proof-Operator-Approval-Consequence-v1 | canonical PendingConsequenceBody containing exactly classification and summary |
| ApprovalBinding.binding_digest | Proof-Operator-Approval-Binding-v1 | canonical ApprovalBinding without binding_digest |
| RunProjection.snapshot_digest | Proof-Operator-Run-Projection-v1 | canonical RunProjection without snapshot_digest |
| CommandEnvelope.request_digest | Proof-Operator-Command-v1 | canonical complete OperatorCommand body |
| AuditEvent.event_digest | Proof-Operator-Audit-Event-v1 | canonical AuditEvent without event_digest |
| CommandReceipt.receipt_digest | Proof-Operator-Command-Receipt-v1 | canonical CommandReceipt without receipt_digest |
| CursorClaims.filter_digest | Proof-Operator-Cursor-Filter-v1 | canonical decoded route query with cursor omitted |
| cursor MAC | Proof-Operator-Cursor-v1 | canonical CursorClaims JSON, keyed BLAKE3 with the process cursor key |
| DispatchIntent.argument_digest | Proof-Operator-Dispatch-Argument-v1 | canonical strict operation input argument document selected by operation::version |
| BeginDispatchRequest.call_digest / BudgetReservation.call_digest / DispatchPermit.call_digest | Proof-Operator-Dispatch-Call-v1 | canonical DispatchIntent |
| RecoveryDirective.intent_digest | Proof-Operator-Dispatch-Intent-v1 | canonical original DispatchIntent |
| RecoveryDirective.directive_digest | Proof-Operator-Recovery-Directive-v1 | canonical RecoveryDirective without directive_digest |
| PreparedExecutionBinding.payload_digest / RuntimeCommit.prepared_execution_digest | Proof-Operator-Prepared-Execution-v1 | canonical prepared governed mutation bundle described in section 12.1 |
| PreparedExecutionBinding.replay_binding_digest | Proof-Operator-Replay-Binding-v1 | canonical exact-replay claim identity |
| PreparedExecutionBinding.result_digest / RuntimeCommit.result_digest | Proof-Operator-Runtime-Result-v1 | canonical PreparedRuntimeResultBody |
| RuntimeFailureRequest.error_digest | Proof-Operator-Runtime-Failure-v1 | canonical RuntimeFailureBody |

`SessionAttestation.signed_bytes_digest` is therefore BLAKE3 over the exact
bytes also signed: ASCII `Proof-Operator-Session-Challenge-v1`, one zero byte,
and canonical SessionChallenge JSON. The cursor MAC is exactly keyed BLAKE3
over its listed domain frame; it is not an unkeyed ControlDigest and no digest
field is appended to CursorClaims. Domain reuse for an unlisted payload is a
contract violation; a new payload requires a new versioned label and Gate B.

Artifact SHA-256 digests and kernel/control BLAKE3 digests are distinct types
and MUST NOT be substituted, relabeled, or compared across algorithms.

`PendingConsequenceBody` is the digest-free source object. The public nested
`PendingConsequence.consequence_digest` and the enclosing
`ApprovalBinding.consequence_digest` MUST both equal the digest of that same
body. Neither digest is included in the payload, and strict decoding plus this
equality check occurs before the consequence is displayed, signed, or stored.

## 4. Trusted workspace, store, and signers

### 4.1 Disposable identity boundary

All product, fixture, evaluation, and dogfood runs MUST use a newly initialized
disposable workspace and newly generated Agent and Human identities. The
repository-root .proof directory and its historically exposed identity are
forbidden even if current permissions appear safe.

The released binary accepts no caller-supplied forbidden-path or trust option.
E0002-15 constructs the mandatory repository-root `.proof` path from the
non-user-controlled compile anchor by taking
`Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent)`,
failing on either missing parent, and joining the ordinary `.proof`
component; there is no
environment, configuration, current-working-directory, argv, or alternate
fallback. A moved artifact whose frozen anchor is absent fails closed.
E0002-15 passes that path through the nonempty forbidden-directory slice on
`acquire_operator_workspace_lock`; storage descriptor-walks and opens it
nofollow, retains the open forbidden descriptor through comparison, and
compares its device/inode tuple with the selected open `.proof` descriptor
inside the same acquisition call. A string or canonical-path comparison and a
caller-certified identity are not conforming. E0002-14 takes one lexical
parent from its corresponding non-user-controlled `CARGO_MANIFEST_DIR` and
joins `.proof`; it may add fixture-owned forbidden descriptors. Neither
anchor result contains `.` or `..` components, uses canonicalization, or
follows a symlink. Product/evaluation assembly MUST always
include its build repository-root `.proof`. A missing, non-absolute, unsafe,
or unopenable mandatory path, an empty set, or a selected identity equal to any
forbidden descriptor fails before database/key access or bind.

The workspace root path MUST be absolute and contain only ordinary path
components. The root, .proof, storage, and approvers directories MUST be
nofollow descriptor-walked, current-effective-user owned directories with
mode exactly 0700. config.json, keypair.json, the selected Human key, the
database, the pre-provisioned `.proof/operator-control.lock`, and any database
sidecar MUST be current-effective-user owned,
single-link regular files with mode exactly 0600. Unsafe modes are rejected,
not repaired. Symlinks, hard links, URI filenames, path movement, directory
replacement, alternate attachments, and unsafe WAL, SHM, or journal sidecars
fail closed.

Linux is the only supported v1 host. Other operating systems fail before bind.

The launcher opens `.proof/operator-control.lock` by descriptor without
creating or truncating it and calls exactly
`rustix::fs::flock(&owned_fd,
rustix::fs::FlockOperation::NonBlockingLockExclusive)` before database or
signer access. The offline schema-14 upgrader uses that identical BSD `flock`
class and rustix operation; POSIX/OFD `fcntl` locks are not conforming because
they occupy a different lock namespace. Each owner retains the same `OwnedFd`
without duplication for its entire protected lifetime and relies on close to
release the lock: the launcher releases it only after trusted-store close at
shutdown, and the offline upgrader releases it only after migration commit,
database close, and final descriptor movement check. Contention or any lock
error fails before database mutation, signer load, or bind. The exact offline
provisioning entrypoint below creates this empty 0600 file and records its device/inode tuple in
the workspace binding. Crash closes the descriptor and releases the OS lock;
takeover repeats every trust check and uses a fresh server instance. Deleting,
replacing, moving, or hard-linking the lock file while held fails the next
movement check and terminates fail-closed. This workspace-wide lock is the v1
split-brain boundary; database fences do not substitute for it.

The only user-callable provisioning path is the E0002-15 product binary:

    proof-operator-control init --workspace <absolute-path> --provision <absolute-path>

Both options occur exactly once; there are no environment/config/default
fallbacks. The provision path names a strict OperatorProvisioningDocument,
which contains only public principal IDs/fingerprints, the fixed six-capability
set, and aggregate budget limits/deadline. It is descriptor-opened nofollow as
a current-user-owned, single-link 0600 regular file, duplicate-detect decoded,
and contains no private key or credential. Init refuses the repository-root
.proof identity and performs no bind, provider/tool call, or network effect.

Storage owns a nonconstructible, non-Clone/Copy/Serialize
`OwnedOperatorWorkspaceLock` containing the single `OwnedFd`, expected
device/inode, and verified workspace descriptors. Its provisioning constructor
descriptor-walks the existing workspace, `.proof`, storage, database, Agent,
Human, and provision files. It creates only a missing
`.proof/operator-control.lock` with Linux `openat` flags
`O_RDWR|O_CREAT|O_EXCL|O_CLOEXEC|O_NOFOLLOW` and mode 0600; if that exact file
already exists, it opens without create/truncate and requires an empty safe
file. It then acquires the exact section 4.1 flock on that same open-file
description. A failed partial first attempt may leave only that safe empty
lock file; retry is idempotent.

E0002-15 retains that one guard while it invokes E0002-06's guarded storage
entry points in this exact order: verify current schema is 13 or exact 14;
upgrade through migration 14 under the guard when needed; descriptor-load and
verify the provisioned Agent/Human public tuples without reading Human private
bytes; pass the stable provision/catalog request plus shared environment and
catalog to guarded initialization, which constructs new bindings only if the
singleton policy is absent; reopen
and verify schema 14 plus exact singleton bindings; perform final movement/
sidecar checks; close SQLite; then consume the guard, recheck the lock
device/inode, and release by closing its same OwnedFd. Every guarded entry
accepts `&mut OwnedOperatorWorkspaceLock`, verifies it is live, and neither
reopens nor reacquires the lock. No boolean/path assertion can substitute.
Migration success followed by initialization failure leaves a safe schema-14
database with no runnable operator policy; a retry may finish initialization.
An exact-existing policy succeeds only if every provision, descriptor,
principal, catalog, capability, and budget binding matches byte-for-byte.

The command writes only the strict ProvisionOperatorWorkspaceResult to stdout;
errors are fixed nonsecret text and a nonzero exit. The sole released serving
command is:

    proof-operator-control serve --workspace <absolute-path>

The option occurs exactly once; it always requests an OS-assigned port and has
no port, database, identity, forbidden-path, or trust override. The ordinary launcher is
existing-only and never invokes the init path. E0002-14 may call the same
guarded library lifecycle directly for disposable backend fixtures; E0002-15
alone owns the released runnable composition.

### 4.2 Authoritative database

The sole authoritative database is:

    <workspace>/.proof/storage/storage.db

The control plane MUST NOT create a missing database, use an in-memory store,
accept a database override, attach another database, or open
.proof/data/proofs/proofs.sqlite3. CLI, MCP, HTTP, runtime, fixtures, and
verification participating in the E0002 journey all receive the same opened
SqliteStore instance or an adapter over it.

Migration 14 is applied only by the explicit guarded
`upgrade_operator_schema14_offline(&mut OwnedOperatorWorkspaceLock, ...)`
storage entry point while the control plane, runtime, and every other writer
are quiescent. Migration 14 is appended
to `MIGRATIONS`, but ordinary `SqliteStore::open`, `in_memory`, connection
constructors, CLI/MCP/HTTP openers, and the existing trusted migrating opener
call `run_migrations_through(connection, 13, Immediate)` and MUST NOT create or
apply 14. `run_migrations_through` owns the sole SQLite transaction; callers
never wrap it in another transaction. The offline caller first acquires and
retains the same workspace guard above; the guarded upgrader alone calls
`run_migrations_through(connection, 14, Exclusive)`, whose internally owned
exclusive transaction applies every pending migration and its
`schema_migrations` row atomically. An already-version-14 database may be opened by ordinary
callers, but no ordinary caller creates or upgrades to it.

The control launcher uses a new existing-only trusted opener that performs all
current nofollow, ownership, link-count, sidecar, moved-file, read/write, and
foreign-key checks but does not run migrations. It then requires
an exact read-only query of the already-existing `schema_migrations` table and
`schema_version == 14`; this query MUST NOT call the current helper that can
create the migration table. Missing, older, newer, corrupt, or moved databases fail
before signer load or bind.

The existing open_existing_nofollow_in_trusted_directory method may keep its
current migrating behavior for compatibility, but the E0002 launcher MUST NOT
call it. Storage owns a separately named no-migration schema-14 opener and
tests that the launcher cannot silently upgrade version 13.

The exact storage-owned inherent lifecycle is:

~~~text
acquire_operator_workspace_lock(workspace: &Path,
                                provision: Option<&Path>,
                                forbidden_proof_directories: &[&Path],
                                OperatorLockMode)
    -> Result<OwnedOperatorWorkspaceLock, OperatorProvisioningError>
upgrade_operator_schema14_offline(&mut OwnedOperatorWorkspaceLock)
    -> Result<(), OperatorProvisioningError>
initialize_operator_workspace_guarded(&mut OwnedOperatorWorkspaceLock,
                                      InitializeWorkspaceRequest,
                                      Arc<dyn OperatorControlEnvironment>,
                                      Arc<OperatorSchemaCatalog>)
    -> Result<ProvisionOperatorWorkspaceResult, OperatorProvisioningError>
open_operator_schema14_existing(&mut OwnedOperatorWorkspaceLock,
                                Arc<dyn OperatorControlEnvironment>,
                                Arc<OperatorSchemaCatalog>)
    -> Result<SqliteStore, OperatorProvisioningError>
release_operator_workspace_lock(OwnedOperatorWorkspaceLock)
    -> Result<(), OperatorProvisioningError>
~~~

`OperatorLockMode` is the closed enum `provisioning | existing_only`.
The function accepts no pre-certified descriptor object, device/inode tuple,
or boolean trust assertion. It requires an absolute workspace path and a
nonempty forbidden path slice; provisioning requires exactly one absolute
provision path and existing-only requires `None`. Storage itself
descriptor-walks, opens, validates, and retains the selected and forbidden
directory objects through the identity comparison. Provisioning mode alone
may create the safe lock file; existing-only mode creates nothing and refuses
a missing lock, database, or policy. These
are proof-storage inherent functions, not the
kernel object-safe store trait, so the nonforgeable storage-owned guard is in
every mutating provisioning signature without reversing crate dependencies.
The launcher retains the guard beside the returned store until that store and
all SQLite handles close, then consumes it through the release function.
`InitializeWorkspaceRequest` contains only the stable provisioning document
and exact catalog binding. The guard supplies verified descriptor identities
and principal public bytes. On an empty schema-14 policy, storage obtains
trusted time and generates workspace/budget UUIDv7 values, constructs the
workspace fingerprint, enrollment, budget policy, and every digest, and
inserts the complete singleton policy in one transaction. On an existing
policy it calls neither clock nor entropy: it loads the persisted IDs/times and
requires every stable provision, descriptor, principal, capability, budget,
and catalog input to match before returning `exact_existing`. Thus retry never
manufactures a new identity merely to compare it with the old one.

`OperatorProvisioningError` is closed to `invalid_arguments`,
`unsupported_platform`, `unsafe_workspace`, `unsafe_provision`,
`lock_unavailable`, `schema_mismatch`, `migration_failed`, `policy_mismatch`,
`catalog_mismatch`, `environment_unavailable`, `storage_unavailable`,
`movement_detected`, and `close_failed`. The init CLI maps these one-to-one to
the same lowercase code prefixed by `operator init failed: `, prints no path
or source chain, and exits 2 for `invalid_arguments`, 3 for
`lock_unavailable`, and 1 for every other variant. The launcher exposes no
HTTP listener for any variant and prints only
`operator launch failed: <code>` before nonzero exit.

### 4.3 Workspace policy and signer identity

Migration 14 stores exactly one operator workspace policy. It binds:

- durable workspace UUIDv7 and workspace fingerprint;
- exact Agent and Human principal UUIDv7 values;
- the six-capability policy set;
- authentication/policy epoch and policy revision;
- one aggregate-budget window and exact limits; and
- creation and update timestamps.

It also binds one immutable OperatorSchemaCatalog digest. The kernel-owned
catalog constructor does not accept an already-parsed `Registry`, because that
type does not retain source bytes or paths. It accepts one closed
`OperatorSchemaSourceInventory`: for every active operation/version, exactly
one triple of normalized registry-root-relative UTF-8 paths and exact raw byte
documents (`registry_entry`, `input_schema`, `output_schema`). Paths use `/`,
contain no empty, `.` or `..` component, are not absolute, and are bytewise
unique. The constructor duplicate-detects and strictly parses the registry
entry itself, requires its operation/version and schema references to resolve
to the other two paths in that same triple, builds and cross-checks a `Registry`
from all parsed entries, and then strictly parses the two referenced Draft
2020-12 schemas. It rejects inactive entries, symlinks or path-inventory extras
at the filesystem/build adapter, duplicate operation/version keys, missing or
extra documents, unsupported remote references, and invalid meta-schemas,
then compiles immutable validators. It never reconstructs source bytes by
serializing a parsed value.

`SchemaCatalogBinding` contains each normalized path and SHA-256 digest of all
three raw documents in operation/version byte order. Its
Proof-Operator-Schema-Catalog-v1 digest is stored in `OperatorWorkspace`. The
catalog exposes only read-only `validate_input` and `validate_output` methods
and performs no database or filesystem call after construction.

The real E0002 binary embeds the complete catalog bytes at build time from the
repository registry in a bytewise path inventory; its build rejects symlinks,
unreferenced extras, and path escape and records rerun dependencies for every
byte source. Offline provisioning persists the embedded catalog digest, and
every launch compares it before signer load or bind. The same `Arc` catalog is
passed to ExecutionEngine and the operator-capable SqliteStore opener. The
store uses it inside begin_dispatch only for non-reentrant read-only validation;
completed replay additionally strict-decodes the full persisted result and
Proof, validates output against the exact operation/version, loads the
immutable actor public key from the same database transaction, and verifies
the complete signature/digests before returning. ExecutionEngine validates
both input and prepared output with the same catalog before creating a prepared
bundle. A catalog mismatch is corrupt/control_unavailable, never an
attacker-selected schema or runtime fallback. Conformance constructs the same
catalog from frozen fixture bytes and proves digest equality.

OperatorWorkspace.capabilities and HumanEnrollment.capabilities MUST be the
same canonically ordered byte sequence, and the enrollment
capability_set_digest MUST verify that sequence. Initialization rejects a
mismatch before inserting any E0002 row. Startup rechecks both stored objects,
their equality, and the digest before challenge allocation, key load, or bind;
there is no wider workspace default that can override a narrower enrollment.

The workspace fingerprint is the
Proof-Operator-Workspace-v1 digest of the strict WorkspaceFingerprintInput:
`schema`, durable `workspace_id`, `{device,inode}` DescriptorIdentity objects
for the selected `.proof` directory, control lock, Agent key file, and Human
key file, plus Agent/Human principal UUIDv7 and immutable public-key bytes.
Those are the only fields. Raw workspace paths, modes, timestamps, and private
key bytes are never fingerprint input or an API field. OperatorWorkspace
stores both the exact input and its digest; startup re-derives descriptor
identities and public tuples and compares the complete input before signer
load or bind.

At startup the Agent file .proof/keypair.json, config actor, policy Agent, and
immutable database principal tuple (ID, kind Agent, public key) MUST match.
The Agent stored-key bytes may then be strictly decoded, their private/public
match verified, and the key retained before bind. For the Human file
.proof/approvers/<human-id>.json, startup checks only its nofollow descriptor
identity, owner/mode/link count, recorded device/inode tuple, exact expected
filename, policy Human, enrollment, and immutable database public tuple. It
MUST NOT read or decode Human private-key bytes before an authorized ceremony
or approval key-access point. At that point strict duplicate/unknown-field
decode derives the public key and rechecks the entire immutable tuple before
signing. Human content substitution therefore fails the authorized operation
as `control_unavailable`; it is not a startup claim and never becomes a 409.
Rotation, replacement, wrong kind, unenrollment, or policy mismatch fails
closed. No E0002 path may call generate_keypair or repair a signer file.

The Agent key may be loaded once after the complete startup trust check and
retained in guarded process memory. The Human key remains unloaded until a
terminal authentication ceremony or an authenticated, capable, freshly
actionable approval mutation reaches its key-access point. Identity, policy,
capability, and budget rotation are unsupported in v1 and require a newly
provisioned disposable workspace. auth_epoch and policy_revision remain
explicit immutable bindings initialized to one; a future version that permits
rotation must increment them and invalidate all sessions.

## 5. Listener and HTTP boundary

The control process MUST:

1. acquire the workspace control lock and finish all workspace, schema,
   policy, identity, terminal, randomness, and static-bundle checks before
   binding;
2. bind one IPv4 TCP listener to exactly 127.0.0.1 and an explicit or
   OS-assigned port;
3. serve HTTP/1.1 directly, with no proxy, wildcard, hostname, IPv6, Unix
   socket, remote interface, automatic browser launch, or port fallback;
4. print only the clean URL http://127.0.0.1:<port>/ and nonsecret
   instructions; and
5. on shutdown, stop accepting and stop issuing permits; while the workspace
   lock and all required lease/dispatch/Agent-signing authority remain live,
   drain held mutation leases, commit or failure-settle every issued custody,
   release proven pre-dispatch reservations, checkpoint durable work, and
   append the shutdown audit; then invalidate volatile sessions/challenges,
   zeroize nonce/token/key buffers, close the trusted store, and release the
   workspace control lock last.

Every request validates the actual socket peer as IPv4 loopback and requires
exactly one Host header equal to 127.0.0.1:<bound-port>. Forwarded, Forwarded-
For, X-Forwarded-For, X-Real-IP, proxy-protocol, and hostname aliases are
ignored as authority and MUST NOT affect rate limiting or scope.

Every exact known POST route that reaches its public-session or
authenticated-protected envelope check requires exactly one Origin equal to
the clean origin and exactly one
Content-Type whose field value is application/json with no parameter. Missing,
duplicated, comma-combined, foreign, null, or malformed security headers fail
at the precedence frozen in section 9.2. No route accepts a credential in a query, fragment,
cookie, form, URL path, WebSocket protocol, argv, environment, or body except
the one-use client nonce in the exchange body.

All responses, including framework errors, carry:

- Cache-Control: no-store
- X-Content-Type-Options: nosniff
- Referrer-Policy: no-referrer
- X-Frame-Options: DENY
- Cross-Origin-Opener-Policy: same-origin
- Cross-Origin-Resource-Policy: same-origin
- Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=(), usb=()

The HTML response additionally carries this exact policy:

    Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'

There are no CORS headers, cookies, service workers, remote resources, inline
script/style, referrer-bearing navigation, directory indexes, filesystem
fallbacks, or reflected request bodies/headers.

Rate limiting uses bounded monotonic token buckets: five challenge creations
and ten exchanges per minute per server instance, and 120 protected requests
per minute per successfully authenticated active session. A protected bucket
is selected only after constant-work authentication, so fake tokens cannot
allocate or evict buckets. Public buckets use the server instance, never an
attacker header. There are at most 32 buckets; deterministic least-recently-
used eviction never evicts the one active-session bucket. Saturation returns
the exact 429 envelope and performs no decode beyond the precedence boundary.

## 6. Independent signed challenge and session

### 6.1 Challenge issue

The browser generates a 32-byte client nonce with crypto.getRandomValues,
retains it only in the current JavaScript closure, and sends its
Proof-Operator-Client-Nonce-v1 digest plus a sorted, unique requested
capability set to:

    POST /operator/v1/session/challenges

The browser does not choose or submit a Human, workspace, instance, origin,
policy epoch, or granted capability. The server constructs the strict
proof.operator.session.challenge/v1 object defined by the auth schema with:

- a new challenge UUIDv7 and 32-byte server nonce;
- the current instance UUIDv7, workspace UUIDv7/fingerprint, selected Human,
  auth epoch, policy revision, and exact origin;
- the requested set and the granted intersection of requested, the enrolled
  Human capability set, durable workspace policy, and compiled supported
  capabilities;
- issue and expiry timestamps; and
- the three fixed lifetime values.

An empty intersection is rejected before challenge allocation with HTTP 400
and the exact `invalid_request` envelope. The response returns the challenge and the
browser displays a prominent short-authentication-string challenge code equal
to the first ten lowercase hex characters of
SHA-256 over the canonical challenge bytes. SHA-256 here is only a visual
correlation code, not an authority digest.

There may be one pending challenge and one active session. A new challenge may
coexist with the active session to permit reauthentication after browser
memory loss. It does not revoke the old session. Only a successful exchange
atomically replaces the active session. A second pending challenge receives a
generic unavailable response. Terminal challenge state permits a later fresh
challenge, subject to rate limits.

### 6.2 Terminal signing ceremony

The launcher owns one controlling TTY opened before bind. When a challenge is
pending it prints the workspace fingerprint, origin, selected Human ID and
public-key fingerprint, sorted granted capabilities, expiry, and the explicit
instruction to compare those values with the requesting browser. It MUST NOT
print, derive, autocomplete, or otherwise disclose the challenge code on the
terminal. The browser is the only display channel for that code. The launcher
then disables terminal echo with a restoration guard and requires exactly:

    AUTHORIZE <10-lowercase-hex-code>

The Human must transcribe the code displayed by the browser; the confirmation
cannot be satisfied from terminal output alone. One attempt is allowed for
that challenge. Input is read directly from the
controlling TTY, never argv, environment, file, pipe, command substitution,
clipboard helper, browser request, or child process. Echo restoration is
guaranteed on success, mismatch, EOF, timeout, error, unwind, SIGINT, and
SIGTERM; inability to guarantee restoration fails before bind.
If the guarded restoration syscall nevertheless fails after bind, the process
consumes the challenge, performs no key load or signature, stops accepting
requests, makes one best-effort descriptor restoration, prints only a
nonsecret terminal failure, and exits fail-closed. It never continues with
uncertain terminal state.

An attacker-created challenge for which the Human has no matching browser
display times out or mismatches, is consumed, and yields no attestation or
session. Only an exact, live cross-channel confirmation permits descriptor-based Human-key load.
The adapter rechecks file identity and the enrolled Human tuple, then signs:

    ASCII "Proof-Operator-Session-Challenge-v1" || 0x00 ||
    canonical UTF-8 challenge JSON

It constructs proof.operator.session.attestation/v1 and submits it directly
to the in-memory authority. The ceremony-scoped Human private-key buffer is
non-Clone/Debug/Serialize and is zeroized and its descriptor closed
immediately after the signing attempt on success, signer error, verification
error, unwind, or shutdown; no challenge path retains it for a later approval.
Every later approval ceremony must descriptor-load and revalidate the Human
tuple again. Cleanup failure terminates fail-closed without publishing an
attestation or session. There is no browser-facing attestation endpoint.
The authority independently verifies Ed25519, every challenge field, current
policy/identity, and expiry. Mismatch, malformed confirmation, key error,
invalid signature, deadline equality, or changed policy consumes that
challenge as failed and produces no session.

### 6.3 One-use exchange

Immediately after challenge issue, the browser performs one bounded long poll:

    POST /operator/v1/session/exchange

The strict body contains only schema, challenge_id, and the raw client_nonce.
The server decodes all 32 bytes and compares its domain-separated digest in
constant work. Under one authority lock, exactly one waiter may perform:

    signed -> exchanged

It rechecks the attestation, current identity/policy, nonce, origin, instance,
capabilities, and expiry; consumes the challenge before publishing a response;
generates a new 32-byte session token and UUIDv7 session ID; replaces any old
active session; and returns the token once. A bad nonce, failed/expired
challenge, concurrent loser, replay, lost response, or cross-binding attempt
cannot recover or reissue that token. Recovery is a new signed challenge.

### 6.4 Session state

The server stores only the session ID, bindings, deadlines, capability set,
domain-separated token digest, and authority digest in process memory. The
authority digest is Proof-Operator-Session-Authority-v1 over the strict
SessionAuthorityBinding containing session/workspace/instance/Human, auth
epoch, policy revision, origin, granted capabilities, issue time, and absolute expiry. It omits
the raw/token digest and mutable idle expiry. Every protected command copies
that digest into its CommandBinding and storage verifies it against the
authority-held scope before any lookup. The raw token is returned
once and then zeroized from the response buffer when possible. Neither raw nor
derived token material is durable.

Every protected request supplies exactly one
X-Proof-Operator-Session header containing the 64-character lowercase token.
Absent, duplicated, non-ASCII, wrong-length, nonhex, invalid, expired,
revoked, wrong-instance, wrong-workspace, wrong-Human, or wrong-policy values
receive the same authentication response. All 32 decoded bytes are compared
in constant work even after a structural failure by comparing a fixed dummy
buffer.

A successful protected request advances the idle deadline to the lesser of
now plus 300 seconds and the absolute deadline. It never extends the absolute
deadline. Expiry is now greater than or equal to either deadline. Challenge
and session live deadlines use an injectable monotonic clock; signed/audit UTC
times remain independently validated and recorded.

Session replacement, revoke, expiry, or shutdown zeroizes the retained digest
and bindings where possible. Explicit revoke is authenticated but requires no
capability. It is a durable command/audit event and is distinct from cancel,
deny, delegation revocation, and decision withdrawal.

## 7. Capabilities

The only v1 capabilities, in canonical order, are:

1. approval.decide
2. approval.read
3. audit.read
4. run.cancel
5. run.read
6. run.resume

The order is bytewise lexical and therefore normative for every set. Granted
capabilities are immutable for a session and bound to Human, workspace,
instance, auth epoch, policy revision, and origin. No request can add a capability or select
another Human.

| Surface | Required capability |
|---|---|
| attention kinds run | run.read |
| attention kinds approval | approval.read |
| run detail | run.read |
| approval list/detail | approval.read |
| command receipt list/detail | audit.read |
| audit chronology | audit.read |
| approve or deny | approval.read and approval.decide |
| cancel | run.read and run.cancel |
| explicit resume | run.read and run.resume |
| session revoke | any valid current session |

An attention request containing both kinds requires both read capabilities.
The authenticated Human may decide only when it equals the immutable
required_approver_id sealed by the exact waiting checkpoint/request. Delegation
and another Human's key are forbidden in v1.

## 8. Exact route inventory

The E0002 router is built from a new empty axum Router. It is not merged,
nested, layered over, or given fallback access to the general HTTP router,
general AppState, generic operation handler, proof/audit handlers, legacy
approval UI/session router, directory service, or SPA fallback.

### 8.1 Public routes

| Method | Path | Request / response definition |
|---|---|---|
| GET | / | embedded exact index; no credential or data |
| GET | /assets/:asset | only a filename in the frozen static manifest |
| POST | /operator/v1/session/challenges | ChallengeIssueRequest / ChallengeIssueResponse |
| POST | /operator/v1/session/exchange | SessionExchangeRequest / SessionExchangeResponse |

### 8.2 Protected routes

| Method | Path | Capability | Request / response definition |
|---|---|---|---|
| POST | /operator/v1/session/revoke | valid session | SessionRevokeRequest / CommandReceipt |
| GET | /operator/v1/attention | kind-dependent reads | AttentionQuery / AttentionPage |
| GET | /operator/v1/runs/:run_id | run.read | path binding / RunDetail |
| GET | /operator/v1/approvals | approval.read | ApprovalQuery / ApprovalPage |
| GET | /operator/v1/approvals/:request_id | approval.read | path binding / ApprovalDetail |
| GET | /operator/v1/commands | audit.read | CommandQuery / CommandPage |
| GET | /operator/v1/commands/:command_id | audit.read | path binding / CommandReceipt |
| GET | /operator/v1/audit | audit.read | AuditQuery / AuditPage |
| POST | /operator/v1/approvals/:request_id/decisions | approval.read + approval.decide | ApprovalDecisionCommand / CommandReceipt |
| POST | /operator/v1/runs/:run_id/cancel | run.read + run.cancel | RunCancelCommand / CommandReceipt |
| POST | /operator/v1/runs/:run_id/resume | run.read + run.resume | RunResumeCommand / CommandReceipt |

The following are explicitly absent: /health, /capabilities, /audit, /proofs,
/v1/proofs, every domain/schema/object/list route,
/v1/operations/:name/:version, every E0006 route, WebSockets, static
directories, and unknown-route or wrong-method fallbacks.

Every listed GET succeeds with 200, challenge creation and exchange succeed
with 201, and every protected mutation or exact receipt replay other than a
session-revoke retry succeeds with 200. V1 emits no 202, 204, redirect, or
content-negotiated success. A path
outside the four public routes and `/operator/v1/**` receives the exact 404
envelope. An unknown path or wrong method under `/operator/v1/**` is a
protected fallback: it authenticates first, then returns that same 404. An
unauthenticated CORS preflight to the protected subtree is therefore 401 and
has no CORS headers.

The protected subtree authenticates before request decoding. It then strictly
decodes only the path/query/body needed to compute the route capability set,
checks that set, and only then performs a target or method-specific lookup.
For attention, the decoded `kinds` selector determines whether `run.read`,
`approval.read`, or both are required; no projection is read during selector
decode. Unknown-route and wrong-method fallbacks have no capability selector
and proceed from successful authentication directly to uniform 404.

## 9. Decode, error, projection, and cursor contract

### 9.1 Strict decoding

Every request and persisted E0002 JSON object has a literal schema
discriminator. Boundary decoding first detects duplicate object names at every
depth, then deserializes a typed structure whose every object denies unknown
fields, then performs semantic validation, and only then converts to a generic
JSON value for canonicalization. Parsing directly into serde_json::Value is
not a conforming boundary because duplicate names have already been lost.

Path and query decoders likewise reject unknown, duplicated, empty, malformed,
noncanonical, or over-limit values. A UUID parser MUST additionally verify
version 7. The Draft 2020-12 schemas in schemas/operator-control are necessary
but do not replace these runtime checks.

GET query objects use one query pair per schema field in the schema `required`
order. The literal `schema` pair is present. Array values use one repeated pair
per item in canonical array order. A nullable field is omitted when null and
appears exactly once otherwise. Empty arrays, comma-joined arrays, brackets,
JSON-in-query, duplicate scalar fields, unknown fields, `+` for space, lowercase
percent hex, and noncanonical percent encoding are invalid. Only RFC 3986
unreserved bytes remain literal; every other UTF-8 byte uses uppercase `%HH`.
Routes whose request shape is null reject any `?`. Cursor length is at most
1536 characters and the complete origin-form request target remains at most
2048 bytes.

### 9.2 Auth-first order

Request precedence is exact:

1. For every request, validate actual loopback peer, the single exact Host,
   origin-form target syntax, and the 2048-byte target limit. Failure returns
   the fixed 404 envelope and does not allocate a rate bucket.
2. Classify the raw method/path as an exact public route, the protected
   `/operator/v1/**` subtree (including its fallbacks), or absent.
3. A protected request structurally decodes the one fixed-size session header
   and, under the authority lock, performs constant-work token comparison plus
   active state, absolute/idle expiry, workspace, instance, Human, and auth
   epoch checks. Failure is 401 before Origin, media type, body collection,
   path/query/cursor/body decode, or lookup.
4. A successfully authenticated protected request consumes its session rate
   bucket. Saturation is 429.
5. An unknown-route or wrong-method protected fallback returns uniform 404 at
   this point. It does not validate Origin/Content-Type, collect a body, decode
   path/query/cursor fields, or perform a lookup.
6. For every exact fixed-capability route, derive requirements only from the
   route table and check them under the authority lock before decoding its
   path/query/cursor/body. Session revoke has the fixed empty set. For attention
   only, strictly decode the minimal `schema` plus ordered `kinds` selector,
   derive run.read/approval.read requirements, and check them before decoding
   page_size, cursor, or any remaining query field. A malformed selector is
   400; missing capability is 403. Under-capability calls therefore cannot
   exercise target, cursor, or body parsing.
7. For an exact protected POST that passed step 6, validate the single exact
   Origin and Content-Type, collect at most 8192 raw bytes, then strictly decode
   its complete body and runtime-only semantics. Origin failure is 400, media
   failure is 415, size failure is 413, and decode failure is 400. For an exact
   GET, strictly decode and cross-check its path and complete query/cursor now;
   attention's full decode MUST reproduce its already-checked selector.
8. Perform target lookup, redaction, current policy/actionability, revision,
   command, fence, budget, signer, provider/tool, and write checks in that
   order.
9. A public session POST instead executes step 1, exact public-route
   classification, Origin/media/body limits (4096 bytes), its server-instance
   rate bucket, strict decode, then challenge authority. All exchange nonce,
   challenge, attestation, state, expiry, replay, and race failures are the
   fixed 401; a second pending challenge or internal authority failure is 503.
   A decoded challenge request whose requested/policy/supported capability
   intersection is empty returns exact 400 `invalid_request` and allocates no
   challenge.

Within the protected subtree after step 1, invalid authentication always
returns HTTP 401 with:

    {"schema":"proof.operator.error/v1","code":"authentication_required","message":"Operator authentication is required."}

A valid session lacking any route capability returns HTTP 403 with:

    {"schema":"proof.operator.error/v1","code":"capability_required","message":"The session lacks the required capability."}

Those bodies do not distinguish absence, format, expiry, revocation, policy,
route, target, key, or requested capability details. After authentication and
capability success, these are the only v1 public error codes:

| HTTP | Code | Exact message |
|---|---|---|
| 400 | invalid_request | The request is invalid. |
| 401 | authentication_required | Operator authentication is required. |
| 403 | capability_required | The session lacks the required capability. |
| 404 | not_found | The requested resource was not found. |
| 409 | not_actionable | The target is not actionable. |
| 409 | stale_revision | The target revision changed. |
| 409 | stale_fence | The run ownership fence changed. |
| 409 | idempotency_conflict | The idempotency key is bound to another command. |
| 409 | cursor_stale | The cursor is no longer valid. |
| 413 | request_too_large | The request is too large. |
| 415 | unsupported_media_type | Content-Type must be application/json. |
| 429 | rate_limited | The request rate limit was exceeded. |
| 503 | control_unavailable | Operator control is unavailable. |

Each error uses the strict ErrorEnvelope and is no-store. Internal errors,
paths, SQL, keys, arguments, credential fragments, counts, and target state
are never reflected or logged. Operator-specific errors are not new
ExecutionError variants; proof-transport-http maps the closed OperatorError
enum exhaustively inside the E0002 router.

The internal-to-boundary mapping is also exhaustive. `OperatorStoreError` is
never mapped by message text, and its `invalid` variant is never an attacker-
visible 400: strict transport decoding has already completed before a store
call, so `invalid` means a trusted-caller/implementation invariant failure.

| Store boundary | Allowed store decision | Public/fatal disposition |
|---|---|---|
| `load_operator_workspace` during guarded startup | only the complete strict workspace value | every `OperatorStoreError` is a startup failure before listener bind, signer load, or audit and exits 1; an unexpected variant is additionally an implementor invariant failure |
| `register_governed_run` | only the closed `RegisterGovernedRunResult` variants, including exact-existing | every `OperatorStoreError` stops the runtime/control attempt; `unavailable` appends no event, while another failure appends only a healthy-store `control_failure` before termination |
| protected list/detail reads | an `Option::None` detail is 404; successful pages/details are 200 | `invalid`, `corrupt`, or `unavailable` is 503; any other `OperatorStoreError` is an illegal implementor result, treated as `corrupt` and 503 |
| `execute_operator_command` | `conflict` is allowed only for the committed altered-idempotency tuple; `not_actionable`, `stale_revision`, and `stale_fence` are allowed only after their exact rejection transaction; `not_found` is allowed only when the transaction proves the target absent and appends no command, receipt, or audit row | respectively 409 `idempotency_conflict`, 409 `not_actionable`, 409 `stale_revision`, 409 `stale_fence`, and 404 `not_found`; `invalid`, `corrupt`, `signer_failed`, or `unavailable` is 503 |
| authority-audit append | no rejection variant is recoverable | every variant is 503 if a response is still possible; clear volatile authority and terminate as specified in section 15 |
| `load_completed_replay` | absence is the typed `not_found` result, not an error | `invalid`, `corrupt`, or `unavailable` is a fatal worker/control failure; every other variant is illegal and treated as `corrupt`; no permit or boundary call |
| lease, budget, dispatch, commit, failure-settlement, and reclaim methods | only a method-specifically named `not_actionable`, `not_found`, `stale_revision`, or `stale_fence` rejection is recoverable; exact-existing/concurrency outcomes use their typed result variants, never generic `conflict` | no HTTP response is synthesized; the scheduler abandons that attempt with zero further authority. `conflict`, `invalid`, `corrupt`, `signer_failed`, `unavailable`, or an unnamed rejection is a fatal worker/control failure; append the contract-required `control_failure` when the store remains healthy, then terminate |
| guarded provisioning/upgrade/open/release | only the closed `OperatorProvisioningError` outcomes documented in section 4 are allowed | init prints fixed nonsecret stderr and exits nonzero; launch fails before bind; neither is translated to an HTTP envelope |

`OperatorAuthError` is translated only at the auth-first boundary: missing,
unknown, expired, replaced, or revoked volatile authority is 401; authenticated
capability failure is 403; malformed authenticated auth bodies are 400; clock,
entropy, audit, or internal verifier failure is 503 and clears authority. The
cursor codec maps authenticated binding/expiry/filter rejection only to 409
`cursor_stale`; missing key, clock, or internal codec failure is 503. The
evaluator's recording-store boundary matrix iterates every row above and every
closed `OperatorStoreError` variant at each applicable boundary; named Tower/
runtime vectors separately freeze the externally distinguishable and fatal
representative paths. Every E0002 store/auth/environment/catalog 503 is fail-closed: after
the one response attempt, the process stops accepting requests, clears
volatile authority, closes the trusted store, and exits. There is no
continue-serving `unavailable` branch in v1.

The evaluator's required `store_error_matrix` enumerates the 21 exact fallible
trait methods in section 15 against all nine closed `OperatorStoreError`
variants (189 cells) and four typed absence results. Its disposition codes are
normative abbreviations for this table: `command_*` codes are the named public
409/404 plus their frozen command/stale-fence/no-audit chronology;
`authority_append_503_no_audit_clear_terminate` clears unpublished/all volatile
authority and stops because the audit append itself failed;
`startup_fatal_no_bind_exit1` fails the trusted open before listener bind or
audit; `http_fatal_503_control_failure_terminate` and
`http_illegal_as_corrupt_503_control_failure_terminate` append the sole
healthy-store `control_failure`, whereas
`http_unavailable_503_no_audit_terminate` cannot rely on that store;
`runtime_fatal_control_failure_terminate` and
`runtime_illegal_as_corrupt_control_failure_terminate` stop further authority
and append the sole healthy-store `control_failure`, whereas
`runtime_unavailable_no_audit_terminate` appends none; and `runtime_{not_actionable,not_found,
stale_fence,stale_revision}_rejection` is permitted only where that exact
method contract names it, with that method's frozen audit sequence and zero
further authority. The four non-error results are `Option::None` for run,
approval, and command-receipt detail, each mapping to HTTP 404, plus typed
`ReplayLookupResult.outcome = not_found`, which creates no reservation or
authority. `execute_operator_command × not_found` remains the allowed-error
matrix cell for an absent command target and is HTTP 404 with zero governed
writes and no audit. Runtime validation requires
the 21 boundary ordinals and variant keys to equal the schema's literal order;
no duplicate pair, derived default, wildcard, or skipped cell is permitted.

### 9.3 Redacted projections

Operator reads are purpose-built projections, never serialized AgentRun,
ExecutionContext, Proof, checkpoint, provider, or legacy audit rows.

Permitted fields include nonsecret UUIDs, statuses, revisions, fixed
operation/version names, timestamps, checkpoint/digest identities, bounded
Human-authored summaries, budget counters, command outcomes, and the exact
required Human UUID. Forbidden fields include raw workspace paths, private or
public key bytes, session/challenge/nonce material, model prompts/responses,
provider credentials, environment, headers, raw tool arguments/outputs,
unredacted execution context, SQL/errors, and arbitrary JSON.

RunDetail always includes the exact redacted authority summary, the complete
ordered attempt history, complete signed Proof-reference history, pending consequence, current
checkpoint/control/fence identity, aggregate budget, approval identity, and a
strict recovery summary when the run is recoverable.
The arrays have no protocol-level item cap: `attempts` is ordered by ascending
attempt sequence and `evidence` by ascending proof timestamp then proof UUID.
Every successful attempt and successful operator transition exposes a Proof
reference that resolves only through trusted storage, never by mounting the
general proof route. ApprovalDetail includes request, argument, consequence,
and immutable binding digests plus the same PendingConsequence shown at the
Human decision point.

Reviewable arguments are an ordered array of strict ReviewField values:
field name, display classification, redacted bounded display value, and
kernel input digest. Secret-classified values show only the literal
`[redacted]`; any other secret display is schema-invalid.
The UI states that the signed request digest, not the display projection, is
the authority binding.

The immutable ApprovalBinding stores that exact ordered ReviewField array and
the exact strict PendingConsequence inside its canonical `binding_json`; the
two independently recomputed digests and `binding_digest` cover those values.
ApprovalDetail is reconstructed from this sealed row after restart, never from
logs, prompts, raw operation input, or an in-memory projection.

Migration 14 adds append-only operator_run_projections. Each governed-run
change appends one complete strict snapshot with a monotonically increasing
global projection_sequence and per-run projection_revision. The current
projection is the greatest sequence for a run. Legacy write methods reject
non-idempotent writes to governed runs, so a projection cannot be bypassed.

Attention is derived, not caller-selected: `running` is only Queued/Running;
`awaiting_decision` is only WaitingForInput with an exact approval and required
Human; `recoverable` is only Failed with a strict pre-dispatch RecoveryDirective;
and `terminal` is only Succeeded/Failed/Cancelled without pending approval or
recovery authority. Projection checkpoint ID/sequence/digest and recovery
directive ID/digest are all-null or all-present as applicable. The same mapping
is enforced by RunDetail, strict schemas, migration checks, and the evaluator.

### 9.4 Keyset pagination

The first page transaction captures a high_water_sequence equal to the
greatest relevant projection/audit/receipt sequence visible in that
transaction. Results are ordered by sequence descending, then UUID descending.
Later inserts have a greater sequence and are excluded. For run projections,
each page selects the latest version for each run whose sequence is at or
below the high water, so concurrent inserts and updates produce neither skips
nor duplicates within the snapshot.

The opaque cursor is unpadded base64url of:

    canonical CursorClaims JSON || 32-byte keyed BLAKE3 MAC

The key is a fresh process-random 32-byte cursor key and never leaves memory.
Claims bind schema, route, workspace, instance, session, Human, auth epoch,
the sorted required capabilities, canonical filter digest, exact sort,
page_size, high-water sequence, last sequence/UUID, issued time, and expiry.
MAC verification is constant-work and precedes database lookup. A cursor is
invalid after expiry, session replacement/revoke, policy change, control
restart, route/filter/sort/page-size change, or cross-scope use. All failures
return cursor_stale after authentication. Cursor tokens are credentials for
scope, not session credentials; they never substitute for the session header.

## 10. Durable commands and Human intent

Every mutation body binds its literal schema, client-generated UUIDv7
command_id and idempotency_key, workspace_id, server_instance_id, session_id,
human_id, auth_epoch, policy_revision, target IDs, expected run/step/control
revisions, expected checkpoint identity/digest where applicable, and expected
fence epoch. The path target and every repeated body binding MUST match.

The request digest is the Proof-Operator-Command-v1 digest of the complete
strict canonical body. The idempotency tuple is
(workspace_id, human_id, idempotency_key). Except for SessionRevokeRequest, an
exact retry under still-valid authority returns the original terminal receipt
byte-equivalently and is read-only. Reusing the tuple with
another request digest returns idempotency_conflict, creates no second command,
receipt, proof, or governed effect, and appends exactly one redacted
command_conflict audit event. V1 uses one SQLite transaction for command plus
terminal receipt, so a durable command without a receipt cannot exist and
`command_in_progress` is not a v1 error or outcome.

`CommandExecutionRequest` carries only the already strict-decoded
timestamp-free `OperatorCommand` plus `OperatorMutationScope`; it never accepts
a caller-built `CommandEnvelope`. Inside the BEGIN IMMEDIATE transaction,
storage recomputes the command digest and route-required capabilities and
performs the idempotency lookup before clock/entropy use. Exact replay returns
read-only without consulting the environment. An altered tuple reads trusted
UTC once only to append `command_conflict`. A new command reads trusted UTC
once, retains it for every expiry comparison and command/signing timestamp,
performs target validation, then constructs the complete durable
`CommandEnvelope` with that `requested_at` before insertion or private-key
access.
The envelope, digest, capability list, and timestamp are immutable outputs of
that trusted construction and are passed unchanged to the signer.

Authentication and mutation share one in-memory authority lease. Revocation
and expiry use that same lease. A mutation therefore either commits its
receipt/effect before revoke/expiry linearizes, or revoke/expiry wins before
Human-key access, provider/tool dispatch, or durable effect.

Rejection audit policy is exact. Peer/Host, authentication, rate, envelope,
strict-decode, capability, cursor, and pre-store failures append no audit row;
they cannot safely name a target. A revoke/expiry race observes only the
winning `session_revoked` or `session_expired` event. An authenticated strict
command that reaches the store but fails target actionability or revision
appends one redacted `command_rejected`; altered idempotency reuse appends one
`command_conflict`. An authenticated command fence mismatch appends one
`stale_fence_rejected`; raw runtime custody failure appends nothing. The strict
reads schema accepts exactly these four command-attributed profiles:

| command branch | exact non-null command-attempt fields | presence mask |
|---|---|---:|
| approval decision | `human_id`, `session_id`, `run_id`, `approval_request_id`, `command_id`, `command_kind = approval_decide`, `fence_epoch` | `0x101e3` |
| run cancel | `human_id`, `session_id`, `run_id`, `command_id`, `command_kind = run_cancel`, `fence_epoch` | `0x101a3` |
| approval-branch run resume | `human_id`, `session_id`, `run_id`, `approval_request_id`, `command_id`, `command_kind = run_resume`, `fence_epoch`, `decision_digest` | `0x2101e3` |
| recovery-branch run resume | `human_id`, `session_id`, `run_id`, `command_id`, `command_kind = run_resume`, `recovery_directive_id`, `fence_epoch`, `recovery_directive_digest` | `0x4181a3` |

Every closed nullable field not listed in the selected row is null. Because
each profile carries `session_id`, its `session_authority_digest` is the exact
matching active-session authority digest under the common audit rule. Session
revoke has no fence and gains no profile. The obsolete runtime-shaped
`0x16c30` profile is invalid: reservation, lease, process, permit, and other
runtime custody references cannot be fabricated for a command rejection. A
decoded budget request rejected by the aggregate account or settlement-state
machine appends one `budget_rejected`. An
approval deadline transition appends `approval_expired`. After an internal
signer/prepared-result failure rolls back its transaction, a healthy store
appends one redacted `control_failure` before the process fails closed; if that
append fails, no success is returned and the process terminates. The
evaluator's typed expected recipe freezes every new event's kind, outcome,
Proof operation, sequence offset, prior-link profile, subject binding, full
strict value, and recomputed digest. An empty `new_events` array forbids a new
event; `prior_events_preserved` requires the complete baseline prefix to remain
byte-identical.

`AuditEvent.command_id` and `reservation_id` are strict logical attempt
references, not SQLite foreign keys. A rejected/conflicting command or a
pre-reservation aggregate-budget rejection intentionally names the proposed
UUID even though the rolled-back/rejected durable row does not exist. The
store validates an existing same-workspace command for every command-bearing
event other than `command_rejected`, `command_conflict`,
`stale_fence_rejected`, and a command-scoped `control_failure`; it validates an
existing same-workspace reservation for
every reservation-bearing event other than `budget_rejected`. Those exception
kinds require the proposed ID carried by the decoded request. All other audit
references retain their SQL foreign keys. This is the only v1 relaxation of
audit referential existence and never authorizes a lookup by the proposed ID.

### 10.1 Approval decision

ApprovalDecisionCommand requires approval.read and approval.decide. The
control process holds the in-memory authority lease while SqliteStore invokes
the kernel-owned `OperatorSigner` callback from inside one BEGIN IMMEDIATE
transaction. The implementation:

1. authenticates and decodes;
2. recomputes command digest/capabilities and performs exact command
   idempotency; exact replay returns without clock/entropy, while conflict
   appends its sole event and returns;
3. reads trusted UTC exactly once for the new-command branch, then reloads
   policy, run, waiting step, checkpoint tail, signed request, sealed
   operator_approval_binding, and existing decision, and verifies the
   persisted Agent signature on SignedApprovalRequest before Human-key access;
4. verifies the authenticated Human equals the sealed required Human and the
   enrolled public key; no delegation is accepted;
5. verifies operation, version, input/argument/consequence digests, expiry
   against that retained trusted time,
   requested outcome, revisions, fence, and no terminal/conflicting decision;
6. obtains decision/proof UUIDv7 values from OperatorControlEnvironment,
   constructs the complete durable CommandEnvelope with the retained time and
   the complete unsigned SignedApprovalDecision, and inserts the immutable row
   without any output decision digest before private-key access;
7. invokes `sign_approval` with the store-constructed strict
   ApprovalSigningRequest containing that decision_id and validated_at; the
   callback may return only decision_digest and signature, cannot choose an ID
   or timestamp, descriptor-loads the one exact Human
   key, rechecks its immutable tuple,
   signs the existing SignedApprovalDecision message, verifies the new
   signature, and owns the non-Clone/Debug/Serialize zeroizing secret buffer
   and descriptor until it has closed the descriptor and zeroized the buffer;
   that cleanup is mandatory on success, signer or verification error,
   public-key mismatch, unwind, and shutdown, and a cleanup failure is a fatal
   `control_unavailable` failure before any decision is published;
8. inserts the existing approval_decisions row and constructs the strict
   ControlTransitionOutcome;
9. invokes `sign_operator_proof` with the persisted Agent and a store-built
   OperatorProofSigningRequest containing the complete strict CommandEnvelope,
   its ControlDigest, the precomputed kernel OperationInput and OperationOutput
   digests, proof UUIDv7, timestamp, and outcome. The callback independently
   recomputes both kernel digests, constructs the exact ProofBody with no
   delegation or expiry, maps the command kind to the exact `operator.*::v1`
   operation, signs it, and returns the complete Proof; and
10. verifies and inserts the Proof, new run projection, chained audit event,
    and terminal command receipt, then commits all rows together.

Any callback error, Human tuple mismatch, Agent proof failure, or transaction
error rolls back the uncommitted command and every effect and returns the exact
503. The store never accepts an already-signed decision from the caller and
the signer never selects a Human or opens the database.

Approve and deny are permanent signed decisions. Neither changes the run out
of WaitingForInput, executes a tool, dispatches a provider, or calls runtime
resume. A second identical request replays the receipt; another outcome/key
conflicts. There is no withdrawal or replacement.

### 10.2 Explicit resume

RunResumeCommand requires run.read and run.resume and is the only operator
action that may continue a waiting/recoverable run. It binds the exact
checkpoint tail and exactly one authority branch: either the approval request
plus decision digest, or a RecoveryDirective ID plus digest.

Resume is eligible only for:

- WaitingForInput with one exact durable approve or deny decision for the
  sealed request; or
- Failed with a strict durable RecoveryDirective classified
  pre_dispatch_recoverable, bound to the same original dispatch intent and
  exact checkpoint, and requiring no ambiguous budget disposition.

Cancelled, Succeeded, an ordinary Failed state, an expired/undecided request,
an ambiguous dispatch, or a stale checkpoint is not resumable. An approved
decision continues toward the governed step only after a later fenced budget
reservation and dispatch permit. A denied decision is consumed by explicit
resume to record a deterministic terminal Failed outcome with zero tool
effect. Resume never creates a new approval decision.

The command transaction validates the selected branch and existing active
lease. It never creates or inherits a lease implicitly. A failed run without a
live lease must first use the explicit reclaim operation, which returns the
directive and new LeaseAuthority. Resume changes run/control state, creates an
Agent-signed `operator.run_resume` Proof for an applied transition, appends
projection/audit, and emits a receipt. The approval branch appends exactly one
`run_resumed` event. The recovery branch atomically marks the exact directive
consumed and appends `recovery_completed` followed immediately by
`run_resumed` in the same command transaction. `recovery_completed` binds the
same run, source reservation/lease, directive digest, and current lease/fence
without a Proof; `run_resumed` binds the command/session authority and the
Agent-signed transition Proof. No other method writes `recovery_completed`. It performs no provider
or tool call inside HTTP.

### 10.3 Cancel

RunCancelCommand requires run.read and run.cancel. The durable cancel
transaction competes with begin_dispatch:

- if cancel commits before begin_dispatch, it sets Cancelled, releases only
  proven pre-dispatch reservations, inserts an Agent-signed
  `operator.run_cancel` Proof plus projection/audit/receipt, and every later
  dispatch CAS fails; provider calls, tool calls, and external effects are
  exactly zero;
- if begin_dispatch committed first, cancel does not claim a zero-effect
  success. It records not_actionable while the active reservation is
  dispatching/ambiguous, and recovery must settle or forfeit it; or
- if the run is already terminal, the first call writes a stable
  already_terminal receipt and one `command_rejected` audit event without
  changing the run or creating a transition Proof. CommandResult.outcome is
  `already_terminal`; exact retries instead return `exact_replay` around that
  same receipt.

Cancellation never revokes a session, denies an approval, deletes evidence, or
unseals terminal run history.

### 10.4 Session revoke

SessionRevokeRequest has the common command bindings but no run, step,
approval, checkpoint, or fence. Under the authority lease it persists the
session_revoke command, Agent-signed `operator.session_revoke` Proof, audit
event, and receipt, commits, then invalidates the session before releasing the
lease or returning. If durable persistence fails,
the server still clears volatile authority and terminates fail-closed without
a success response. No later mutation can cross that failure. Because revoke
invalidates the only authorizing session before response publication, a lost
response cannot be replayed by POST: the old-session retry is auth-first 401
with no new audit row, and a newly authenticated session cannot reproduce the
body's session binding. Recovery is a fresh signed session that includes
`audit.read`, followed by protected GET of the original command_id receipt.

## 11. Fenced ownership and crash recovery

Each governed run has at most one active lease but retains every historical
lease row. `lease_id` is the primary key, `(run_id,fence_epoch)` is unique,
and a partial unique index over `run_id WHERE state='active'` enforces the
singleton. A lease contains process_epoch_id, owner server instance, random
token digest, strict UTC acquisition/renewal/expiry, revision, and
monotonically increasing fence_epoch. The raw 32-byte lease token is
process-memory only. Kernel-owned LeaseTokenCustody owns that zeroizing buffer
and is not Clone, Copy, Debug, Display, Serialize, or Deserialize. It exposes
one pre-binding claim/reclaim proof borrow and, after a successful store result
binds the exact lease/fence tuple, repeatable immutable LeaseAuthority borrows
for fenced mutations. Release consumes the bound custody; failed claim,
ambiguous reclaim, process loss, and Drop zeroize it. No store result returns
raw bytes and no caller can construct a bound authority from IDs or a digest.

The `lease_token`, `new_lease_token`, and `LeaseAuthority.lease_token` entries
in store-v1 are logical fields describing the exact borrowed 32 bytes checked
by storage; the corresponding kernel request structs are lifetime-bearing and
explicitly non-serializing. They are never JSON payloads. Durable-v1 contains
no secret-bearing claim shape. Test-only constructors may inject a guarded
buffer but must use zero-secret sentinels and may not serialize fixtures or
evidence containing it.

The fixed lease TTL is 30 seconds and the renewal cadence
is a scheduler target of 10 seconds. Claim, reclaim, renew, and release carry
no caller-selected transition timestamps. The store reads trusted UTC once
inside the transaction, assigns acquisition/renewal/release from that value,
and sets every active expiry to exactly trusted now plus 30 seconds. Renewal
is accepted only strictly before expiry; the 10-second cadence is not a
client-controlled extension. Equality with the expiry is expired. Live holders enforce a
derived monotonic deadline; durable takeover also requires trusted UTC now
greater than or equal to expires_at.

`release_run_lease` is a quiescent-only fenced CAS. The exact lease must still
be active and unexpired, `operator_run_control.active_dispatch_reservation_id`
must be NULL, and no reservation for that lease may be `reserved` or
`dispatching`. Callers must first explicitly release every proven
pre-dispatch reservation or settle the one post-permit failure. Otherwise
it returns `not_actionable` without an audit or other mutation and leaves the
lease, fence, run, replay, and budget unchanged. A holder or shutdown path with
an authorized dispatch must first call `settle_runtime_failure`; it may release
the lease only after the full forfeit/replay/recovery transaction commits.

Every mutable runtime/storage request embeds strict LeaseAuthority: workspace,
run, lease, owner instance, process epoch, fence, expected control revision,
and the raw lease token. Inside the same transaction as the proposed write,
storage derives Proof-Operator-Lease-Token-v1, compares all 32 digest bytes in
constant work, verifies the exact active row, obtains trusted store time, and
requires trusted now strictly before `expires_at`. Equality or later rejects as
`stale_fence` before mutation; renew cannot resurrect an expired lease and no
caller-supplied timestamp substitutes for this check. The rule applies to
renew, release, reserve, pre-dispatch settlement, begin, commit, runtime-failure
settlement, and recovery completion. Database-visible IDs or the
stored digest are not possession. The token is denied Debug/Display/Serialize,
never persisted in JSON, logs, errors, fixtures, or evidence, and is zeroized
on release/loss. Wrong-token failure is stale_fence with zero mutation.

The two ownership-establishing exceptions are `claim_run_lease` and
`reclaim_run`: they cannot prove a pre-existing raw token. Their strict claim
requests instead borrow a fresh unbound LeaseTokenCustody and bind the new
random token plus workspace, run, instance,
process epoch, expected fence/control revision, and (for reclaim) the
expired historical lease and exact checkpoint. The transaction stores only
the derived token digest before returning authority. Every later mutable call,
including renew, release, reserve, begin, commit, failure settlement, and
recovery completion, proves that raw token.

At most one reservation per run may be open (`reserved` or `dispatching`),
enforced by a partial unique index and by the reserve transaction before it
increments aggregate counters. Reclaim occurs only after expiry under BEGIN
IMMEDIATE and first marks the old lease released without changing its
lease_id. It then resolves that single historical open reservation before any
new authority is exposed:

- zero open rows is valid for a Queued, Running, or WaitingForInput run between
  governed boundaries. Reclaim preserves the checkpoint and budget, inserts a
  new active lease at old fence plus one, and returns `idle_reclaimed` with a
  null directive;
- zero open rows is also valid for Failed only when the current projection
  carries one exact existing `pre_dispatch_recoverable` RecoveryDirective,
  no `recovery_completed`/run-resume event has consumed it, and its checkpoint
  equals the request tail. Reclaim preserves that immutable directive and the
  Failed projection, inserts a new active lease at old fence plus one, appends
  only the lease-reclaimed audit event, and returns `recoverable_reclaimed`
  with the new lease and same directive;
- a `reserved` row is proven pre-dispatch, transitions to `released`, and is
  subtracted from all five aggregate reserved counters. Its immutable intent
  seeds one `pre_dispatch_recoverable` RecoveryDirective. The transaction then
  inserts a new active lease at old fence plus one and returns
  `pre_dispatch_recovered` with that lease and directive; or
- a `dispatching` row is ambiguous and transitions to `forfeited` at exactly
  its full five-dimensional ceiling. Replay is resolved failed/indeterminate,
  active dispatch is cleared, and the run becomes terminal Failed. No new
  lease or RecoveryDirective is created; `ReclaimResult` is
  `ambiguous_forfeited` with both fields null.

Multiple-open-reservation states fail closed as corruption. Zero-open reclaim
of a terminal or ordinary Failed run without that exact still-live directive
is `not_actionable`; it cannot manufacture recovery authority. Reservations
keep their composite foreign key to the historical `(run_id,lease_id)`.
`idle_reclaimed` and `recoverable_reclaimed` append one `lease_reclaimed`
event. `pre_dispatch_recovered` appends, in order, `budget_released`,
`recovery_started`, and `lease_reclaimed`, with the new directive present only
in the latter two profiles. Every row, projection, and audit event commits
atomically. `ambiguous_forfeited` appends `budget_forfeited` followed by
`control_failure` and returns no authority. The stale owner cannot dispatch or
commit because every barrier includes both the old fence and raw token proof.
An already-authorized external response received after authority loss returns
only the typed redacted runtime/store `stale_fence` failure observation. It
appends no `AuditEvent` and cannot mutate run, budget, proof, tool, or
projection state.

Worker and control restarts are distinct:

- runtime_worker_restart keeps the control process, instance, cursor key, and
  session alive. The worker reclaims only after lease expiry, preserves the
  same durable checkpoint/command semantics, increments the fence, and
  reconciles ambiguous budget before dispatch.
- control_plane_restart destroys challenges, session, token/cursor/lease
  secrets, instance identity, and in-memory waiters. Durable runs, commands,
  receipts, audit, projections, budgets, and lease rows remain. Startup uses a
  fresh instance, reclaims only after expiry, and requires a new signed Human
  challenge before any protected read or action.

Crash recovery MUST NOT silently replace the requested model, tool,
operation/version, arguments, Human, outcome, idempotency key, checkpoint,
budget ceiling, or effect.

## 12. Aggregate budgets and dispatch barriers

One active budget account governs all v1 controlled runs in the workspace.
Its immutable limit set is:

- steps;
- provider tokens;
- wall-clock duration in milliseconds;
- cost in micro-USD; and
- tool dispatches.

Every value is a safe integer. The account also stores reserved and committed
counters for every unit plus an absolute UTC deadline. No v1 route changes or
resets limits.

Before any provider or tool boundary, the active fenced owner reserves the
adapter's declared worst-case ceiling in one transaction. The store checks,
without overflowing, that:

    requested <= limit - committed - reserved

for every unit, and trusted now is strictly before the account deadline. The
aggregate account deadline is the sole v1 run-budget deadline; canonical
AgentRun has no separate deadline and none may be inferred from a checkpoint,
lease, session, or caller field. Unknown or unbounded adapter maxima fail
before dispatch. Concurrent
reservations serialize; at most the requests fitting all dimensions win.

`begin_dispatch` reads trusted UTC again inside its transaction. For a new
permit it requires trusted now strictly before the linked account deadline;
equality is expired. On failure it atomically releases the still-proven
pre-dispatch reservation, decrements all reserved counters, appends exactly one
`budget_rejected`, returns `not_actionable`, and creates no permit, dispatch
token binding, provider/tool call, or replay completion. DispatchPermit carries
the immutable account `budget_deadline_at`. Immediately before handler entry,
the runtime rechecks lease/fence/custody and requires environment UTC strictly
before that permit deadline; failure consumes custody through the pre-effect
failure settlement and performs no external call.

Required exact-replay calls first invoke the read-only
`load_completed_replay(ReplayLookupRequest)` before reserving budget. A
verified `completed` result returns the typed original completion with no
lease, reservation, permit, token, audit append, counter change, or external
call; it remains available after deadline or full budget exhaustion because it
is not dispatch authority. `not_found` permits the normal reserve/begin path.
If reserve then rejects for deadline/capacity, runtime performs one more exact
lookup before surfacing rejection so a concurrent completed commit wins over a
false budget failure. The lookup uses the same strict replay-binding/current-
state/catalog/Proof verification described below; malformed, conflicting, or
unverifiable rows are corrupt rather than not_found. Generic reserve never
waives a deadline or limit, and no absent/in-progress/failed replay can use
this read-only bypass.

BudgetReserveRequest carries the complete strict DispatchIntent, whose sole
`kind` and sole five-dimensional `ceiling` are the adapter-declared worst-case
maximum, its Proof-Operator-Dispatch-Intent-v1 digest, either null or the complete
ReplayClaimBinding, and a nullable complete RecoveryDirective. The replay
binding is stable scope: workspace, run, step,
checkpoint identity, operation/version, UUIDv7 idempotency key, kernel input
digest, and Agent actor. It deliberately excludes the per-attempt claim token
and claim time. The reservation persists the canonical intent and complete
stable binding before begin; provider and step
reservations require null replay, while a tool's declared idempotency policy
determines whether replay is null or present. Reusing a reservation tuple with
different intent or replay bytes conflicts. There is no duplicate request-level
kind or ceiling from which the stored debit or later permit could diverge;
storage debits and persists `intent.ceiling` byte-for-byte before any permit.
Normal reservation requires null recovery. A post-resume recovery reservation
requires the consumed directive, a fresh budget-request idempotency key (the
old key remains permanently bound to its released source row), and exact
byte-equality of intent and replay with the immutable source reservation. The
directive binds that source reservation/budget/request digest/idempotency key,
so the fresh budget retry identity cannot change the effect/replay UUIDv7 or
semantics. Storage persists the directive link in the new reservation and
rejects a directive that is unconsumed, already reused, cross-run, or whose
historical source row does not match every bound byte. The retry reservation's
directive ID is a UNIQUE logical link, not a SQL foreign key, to keep migration
down acyclic; the directive retains the one-way foreign key to its historical
source reservation. Strict JSON/column equality and the same-transaction
lookup enforce the logical link before mutation.

The begin_dispatch transaction verifies raw LeaseAuthority, derives the
dispatch-token digest without persisting the raw token, and CASes the exact
lease/fence/control revision, uncancelled run, reserved row, canonical intent,
intent digest, replay binding, token digest, and call digest. Storage generates
the permit UUIDv7 from OperatorControlEnvironment inside that transaction and
returns it only in DispatchPermit; BeginDispatchRequest cannot select it.
For a required UUIDv7 exact-replay tool, BeginDispatchRequest also carries one
fresh replay claim UUIDv7; trusted store time supplies `claimed_at`. Neither is
part of ReplayClaimBinding. The store invokes a new transaction-borrowing replay helper
on the same SQLite connection; the helper implements the existing
ExecutionReplayClaim decision without opening or committing another
transaction. The legacy execution engine MUST NOT claim it earlier.

Only `Acquired` (or a no-replay intent) sets reservation `dispatching`. For an
acquired replay claim, the same transaction also inserts the immutable
`operator_replay_bindings` row; an existing row must be byte-identical. It stores
the immutable permit/call/intent/replay binding and authorization time, sets
the run's active reservation, appends `dispatch_authorized`, increments the
control revision, and returns DispatchResult `dispatch_authorized` with the
permit. This commit and permit are the sole provider/tool dispatch authority.

`Completed` is accepted only when `operator_replay_bindings` proves the replay
belongs to the same workspace/run/step/checkpoint and the current governed
run, step, checkpoint, control revisions, and linked Proof already reflect the
original successful commit. This proof joins the replay binding to its unique
committed operator reservation, strict persisted PreparedExecutionBinding and
RuntimeCommit JSON, their prepared/result digests, the referenced proof, and
the current run/step/checkpoint rows, then recomputes every digest. A globally completed replay with no operator
binding, a different binding, or state not already committed is `Conflict`;
it can never advance another run. The accepted path releases the
still-pre-dispatch reservation, leaves active dispatch null, appends exactly
`budget_released`, increments only the control/audit chronology needed for
that release, and returns `exact_replay` as a typed reload of the already
committed revisions plus the original canonical output and complete verified
Proof. It creates no permit, proof, run/step/checkpoint write, provider/tool
call, or new replay completion. The canonical output string is duplicate-detecting parsed,
strictly decoded by the registered operation/version output schema, and its
kernel output digest and full Proof are independently reverified before
return. `Conflict`, `InProgress`, `Failed`, or `Unsupported` likewise release
the reserved budget, leave active dispatch null, append exactly one
`budget_rejected`, increment control revision, and return their matching
tagged result with no permit or completion. No non-authorized replay outcome
may leave a reserved row, call a handler, or authorize dispatch.

### 12.1 Prepared execution and atomic commit

E0002-05 adds these exact kernel-owned types; no storage- or runtime-owned
lookalike is permitted:

~~~text
GovernedEffectPolicy = Ineligible | NoDurableOrExternalEffect
PreparedHandlerMutation = NoEffect

PreparedHandlerOutput {
  output: serde_json::Value,
  mutation: PreparedHandlerMutation,
  boundary_usage: PreparedBoundaryUsage,
}

PreparedBoundaryUsage {
  boundary_kind: Provider | Tool,
  tokens: u64,
  cost_microusd: u64,
  tool_dispatches: u64,
}

GovernedExecutionPlan {
  authorization: DispatchAuthorization,
  intent: DispatchIntent,
  run_before: AgentRun,
  step_before: AgentRunStep,
  checkpoint_tail: Option<AgentCheckpointTail>,
  replay_claim: Option<ExecutionReplayClaim>,
}

PreparedApprovalBundle {
  request: SignedApprovalRequest,
  binding: ApprovalBinding,
}

PreparedReplayTransition = None | Complete(ExecutionReplayClaim)

PreparedUsage {
  boundary_kind: Provider | Tool,
  boundary_calls: 1,
  adapter: String,
  model: Option<String>,
  steps: u64,
  tokens: u64,
  cost_microusd: u64,
  tool_dispatches: u64,
  input_digest: ContentDigest,
  output_digest: ContentDigest,
}

PreparedGovernedExecution {
  output: serde_json::Value,
  execution_context_id: Uuid,
  context: ExecutionContext,
  proof: Proof,
  run_after: AgentRun,
  step_after: AgentRunStep,
  checkpoint: Option<AgentCheckpoint>,
  events: Vec<AgentRunEvent>,
  evaluation: Option<AgentRunEvaluation>,
  approval: Option<PreparedApprovalBundle>,
  handler_mutation: PreparedHandlerMutation,
  replay: PreparedReplayTransition,
  usage: PreparedUsage,
}
~~~

The kernel generates `execution_context_id` before engine execution and the
field is part of the exact PreparedGovernedExecution serialization order shown
above. `commit_runtime_barrier` inserts the context under that exact UUID; the
store MUST NOT synthesize or substitute an ID. PreparedExecutionBinding
contains the same ID. Its `result` is the strict
PreparedRuntimeResultBody projection, and `result_digest` is recomputed from
that body. The duplicate values in the prepared Rust bundle, result body, and
event/proof rows must agree before any write.

`DispatchTokenCustody` and its `DispatchAuthorization<'a>` borrow are
kernel-owned, deliberately not Clone, Copy, Debug, Display, Serialize, or
Deserialize. Custody owns one zeroizing raw 32-byte dispatch token and two
atomic states, `effect_unused|effect_consumed` and
`settlement_unused|settlement_consumed`. It exposes exactly one mutable
authorization borrow and then exactly one consuming conversion into either a
RuntimeCommitRequest or RuntimeFailureRequest. The borrow contains the verified DispatchPermit
and access to that single token whose
Proof-Operator-Dispatch-Token-v1 digest is persisted in the reservation and
permit, plus the process lease-liveness guard and derived monotonic deadline.
The runtime generates custody immediately before begin_dispatch and passes a
nonserializing proof borrow in BeginDispatchRequest; it retains the sole owning
custody value. No byte copy is returned from storage.
ExecutionEngine maintains a process-local atomic consumed-permit set and takes
GovernedExecutionPlan by value; it consumes and zeroizes the authorization
borrow, marking only `effect_consumed` (not erasing the sole custody bytes),
only after atomically rechecking that the lease guard is live and the monotonic
deadline has not reached equality, immediately before handler entry. A repeated permit ID or token, even if reconstructed by
trusted test code, fails before provider/tool entry. Process loss destroys the
token and forces post-permit failure settlement/recovery; it never recreates
dispatch authority.

`OperationHandler` gains default-compatible
`governed_effect_policy_for(version)`, which defaults to `Ineligible`, and
`execute_governed_versioned(version, input, context)`, whose default fails
before handler entry. The latter may return only `PreparedHandlerOutput`.
E0002 v1 registration accepts only handlers declaring
`NoDurableOrExternalEffect`; their returned mutation must be exactly
`NoEffect`. A handler that opens storage, mutates a domain, performs a live or
unbounded effect, or requires a transaction-local domain applier is
ineligible. This is intentionally narrower than general OperationHandler and
matches the edition's no-new-domain-operation/no-live-effect scope.

Every registered governed run has an initial checkpoint before its first
lease or dispatch. GovernedExecutionPlan.checkpoint_tail remains an Option for
source compatibility but MUST be `Some` for E0002. ReplayClaimBinding,
ReclaimRequest, and RuntimeCommit always bind the same non-null pre-state
checkpoint ID, sequence, and digest.

The exact new engine method is
`ExecutionEngine::execute_evidenced_unpersisted(operation, version, input,
context, GovernedExecutionPlan) -> Result<PreparedGovernedExecution,
ExecutionError>`. It verifies the plan/intent/permit/replay tuple, strictly
decodes and canonicalizes input and output under the registered schemas,
performs at most the one permit-authorized synthetic provider/tool boundary,
constructs the complete run/step/checkpoint/event/evaluation/approval and
signed Proof values, and performs no store, replay-ledger, or audit write.
Required-replay execution returns `Complete` with the exact already-acquired
claim; a non-replay call returns `None`. An execution error returns no prepared
value and is settled only through `settle_runtime_failure`.

PreparedGovernedExecution, PreparedUsage, and PreparedBoundaryUsage have
private fields and no public constructor. Only
`execute_evidenced_unpersisted` can construct the first two, and the selected
bounded-adapter module alone constructs PreparedBoundaryUsage through
`PreparedHandlerOutput`. PreparedBoundaryUsage is an internal
nonserializing Rust value and deliberately has no logical JSON schema.
Provider reports require `boundary_kind = Provider` and tool_dispatches = 0;
tool reports require `boundary_kind = Tool`, tokens = 0, cost_microusd = 0,
and tool_dispatches = 1.
Kernel constructors are available only to the registered bounded-adapter
module, not HTTP/store/request DTOs. The engine cross-checks the report kind,
adapter, model, and every value against the permit intent and its ceiling,
sets steps = 1 and exactly one boundary call, binds the canonical input/output
digests, and sets the step count for the prepared transition. Provider usage
has zero tool dispatches; tool usage has one tool dispatch and zero
provider-token/cost charge in v1. A caller cannot supply or revise metering,
and a missing, mismatched, or over-ceiling report is `result_invalid` settled
through the post-permit failure path.

PreparedGovernedExecution is a closed `Serialize` Rust struct in the field
order above and serializes with the exact snake_case field names shown; every
Option is present as either its value or JSON null. PreparedHandlerMutation is
the JSON string `"no_effect"`. PreparedReplayTransition is JSON null for
`None`, or the closed object
`{"schema":"proof.operator.prepared-replay-completion/v1","claim":<canonical ExecutionReplayClaim>}`
for `Complete`; ExecutionReplayClaim gains strict Serialize using its existing
field names and a closed key object. Its output is canonical JSON that has
already passed the exact registered output schema. Existing nested kernel
types use their current strict serde names, and E0002-05 freezes golden
serialization tests for both replay branches. `Proof-Operator-Prepared-Execution-v1` covers the
canonical serialization of every listed field and nested kernel value with no
omission. PreparedExecutionBinding exposes the context ID,
`handler_mutation = no_effect`, nullable replay-binding digest, and a strict
PreparedRuntimeResultBody containing the usage record, revisions, sequence
range, output/proof identities, and nullable checkpoint triple. Its payload
and result digests MUST be recomputed from the full struct and exact result
body before commit.

Current `execute_evidenced`, handler-owned persistence, handler calls to
save/claim/complete/fail, and legacy replay methods are forbidden for governed
runs.

The commit_runtime_barrier transaction verifies raw LeaseAuthority and again
derives and constant-work compares the raw dispatch token carried by
RuntimeCommitRequest, then CASes permit/fence/control/replay state. It validates the complete prepared
bundle and atomically writes replay completion, context, proof, run, step,
checkpoint, events, optional evaluation/approval binding, budget charge,
projection, and audit. RuntimeCommitRequest has no caller-supplied charge. The
store takes steps, tokens, cost, and tool dispatches from the sealed private
PreparedUsage after matching it to the permit/intent/adapter and recomputed
prepared digests. It derives duration_ms from its own trusted commit time minus
the persisted dispatch_started_at, rounded up to a nonnegative integer. These
five values form the exact persisted RuntimeCommit.actual_charge and returned
RuntimeCommitResult.charged. Actual values cannot exceed the reservation. It
releases unused quantities, clears active dispatch, and increments control
revision. No component may be committed before this barrier.

A proven pre-dispatch failure calls fenced
`settle_budget_reservation(ReleasePreDispatch)` and may transition only a
`reserved` row. An ambiguous or crashed post-permit dispatch calls fenced
`settle_runtime_failure(ForfeitPostPermit)` and may transition only a
`dispatching` row; it forfeits the full ceiling, resolves the replay claim as
failed/indeterminate, records the exact recovery classification, and updates
run/projection/audit atomically before any recovery attempt. Neither path is
automatic or inferred from silence. An observed actual value above a
reservation is corruption. Because the commit path already owns and has
verified the sole DispatchTokenCustody, the same transaction MUST reject every
prepared result/proof write, forfeit the full reservation ceiling, resolve any
replay claim as failed/indeterminate, clear the active dispatch, append
`budget_forfeited` followed by `control_failure`, and commit those control
records atomically. It then zeroizes custody and fails the control process
closed. No later settlement is possible or required, and no
actual-over-ceiling value is ever charged.

RuntimeCommitRequest and RuntimeFailureRequest consume DispatchTokenCustody by
value through its sole settlement conversion; their schema's `dispatch_token`
is the logical raw-byte field, not a serializable Rust field. The store verifies
its digest before any row read that could disclose target state and before
every mutation, then zeroizes custody on every return path. Failure before
handler entry still uses the failure conversion when a permit exists. A
caller-held permit or digest without custody cannot commit, settle, or recover
an effect. Raw dispatch-token bytes are forbidden from JSON serialization,
Debug/Display, argv, environment, logs, fixtures, browser state, audit, or
evidence under the same sentinel policy as lease tokens.

## 13. Append-only command chronology

operator_audit_events is the only operator audit source. Legacy audit is never
mounted. Each workspace has a serialized audit head. Appending sequence N
requires N = previous sequence + 1 and binds the previous event digest (null
only for sequence one). The event digest is the
Proof-Operator-Audit-Event-v1 digest of strict canonical AuditEvent including
its sequence and previous digest but excluding only event_digest itself.

The closed v1 event kinds are:

- session_challenge_issued, session_issued, session_replaced,
  session_revoked, session_expired;
- approval_decided, approval_expired;
- command_rejected, command_conflict, stale_fence_rejected;
- run_cancelled, run_resumed;
- lease_acquired, lease_renewed, lease_reclaimed, lease_released;
- budget_reserved, budget_committed, budget_released, budget_forfeited,
  budget_rejected;
- dispatch_authorized, runtime_result_committed;
- recovery_started, recovery_completed; and
- control_failure, control_shutdown.

For every atomic operation, the complete ordered append list is frozen below.
An empty list means no audit-head mutation. When a command transaction appends
more than one event, its terminal receipt links the final event; every event
still chains to its immediate predecessor. No implementation may add a
convenience event or reorder this list.

| Operation/outcome | Ordered event kinds |
|---|---|
| challenge accepted | `session_challenge_issued` |
| first successful exchange | `session_issued` |
| exchange replacing the active session | `session_replaced` |
| session expiry | `session_expired` |
| explicit session-revoke command | `session_revoked` |
| clean control shutdown | `control_shutdown` |
| approval deadline transition | `approval_expired` |
| applied approval decision | `approval_decided` |
| applied approval-branch resume | `run_resumed` |
| applied recovery-branch resume | `recovery_completed`, `run_resumed` |
| applied cancel with no reserved row | `run_cancelled` |
| applied cancel releasing a proven pre-dispatch row | `budget_released`, `run_cancelled` |
| absent command target | *(none; no command or receipt is inserted)* |
| terminal cancel, nonactionable existing target, or stale revision | `command_rejected` |
| authenticated command fence mismatch | `stale_fence_rejected` |
| altered idempotency tuple | `command_conflict` |
| governed-run registration or exact-existing registration | *(none)* |
| new lease claim | `lease_acquired` |
| lease renewal | `lease_renewed` |
| quiescent lease release | `lease_released` |
| lease/runtime rejection before authority is proven | *(none)* |
| new aggregate reservation | `budget_reserved` |
| exact-existing reservation | *(none)* |
| capacity/deadline/settlement rejection after authority is proven | `budget_rejected` |
| explicit pre-dispatch release | `budget_released` |
| successful begin dispatch | `dispatch_authorized` |
| begin-dispatch deadline equality/lapse, which also releases the row | `budget_rejected` |
| pre-reserve completed replay lookup | *(none)* |
| completed replay discovered after reservation | `budget_released` |
| conflicting/in-progress/failed/unsupported replay after reservation | `budget_rejected` |
| successful runtime commit | `budget_committed`, `runtime_result_committed` |
| post-permit failure, ambiguous reclaim, or invalid/over-ceiling prepared result | `budget_forfeited`, `control_failure` |
| idle or existing-recoverable reclaim | `lease_reclaimed` |
| pre-dispatch reserved-row reclaim | `budget_released`, `recovery_started`, `lease_reclaimed` |
| separately detected internal control/storage failure with a healthy audit store | `control_failure` |

Wrong raw lease/dispatch tokens, expired authority, malformed strict input,
pre-auth failures, and illegal cross-scope lookups append nothing because the
caller has not established safe audit authority. `stale_fence_rejected` is
therefore limited to an authenticated operator command whose target/fence can
be named without trusting raw runtime custody. A successful exact replay of a
terminal command or completed runtime result is read-only and appends nothing.

An applied operator-command semantic event (`approval_decided`,
`run_cancelled`, `run_resumed`, or `session_revoked`) and receipt link the same Agent-signed
kernel Proof reference; approval_decided additionally links the immutable
Human SignedApprovalDecision digest. The Proof input is the command and its
output is ControlTransitionOutcome, so neither audit nor receipt digests are
circular. Events contain only strict redacted references and outcomes. They never carry
paths, credentials, key bytes, raw arguments/output, prompts, provider
responses, or arbitrary error text. Immutable-table triggers reject UPDATE
and DELETE of audit events, commands, receipts, projections, approval
bindings, enrollments, and workspace identity. Audit head, leases, run
control, budget accounts, and reservations are mutable only through the
atomic store operations in this contract.

AuditEvent itself includes `workspace_id` inside the hashed JSON plus the
closed nullable reference set for Human/session/challenge, server instance,
run/approval/command kind, budget/reservation, current/source lease, process
epoch, permit, recovery directive, fence/auth/policy epochs, intent/call/
decision/recovery digests, failure scope, and Proof. The strict reads schema
freezes every kind/outcome/reference/null profile and exact transition-Proof
operation. SQL columns are denormalized copies of those fields; strict decode,
column equality, foreign keys, proof verification, and digest recomputation
all precede append.

The audit chain durably authenticates volatile ceremony scope without storing
a credential. `challenge_digest` equals the existing
SessionAttestation.signed_bytes_digest (Proof-Operator-Session-Challenge-v1
over canonical SessionChallenge) for challenge-issued, session-issued, and
session-replaced events and is null otherwise. `session_authority_digest`
equals the active SessionClaims.authority_digest whenever session_id is
non-null and is null otherwise. Thus issued/replaced events bind both the
signed challenge and resulting Human/workspace/instance/origin/capabilities/
absolute-expiry authority; expired/revoked and command events retain the same
session-scope link after volatile state disappears. Neither digest can be used
as a session credential.

## 14. Exact SQLite migration 14

Migration 14 is appended after version 13 without editing versions 1 through
13.

**Description:** create governed operator control, projection, fence, budget,
command, and audit schema

The exact up SQL is:

~~~sql
CREATE TABLE operator_workspaces (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    workspace_id TEXT NOT NULL UNIQUE
        CHECK (length(workspace_id) = 36 AND lower(workspace_id) = workspace_id),
    schema TEXT NOT NULL
        CHECK (schema = 'proof-operator-workspace/v1'),
    database_name TEXT NOT NULL
        CHECK (database_name = 'storage.db'),
    fingerprint_json TEXT NOT NULL CHECK (json_valid(fingerprint_json)),
    workspace_fingerprint TEXT NOT NULL UNIQUE
        CHECK (length(workspace_fingerprint) = 75
               AND workspace_fingerprint GLOB 'blake3-256:[0-9a-f]*'),
    schema_catalog_digest TEXT NOT NULL
        CHECK (length(schema_catalog_digest) = 75
               AND schema_catalog_digest GLOB 'blake3-256:[0-9a-f]*'),
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*'),
    agent_id TEXT NOT NULL REFERENCES principals(id),
    human_id TEXT NOT NULL REFERENCES principals(id),
    auth_epoch INTEGER NOT NULL
        CHECK (auth_epoch BETWEEN 1 AND 9007199254740991),
    policy_revision INTEGER NOT NULL
        CHECK (policy_revision BETWEEN 1 AND 9007199254740991),
    capabilities_json TEXT NOT NULL CHECK (json_valid(capabilities_json)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    CHECK (COALESCE(json_extract(fingerprint_json, '$.schema') =
                     'proof.operator.workspace-fingerprint-input/v1', 0) = 1),
    CHECK (COALESCE(json_extract(binding_json, '$.schema_catalog_digest') =
                     schema_catalog_digest, 0) = 1)
);

CREATE TABLE operator_human_enrollments (
    workspace_id TEXT PRIMARY KEY REFERENCES operator_workspaces(workspace_id),
    human_id TEXT NOT NULL UNIQUE REFERENCES principals(id),
    schema TEXT NOT NULL
        CHECK (schema = 'proof-operator-human-enrollment/v1'),
    capability_set_digest TEXT NOT NULL
        CHECK (length(capability_set_digest) = 75
               AND capability_set_digest GLOB 'blake3-256:[0-9a-f]*'),
    enrolled_at TEXT NOT NULL,
    enrollment_json TEXT NOT NULL CHECK (json_valid(enrollment_json))
);

CREATE TABLE operator_budget_accounts (
    budget_id TEXT PRIMARY KEY
        CHECK (length(budget_id) = 36 AND lower(budget_id) = budget_id),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES operator_workspaces(workspace_id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-budget-account/v1'),
    revision INTEGER NOT NULL
        CHECK (revision BETWEEN 0 AND 9007199254740991),
    state TEXT NOT NULL CHECK (state IN ('active', 'exhausted', 'closed')),
    max_steps INTEGER NOT NULL CHECK (max_steps BETWEEN 0 AND 9007199254740991),
    max_tokens INTEGER NOT NULL CHECK (max_tokens BETWEEN 0 AND 9007199254740991),
    max_duration_ms INTEGER NOT NULL
        CHECK (max_duration_ms BETWEEN 0 AND 9007199254740991),
    max_cost_microusd INTEGER NOT NULL
        CHECK (max_cost_microusd BETWEEN 0 AND 9007199254740991),
    max_tool_dispatches INTEGER NOT NULL
        CHECK (max_tool_dispatches BETWEEN 0 AND 9007199254740991),
    reserved_steps INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_steps BETWEEN 0 AND 9007199254740991),
    reserved_tokens INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_tokens BETWEEN 0 AND 9007199254740991),
    reserved_duration_ms INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_duration_ms BETWEEN 0 AND 9007199254740991),
    reserved_cost_microusd INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_cost_microusd BETWEEN 0 AND 9007199254740991),
    reserved_tool_dispatches INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_tool_dispatches BETWEEN 0 AND 9007199254740991),
    committed_steps INTEGER NOT NULL DEFAULT 0
        CHECK (committed_steps BETWEEN 0 AND 9007199254740991),
    committed_tokens INTEGER NOT NULL DEFAULT 0
        CHECK (committed_tokens BETWEEN 0 AND 9007199254740991),
    committed_duration_ms INTEGER NOT NULL DEFAULT 0
        CHECK (committed_duration_ms BETWEEN 0 AND 9007199254740991),
    committed_cost_microusd INTEGER NOT NULL DEFAULT 0
        CHECK (committed_cost_microusd BETWEEN 0 AND 9007199254740991),
    committed_tool_dispatches INTEGER NOT NULL DEFAULT 0
        CHECK (committed_tool_dispatches BETWEEN 0 AND 9007199254740991),
    deadline_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    limits_digest TEXT NOT NULL
        CHECK (length(limits_digest) = 75
               AND limits_digest GLOB 'blake3-256:[0-9a-f]*'),
    limits_json TEXT NOT NULL CHECK (json_valid(limits_json)),
    CHECK (committed_steps <= max_steps
           AND reserved_steps <= max_steps - committed_steps),
    CHECK (committed_tokens <= max_tokens
           AND reserved_tokens <= max_tokens - committed_tokens),
    CHECK (committed_duration_ms <= max_duration_ms
           AND reserved_duration_ms <= max_duration_ms - committed_duration_ms),
    CHECK (committed_cost_microusd <= max_cost_microusd
           AND reserved_cost_microusd <= max_cost_microusd - committed_cost_microusd),
    CHECK (committed_tool_dispatches <= max_tool_dispatches
           AND reserved_tool_dispatches <= max_tool_dispatches - committed_tool_dispatches)
);

CREATE TABLE operator_run_control (
    run_id TEXT PRIMARY KEY REFERENCES agent_runs(id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    budget_id TEXT NOT NULL REFERENCES operator_budget_accounts(budget_id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-run-control/v1'),
    control_revision INTEGER NOT NULL
        CHECK (control_revision BETWEEN 0 AND 9007199254740991),
    active_dispatch_reservation_id TEXT,
    recovery_directive_id TEXT,
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    last_command_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*'),
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    UNIQUE (budget_id, run_id),
    CHECK ((recovery_directive_id IS NULL AND recovery_directive_digest IS NULL)
           OR (recovery_directive_id IS NOT NULL
               AND recovery_directive_digest IS NOT NULL))
);

CREATE TABLE operator_run_leases (
    lease_id TEXT PRIMARY KEY
        CHECK (length(lease_id) = 36 AND lower(lease_id) = lease_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    owner_instance_id TEXT NOT NULL
        CHECK (length(owner_instance_id) = 36 AND lower(owner_instance_id) = owner_instance_id),
    process_epoch_id TEXT NOT NULL
        CHECK (length(process_epoch_id) = 36 AND lower(process_epoch_id) = process_epoch_id),
    lease_token_digest TEXT NOT NULL
        CHECK (length(lease_token_digest) = 75
               AND lease_token_digest GLOB 'blake3-256:[0-9a-f]*'),
    fence_epoch INTEGER NOT NULL
        CHECK (fence_epoch BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL
        CHECK (revision BETWEEN 0 AND 9007199254740991),
    state TEXT NOT NULL CHECK (state IN ('active', 'released')),
    acquired_at TEXT NOT NULL,
    renewed_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    released_at TEXT,
    lease_json TEXT NOT NULL CHECK (json_valid(lease_json)),
    lease_digest TEXT NOT NULL
        CHECK (length(lease_digest) = 75
               AND lease_digest GLOB 'blake3-256:[0-9a-f]*'),
    UNIQUE (run_id, fence_epoch),
    UNIQUE (run_id, lease_id, fence_epoch),
    CHECK (COALESCE((state = 'active' AND released_at IS NULL)
           OR (state = 'released' AND released_at IS NOT NULL), 0) = 1)
);

CREATE UNIQUE INDEX idx_operator_run_leases_one_active
    ON operator_run_leases(run_id) WHERE state = 'active';
CREATE INDEX idx_operator_run_leases_recovery
    ON operator_run_leases(state, expires_at, run_id);

CREATE TABLE operator_budget_reservations (
    reservation_id TEXT PRIMARY KEY
        CHECK (length(reservation_id) = 36 AND lower(reservation_id) = reservation_id),
    budget_id TEXT NOT NULL REFERENCES operator_budget_accounts(budget_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    lease_id TEXT NOT NULL,
    fence_epoch INTEGER NOT NULL
        CHECK (fence_epoch BETWEEN 1 AND 9007199254740991),
    idempotency_key TEXT NOT NULL
        CHECK (length(idempotency_key) = 36 AND lower(idempotency_key) = idempotency_key),
    request_digest TEXT NOT NULL
        CHECK (length(request_digest) = 75
               AND request_digest GLOB 'blake3-256:[0-9a-f]*'),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-budget-reservation/v1'),
    kind TEXT NOT NULL CHECK (kind IN ('provider', 'tool')),
    intent_digest TEXT NOT NULL
        CHECK (length(intent_digest) = 75
               AND intent_digest GLOB 'blake3-256:[0-9a-f]*'),
    intent_json TEXT NOT NULL CHECK (json_valid(intent_json)),
    replay_binding_digest TEXT
        CHECK (replay_binding_digest IS NULL
               OR (length(replay_binding_digest) = 75
                   AND replay_binding_digest GLOB 'blake3-256:[0-9a-f]*')),
    replay_operation TEXT,
    replay_version TEXT,
    replay_idempotency_key TEXT
        CHECK (replay_idempotency_key IS NULL
               OR (length(replay_idempotency_key) = 36
                   AND lower(replay_idempotency_key) = replay_idempotency_key)),
    replay_input_digest TEXT
        CHECK (replay_input_digest IS NULL OR length(replay_input_digest) = 64),
    replay_claimed_by TEXT REFERENCES principals(id),
    replay_json TEXT CHECK (replay_json IS NULL OR json_valid(replay_json)),
    recovery_directive_id TEXT UNIQUE,
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    recovery_json TEXT CHECK (recovery_json IS NULL OR json_valid(recovery_json)),
    state TEXT NOT NULL
        CHECK (state IN ('reserved', 'dispatching', 'committed', 'released', 'forfeited')),
    reserved_steps INTEGER NOT NULL CHECK (reserved_steps BETWEEN 0 AND 9007199254740991),
    reserved_tokens INTEGER NOT NULL CHECK (reserved_tokens BETWEEN 0 AND 9007199254740991),
    reserved_duration_ms INTEGER NOT NULL
        CHECK (reserved_duration_ms BETWEEN 0 AND 9007199254740991),
    reserved_cost_microusd INTEGER NOT NULL
        CHECK (reserved_cost_microusd BETWEEN 0 AND 9007199254740991),
    reserved_tool_dispatches INTEGER NOT NULL
        CHECK (reserved_tool_dispatches BETWEEN 0 AND 9007199254740991),
    charged_steps INTEGER NOT NULL DEFAULT 0
        CHECK (charged_steps BETWEEN 0 AND 9007199254740991),
    charged_tokens INTEGER NOT NULL DEFAULT 0
        CHECK (charged_tokens BETWEEN 0 AND 9007199254740991),
    charged_duration_ms INTEGER NOT NULL DEFAULT 0
        CHECK (charged_duration_ms BETWEEN 0 AND 9007199254740991),
    charged_cost_microusd INTEGER NOT NULL DEFAULT 0
        CHECK (charged_cost_microusd BETWEEN 0 AND 9007199254740991),
    charged_tool_dispatches INTEGER NOT NULL DEFAULT 0
        CHECK (charged_tool_dispatches BETWEEN 0 AND 9007199254740991),
    created_at TEXT NOT NULL,
    permit_id TEXT UNIQUE
        CHECK (permit_id IS NULL
               OR (length(permit_id) = 36 AND lower(permit_id) = permit_id)),
    dispatch_token_digest TEXT
        CHECK (dispatch_token_digest IS NULL
               OR (length(dispatch_token_digest) = 75
                   AND dispatch_token_digest GLOB 'blake3-256:[0-9a-f]*')),
    call_digest TEXT
        CHECK (call_digest IS NULL
               OR (length(call_digest) = 75
                   AND call_digest GLOB 'blake3-256:[0-9a-f]*')),
    prepared_execution_digest TEXT
        CHECK (prepared_execution_digest IS NULL
               OR (length(prepared_execution_digest) = 75
                   AND prepared_execution_digest GLOB 'blake3-256:[0-9a-f]*')),
    result_digest TEXT
        CHECK (result_digest IS NULL
               OR (length(result_digest) = 75
                   AND result_digest GLOB 'blake3-256:[0-9a-f]*')),
    prepared_binding_json TEXT
        CHECK (prepared_binding_json IS NULL OR json_valid(prepared_binding_json)),
    runtime_commit_json TEXT
        CHECK (runtime_commit_json IS NULL OR json_valid(runtime_commit_json)),
    dispatch_started_at TEXT,
    settled_at TEXT,
    reservation_json TEXT NOT NULL CHECK (json_valid(reservation_json)),
    UNIQUE (budget_id, idempotency_key),
    FOREIGN KEY (budget_id, run_id)
        REFERENCES operator_run_control(budget_id, run_id),
    FOREIGN KEY (run_id, lease_id, fence_epoch)
        REFERENCES operator_run_leases(run_id, lease_id, fence_epoch),
    CHECK (COALESCE((json_extract(intent_json, '$.schema') =
                     'proof.operator.dispatch-intent/v1'
                     AND json_extract(intent_json, '$.kind') = kind), 0) = 1),
    CHECK (COALESCE((replay_binding_digest IS NULL
                     AND replay_operation IS NULL AND replay_version IS NULL
                     AND replay_idempotency_key IS NULL
                     AND replay_input_digest IS NULL
                     AND replay_claimed_by IS NULL AND replay_json IS NULL)
           OR (kind = 'tool' AND replay_binding_digest IS NOT NULL
               AND replay_operation IS NOT NULL AND replay_version IS NOT NULL
               AND replay_idempotency_key IS NOT NULL
               AND replay_input_digest IS NOT NULL
               AND replay_claimed_by IS NOT NULL
               AND replay_json IS NOT NULL
               AND json_extract(replay_json, '$.run_id') = run_id
               AND json_extract(replay_json, '$.operation') = replay_operation
               AND json_extract(replay_json, '$.version') = replay_version
               AND json_extract(replay_json, '$.idempotency_key') = replay_idempotency_key
               AND json_extract(replay_json, '$.input_digest') = replay_input_digest
               AND json_extract(replay_json, '$.claimed_by') = replay_claimed_by), 0) = 1),
    CHECK (COALESCE((recovery_directive_id IS NULL
                     AND recovery_directive_digest IS NULL AND recovery_json IS NULL)
           OR (recovery_directive_id IS NOT NULL
               AND recovery_directive_digest IS NOT NULL AND recovery_json IS NOT NULL
               AND json_extract(recovery_json, '$.directive_id') = recovery_directive_id
               AND json_extract(recovery_json, '$.directive_digest') = recovery_directive_digest
               AND json_extract(recovery_json, '$.run_id') = run_id
               AND json_extract(recovery_json, '$.intent_digest') = intent_digest), 0) = 1),
    CHECK (charged_steps <= reserved_steps
           AND charged_tokens <= reserved_tokens
           AND charged_duration_ms <= reserved_duration_ms
           AND charged_cost_microusd <= reserved_cost_microusd
           AND charged_tool_dispatches <= reserved_tool_dispatches),
    CHECK (COALESCE((state = 'reserved'
            AND permit_id IS NULL AND dispatch_token_digest IS NULL
            AND call_digest IS NULL
            AND prepared_execution_digest IS NULL AND result_digest IS NULL
            AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
            AND dispatch_started_at IS NULL AND settled_at IS NULL
            AND charged_steps = 0 AND charged_tokens = 0
            AND charged_duration_ms = 0 AND charged_cost_microusd = 0
            AND charged_tool_dispatches = 0)
           OR (state = 'dispatching'
               AND permit_id IS NOT NULL AND dispatch_token_digest IS NOT NULL
               AND call_digest IS NOT NULL
               AND prepared_execution_digest IS NULL AND result_digest IS NULL
               AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
               AND dispatch_started_at IS NOT NULL AND settled_at IS NULL
               AND charged_steps = 0 AND charged_tokens = 0
               AND charged_duration_ms = 0 AND charged_cost_microusd = 0
               AND charged_tool_dispatches = 0)
           OR (state = 'committed'
               AND permit_id IS NOT NULL AND dispatch_token_digest IS NOT NULL
               AND call_digest IS NOT NULL
               AND prepared_execution_digest IS NOT NULL AND result_digest IS NOT NULL
               AND prepared_binding_json IS NOT NULL AND runtime_commit_json IS NOT NULL
               AND json_extract(prepared_binding_json, '$.payload_digest') = prepared_execution_digest
               AND json_extract(prepared_binding_json, '$.result_digest') = result_digest
               AND json_extract(runtime_commit_json, '$.prepared_execution_digest') = prepared_execution_digest
               AND json_extract(runtime_commit_json, '$.result_digest') = result_digest
               AND dispatch_started_at IS NOT NULL AND settled_at IS NOT NULL)
           OR (state = 'released'
               AND permit_id IS NULL AND dispatch_token_digest IS NULL
               AND call_digest IS NULL
               AND prepared_execution_digest IS NULL AND result_digest IS NULL
               AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
               AND dispatch_started_at IS NULL AND settled_at IS NOT NULL
               AND charged_steps = 0 AND charged_tokens = 0
               AND charged_duration_ms = 0 AND charged_cost_microusd = 0
               AND charged_tool_dispatches = 0)
           OR (state = 'forfeited'
               AND permit_id IS NOT NULL AND dispatch_token_digest IS NOT NULL
               AND call_digest IS NOT NULL
               AND prepared_execution_digest IS NULL AND result_digest IS NULL
               AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
               AND dispatch_started_at IS NOT NULL AND settled_at IS NOT NULL
               AND charged_steps = reserved_steps
               AND charged_tokens = reserved_tokens
               AND charged_duration_ms = reserved_duration_ms
               AND charged_cost_microusd = reserved_cost_microusd
               AND charged_tool_dispatches = reserved_tool_dispatches), 0) = 1)
);

CREATE INDEX idx_operator_budget_reservations_open
    ON operator_budget_reservations(budget_id, state, created_at, reservation_id);
CREATE UNIQUE INDEX idx_operator_budget_reservations_one_open_run
    ON operator_budget_reservations(run_id)
    WHERE state IN ('reserved', 'dispatching');
CREATE INDEX idx_operator_budget_reservations_run
    ON operator_budget_reservations(run_id, created_at, reservation_id);

CREATE TABLE operator_replay_bindings (
    operation TEXT NOT NULL,
    version TEXT NOT NULL,
    idempotency_key TEXT NOT NULL
        CHECK (length(idempotency_key) = 36
               AND lower(idempotency_key) = idempotency_key),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    step_id TEXT NOT NULL REFERENCES agent_run_steps(id),
    origin_reservation_id TEXT NOT NULL UNIQUE
        REFERENCES operator_budget_reservations(reservation_id),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    input_digest TEXT NOT NULL CHECK (length(input_digest) = 64),
    claimed_by TEXT NOT NULL REFERENCES principals(id),
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*'),
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    created_at TEXT NOT NULL,
    PRIMARY KEY (operation, version, idempotency_key),
    UNIQUE (run_id, step_id),
    FOREIGN KEY (operation, version, idempotency_key)
        REFERENCES execution_replays(operation, version, idempotency_key),
    CHECK (COALESCE(
        json_extract(binding_json, '$.schema') =
            'proof.operator.replay-claim-binding/v1'
        AND json_extract(binding_json, '$.workspace_id') = workspace_id
        AND json_extract(binding_json, '$.run_id') = run_id
        AND json_extract(binding_json, '$.step_id') = step_id
        AND json_extract(binding_json, '$.checkpoint_id') = checkpoint_id
        AND json_extract(binding_json, '$.checkpoint_sequence') = checkpoint_sequence
        AND json_extract(binding_json, '$.checkpoint_digest') = checkpoint_digest
        AND json_extract(binding_json, '$.operation') = operation
        AND json_extract(binding_json, '$.version') = version
        AND json_extract(binding_json, '$.idempotency_key') = idempotency_key
        AND json_extract(binding_json, '$.input_digest') = input_digest
        AND json_extract(binding_json, '$.claimed_by') = claimed_by
        AND json_extract(binding_json, '$.binding_digest') = binding_digest,
        0) = 1)
);

CREATE INDEX idx_operator_replay_bindings_run
    ON operator_replay_bindings(run_id, step_id);

CREATE TABLE operator_recovery_directives (
    directive_id TEXT PRIMARY KEY
        CHECK (length(directive_id) = 36 AND lower(directive_id) = directive_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    source_lease_id TEXT NOT NULL REFERENCES operator_run_leases(lease_id),
    source_reservation_id TEXT NOT NULL REFERENCES operator_budget_reservations(reservation_id),
    source_budget_id TEXT NOT NULL REFERENCES operator_budget_accounts(budget_id),
    source_idempotency_key TEXT NOT NULL
        CHECK (length(source_idempotency_key) = 36
               AND lower(source_idempotency_key) = source_idempotency_key),
    source_request_digest TEXT NOT NULL
        CHECK (length(source_request_digest) = 75
               AND source_request_digest GLOB 'blake3-256:[0-9a-f]*'),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.recovery-directive/v1'),
    classification TEXT NOT NULL
        CHECK (classification = 'pre_dispatch_recoverable'),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    source_fence_epoch INTEGER NOT NULL
        CHECK (source_fence_epoch BETWEEN 1 AND 9007199254740991),
    source_control_revision INTEGER NOT NULL
        CHECK (source_control_revision BETWEEN 0 AND 9007199254740991),
    intent_digest TEXT NOT NULL
        CHECK (length(intent_digest) = 75
               AND intent_digest GLOB 'blake3-256:[0-9a-f]*'),
    replay_binding_digest TEXT
        CHECK (replay_binding_digest IS NULL
               OR (length(replay_binding_digest) = 75
                   AND replay_binding_digest GLOB 'blake3-256:[0-9a-f]*')),
    replay_json TEXT CHECK (replay_json IS NULL OR json_valid(replay_json)),
    required_budget_disposition TEXT NOT NULL
        CHECK (required_budget_disposition = 'none'),
    created_at TEXT NOT NULL,
    directive_json TEXT NOT NULL CHECK (json_valid(directive_json)),
    directive_digest TEXT NOT NULL UNIQUE
        CHECK (length(directive_digest) = 75
               AND directive_digest GLOB 'blake3-256:[0-9a-f]*'),
    UNIQUE (run_id, directive_id),
    FOREIGN KEY (run_id, source_lease_id, source_fence_epoch)
        REFERENCES operator_run_leases(run_id, lease_id, fence_epoch),
    FOREIGN KEY (source_budget_id, source_idempotency_key)
        REFERENCES operator_budget_reservations(budget_id, idempotency_key),
    CHECK (COALESCE((replay_binding_digest IS NULL AND replay_json IS NULL)
           OR (replay_binding_digest IS NOT NULL AND replay_json IS NOT NULL
               AND json_extract(replay_json, '$.binding_digest') = replay_binding_digest), 0) = 1),
    CHECK (COALESCE(json_extract(directive_json, '$.source_reservation_id') = source_reservation_id
           AND json_extract(directive_json, '$.source_budget_id') = source_budget_id
           AND json_extract(directive_json, '$.source_idempotency_key') = source_idempotency_key
           AND json_extract(directive_json, '$.source_request_digest') = source_request_digest
           AND json_extract(directive_json, '$.intent_digest') = intent_digest, 0) = 1),
    CHECK (classification = 'pre_dispatch_recoverable'
           AND required_budget_disposition = 'none')
);

CREATE INDEX idx_operator_recovery_directives_run
    ON operator_recovery_directives(run_id, created_at DESC, directive_id);

CREATE TABLE operator_approval_bindings (
    approval_request_id TEXT PRIMARY KEY REFERENCES approval_requests(id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    step_id TEXT NOT NULL UNIQUE REFERENCES agent_run_steps(id),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    required_human_id TEXT NOT NULL REFERENCES principals(id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-approval-binding/v1'),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    input_digest TEXT NOT NULL CHECK (length(input_digest) = 64),
    argument_digest TEXT NOT NULL
        CHECK (length(argument_digest) = 75
               AND argument_digest GLOB 'blake3-256:[0-9a-f]*'),
    consequence_digest TEXT NOT NULL
        CHECK (length(consequence_digest) = 75
               AND consequence_digest GLOB 'blake3-256:[0-9a-f]*'),
    created_at TEXT NOT NULL,
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*')
);

CREATE INDEX idx_operator_approval_bindings_run
    ON operator_approval_bindings(run_id, approval_request_id);

CREATE TABLE operator_run_projections (
    projection_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    projection_id TEXT NOT NULL UNIQUE
        CHECK (length(projection_id) = 36 AND lower(projection_id) = projection_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-run-projection/v1'),
    projection_revision INTEGER NOT NULL
        CHECK (projection_revision BETWEEN 0 AND 9007199254740991),
    source_run_revision INTEGER NOT NULL
        CHECK (source_run_revision BETWEEN 0 AND 9007199254740991),
    source_control_revision INTEGER NOT NULL
        CHECK (source_control_revision BETWEEN 0 AND 9007199254740991),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    fence_epoch INTEGER NOT NULL
        CHECK (fence_epoch BETWEEN 0 AND 9007199254740991),
    run_status TEXT NOT NULL
        CHECK (run_status IN ('queued', 'running', 'waiting_for_input',
                              'succeeded', 'failed', 'cancelled')),
    attention TEXT NOT NULL
        CHECK (attention IN ('awaiting_decision', 'running', 'recoverable', 'terminal')),
    required_human_id TEXT REFERENCES principals(id),
    approval_request_id TEXT REFERENCES approval_requests(id),
    recovery_directive_id TEXT REFERENCES operator_recovery_directives(directive_id),
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    projected_at TEXT NOT NULL,
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    snapshot_digest TEXT NOT NULL
        CHECK (length(snapshot_digest) = 75
               AND snapshot_digest GLOB 'blake3-256:[0-9a-f]*'),
    UNIQUE (workspace_id, run_id, projection_revision),
    CHECK (COALESCE(
        (attention = 'awaiting_decision'
         AND run_status = 'waiting_for_input'
         AND required_human_id IS NOT NULL AND approval_request_id IS NOT NULL
         AND recovery_directive_id IS NULL AND recovery_directive_digest IS NULL)
        OR (attention = 'recoverable'
            AND run_status = 'failed'
            AND required_human_id IS NULL AND approval_request_id IS NULL
            AND recovery_directive_id IS NOT NULL
            AND recovery_directive_digest IS NOT NULL)
        OR (attention = 'running'
            AND run_status IN ('queued', 'running')
            AND required_human_id IS NULL AND approval_request_id IS NULL
            AND recovery_directive_id IS NULL AND recovery_directive_digest IS NULL)
        OR (attention = 'terminal'
            AND run_status IN ('succeeded', 'failed', 'cancelled')
            AND required_human_id IS NULL AND approval_request_id IS NULL
            AND recovery_directive_id IS NULL AND recovery_directive_digest IS NULL), 0) = 1),
    CHECK (length(checkpoint_digest) = 64)
);

CREATE INDEX idx_operator_run_projections_latest
    ON operator_run_projections(workspace_id, run_id, projection_sequence DESC);
CREATE INDEX idx_operator_run_projections_page
    ON operator_run_projections(workspace_id, attention, projection_sequence DESC, run_id);
CREATE INDEX idx_operator_run_projections_approval
    ON operator_run_projections(approval_request_id, projection_sequence DESC);

CREATE TABLE operator_commands (
    command_id TEXT PRIMARY KEY
        CHECK (length(command_id) = 36 AND lower(command_id) = command_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    idempotency_key TEXT NOT NULL
        CHECK (length(idempotency_key) = 36 AND lower(idempotency_key) = idempotency_key),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.command-envelope/v1'),
    kind TEXT NOT NULL
        CHECK (kind IN ('approval_decide', 'run_cancel', 'run_resume', 'session_revoke')),
    human_id TEXT NOT NULL REFERENCES principals(id),
    server_instance_id TEXT NOT NULL
        CHECK (length(server_instance_id) = 36 AND lower(server_instance_id) = server_instance_id),
    session_id TEXT NOT NULL
        CHECK (length(session_id) = 36 AND lower(session_id) = session_id),
    required_capability TEXT
        CHECK (required_capability IN ('approval.decide', 'run.cancel', 'run.resume')),
    target_run_id TEXT REFERENCES operator_run_control(run_id),
    target_step_id TEXT REFERENCES agent_run_steps(id),
    approval_request_id TEXT REFERENCES approval_requests(id),
    expected_run_revision INTEGER
        CHECK (expected_run_revision BETWEEN 0 AND 9007199254740991),
    expected_step_revision INTEGER
        CHECK (expected_step_revision BETWEEN 0 AND 9007199254740991),
    expected_control_revision INTEGER
        CHECK (expected_control_revision BETWEEN 0 AND 9007199254740991),
    expected_checkpoint_id TEXT REFERENCES agent_checkpoints(id),
    expected_checkpoint_sequence INTEGER
        CHECK (expected_checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    expected_checkpoint_digest TEXT,
    expected_fence_epoch INTEGER
        CHECK (expected_fence_epoch BETWEEN 1 AND 9007199254740991),
    recovery_directive_id TEXT REFERENCES operator_recovery_directives(directive_id),
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    request_digest TEXT NOT NULL UNIQUE
        CHECK (length(request_digest) = 75
               AND request_digest GLOB 'blake3-256:[0-9a-f]*'),
    decision_digest TEXT CHECK (decision_digest IS NULL OR length(decision_digest) = 64),
    requested_at TEXT NOT NULL,
    command_json TEXT NOT NULL CHECK (json_valid(command_json)),
    UNIQUE (workspace_id, human_id, idempotency_key),
    CHECK (COALESCE((kind = 'approval_decide'
            AND required_capability IS NOT NULL
            AND required_capability = 'approval.decide'
            AND target_run_id IS NOT NULL AND target_step_id IS NOT NULL
            AND approval_request_id IS NOT NULL
            AND expected_run_revision IS NOT NULL
            AND expected_step_revision IS NOT NULL
            AND expected_control_revision IS NOT NULL
            AND expected_checkpoint_id IS NOT NULL
            AND expected_checkpoint_sequence IS NOT NULL
            AND expected_checkpoint_digest IS NOT NULL
            AND length(expected_checkpoint_digest) = 64
            AND expected_fence_epoch IS NOT NULL
            AND recovery_directive_id IS NULL
            AND recovery_directive_digest IS NULL
            AND decision_digest IS NULL)
           OR (kind = 'run_cancel'
               AND required_capability IS NOT NULL
               AND required_capability = 'run.cancel'
               AND target_run_id IS NOT NULL
               AND target_step_id IS NULL AND approval_request_id IS NULL
               AND expected_run_revision IS NOT NULL
               AND expected_step_revision IS NULL
               AND expected_control_revision IS NOT NULL
               AND expected_checkpoint_id IS NULL
               AND expected_checkpoint_sequence IS NULL
               AND expected_checkpoint_digest IS NULL
               AND expected_fence_epoch IS NOT NULL
               AND recovery_directive_id IS NULL
               AND recovery_directive_digest IS NULL
               AND decision_digest IS NULL)
           OR (kind = 'run_resume'
               AND required_capability IS NOT NULL
               AND required_capability = 'run.resume'
               AND target_run_id IS NOT NULL AND target_step_id IS NOT NULL
               AND expected_run_revision IS NOT NULL
               AND expected_step_revision IS NOT NULL
               AND expected_control_revision IS NOT NULL
               AND expected_checkpoint_id IS NOT NULL
               AND expected_checkpoint_sequence IS NOT NULL
               AND expected_checkpoint_digest IS NOT NULL
               AND length(expected_checkpoint_digest) = 64
               AND expected_fence_epoch IS NOT NULL
               AND ((approval_request_id IS NOT NULL
                     AND decision_digest IS NOT NULL
                     AND length(decision_digest) = 64
                     AND recovery_directive_id IS NULL
                     AND recovery_directive_digest IS NULL)
                    OR (approval_request_id IS NULL
                        AND decision_digest IS NULL
                        AND recovery_directive_id IS NOT NULL
                        AND recovery_directive_digest IS NOT NULL)))
           OR (kind = 'session_revoke'
               AND required_capability IS NULL
               AND target_run_id IS NULL AND target_step_id IS NULL
               AND approval_request_id IS NULL
               AND expected_run_revision IS NULL
               AND expected_step_revision IS NULL
               AND expected_control_revision IS NULL
               AND expected_checkpoint_id IS NULL
               AND expected_checkpoint_sequence IS NULL
               AND expected_checkpoint_digest IS NULL
               AND expected_fence_epoch IS NULL
               AND recovery_directive_id IS NULL
               AND recovery_directive_digest IS NULL
               AND decision_digest IS NULL), 0) = 1)
);

CREATE INDEX idx_operator_commands_run
    ON operator_commands(target_run_id, requested_at, command_id);
CREATE INDEX idx_operator_commands_approval
    ON operator_commands(approval_request_id, requested_at, command_id);
CREATE INDEX idx_operator_commands_workspace
    ON operator_commands(workspace_id, requested_at, command_id);

CREATE TABLE operator_audit_heads (
    workspace_id TEXT PRIMARY KEY REFERENCES operator_workspaces(workspace_id),
    last_sequence INTEGER NOT NULL
        CHECK (last_sequence BETWEEN 0 AND 9007199254740991),
    last_digest TEXT,
    CHECK (COALESCE((last_sequence = 0 AND last_digest IS NULL)
           OR (last_sequence > 0 AND last_digest IS NOT NULL
               AND length(last_digest) = 75
               AND last_digest GLOB 'blake3-256:[0-9a-f]*'), 0) = 1)
);

CREATE TABLE operator_audit_events (
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    sequence INTEGER NOT NULL
        CHECK (sequence BETWEEN 1 AND 9007199254740991),
    event_id TEXT NOT NULL UNIQUE
        CHECK (length(event_id) = 36 AND lower(event_id) = event_id),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.audit-event/v1'),
    kind TEXT NOT NULL CHECK (kind IN (
        'session_challenge_issued', 'session_issued', 'session_replaced',
        'session_revoked', 'session_expired',
        'approval_decided', 'approval_expired',
        'command_rejected', 'command_conflict',
        'run_cancelled', 'run_resumed',
        'lease_acquired', 'lease_renewed', 'lease_reclaimed', 'lease_released',
        'stale_fence_rejected',
        'budget_reserved', 'budget_committed', 'budget_released',
        'budget_forfeited', 'budget_rejected',
        'dispatch_authorized', 'runtime_result_committed',
        'recovery_started', 'recovery_completed',
        'control_failure', 'control_shutdown'
    )),
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected', 'conflict', 'expired', 'failed')),
    previous_digest TEXT,
    event_digest TEXT NOT NULL UNIQUE
        CHECK (length(event_digest) = 75
               AND event_digest GLOB 'blake3-256:[0-9a-f]*'),
    human_id TEXT REFERENCES principals(id),
    session_id TEXT,
    challenge_id TEXT,
    challenge_digest TEXT
        CHECK (challenge_digest IS NULL
               OR (length(challenge_digest) = 75
                   AND challenge_digest GLOB 'blake3-256:[0-9a-f]*')),
    session_authority_digest TEXT
        CHECK (session_authority_digest IS NULL
               OR (length(session_authority_digest) = 75
                   AND session_authority_digest GLOB 'blake3-256:[0-9a-f]*')),
    related_session_id TEXT,
    server_instance_id TEXT,
    run_id TEXT REFERENCES operator_run_control(run_id),
    approval_request_id TEXT REFERENCES approval_requests(id),
    command_id TEXT,
    command_kind TEXT
        CHECK (command_kind IS NULL OR command_kind IN (
            'approval_decide', 'run_cancel', 'run_resume', 'session_revoke')),
    budget_id TEXT REFERENCES operator_budget_accounts(budget_id),
    reservation_id TEXT,
    lease_id TEXT REFERENCES operator_run_leases(lease_id),
    source_lease_id TEXT REFERENCES operator_run_leases(lease_id),
    process_epoch_id TEXT,
    permit_id TEXT,
    recovery_directive_id TEXT REFERENCES operator_recovery_directives(directive_id),
    fence_epoch INTEGER
        CHECK (fence_epoch BETWEEN 0 AND 9007199254740991),
    auth_epoch INTEGER
        CHECK (auth_epoch BETWEEN 1 AND 9007199254740991),
    policy_revision INTEGER
        CHECK (policy_revision BETWEEN 1 AND 9007199254740991),
    intent_digest TEXT
        CHECK (intent_digest IS NULL
               OR (length(intent_digest) = 75
                   AND intent_digest GLOB 'blake3-256:[0-9a-f]*')),
    call_digest TEXT
        CHECK (call_digest IS NULL
               OR (length(call_digest) = 75
                   AND call_digest GLOB 'blake3-256:[0-9a-f]*')),
    decision_digest TEXT
        CHECK (decision_digest IS NULL OR length(decision_digest) = 64),
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    failure_scope TEXT
        CHECK (failure_scope IS NULL
               OR failure_scope IN ('command', 'runtime', 'storage', 'workspace')),
    proof_id TEXT REFERENCES proofs(id),
    proof_operation TEXT,
    proof_digest TEXT
        CHECK (proof_digest IS NULL OR length(proof_digest) = 64),
    occurred_at TEXT NOT NULL,
    event_json TEXT NOT NULL CHECK (json_valid(event_json)),
    PRIMARY KEY (workspace_id, sequence),
    CHECK (COALESCE((sequence = 1 AND previous_digest IS NULL)
           OR (sequence > 1 AND previous_digest IS NOT NULL
               AND length(previous_digest) = 75
               AND previous_digest GLOB 'blake3-256:[0-9a-f]*'), 0) = 1),
    CHECK ((proof_id IS NULL AND proof_operation IS NULL AND proof_digest IS NULL)
           OR (proof_id IS NOT NULL AND proof_operation IS NOT NULL
               AND proof_digest IS NOT NULL)),
    CHECK (((kind IN ('session_challenge_issued', 'session_issued',
                     'session_replaced')) AND challenge_digest IS NOT NULL)
           OR ((kind NOT IN ('session_challenge_issued', 'session_issued',
                            'session_replaced')) AND challenge_digest IS NULL)),
    CHECK ((session_id IS NULL AND session_authority_digest IS NULL)
           OR (session_id IS NOT NULL AND session_authority_digest IS NOT NULL)),
    CHECK (session_id IS NULL
           OR (length(session_id) = 36 AND lower(session_id) = session_id)),
    CHECK (server_instance_id IS NULL
           OR (length(server_instance_id) = 36
               AND lower(server_instance_id) = server_instance_id)),
    CHECK (challenge_id IS NULL
           OR (length(challenge_id) = 36 AND lower(challenge_id) = challenge_id)),
    CHECK (related_session_id IS NULL
           OR (length(related_session_id) = 36
               AND lower(related_session_id) = related_session_id)),
    CHECK (command_id IS NULL
           OR (length(command_id) = 36 AND lower(command_id) = command_id)),
    CHECK (reservation_id IS NULL
           OR (length(reservation_id) = 36
               AND lower(reservation_id) = reservation_id)),
    CHECK (process_epoch_id IS NULL
           OR (length(process_epoch_id) = 36
               AND lower(process_epoch_id) = process_epoch_id)),
    CHECK (permit_id IS NULL
           OR (length(permit_id) = 36 AND lower(permit_id) = permit_id)),
    CHECK (COALESCE(
        json_extract(event_json, '$.schema') = schema
        AND json_extract(event_json, '$.workspace_id') = workspace_id
        AND json_extract(event_json, '$.event_id') = event_id
        AND json_extract(event_json, '$.sequence') = sequence
        AND json_extract(event_json, '$.kind') = kind
        AND json_extract(event_json, '$.outcome') = outcome
        AND json_extract(event_json, '$.previous_digest') IS previous_digest
        AND json_extract(event_json, '$.event_digest') = event_digest
        AND json_extract(event_json, '$.human_id') IS human_id
        AND json_extract(event_json, '$.session_id') IS session_id
        AND json_extract(event_json, '$.challenge_id') IS challenge_id
        AND json_extract(event_json, '$.challenge_digest') IS challenge_digest
        AND json_extract(event_json, '$.session_authority_digest') IS session_authority_digest
        AND json_extract(event_json, '$.related_session_id') IS related_session_id
        AND json_extract(event_json, '$.server_instance_id') IS server_instance_id
        AND json_extract(event_json, '$.run_id') IS run_id
        AND json_extract(event_json, '$.approval_request_id') IS approval_request_id
        AND json_extract(event_json, '$.command_id') IS command_id
        AND json_extract(event_json, '$.command_kind') IS command_kind
        AND json_extract(event_json, '$.budget_id') IS budget_id
        AND json_extract(event_json, '$.reservation_id') IS reservation_id
        AND json_extract(event_json, '$.lease_id') IS lease_id
        AND json_extract(event_json, '$.source_lease_id') IS source_lease_id
        AND json_extract(event_json, '$.process_epoch_id') IS process_epoch_id
        AND json_extract(event_json, '$.permit_id') IS permit_id
        AND json_extract(event_json, '$.recovery_directive_id') IS recovery_directive_id
        AND json_extract(event_json, '$.fence_epoch') IS fence_epoch
        AND json_extract(event_json, '$.auth_epoch') IS auth_epoch
        AND json_extract(event_json, '$.policy_revision') IS policy_revision
        AND json_extract(event_json, '$.intent_digest') IS intent_digest
        AND json_extract(event_json, '$.call_digest') IS call_digest
        AND json_extract(event_json, '$.decision_digest') IS decision_digest
        AND json_extract(event_json, '$.recovery_directive_digest') IS recovery_directive_digest
        AND json_extract(event_json, '$.failure_scope') IS failure_scope
        AND json_extract(event_json, '$.proof.proof_id') IS proof_id
        AND json_extract(event_json, '$.proof.operation') IS proof_operation
        AND json_extract(event_json, '$.proof.proof_digest') IS proof_digest
        AND json_extract(event_json, '$.occurred_at') = occurred_at,
        0) = 1)
);

CREATE INDEX idx_operator_audit_run
    ON operator_audit_events(workspace_id, run_id, sequence DESC);
CREATE INDEX idx_operator_audit_approval
    ON operator_audit_events(workspace_id, approval_request_id, sequence DESC);
CREATE INDEX idx_operator_audit_human
    ON operator_audit_events(workspace_id, human_id, sequence DESC);
CREATE INDEX idx_operator_audit_kind
    ON operator_audit_events(workspace_id, kind, sequence DESC);

CREATE TABLE operator_command_receipts (
    receipt_id TEXT PRIMARY KEY
        CHECK (length(receipt_id) = 36 AND lower(receipt_id) = receipt_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    command_id TEXT NOT NULL UNIQUE REFERENCES operator_commands(command_id),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.command-receipt/v1'),
    outcome TEXT NOT NULL
        CHECK (outcome IN ('applied', 'already_terminal')),
    observed_run_revision INTEGER
        CHECK (observed_run_revision BETWEEN 0 AND 9007199254740991),
    resulting_run_revision INTEGER
        CHECK (resulting_run_revision BETWEEN 0 AND 9007199254740991),
    resulting_step_revision INTEGER
        CHECK (resulting_step_revision BETWEEN 0 AND 9007199254740991),
    resulting_control_revision INTEGER
        CHECK (resulting_control_revision BETWEEN 0 AND 9007199254740991),
    resulting_fence_epoch INTEGER
        CHECK (resulting_fence_epoch BETWEEN 0 AND 9007199254740991),
    decision_id TEXT REFERENCES approval_decisions(id),
    decision_digest TEXT
        CHECK (decision_digest IS NULL OR length(decision_digest) = 64),
    proof_id TEXT REFERENCES proofs(id),
    proof_digest TEXT
        CHECK (proof_digest IS NULL OR length(proof_digest) = 64),
    audit_sequence INTEGER NOT NULL,
    completed_at TEXT NOT NULL,
    receipt_json TEXT NOT NULL CHECK (json_valid(receipt_json)),
    receipt_digest TEXT NOT NULL UNIQUE
        CHECK (length(receipt_digest) = 75
               AND receipt_digest GLOB 'blake3-256:[0-9a-f]*'),
    FOREIGN KEY (workspace_id, audit_sequence)
        REFERENCES operator_audit_events(workspace_id, sequence),
    CHECK ((decision_id IS NULL AND decision_digest IS NULL)
           OR (decision_id IS NOT NULL AND decision_digest IS NOT NULL)),
    CHECK (COALESCE((outcome = 'applied'
                     AND proof_id IS NOT NULL AND proof_digest IS NOT NULL)
           OR (outcome = 'already_terminal'
               AND proof_id IS NULL AND proof_digest IS NULL), 0) = 1)
);

CREATE INDEX idx_operator_command_receipts_page
    ON operator_command_receipts(workspace_id, audit_sequence DESC, receipt_id);

CREATE INDEX idx_agent_runs_operator_attention
    ON agent_runs(status, updated_at DESC, id);

CREATE TRIGGER operator_workspaces_no_update
BEFORE UPDATE ON operator_workspaces
BEGIN SELECT RAISE(ABORT, 'operator workspace identity is immutable'); END;
CREATE TRIGGER operator_workspaces_no_delete
BEFORE DELETE ON operator_workspaces
BEGIN SELECT RAISE(ABORT, 'operator workspace identity is immutable'); END;

CREATE TRIGGER operator_human_enrollments_no_update
BEFORE UPDATE ON operator_human_enrollments
BEGIN SELECT RAISE(ABORT, 'operator Human enrollment is immutable'); END;
CREATE TRIGGER operator_human_enrollments_no_delete
BEFORE DELETE ON operator_human_enrollments
BEGIN SELECT RAISE(ABORT, 'operator Human enrollment is immutable'); END;

CREATE TRIGGER operator_approval_bindings_no_update
BEFORE UPDATE ON operator_approval_bindings
BEGIN SELECT RAISE(ABORT, 'operator approval binding is immutable'); END;
CREATE TRIGGER operator_approval_bindings_no_delete
BEFORE DELETE ON operator_approval_bindings
BEGIN SELECT RAISE(ABORT, 'operator approval binding is immutable'); END;

CREATE TRIGGER operator_recovery_directives_no_update
BEFORE UPDATE ON operator_recovery_directives
BEGIN SELECT RAISE(ABORT, 'operator recovery directive is immutable'); END;
CREATE TRIGGER operator_recovery_directives_no_delete
BEFORE DELETE ON operator_recovery_directives
BEGIN SELECT RAISE(ABORT, 'operator recovery directive is immutable'); END;

CREATE TRIGGER operator_replay_bindings_no_update
BEFORE UPDATE ON operator_replay_bindings
BEGIN SELECT RAISE(ABORT, 'operator replay binding is immutable'); END;
CREATE TRIGGER operator_replay_bindings_no_delete
BEFORE DELETE ON operator_replay_bindings
BEGIN SELECT RAISE(ABORT, 'operator replay binding is immutable'); END;

CREATE TRIGGER operator_run_projections_no_update
BEFORE UPDATE ON operator_run_projections
BEGIN SELECT RAISE(ABORT, 'operator run projection is append-only'); END;
CREATE TRIGGER operator_run_projections_no_delete
BEFORE DELETE ON operator_run_projections
BEGIN SELECT RAISE(ABORT, 'operator run projection is append-only'); END;

CREATE TRIGGER operator_commands_no_update
BEFORE UPDATE ON operator_commands
BEGIN SELECT RAISE(ABORT, 'operator command is immutable'); END;
CREATE TRIGGER operator_commands_no_delete
BEFORE DELETE ON operator_commands
BEGIN SELECT RAISE(ABORT, 'operator command is immutable'); END;

CREATE TRIGGER operator_audit_events_no_update
BEFORE UPDATE ON operator_audit_events
BEGIN SELECT RAISE(ABORT, 'operator audit event is append-only'); END;
CREATE TRIGGER operator_audit_events_no_delete
BEFORE DELETE ON operator_audit_events
BEGIN SELECT RAISE(ABORT, 'operator audit event is append-only'); END;

CREATE TRIGGER operator_command_receipts_no_update
BEFORE UPDATE ON operator_command_receipts
BEGIN SELECT RAISE(ABORT, 'operator command receipt is immutable'); END;
CREATE TRIGGER operator_command_receipts_no_delete
BEFORE DELETE ON operator_command_receipts
BEGIN SELECT RAISE(ABORT, 'operator command receipt is immutable'); END;

CREATE TRIGGER operator_linked_proofs_no_update
BEFORE UPDATE ON proofs
WHEN EXISTS (
    SELECT 1 FROM operator_command_receipts WHERE proof_id = OLD.id
    UNION ALL
    SELECT 1 FROM operator_audit_events WHERE proof_id = OLD.id
)
BEGIN SELECT RAISE(ABORT, 'operator-linked proof is immutable'); END;
CREATE TRIGGER operator_linked_proofs_no_delete
BEFORE DELETE ON proofs
WHEN EXISTS (
    SELECT 1 FROM operator_command_receipts WHERE proof_id = OLD.id
    UNION ALL
    SELECT 1 FROM operator_audit_events WHERE proof_id = OLD.id
)
BEGIN SELECT RAISE(ABORT, 'operator-linked proof is immutable'); END;
~~~

Strict Rust constructors duplicate UUIDv7, exact digest alphabet, RFC 3339,
canonical JSON, enum, cross-row principal-kind, monotonic revision, and
transaction invariants that SQLite CHECK cannot fully express. In particular,
the GLOB checks above are length guards plus alphabet prefilters, not a
substitute for the strict digest parser.

Migration 14 creates exactly fourteen `operator_*` tables and leaves every
new table empty. `initialize_operator_workspace` inserts only the singleton
identity, one Human enrollment, one budget account, and audit head zero in one
BEGIN IMMEDIATE transaction. `register_governed_run` is a separate atomic
operation: it validates an existing fresh AgentRun and the supplied strict
InitialRunProjectionInput, including a required existing initial checkpoint
ID/sequence/digest. It derives projection UUID/sequence/time/digest, trusted
store time, and the complete initial RunControl
(revision zero, no active dispatch/recovery/last command), computes its binding
digest, and inserts control plus projection together. Registration accepts
only Queued or Running runs with no approval request or recovery directive;
an initial ApprovalBinding is forbidden. A later runtime commit that creates a
SignedApprovalRequest inserts its exact ApprovalBinding atomically with that
request. Each operation is
exact-idempotent or a conflict. V1 workspace identity, selected Human,
capabilities, and budget limits are immutable after provisioning; rotation or
policy change requires a new disposable workspace. auth_epoch and
policy_revision are therefore initialized to one and still remain explicit
bindings for forward compatibility and rejection tests.

The exact down SQL is schema rollback only:

~~~sql
DROP TRIGGER IF EXISTS operator_linked_proofs_no_delete;
DROP TRIGGER IF EXISTS operator_linked_proofs_no_update;
DROP TRIGGER IF EXISTS operator_command_receipts_no_delete;
DROP TRIGGER IF EXISTS operator_command_receipts_no_update;
DROP TRIGGER IF EXISTS operator_audit_events_no_delete;
DROP TRIGGER IF EXISTS operator_audit_events_no_update;
DROP TRIGGER IF EXISTS operator_commands_no_delete;
DROP TRIGGER IF EXISTS operator_commands_no_update;
DROP TRIGGER IF EXISTS operator_run_projections_no_delete;
DROP TRIGGER IF EXISTS operator_run_projections_no_update;
DROP TRIGGER IF EXISTS operator_approval_bindings_no_delete;
DROP TRIGGER IF EXISTS operator_approval_bindings_no_update;
DROP TRIGGER IF EXISTS operator_recovery_directives_no_delete;
DROP TRIGGER IF EXISTS operator_recovery_directives_no_update;
DROP TRIGGER IF EXISTS operator_replay_bindings_no_delete;
DROP TRIGGER IF EXISTS operator_replay_bindings_no_update;
DROP TRIGGER IF EXISTS operator_human_enrollments_no_delete;
DROP TRIGGER IF EXISTS operator_human_enrollments_no_update;
DROP TRIGGER IF EXISTS operator_workspaces_no_delete;
DROP TRIGGER IF EXISTS operator_workspaces_no_update;

DROP INDEX IF EXISTS idx_agent_runs_operator_attention;
DROP INDEX IF EXISTS idx_operator_command_receipts_page;
DROP INDEX IF EXISTS idx_operator_audit_kind;
DROP INDEX IF EXISTS idx_operator_audit_human;
DROP INDEX IF EXISTS idx_operator_audit_approval;
DROP INDEX IF EXISTS idx_operator_audit_run;
DROP INDEX IF EXISTS idx_operator_commands_workspace;
DROP INDEX IF EXISTS idx_operator_commands_approval;
DROP INDEX IF EXISTS idx_operator_commands_run;
DROP INDEX IF EXISTS idx_operator_run_projections_approval;
DROP INDEX IF EXISTS idx_operator_run_projections_page;
DROP INDEX IF EXISTS idx_operator_run_projections_latest;
DROP INDEX IF EXISTS idx_operator_approval_bindings_run;
DROP INDEX IF EXISTS idx_operator_recovery_directives_run;
DROP INDEX IF EXISTS idx_operator_replay_bindings_run;
DROP INDEX IF EXISTS idx_operator_budget_reservations_one_open_run;
DROP INDEX IF EXISTS idx_operator_budget_reservations_run;
DROP INDEX IF EXISTS idx_operator_budget_reservations_open;
DROP INDEX IF EXISTS idx_operator_run_leases_recovery;
DROP INDEX IF EXISTS idx_operator_run_leases_one_active;

DROP TABLE IF EXISTS operator_command_receipts;
DROP TABLE IF EXISTS operator_audit_events;
DROP TABLE IF EXISTS operator_audit_heads;
DROP TABLE IF EXISTS operator_commands;
DROP TABLE IF EXISTS operator_run_projections;
DROP TABLE IF EXISTS operator_approval_bindings;
DROP TABLE IF EXISTS operator_recovery_directives;
DROP TABLE IF EXISTS operator_replay_bindings;
DROP TABLE IF EXISTS operator_budget_reservations;
DROP TABLE IF EXISTS operator_run_leases;
DROP TABLE IF EXISTS operator_run_control;
DROP TABLE IF EXISTS operator_budget_accounts;
DROP TABLE IF EXISTS operator_human_enrollments;
DROP TABLE IF EXISTS operator_workspaces;
~~~

Migration-14 down is not a released product API. The existing public
`rollback_to` MUST reject every request whose current schema is at least 14 and
target is below 14, and no `rollback_operator_schema14_offline` function,
authorization DTO, or receipt verifier is added. The down SQL above is retained
only for disposable migration round-trip tests and a separately owner-directed
manual emergency procedure under repository change control. Such a procedure
requires a stopped control plane, successful acquisition of the exact section
4.1 workspace lock, an independently exported and digest-recorded evidence
snapshot plus database backup, and a new dated owner decision naming both
digests. It runs one `Exclusive` transaction containing exactly this down SQL
and migration-row deletion, then closes the database before releasing the
lock. It removes only E0002 tables/indexes and cannot undo applied
run/approval/proof effects or restore an old credential, general router,
alternate database, or unsafe opener. E0002 implementation and release tests
exercise the down SQL only on disposable databases; they do not simulate owner
authority or expose a generic crossing path.

## 15. Atomic store API and dependency direction

Adding fields or methods to existing shared structs/stores would create
partial, non-atomic implementations. E0002 therefore adds separate strict
kernel types and object-safe traits:

~~~text
OperatorSchemaCatalog (immutable Arc value)
  from_source_inventory(OperatorSchemaSourceInventory) ->
      Result<Self, OperatorCatalogError>
  binding() -> &SchemaCatalogBinding
  digest() -> ControlDigest
  validate_input(operation, version, value) -> Result<(), OperatorCatalogError>
  validate_output(operation, version, value) -> Result<(), OperatorCatalogError>

OperatorControlEnvironment
  trusted_utc_now() -> Result<DateTime<Utc>, OperatorEnvironmentError>
  monotonic_millis() -> Result<u64, OperatorEnvironmentError>
  fill_random(purpose: OperatorRandomPurpose, output: &mut [u8])
      -> Result<(), OperatorEnvironmentError>
  new_uuid_v7() -> Result<Uuid, OperatorEnvironmentError>

OperatorDirectoryStore
  load_operator_workspace() -> OperatorWorkspace
  register_governed_run(RegisterGovernedRunRequest) -> RegisterGovernedRunResult

OperatorAuthorityAuditStore
  append_authority_event(ControlAuditAppendRequest) -> ControlAuditAppendResult

OperatorCursorCodec
  open_page(OperatorReadScope, cursor: Option<&str>, page_size) -> VerifiedPageWindow
  seal_page(OperatorReadScope, page_size, high_water_sequence,
            last_sequence, last_id) -> String

OperatorReadStore
  page_attention(query, OperatorReadScope, &dyn OperatorCursorCodec) -> AttentionPage
  load_run_detail(run_id, OperatorReadScope) -> Option<RunDetail>
  page_approvals(query, OperatorReadScope, &dyn OperatorCursorCodec) -> ApprovalPage
  load_approval_detail(request_id, OperatorReadScope) -> Option<ApprovalDetail>
  page_commands(query, OperatorReadScope, &dyn OperatorCursorCodec) -> CommandPage
  load_command_receipt(command_id, OperatorReadScope) -> Option<CommandReceipt>
  page_operator_audit(query, OperatorReadScope, &dyn OperatorCursorCodec) -> AuditPage

OperatorCommandStore
  execute_operator_command(CommandExecutionRequest, &dyn OperatorSigner) -> CommandResult

OperatorRuntimeStore
  load_completed_replay(ReplayLookupRequest) -> ReplayLookupResult
  claim_run_lease(LeaseClaimRequest) -> LeaseMutationResult
  renew_run_lease(LeaseRenewRequest) -> LeaseMutationResult
  release_run_lease(LeaseReleaseRequest) -> LeaseMutationResult
  reserve_aggregate_budget(BudgetReserveRequest) -> BudgetReserveResult
  settle_budget_reservation(BudgetSettlementRequest) -> BudgetSettlementResult
  begin_dispatch(BeginDispatchRequest) -> DispatchResult
  commit_runtime_barrier(RuntimeCommitRequest, PreparedGovernedExecution) -> RuntimeCommitResult
  settle_runtime_failure(RuntimeFailureRequest) -> RuntimeFailureResult
  reclaim_run(ReclaimRequest) -> ReclaimResult

OperatorControlStore:
  OperatorDirectoryStore + OperatorAuthorityAuditStore + OperatorReadStore +
  OperatorCommandStore + OperatorRuntimeStore
~~~

Every named boundary shape is frozen in `store-v1.schema.json`; public and
durable JSON use the other strict schemas. PreparedGovernedExecution itself is
a closed kernel Rust struct because it contains existing kernel types rather
than a public JSON body; its complete canonical serialization is bound by the
strict PreparedExecutionBinding. Every fallible trait method returns
`Result<the shown closed type, OperatorStoreError>`; every mutating storage
method is one BEGIN IMMEDIATE transaction and never returns strings. Guarded
initialization is deliberately the storage-owned inherent lifecycle in section
4.2, not an unguarded kernel trait method. The kernel owns a
RecordingOperatorControlStore test helper.
Existing trait implementors remain compatible because these are new traits,
not required methods on ExecutionStore, ApprovalStore, or AgentRunStore.

OperatorControlEnvironment is the sole E0002 clock, entropy, and generated-ID
seam. OperatorEnvironmentError is a closed kernel error with only
`clock_unavailable` and `entropy_unavailable`; callers map either to
control_unavailable and terminate before publishing new authority. Its
kernel-owned OperatorRandomPurpose is closed to challenge_nonce,
session_token, cursor_key, lease_token, dispatch_token, and uuid_entropy;
unknown purposes are impossible. `monotonic_millis` is a process-private
nonserializable tick used only for live deadlines and must never be compared
across processes. Production E0002 assembly supplies the control-owned final
OS implementation backed by SystemTime, Instant, and OsRng; all methods fail
closed on clock/entropy error. Conformance supplies a deterministic fake with
fixed UTC, an explicitly advanced monotonic tick, and a seeded byte stream.
The operator schema-14 SqliteStore opener requires
`Arc<dyn OperatorControlEnvironment>` and derives every store-owned timestamp
and UUIDv7 internally. Auth and runtime receive the same environment instance;
no mutation request may add a transition timestamp, generated event/receipt/
permit/directive ID, or random bytes beyond the explicitly client-generated
IDs and raw custody inputs frozen in its schema. The environment is process
authority, is never serialized, and is not exposed through HTTP or another
trait object supplied by an untrusted request.

`append_authority_event` accepts only challenge-issued, session-issued,
session-replaced, session-expired, and control-shutdown intents. Storage owns
trusted time, UUIDv7 event ID, sequence, previous digest, full strict
AuditEvent construction, and the one BEGIN IMMEDIATE audit-head append. The
control authority holds its exclusive in-memory lease throughout: it prepares
but does not publish the volatile transition, appends the event, then publishes
the challenge/session transition before releasing the lease. Expiry marks the
session unusable under that lease, appends, then zeroizes/removes it. If append
fails, no new challenge/session/token is published; existing volatile
authority is cleared and the control process terminates fail-closed. A crash
after a successful append but before publication may leave an accepted audit
event for an unusable volatile token, which is safe and is the frozen lost-
response semantic. Explicit SessionRevokeRequest is instead appended inside
the command transaction as `session_revoked` with its transition Proof.

OperatorReadScope is kernel-owned and is constructed by HTTP only after
authentication/capability checks while it holds the authority lease. It binds
workspace, server instance, session, Human, auth/policy epochs, session
absolute expiry, exact route, the canonical query-without-cursor filter
digest (null only for detail routes), sorted granted capabilities, and the
sorted capabilities required by that exact route. The store recomputes the
filter digest from the strict decoded page query, rejects a required
capability not present in granted, any noncanonical ordering/duplicate,
route/query mismatch, and any workspace/epoch mismatch before querying.

OperatorCursorCodec is a kernel-owned object-safe trait implemented only by
the control/auth process that owns the cursor key and trusted clock. For every
page read, storage MUST call `open_page` before its first database lookup. No
cursor returns the `first` VerifiedPageWindow with all positions null; a
cursor is constant-work authenticated, decoded, expiry-checked, and matched to
the complete OperatorReadScope plus page size before returning a
`continuation` window. Storage opens one read transaction, captures the
route's high-water sequence for a first window or uses the verified one for a
continuation, and applies the returned last-sequence/last-ID boundary. If
another page exists it calls `seal_page` with that exact high water and final
emitted key before ending the authority lease; the codec caps expiry at both
300 seconds and `session_absolute_expires_at`. Codec rejection maps only to
`cursor_stale`; missing/unusable key or clock maps to `control_unavailable`.
The codec performs no database access, and storage never receives the key or
imports SessionClaims/auth. Detail reads accept no codec or cursor.

OperatorMutationScope is the equivalent kernel-owned command boundary. HTTP
constructs it under the same authority lease after authentication and fixed
route capability checks. It carries the complete strict
SessionAuthorityBinding, its recomputed authority digest, policy revision,
exact mutation route, and canonical required-capability set. Before target or
idempotency lookup, storage recomputes the authority digest, verifies all scope
fields and capabilities, and requires exact equality with CommandBinding's
workspace/instance/session/Human/auth/policy/session-authority fields and the
command kind. Storage therefore needs no auth import or volatile lookup but
cannot append a session-bound event from an unverified bare digest.

SqliteStore implements the traits. When operator_run_control contains a run,
legacy generic save_agent_run, save_agent_run_step, checkpoint/event append,
approval decision/execution, and other non-idempotent write paths MUST reject
unless invoked inside the exact guarded operator transaction. This prevents a
second transport/runtime writer from bypassing fences, projections, aggregate
budgets, commands, or audit.

The kernel-owned object-safe OperatorSigner has exactly two methods:

~~~text
sign_approval(ApprovalSigningRequest) -> Result<SignedDecisionResult, OperatorSignerError>
sign_operator_proof(OperatorProofSigningRequest) -> Result<Proof, OperatorSignerError>
~~~

OperatorSignerError is the closed kernel enum `identity_mismatch |
key_load_failed | signing_failed | verification_failed`. SqliteStore maps
every variant to its single internal `signer_failed` outcome, rolls back, and
exposes only `control_unavailable`; a callback never uses panic for an expected
failure.

SqliteStore constructs either request only after revalidation and immutable
command insertion inside its transaction, invokes the callback while the
control layer still holds the session authority lease, verifies the result,
and either commits all rows or rolls back all rows. The implementation owns the
already-selected Human/Agent descriptors; neither method exposes key bytes,
selects a principal, accesses storage, or signs caller-chosen bytes. The
session challenge signer is a distinct ChallengeSigner used only by the TTY
ceremony.

The dependency graph is acyclic:

~~~text
proof-kernel
  ↑       ↑       ↑
auth   storage  runtime
  ↑       ↑       ↑
  +------ HTTP ---+
           ↑
  proof-operator-control
           ↑
       conformance
~~~

- proof-storage implements kernel traits and imports no auth/runtime/control;
- proof-agent-runtime continues to depend on kernel/content, accepts
  Arc<dyn OperatorRuntimeStore>, and imports no storage/control;
- proof-operator-auth depends on kernel and imports no storage/HTTP/control;
- proof-transport-http may depend on auth and kernel, exports a constructor
  taking trait objects, and never imports proof-operator-control;
- W4 proof-operator-control depends only on auth/kernel plus synthetic
  StoreOpener, RouterFactory, StaticBundle, Clock, and signer interfaces;
- E0002-14 composes real backend traits only in conformance; and
- E0002-15 later adds the concrete HTTP/storage/runtime/static composition to
  proof-operator-control. No earlier conformance claim is a runnable product.

## 16. Exact dependency and manifest policy

Gate B authorizes no dependency fetch or source work by itself. If approved,
the implementation owners apply only these manifest changes in their assigned
waves.

### 16.1 W3 auth change

Root Cargo.toml adds workspace member crates/proof-operator-auth and:

~~~toml
sha2 = "=0.10.9"
subtle = "=2.6.1"
zeroize = "=1.9.0"
~~~

crates/proof-operator-auth/Cargo.toml uses:

~~~toml
[package]
name = "proof-operator-auth"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
proof-kernel = { path = "../proof-kernel" }
base64.workspace = true
blake3.workspace = true
chrono.workspace = true
ed25519-dalek.workspace = true
rand.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
subtle.workspace = true
thiserror.workspace = true
uuid = { workspace = true, features = ["serde"] }
zeroize.workspace = true
~~~

These exact package versions already occur in Cargo.lock. No password,
session, web-auth, JWT, cookie, OpenSSL, keyring, or network dependency is
allowed. E0002-08 alone owns the W3 root manifest and lock reconciliation.
The serialized W3 manifest/lock barrier is exact:

1. Before product source fan-out, E0002-08 writes only the root member/
   dependency entries, complete new auth package manifest, and the exact inert
   target `crates/proof-operator-auth/src/lib.rs` containing only
   `#![forbid(unsafe_code)]` plus the module doc `Gate-B dependency scaffold.`;
   it runs no Cargo command, then signals `W3 root ready`. This target defines
   no item or behavior and is the sole pre-lock source-scaffold exception.
2. E0002-05 waits for that signal, adds exactly `sha2.workspace = true`,
   `subtle.workspace = true`, and `zeroize.workspace = true` to
   `crates/proof-kernel/Cargo.toml`, runs no Cargo command or source edit,
   freezes that package manifest, and signals `W3 kernel manifest frozen`.
   These back the kernel-owned raw artifact SHA-256 helper, exact 32-byte
   constant-work comparison helper, DispatchTokenCustody, and other raw
   authority buffers. Storage and conformance use those helpers rather than
   adding independent crypto seams.
3. E0002-08 waits for the kernel signal and alone reconciles Cargo.lock with:

       rtk cargo check -p proof-operator-auth -p proof-kernel --offline

4. E0002-08 signals `W3 lock stable`; only then may any W3 owner begin source
   edits or run another Cargo command. A later W3 dependency change stops all
   W3 writers, restores source quiescence, and repeats this barrier. E0002-05
   never edits Cargo.lock or the root manifest.

### 16.2 W4 control change

Root Cargo.toml later adds workspace member crates/proof-operator-control and:

~~~toml
rustix = { version = "=1.1.4", features = ["fs", "process", "termios"] }
~~~

The W4 shell depends on proof-kernel, proof-operator-auth, axum, base64,
blake3, chrono, ed25519-dalek, tokio, serde, serde_json, thiserror, rand,
rustix, sha2, subtle, uuid (with serde), and zeroize through workspace entries.
Those direct dependencies implement its concrete environment, descriptor key
decode/signing, cursor MAC, and static integrity seams; none is satisfied by a
transitive import. `sha2` is already a W3 workspace dependency. The shell does
not yet import storage/runtime/HTTP source. rustix 1.1.4 is
already locked; fs supplies descriptor traversal, process supplies the
current-effective-user identity check, and termios supplies the non-echo
controlling-TTY guard. It invokes no stty or other child.

The storage-owned offline migration-14 upgrader adds exactly
`rustix.workspace = true` to `crates/proof-storage/Cargo.toml` and uses
`rustix::fs::flock` with the exact lock operation and descriptor lifetime in
section 4.1 before opening SQLite. It does not accept a forgeable boolean/path
assertion or depend on the control crate, and it adds no root or lockfile
delta.

E0002-11 depends on both the completed E0002-05 kernel API and E0002-08 auth
API and owns the W4 root/member/lock change. The serialized W4 manifest/lock
barrier is exact:

1. Before product source fan-out, E0002-11 writes only the root rustix/member
   entries, complete new control package manifest, and the exact inert target
   `crates/proof-operator-control/src/lib.rs` containing only
   `#![forbid(unsafe_code)]` plus the module doc `Gate-B dependency scaffold.`;
   it runs no Cargo command, then signals `W4 root ready`. This target defines
   no item or behavior and is the sole pre-lock source-scaffold exception.
2. E0002-06 waits for that signal, adds only the storage rustix dependency,
   runs no Cargo command or source edit, freezes its package manifest, and signals
   `W4 storage manifest frozen`.
3. E0002-11 waits for the storage signal and alone reconciles Cargo.lock with:

       rtk cargo check -p proof-operator-control -p proof-storage --offline

4. E0002-11 signals `W4 lock stable`; only then may E0002-06, E0002-07, or
   E0002-11 begin source edits or run another Cargo command. A later W4
   dependency change stops all writers, restores source quiescence, and
   repeats this barrier.

E0002-15
later adds path dependencies on proof-storage, proof-agent-runtime, and
proof-transport-http to the control package and alone reconciles that later
lockfile. HTTP and conformance package-manifest deltas remain their assigned
owners. No UI package or JavaScript dependency is introduced.

The complete W4 control package manifest starts with the standard package
name/version/workspace edition/license stanza and exactly those dependencies.

In W5, proof-transport-http adds only the path dependency on
proof-operator-auth. Conformance adds normal dependencies on proof-storage,
proof-agent-runtime, proof-operator-auth, proof-operator-control, and
proof-transport-http because its normal `evaluate-operator-control` binary
imports and composes them. Its existing direct proof-kernel dependency exposes
the frozen raw artifact SHA-256 helper, so no transitive sha2 import or new W5
crypto dependency is permitted. Before either W5 owner edits source, each edits only
its owned package manifest and signals it frozen. E0002-14 then alone
reconciles the lock while both source trees remain quiescent and signals it
stable with exactly:

    rtk cargo check -p proof-conformance -p proof-transport-http --offline

Only after that stable signal may either W5 owner begin source edits or run
another Cargo command. A later dependency change stops both writers, requires
source quiescence, and repeats the entire barrier.

In W8, proof-operator-control adds only path dependencies on proof-storage,
proof-agent-runtime, and proof-transport-http. E0002-15 alone reconciles and
signals the later lock with exactly:

    rtk cargo check -p proof-operator-control --offline

No `cargo update`, online fetch, unlocked alternate command, or Cargo command
by another same-wave owner is allowed before each stable-lock signal.

### 16.3 Error decision

W3 adds no ExecutionError variant. It adds a distinct closed OperatorStoreError
in proof-kernel and OperatorAuthError in proof-operator-auth. The E0002 HTTP
router converts the public closed OperatorError taxonomy in section 9
exhaustively and tests every variant. The general HTTP ExecutionError mapping
is unchanged.

## 17. Static application and browser authority

The console is deterministic vanilla HTML, CSS, and JavaScript with zero
third-party packages, fonts, images, analytics, telemetry, or remote fetches.
Its build uses only a frozen Node built-in script and produces an exact
manifest whose filenames include SHA-256 content digests. E0002-15 embeds the
manifest and bytes at compile time. The asset route compares a requested
filename against this closed manifest; it never joins or opens a filesystem
path.

Session token and client nonce exist only in JavaScript closure variables.
They MUST NOT enter URL/history, DOM text/attributes, console, errors,
clipboard helpers, cookies, Cache API, local/session storage, IndexedDB,
service workers, referrers, screenshots, accessibility labels, or evidence.
Reload/navigation clears authority and initiates a new signed challenge; a
successful exchange replaces the inaccessible old session.

The UI keeps decision, resume, cancel, and End Session as separate controls.
Each mutation confirmation displays exact nonsecret run/request identity,
current revision, effect, and a fresh locally generated command/idempotency
key. Stale or uncertain responses disable repeat action and use protected
receipt/detail readback. Approval completion leaves the run visibly waiting
and presents a separate Resume control. Keyboard, focus, name/role/value,
contrast, live-region, reduced-motion, and 200-percent zoom checks are
all-required.

## 18. Restart, shutdown, evidence, and cleanup

The two required restart vectors are not interchangeable:

| Vector | Session | Instance/cursor key | Durable authority | Required next step |
|---|---|---|---|---|
| runtime_worker_restart | retained | retained | fence increments after lease expiry; commands/budgets preserved | recover exact checkpoint under current session |
| control_plane_restart | destroyed | replaced | commands/budgets/audit/runs preserved; old lease fenced after expiry | fresh signed Human challenge |

Graceful shutdown follows the single order in section 5: stop accepts and new
permits; drain or failure-settle while secret custody and the Agent signer are
still usable; release proven pre-dispatch reservations; checkpoint; append
exactly one `control_shutdown` event when storage is healthy; invalidate
volatile challenges/sessions and zeroize their buffers plus lease, dispatch,
cursor, and signer material; close the trusted store; then release the
workspace control lock truly last. Shutdown invalidation is not the explicit
durable SessionRevokeRequest: it creates no command, receipt, transition Proof,
or `session_revoked` event. Forced crash is recovered by the fence and full-reservation
forfeit rules; cleanup never guesses that an ambiguous effect did not occur.

Evaluation uses no live provider, network, payment, publication, or external
effect. Synthetic providers/tools expose exact counters. For a valid case, the
typed setup recipe provisions and arranges every prerequisite, then the harness
records the post-setup baseline and resets the six counters before executing
the typed action recipe. For a rejection case, its vector-specific typed setup
recipe first reproduces the named valid baseline by semantic digest, may arrange
the exact prerequisite state needed by that vector, then records the baseline
and resets the counters; the harness next performs exactly one typed mutation
operation and the typed action recipe. Setup and prerequisite arrangement are
therefore never hidden inside an action or counted as the tested effect.
`fixture_blueprint.expected.effect_deltas` is the exact delta from that
post-setup baseline, never the lifetime total. The counters are:

- `provider_calls`: provider boundary invocations after a dispatch permit;
- `tool_calls`: tool boundary invocations after a dispatch permit;
- `governed_writes`: committed INSERT/UPDATE/DELETE operations against
  agent run/step/checkpoint, approval request/decision, Proof, command,
  receipt, projection, run-control, lease, recovery-directive, budget account,
  or budget-reservation rows;
- `human_key_loads`: authorized attempts to open and decode the Human private
  key descriptor, including a later public-key mismatch;
- `signatures`: successfully produced Human or Agent signatures; and
- `external_effects`: independently instrumented effects outside the
  disposable process/database boundary.

`governed_writes` deliberately excludes the separately asserted append-only
operator audit event/head delta and volatile challenge/session bookkeeping.
Thus a rejection can require an exact nonempty
`fixture_blueprint.expected.audit.new_events` sequence while requiring zero
governed writes. Setup rows and setup calls never count. A case that needs an
existing dispatch/provider baseline establishes it during typed setup before
the counter reset; the tested action remains entirely post-reset, and only its
mutation/action aftermath counts. Every
unauthorized, stale, cancelled-before-dispatch, over-budget, and stale-fence
vector freezes all six exact deltas plus its exact durable-state and audit
postcondition.

`fixture_blueprint.expected.durable` describes the non-audit postcondition
relative to the post-setup baseline. `no_change` means byte-identical non-audit
rows; `allowed_audit_only` additionally asserts that an existing idempotency
command/receipt remains the sole command outcome and only its separately
declared conflict audit may be new. The remaining closed states name the sole
permitted transition: session issuance/invalidation, approval decision or
expiry rejection, explicit resume or cancel, pagination insertion, lease or
budget race winner, recovery reclaim, runtime commit/failure settlement,
unchanged dispatching reservation, full reservation forfeit, held control lock,
unchanged migration, removed workspace, exact prior receipt, or completed
control restart. Audit is always checked independently by the ordered expected
event list, including its sequence offsets, prior-link profile, subject binding,
full strict `AuditEvent` validation, and digest recomputation.

Evidence bundles contain only redacted DTOs, artifact digests, deterministic
fixture IDs, counters, timestamps, process/browser assertions, and strict
ProofReference values whose signatures are independently verified against the
fresh persisted Agent. A transport receipt alone never satisfies transition
evidence. Before
retention, a sentinel scan covers stdout/stderr, argv, environment snapshots,
logs, browser URL/history/DOM/storage/cookies/cache/referrer/console,
screenshots, database dumps, fixture output, evidence, and the worktree.
Disposable workspaces and process credentials are removed after quiescence.

## 19. Schema, evaluator, and digest authority

schemas/operator-control/manifest-v1.json maps every logical shape to one
Draft 2020-12 schema path and JSON Pointer. All schema objects are recursively
closed. The manifest records raw SHA-256 for each schema and E0002-13 self-test
example. evals/operator-control-v1.json records raw SHA-256 for this contract,
the manifest, and every schema, plus the semantic digests below.

Raw artifact digest reproduction is:

    sha256:<lowercase output of sha256sum over exact file bytes>

The semantic digest input is the named bare value encoded as compact UTF-8 JSON
with every object key recursively sorted lexicographically and no trailing
newline. Five independent values are frozen: the ordered `checks` array, the
ordered `valid_scenarios` array, the ordered `rejection_vectors` array, the
`backend_subset` object, and the complete `store_error_matrix` object. Their encoded values are
`sha256:<64 lowercase hex>`.

The evaluator command is exactly:

    rtk cargo run -p proof-conformance --bin evaluate-operator-control --offline -- --policy evals/operator-control-v1.json --fixtures evals/fixtures/operator-control --output target/operator-control-eval/result.json

It first validates the policy, manifest, all eight schemas, every raw artifact
digest, and all five semantic digests. It then loads the strict
`FixtureIndex` at `evals/fixtures/operator-control/index-v1.json`, which lists
exactly sixteen valid fixture envelopes and 105 rejection fixture envelopes
in policy ordinal order. Each envelope binds the policy digest, its canonical
policy-case digest, deterministic seed, and exact paths plus SHA-256 digests
for setup/action/expected documents; a rejection also binds exactly one
mutation document, its valid baseline fixture ID, and the frozen semantic
digest of that baseline. The path stem, fixture ID,
baseline ID, subject schema, action kind, and every digest MUST agree with the
policy. Every referenced document validates as `FixtureRecipeDocument`;
its `payload` is a structured `SetupRecipe`, `MutationRecipe`, `ActionRecipe`,
or `ExpectedRecipe`, and `payload_sha256` covers that payload encoded by the
same canonical compact-JSON rule. Duplicate-detecting parsing occurs before
schema validation; the phase selects the one permitted payload schema. The
document fixture/recipe/subject/phase must match its envelope, policy entry,
and path. Runtime checks additionally bind recipe ID to fixture ID and phase,
validate every generic typed document against the exact manifest reference,
and execute the typed adapter/operation/input/barrier program rather than
dispatching on a case ID or prose description. Unknown recipe IDs, fields,
phases, payload members, shape references, or adapter operations fail.
Hash-only or description-only execution is forbidden.

### 19.1 Executable recipe construction

The recipe is a program, not a case-name dispatch table. Every `FixtureStep`
has an empty `arguments` array. Its typed `input` is the sole control input;
the harness MUST NOT branch on fixture ID, ordinal, prose, path stem, expected
outcome, or owner task. `output_binding = exact_expected_recipe` means capture
the actual independently decoded observation and compare it with the expected
recipe after execution; it never means return or synthesize that expectation.

Fixture construction is deterministic. The envelope's lowercase 32-byte seed
is independent of expected values and is:

    SHA-256("Proof-Operator-Fixture-Seed-v1" || 0x00 ||
            ASCII(fixture_id))

The case UTC origin is `2032-01-01T00:00:00Z` plus the policy ordinal in whole
seconds, and monotonic time starts at zero. A derived byte string is successive
blocks of:

    SHA-256("Proof-Operator-Fixture-Value-v1" || 0x00 || seed || 0x00 ||
            UTF-8(symbolic JSON pointer) || 0x00 || u64be(block_index))

UUIDv7 values use the case UTC milliseconds, the required version/variant
bits, and derived remaining bits. Nonces, tokens, cursor keys, and Ed25519
seeds use the first required bytes of their distinct symbolic pointer streams.
Schema/catalog/request/authority/content digests are never filled with derived
opaque bytes: they are recomputed with their contract domain separator over
the constructed canonical value. All initial revisions, sequence numbers, and
fence epochs are one unless the selected setup record below says zero,
historical, or incremented; all page sizes are two; the aggregate fixture
limits are `(steps=10,tokens=1000,duration_ms=10000,cost_microusd=100000,
tool_dispatches=10)` with requested per-dispatch ceiling `(1,100,1000,10000,1)`.
Challenge lifetime is 120 seconds, session idle/absolute lifetimes are
300/900 seconds, cursor lifetime is 300 seconds, lease lifetime/renewal are
30/10 seconds, and the aggregate deadline is origin plus 600 seconds. A
mutation that says deadline equality advances exactly to, never beyond, that
deadline. Secret values are generated only inside the disposable process and
are represented in recipes/evidence solely by their named selector and digest.

The exact adapter/step-operation pairs are closed:

| Adapter | Permitted step operation and behavior |
|---|---|
| `workspace` | `provision_workspace` invokes the guarded schema-14 lifecycle; `seed_fixture` constructs exactly the supplied `EvaluatorSetupSpec`; `clone_baseline` re-executes only the named valid setup whose semantic digest matches, then applies the supplied rejection arrangements; `cleanup_workspace` runs only after quiescence. |
| `fault` | `inject_fault` applies the one `EvaluatorMutationSpec`; `corrupt_document` applies its one raw byte/document mutation before any typed value construction. |
| `http` | `call_http` sends the exact `EvaluatorActionSpec` request to the dedicated listener and records peer, method, path, headers, body, delivery, status, body bytes, and process disposition. |
| `store` | `call_store` invokes only the named `store_method` on the recording or real store and records the closed result variant or `OperatorStoreError`. |
| `runtime` | `call_runtime` invokes the named stage through the real runtime/store seam; `execute_synthetic_boundary` is the one instrumented provider/tool boundary after a permit. |
| `launch` | `launch_control` or `run_process` invokes the frozen init/serve lifecycle and records the closed `OperatorProvisioningError`, bind flag, and exit code. |
| `browser` | `browser_assert` drives the local static application only and records the named accessibility, secret-surface, or positive browser-only SAS display assertion; SAS display exposes only the challenge's Human-comparable short code and never an attestation or key. |
| `evaluator` | `evaluator_assert` and `verify_state` decode/recompute the named artifact, schema, result, route, audit, counter, or cleanup invariant without product mutation. |
| `tty` | `tty_authorize` performs the exact controlling-TTY/SAS/key ceremony and records key-open/sign/cleanup counters and process disposition. |
| `barrier` | `await_barrier` and `release_barrier` operate only the exact non-null barrier named by a barrier-ordered action. |
| `auth` | no direct action is permitted in v1; authority is exercised through the HTTP or TTY adapter. |

An adapter/operation pair outside that table is a harness error. A sequential
action has null barriers. Every step in a barrier-ordered action names the same
barrier; all participants reach it before the frozen winner order is released.
For a non-journey action, the action kind must equal every step's typed
`surface`. A `journey` contains at least two ordered steps. The authentication
and restart journeys cross at least two surface kinds; the UI-intent journey
may deliberately retain the browser surface for both independent interactions.
Its `JourneyExpectedOutcome.outcomes` covers those steps in
their exact execution order, with no missing or extra step. An HTTP response
observation's `count` is the number of contiguous byte-equivalent iterations
having that delivery/status/body. For each HTTP step, the sum of its ordered
response counts equals that step's `repeat`; every non-HTTP action has repeat
one. Thus the rate-limit cases record all initial and terminal requests, and a
single final response can never stand in for an unobserved flood. The journey's
final process disposition must equal the last executed outcome.

A delivered HTTP observation never has a null body. `TypedDocument` requires
an exact canonical response value. `SchemaBodyExpectation` requires the named
manifest shape to validate with duplicate-name rejection and requires the
canonical response-body SHA-256 to be equal across both clean replays; it is
used where E0002-12 must materialize deterministic IDs and cursors after Gate B
rather than embedding those volatile values in this policy. It is not a body
wildcard: the complete body bytes, schema result, and replay digest are retained
in evidence. Only a deliberately dropped response has a null body, because no
response bytes reached the client.

`EvaluatorSetupSpec` labels have these exact meanings. Combining labels takes
the union of the named rows; duplicate or inconsistent rows are a harness
error, and no unlisted durable row may be inserted.

| Label | Exact constructed state |
|---|---|
| `challenge_pending`, `challenge_signed`, `challenge_consumed` | One challenge in that exact volatile transition, with no session; signed carries one verified attestation and consumed cannot issue a token again. |
| `session_active`, `session_revoked` | One claims record bound to the setup workspace/Human/instance/policy; active retains only the token digest and revoked retains no usable token. |
| `cursor_issued` | One cursor over page size two with its frozen filters/sort/high-water and the active session binding. |
| `receipt_committed` | One terminal command and receipt with its transition Proof, projection, and audit already in the baseline. |
| `approval_waiting` | One waiting run/checkpoint, immutable approval binding, and signed request; no decision. |
| `approved_waiting`, `denied_waiting` | The approval-waiting row plus exactly one permanent signed decision of the named outcome; run remains waiting. |
| `actionable_command` | One governed run/checkpoint/control row targeted by a fresh cancel or resume command and no row for that new command ID. |
| `attention_awaiting_approval`, `attention_running`, `attention_pre_dispatch_recoverable`, `attention_terminal` | Four distinct projections at the case high-water, respectively waiting, running, recoverable-before-dispatch, and succeeded, ordered by their derived UUIDv7 IDs. |
| `pagination_projection_set` | Five projections at the cursor high-water plus one later projection already arranged after page-one capture and before the counter baseline. |
| `reserved_pre_dispatch_run` | One live lease/control row and one open `reserved` aggregate reservation with no permit. |
| `expired_lease_checkpoint` | One pre-dispatch reservation under an expired historical lease and non-null checkpoint tail; no resume/recovery completion has occurred. |
| `reclaimed_live_lease_checkpoint` | The prior row after its single reclaim winner, with fence incremented once and its returned recovery directive preserved. |
| `runtime_live_lease_checkpoint` | One live lease, checkpoint, replay binding, dispatch intent, and sufficient aggregate account, with no reservation unless separately selected. |
| `budget_contender_a`, `budget_contender_b` | Two distinct live governed runs sharing one aggregate account and each requesting the fixed per-dispatch ceiling at the named barrier; setup pre-commits or reserves exactly nine steps, 900 tokens, 9000 ms, 90000 microusd, and nine tool dispatches, leaving exactly one full fixed ceiling so precisely one contender can win. |
| `durable_control_restart_state` | Durable workspace/runs/commands/budgets/audit from the old instance, new locked instance metadata, and no volatile old authority. |
| `ui_actionable_approval`, `ui_cancellable_run` | One approval and one run projection containing only redacted UI fields and distinct confirmation intents. |

`reservation_state = none` adds nothing; `reserved_pre_dispatch` adds exactly
the corresponding open row; `dispatching_ambiguous` adds its permit and active
dispatch under live custody established during setup; `aggregate_contention`
binds both budget contenders to the one account. `synthetic_shell` supplies
recording adapters, `real_backend` supplies real SQLite/runtime with synthetic
boundaries, and `real_product` requires the E0002-15 process to supply a strict
`RealProductAssembly` evidence document that satisfies the policy's
`RealProductAssemblyExpectation`. The expectation freezes the current schema-
manifest digest, exact components/routes/database/lock and negative flags, and
requires the future binary and static-bundle SHA-256 values to be measured and
equal across both clean replays. E0002-13 does not fabricate future build
digests, principal IDs/keys, or workspace descriptor digests; the actual
assembly document records those values and the evaluator independently
recomputes them. Workspace,
control, session, capability, and authority labels select only the states named
by their enum values; a restarted control always has a new instance/cursor key
and an old session is absent rather than silently rebound.

`EvaluatorActionSpec.operation` selects exactly one public or internal method:
the HTTP operations map to the method/path frozen in section 5 (`session_*`,
`attention_read`, `run_detail`, `approval_decide`, `run_cancel`, `run_resume`,
`command_read`, `audit_read`, or auth-first fallback); the store/runtime
operations map one-for-one to their identically named section-11/12 method;
`workspace_init` and `control_serve` map to the two released commands;
browser/evaluator/TTY operations map to the adapter row above. The
`subject_schema` is the exact manifest shape decoded for that call.
`authority_selector = none` is mandatory for a call that consumes no challenge,
session, lease, or command scope. `fixture_primary` selects the setup's pending
challenge or primary lease/scope, while `active`, `mutated`, `replayed`, and
`revoked` select only that exact setup symbol. A public challenge, launch,
browser/evaluator assertion, or unauthenticated probe uses `none`; the TTY
ceremony and exchange select the pending challenge; protected and runtime calls
select their exact live or deliberately altered authority. `strict_fixture_document`, `raw_json`, and
`raw_http` consume the corresponding recipe document; `internal_state`
constructs the method request exclusively from the named setup symbols using
the deterministic rules above. Method, route, store method, input source,
subject shape, authority selector, repeat count, schedule, and barrier must all
match this mapping before execution. No expected value participates in request
construction.

For `raw_http`, the typed method/path are the exact post-mutation request line,
not the valid baseline route. The only negative request-line values are
`OPTIONS`, `/v1/proofs`, `/operator/v1/unknown`, and
`/assets/..%2Fconfig.json`; their bytes must agree exactly with the ActionSpec.

`EvaluatorMutationSpec` is literal. JSON replace/insert/delete/duplicate and
digest flips affect only `target` at `field_path`; raw mutations occur before
decode. Filesystem link/mode/remove/symlink/sidecar/database-move operations
perform exactly one named syscall effect at `timing`. `advance_clock` moves to
the named equality point; `state_transition` performs the one replacement;
identity/signer/route/static/evidence/counter/external-effect substitutions
replace only the named target; replay operators submit the already captured
bytes; `inject_store_error`/`inject_environment_error` return the exact closed
`injected_error` at the named boundary; `race_at_barrier` duplicates only the
action participants; process restart and TTY fault perform only their named
lifecycle transition. A non-null replacement or error not consumed by the
selected operator, a missing required field path/barrier, or more than one
mutation is a harness error.

Every `AuditEventExpectation` independently constructs the complete expected
strict `AuditEvent` from the case seed, setup symbol table, kind, outcome,
Proof operation, sequence offset, and subject binding. `session_challenge`
selects the pending/signed challenge and its Human/workspace/instance;
`session_authority` selects the issued/expired session and challenge;
`session_command` additionally selects the revoke command and transition
Proof; `approval_command` selects the run, approval, Human decision, command,
session, fence, and transition Proof; `approval_expiry` selects the run and
approval without a command; `run_command` selects the cancel/resume run,
command, session, fence, checkpoint, and transition Proof; `command_attempt`
selects the proposed command and every safely established target/session
reference; `lease_authority` selects run/current/source lease, process epoch,
checkpoint, fence, and control revision; `budget_reservation` adds budget,
reservation, intent, lease, and run; `runtime_commit` adds permit, replay,
reservation, prepared-result, and Proof references; `recovery_directive`
selects old/new leases, run, reservation disposition, checkpoint, directive,
and recovery digest; `control_process` selects only the safely known workspace,
instance/process, and failure scope. The kind-specific strict reads schema then
determines which of those named values must be non-null and forces every other
nullable reference to null. There is no "any valid event" comparison.

Offset one uses the captured `baseline_head`; every later offset uses the
immediately prior expected event digest. Each action step reads or retains the
trusted time exactly as its contract operation specifies. All events from one
atomic store operation use that single retained timestamp; a later action step
uses the clock value established by the recipe, and sequence offset never
implies another clock read. Event/Proof UUIDv7 values use their distinct
symbolic pointer streams while retaining the environment-supplied timestamp.
`event_semantic_sha256` is SHA-256 over the canonical construction descriptor
containing, in schema order, `kind`, `outcome`, `proof_operation`,
`sequence_offset`, `previous_digest`, `subject_binding`, `full_event_schema`,
and `digest_recomputed`; the digest field itself is omitted. This freezes the
relational audit commitment without pretending that E0002-13 already owns the
post-Gate-B fixture UUIDs. At execution, the harness independently constructs
the complete expected `AuditEvent` from that descriptor and the retained setup,
command, lease, reservation, and Proof symbols. It validates both expected and
actual events against `reads/v1#/$defs/AuditEvent`, recomputes the actual chained
digest, and compares every field. Actual event bytes and their independent
canonical SHA-256 are retained in evidence for replay comparison.
`sequence_offset = 1` with any link other than `baseline_head`, a later offset
with any link other than `prior_expected_event`, a missing offset, or
`prior_events_preserved = false` is a harness error.

Each case runs twice from two new, isolated disposable workspaces with the same
seed. `replay_count = 2` means two complete clean replays, not two commands in
one workspace; command retry is exercised only by the scenario that explicitly
specifies it. A rejection replay executes its vector-specific setup described
above, applies its single mutation after the reset, performs the action, and
compares the action-discriminated HTTP/store/runtime/launch/browser/evaluator/
TTY outcome, durable state, full audit events, and all six effect deltas. The
two replay records must have equal normalized semantic-observation digests and
distinct workspace-instance digests. No fixture can be skipped, merged, or
satisfy another ID.

The strict `EvaluationResult` always preserves sixteen ordered
`ScenarioResult` records, twenty ordered `CheckResult` records, 105 ordered
`VectorResult` records, two replay records per scenario/vector, the complete
redacted `EvidenceRecord` set, a complete `StoreErrorMatrixResult`, and
root/child failure codes. Exit 0 requires
16/16 scenarios, 20/20 checks, and 105/105 vectors to pass, no skips, and score
10000 basis points, and also requires 189/189 matrix cells plus 4/4 typed
absence cases to pass. Any required failure has score zero. Exit 1 writes the
complete failed result and evidence; exit 2 or greater denotes a harness/setup
error and is never a passing or scored evaluation. The harness additionally
enforces ID uniqueness and exact ordinal/result order, assertions-passed not
exceeding total, all evidence-reference closure with no orphan, the exact
twelve evidence requirements, replay equality/workspace distinction, and
completion time not preceding start time. Reordering, adding, dropping, or
weakening an assertion is an artifact change requiring a new Gate B digest.

The unscored `E0002-14` backend subset is also literal and digest-bound. It
contains the four positive scenarios `valid_runtime_worker_restart`,
`valid_aggregate_budget_concurrency`, `valid_runtime_dispatch_commit`, and
`valid_runtime_dispatch_failure`, plus the exact ordered vector IDs frozen in
the evaluator. Its `BackendSubsetResult` contains only subset results and
evidence plus its own complete passing `StoreErrorMatrixResult`; it cannot
validate as, or claim, a scored `EvaluationResult`.

E0002-13 self-tests cover meta-schema validity, all valid examples,
unknown-field rejection, and raw duplicate-name detection. E0002-12 later owns
the exact fixture/index/document corpus only. E0002-14 owns the conformance
binary and executes its backend-owned scenario/vector subset against real W4
APIs; it cannot claim the all-required score before the UI, protected HTTP, and
real assembly exist. After E0002-15, E0002-04 is the first independent owner to
run the complete one-fixture-per-valid/vector replay against the assembled
product and produce the scored EvaluationResult.
E0002-13 does not fabricate product/runtime fixtures before Gate B.

## 20. Gate B decision boundary and rollback

Gate B must accept, revise, or reject the exact digests for:

- this contract;
- every schema and schema manifest;
- the evaluator, ordered valid/check/rejection sets, backend subset, and
  exhaustive store-error matrix; and
- the E0002-13 handoff/decision packet naming those values.

Acceptance also explicitly approves or rejects:

1. independent terminal-signed Human challenge and volatile session ceremony;
2. six-capability intersection and exact-Human/no-delegation policy;
3. loopback/request/error/secret boundary and residual same-UID/root limits;
4. disposable workspace, forbidden repository-root identity, authoritative
   existing schema-14 database, no-migration trusted opener, and persisted
   Agent/Human signer lifecycle;
5. exact dedicated router and legacy-route exclusions;
6. migration 14 up/down SQL, immutable triggers, provisioning, atomic store
   traits, and legacy governed-run write rejection;
7. approval/no-auto-resume, explicit resume, cancel/dispatch,
   idempotency/uncertain-response, revoke/expiry, and signer ordering;
8. 30-second lease, 10-second renewal, fenced recovery, and the two restart
   semantics;
9. five-dimensional aggregate reservation/forfeit rules;
10. append-only projections, cursor snapshot/MAC binding, audit hash chain,
    redaction, and static-app constraints;
11. exact root/member/dependency/lock deltas; and
12. no new global ExecutionError plus all-required zero-effect evaluation.

Without a dated digest-bound owner acceptance, E0002-13 remains review and
every implementation task remains pending. Acceptance marks E0002-13 done and
authorizes only dependency-satisfied W3 tasks E0002-05, E0002-08, and
E0002-12; later waves still require normal dependency/ownership dispatch.

Operational rollback disables the operator process and returns users to
existing terminal controls while preserving durable requests, decisions,
runs, proofs, commands, receipts, projections, budgets, and audit. Schema
rollback is the quiescent, evidence-exported migration-14 down operation in
section 14 and cannot undo domain effects. No rollback may restore E0006
authority, URL credentials, browser persistence, wildcard binding, the general
router, alternate database, ephemeral signer, or automatic resume.
