# Approval Console Session Contract

**Contract:** `proof-approval-console-session/v1`
**Status:** proposed for AXP-E0006 Gate A/B
**Owner:** product owner
**Last updated:** 2026-08-31

This contract defines the only standalone browser approval-session design that
AXP-E0006 may implement. It replaces the current reusable URL-fragment bearer
and adds one fresh-registry pre-signing check; it does not change approval
signatures, runtime continuation, or tool-execution semantics.

## Security outcome

A local Human can reach the approval console through a clean loopback URL,
prove local interactive presence through one non-URL handoff, and receive a
bounded session without placing reusable approval authority in:

- a URL query or fragment;
- ordinary stdout/stderr, process arguments, or child environment;
- browser history, Web Storage, IndexedDB, cookies, or referrers;
- logs, screenshots, test artifacts, evidence packets, or repository files; or
- a durable Proof store, workspace file, or key file.

The browser necessarily holds an active session in volatile page memory and
sends it in an authenticated request header. Browser/process memory inspection
by the same UID, browser developer tooling controlled by the operator, root
compromise, a compromised Human private key, and local denial of service are
outside this contract. They MUST be reported as residual limitations.

## Preserved authority boundary

E0006 MUST NOT change these existing rules:

- the server binds only to `127.0.0.1`;
- the workspace is opened through the existing trusted workspace boundary;
- only a complete, exact, currently actionable signed approval request may be
  approved or denied;
- native run, step, request, operation/version, canonical arguments/input
  digest, and required Human are reloaded and checked immediately before
  signing;
- the selected approver is an enrolled local `Human`, and live-v2 must use its
  exact sealed Human;
- one request has at most one durable decision; duplicate or conflicting
  decisions fail without replacement;
- signing a decision does not execute or resume the governed tool; and
- live-v2 never advertises generic `agent resume`.

An authentication failure MUST occur before approval enumeration, signing-key
load, signature creation, or decision persistence.

E0006 strengthens one stale-cache boundary: the mutation handler MUST load a
fresh registry snapshot after final session validation and re-check that the
operation/version still exists with `HumanOnly` governance. The current server
caches the registry at process start, so this is an explicitly Gate-B-approved
correction rather than a claim about preserved behavior. A registry change
between detail review and the decision POST must fail before key load or write.

## Threat model

In scope:

- malicious webpages attempting loopback reads, CSRF, DNS rebinding, or CORS
  abuse;
- unprivileged local processes attempting unauthenticated loopback requests;
- accidental disclosure through terminal capture, URL/browser state,
  clipboard/manual copy, logs, test output, screenshots, referrers, or storage;
- malformed, guessed, duplicated, expired, replayed, concurrent, cross-instance,
  and cross-workspace bootstrap or session credentials;
- process/browser restart and lost-response failure windows; and
- stale browser pages attempting to act after expiry or revocation.

Out of scope:

- root or equivalent host compromise;
- unrestricted same-UID process-memory or browser-memory inspection;
- a compromised browser executable/extension or Human private signing key;
- availability against a local process that races or exhausts the one pending
  bootstrap; and
- remote or production access. E0006 is loopback-local only.

## Launch contract

`proof approval ui [--port <PORT>]` MUST:

1. run only on Linux, matching the current trusted-workspace boundary, and
   require an interactive controlling terminal capable of non-echoing input;
2. open and bind one exact workspace before serving;
3. generate a fresh random 128-bit server-instance identifier in memory;
4. bind only `127.0.0.1`, never a wildcard, hostname, IPv6 peer, Unix socket,
   external interface, or proxy;
5. emit only `http://127.0.0.1:<bound-port>/` plus non-secret instructions;
6. emit no query, fragment, bootstrap/session value, private workspace value,
   automatic browser-launch command, or secret child argv/environment; and
7. fail closed before serving if the terminal, workspace, listener, randomness,
   or security state cannot be established.

The CLI MUST NOT automatically launch a browser. The Human manually opens the
clean URL. Ordinary launch output is a public value and MAY be logged.

Gate B permits one exact terminal dependency change: add the already-locked
`rustix` 1.1.4 package with only its `termios` feature to the CLI crate. Use
`isatty`, `tcgetattr`, and `tcsetattr` through a restoration guard; do not add a
password-prompt package or invoke `stty`/another child. Echo is restored before
publishing the verification result on success, mismatch, EOF, timeout, error,
unwind, SIGINT, or SIGTERM. If safe restoration cannot be established, the UI
fails before bind. Non-Linux builds keep the command fail-closed.

## Bootstrap protocol

### Browser code

The clean index page generates eight random bytes using
`crypto.getRandomValues`, renders the resulting 16 lowercase hexadecimal
characters in four groups for manual entry, and keeps the canonical bytes only
in its current JavaScript closure. The value MUST NOT enter URL state, browser
storage, cookies, referrers, console output, DOM attributes, or retained
evidence. Visible text is cleared immediately after the exchange response or
any reported terminal state.

The page sends the ungrouped lowercase value once as:

```http
POST /api/session/bootstrap
Host: 127.0.0.1:<bound-port>
Origin: http://127.0.0.1:<bound-port>
Content-Type: application/json

{"code":"<16-lowercase-hex>"}
```

The request shape is exact: one `code`, no unknown or duplicate fields. The
server decodes the value to fixed bytes before storing it. It accepts at most
one pending bootstrap for the server instance and workspace and returns only a
non-secret `awaiting_local_confirmation` status and the fixed 120-second
lifetime. It never echoes or logs the code.

### Local confirmation

After one pending code exists, the CLI prompts on the controlling terminal for
the displayed code. Input MUST be non-echoing and read directly by the running
process, not through an argument, environment variable, file, pipe, command
substitution, clipboard helper, or child process. The parser may ignore the
four display separators but MUST otherwise require exactly 16 hexadecimal
characters.

The terminal supplies exactly one verification attempt. The server compares
all eight decoded bytes in constant work. Success atomically transitions
`pending -> verified`. Malformed input, mismatch, EOF, terminal loss, or the
120-second monotonic deadline atomically transitions to `revoked` or `expired`.
The deadline is expired when `now >= deadline`. No failure permits a second
code or verification attempt in the same server instance. The page clears the
visible code after the exchange completes or any bootstrap terminal state is
reported; no impossible browser-side inference of terminal completion is
assumed.

### Exchange

Immediately after successful bootstrap registration, the browser sends exactly
one strict JSON exchange request:

```http
POST /api/session/exchange
Host: 127.0.0.1:<bound-port>
Origin: http://127.0.0.1:<bound-port>
Content-Type: application/json

{"code":"<16-lowercase-hex>"}
```

The exchange is one bounded long poll. After fixed-time code comparison, it
waits without returning approval data or session authority until terminal
verification, revocation, terminal loss, or the 120-second deadline. Under one
authority lock it permits exactly one `verified -> exchanged` winner and uses
OS randomness to create a distinct 32-byte session secret. No response can
contain session authority before `pending -> verified` has linearized. A
second exchange waiter, concurrent loser, duplicate, replay, malformed or
mismatched code, expired code, or code from another server/workspace receives
a generic failure and creates no session.

The successful response contains the session secret exactly once. The server
MUST mark the bootstrap consumed before publishing the response. If the
response is lost, the browser reloads, or the page crashes, recovery is to stop
and restart the console; the bootstrap cannot be replayed and no alternate
credential is emitted.

## Session contract

The session secret:

- is 32 bytes from the OS CSPRNG, encoded as exactly 64 lowercase hexadecimal
  characters on the wire;
- differs from the bootstrap code and instance identifier;
- is stored only in server process memory and the current page's JavaScript
  closure;
- is scoped to the exact server instance and opened workspace;
- has a 15-minute absolute lifetime and a 5-minute idle lifetime, enforced
  with monotonic time; and
- is erased/invalidated on explicit revoke, either expiry, server shutdown, or
  unrecoverable authentication-state error.

Every protected request supplies exactly one `X-Proof-Session` header. The
server rejects absent, duplicate, non-ASCII, wrong-length, non-hex, expired,
revoked, cross-instance, or cross-workspace values before protected work. Once
decoded, all 32 bytes are compared in constant work. A successful protected
request advances only the idle deadline; it never extends the absolute
deadline.

For approve/deny, authentication is not a detached precheck. One mutation
authority lease MUST linearize final session/deadline validation, fresh
registry reload and governance validation, exact actionable-context reload,
signing-key access, signature creation, and durable decision persistence.
Revocation and expiry use that same lease. Therefore exactly one race outcome
is possible: the decision commits before revoke/expiry linearizes, or revoke/
expiry wins and no signing key, signature, or decision is produced. Session
expiry uses `now >= absolute_deadline` and `now >= idle_deadline`; a mutation
that acquired the lease while valid may complete before a waiting expiry.

The browser MUST NOT persist or clone the session. Reload, navigation, process
restart, or page-memory loss removes browser authority. The page provides an
explicit End Session action:

```http
POST /api/session/revoke
```

It requires the active session plus the exact Host, Origin, and JSON content-
type boundary. Revocation clears server authority before returning. A best-
effort page-close request MAY be made but is not a substitute for server-side
absolute/idle expiry.

## Request boundary

All `/api/**` requests MUST require the exact bound Host. Every request that
creates, exchanges, revokes, approves, or denies MUST additionally require the
exact same Origin and `application/json` content type. Missing or duplicated
security headers fail. The server sends no permissive CORS header and accepts
no `null`, hostname-alias, wildcard, opaque, or foreign origin.

The index and every API response MUST use `Cache-Control: no-store` and
`X-Content-Type-Options: nosniff`. The index MUST retain a restrictive CSP,
`Referrer-Policy: no-referrer`, and frame denial. It MUST register no service
worker and load no remote script, image, font, frame, form action, or network
origin.

Framework-generated errors for unknown routes, wrong methods, rejected or
duplicate headers, unsupported media, and malformed JSON MUST carry the same
no-store and nosniff baseline and MUST NOT reflect request bodies or secrets.

Errors are generic and MUST NOT echo credential bytes, distinguish a guessed
byte prefix, reveal approval data, or serialize internal state. Raw request
bodies and credential headers are never logged.

## State machines

Bootstrap state is one of:

```text
absent -> pending -> verified -> exchanged
             |          |           |
             +----------+-----------+-> revoked
             +--------------------------> expired
```

Only `pending -> verified` and `verified -> exchanged` are success transitions.
Every terminal bootstrap state is immutable for the server instance.

Session state is one of:

```text
absent -> active -> revoked
                  -> expired_absolute
                  -> expired_idle
```

There is at most one bootstrap and one session per server instance. State
transitions that can race MUST be serialized under one in-memory authority
lock and tested with deterministic barriers and an injectable monotonic clock.
The long-poll waiter is part of bootstrap authority state; losing its response
does not make the consumed bootstrap replayable.

## Compatibility and storage

- No SQLite migration or durable session record is allowed.
- The approved direct CLI dependency is `rustix` 1.1.4 with `termios`; its
  manifest/lockfile delta is owned by E0006-02. No other new package is allowed.
- No kernel, runtime, proof, approval, registry, delegation, or signing-key
  shape changes are allowed.
- Existing terminal `approval approve` and `approval deny` commands remain the
  supported rollback path and retain their behavior.
- Existing v1/v2 approval review and signing tests must pass unchanged.
- E0002 may reuse this contract only after E0006 Gate C; it may not reuse the
  historical fragment bearer or expand session authority implicitly.

## Required rejection and recovery evidence

Implementation and independent verification MUST prove:

- clean URL, output, argv, environment, browser history/storage/cookie/referrer,
  and repository/test artifacts;
- no controlling TTY, malformed code, terminal mismatch, and bootstrap timeout;
- a single long-poll exchange releases no session before terminal verification,
  then exactly one winner under a deterministic concurrent race;
- replay, deadline equality, terminal EOF/loss, lost exchange response,
  expired, cross-instance, and cross-workspace rejection;
- malformed/duplicate session header, absolute expiry, idle expiry, explicit
  revoke, stale page, and server-restart rejection;
- wrong/missing/duplicate Host or Origin, duplicate credential/content-type
  headers, non-JSON, malformed JSON, wrong method/route, and CORS attempts;
- no approval enumeration, private-key load, signature, or decision on every
  failed authentication path;
- deterministic decision-versus-revoke and decision-versus-expiry races with
  exactly one linearized outcome;
- registry/governance drift between detail review and POST fails before key
  load, signature, or decision persistence;
- unchanged exact v1/v2 actionability, required Human, approve/deny,
  double-submit, and no-auto-resume behavior; and
- secret-sentinel scans over stdout/stderr, child argv, browser surfaces,
  screenshots, logs, test artifacts, and the worktree.

The evaluator in `evals/approval-console-security-v1.json` requires all 14
checks. A failed check is a release stop; deterministic unit coverage cannot
substitute for the independent browser/process evidence.

## Rollback

Rollback disables `approval ui` or reverts users to terminal approve/deny while
preserving durable requests and decisions. It MUST NOT restore the fragment
URL, Web Storage bearer, persistent credential, wildcard bind, or relaxed
request boundary. No credential cleanup migration exists because approved
E0006 credentials are never durable.
