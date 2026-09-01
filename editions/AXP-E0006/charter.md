# AXP-E0006 Charter — Secure Standalone Approval Console

- Edition ID: `AXP-E0006`
- Owner: product owner
- Orchestrator: Codex primary agent
- Base revision: `d76f44f960d905398a266fa8858562ad15fb2366`
- Status: `blocked and quiescent — Gate C deferred/no-go in D-E0006-025`
- Dependency: E0001 Gate C defer recorded in D-E0001-020; E0001 is blocked,
  quiescent, and writer-free
- Discovery: three read-only `gpt-5.6-luna` audits completed 2026-08-31

## Outcome

User/problem: the standalone approval UI can display and sign exact governed
consequences, but its current launch URL contains one reusable workspace-wide
Human signing-session bearer. The browser copies that bearer into Web Storage.
Terminal capture, browser history/session restore, clipboard use, logs, or any
holder of the fragment can therefore obtain approval authority for the server
lifetime.

North-star journey:

1. A local Human starts `proof approval ui` from a verified terminal in a
   trusted fresh workspace. The CLI binds only to IPv4 loopback and emits one
   clean URL with no query or fragment credential.
2. The Human manually opens that URL. The browser creates one short-lived
   random bootstrap code in page memory and displays it without placing it in
   URL state, Web Storage, cookies, referrers, or logs.
3. The page opens one bounded long-poll exchange, then the Human enters the
   displayed code through a non-echoing Linux controlling-terminal prompt.
   The server atomically verifies that exact pending code once; mismatch,
   timeout, duplicate, or replay revokes the bootstrap.
4. Only after terminal verification may that single exchange return a distinct
   random session credential. The browser retains it only in JavaScript memory.
   It is bound to one server instance and workspace and expires after 15
   minutes absolute or 5 minutes idle.
5. The Human reviews the exact signed request, run, step, operation, version,
   canonical arguments, and required Human, then approves or denies. Existing
   v1/v2 actionability and signing checks remain authoritative.
6. An independent verifier proves the clean launch surface, one-use and expiry
   behavior, cross-scope rejection, browser secrecy, signing boundary, and
   fail-closed rollback against the sealed 14-check evaluation.

Success metric and declared evaluation:

- `proof-approval-console-security/v1` passes 14/14 and 10,000 basis points.
- No reusable bootstrap or session credential appears in a URL, ordinary
  stdout/stderr, process argument, browser history, Web Storage, cookie,
  referrer, log, screenshot, test artifact, or repository file.
- Exactly one verified bootstrap can create exactly one distinct session;
  malformed, mismatched, expired, replayed, concurrent-loser, cross-instance,
  and cross-workspace attempts fail before approval data, signing-key access,
  or decision persistence.
- Session absolute/idle expiry and explicit revocation fail closed. Existing
  Host, Origin, JSON content-type, exact actionability, enrolled-Human,
  separation-of-duties, and one-decision rules remain green.
- A single mutation authority lease linearizes current-session validation,
  fresh registry/governance reload, exact request review, key access, signing,
  and durable decision against revoke/expiry races.
- Scoped CLI tests and formatting, reverse-impact selection, process tests,
  browser verification, secret-sentinel scans, edition validation, and the
  quiescent final gate pass with zero paid provider use.

## Scope

In scope:

- Freeze `proof-approval-console-session/v1`, its threat model, HTTP exchange,
  credential state machines, expiry values, bindings, and error behavior.
- Replace the reusable fragment bearer and `sessionStorage` path with a
  browser-to-terminal one-use bootstrap and a distinct memory-only session.
- Preserve the current approval inbox/detail UI and exact v1/v2 approval
  actionability/signing behavior.
- Add deterministic router, concurrency, clock/expiry, process-output/argv,
  and browser secrecy verification.
- Reproduce and correct the D-E0006-014 current-document delivery ambiguity
  under the unchanged public state machines, generic error boundary, and
  credential-leak prohibitions.
- Add a request-bound, outcome-specific in-browser Human-intent confirmation
  before the existing decision POST, without treating it as a new backend
  authority boundary.
- Use only the already-locked `rustix` 1.1.4 `termios` surface for non-echoing
  Linux TTY input; E0006-02 owns its exact crate-manifest/lockfile delta.
- Update public guidance only after the secure path passes independent review.

Non-goals:

- E0002 multi-run control-plane expansion, run cancellation, delegation
  redesign, aggregate budgets, or new operator actions.
- Remote access, wildcard binding, accounts, TLS, hosted UI, production
  deployment, browser extensions, automatic browser launch, or credentials in
  child argv/environment.
- Kernel/runtime/storage API changes, database migrations, persistent
  bootstrap/session state, cookie authentication, or new signing-key formats.
- Decision request/response schema changes, server-side signing/authority
  changes, or any claim that a browser intent challenge authenticates a Human.
- Provider calls, external publication, destructive cleanup, or weakening the
  terminal approve/deny path.

## Budget

- Delivery: one orchestrator plus at most one active security writer in each
  implementation or verification wave. No two editions have active writers.
- Routing: read-only discovery uses `gpt-5.6-luna`; contract, security
  implementation, independent security/browser verification, and integration
  use `gpt-5.6-sol` under `editions/MODEL_POLICY.md`.
- Scope: eleven tasks across W1-W11, including the owner-authorized delivery
  remediation, launcher diagnostic/ceremony, bounded W8 Human-intent guard, W9
  non-author source/test review, and non-dispatchable W10 ceremony packet. Any
  kernel, runtime, storage, migration, remote-access, persistent-secret, or
  backend decision-schema need stops for a new Gate B decision.
- External/live spend: zero. No provider credential, network service, or paid
  model call is part of the product evaluation.
- Credential limits proposed for Gate B: one 64-bit browser-generated bootstrap
  code, 120-second bootstrap lifetime, one terminal verification attempt, one
  exchange, one 256-bit session, 15-minute absolute lifetime, and 5-minute idle
  lifetime. All credential state is process memory only.

## Material-risk triggers requiring Gate B

- The public approval-console session contract, bootstrap transport, secret
  comparison, expiry, replay, instance/workspace binding, or revocation rules.
- Any change to approval authority, signing, required Human, actionability,
  Host/Origin/content-type enforcement, loopback binding, or failure ordering.
- Any persistent credential, migration, new signing key, remote bind, browser
  launcher, child-process secret, dependency beyond the explicitly proposed
  locked `rustix` termios feature, or relaxation of a prohibited leakage channel.
- Any expansion into E0002 operator-control scope or an external effect.

## Approval

- Gate A approver/date: product owner / 2026-08-31 / D-E0006-004
- Gate B decision/date: product owner / 2026-08-31 / D-E0006-004, binding
  D-E0006-003 and the frozen contract/evaluation digests
- Gate B remediation decision/date: product owner / 2026-08-31 /
  D-E0006-015, preserving the frozen contract and authorizing one independent
  post-repair 14/14 ceremony
- Launcher-first ceremony decision/date: product owner / 2026-09-01 /
  D-E0006-018, exactly one provider-free E0006-08 run with no retry
- Decision-intent remediation/date: product owner / 2026-09-01 /
  D-E0006-020, UI-only request/outcome challenge with no product-run authority
- Independent intent review/date: orchestrator / 2026-09-01 /
  D-E0006-022, exact source/test PASS with no runtime authority
- Final intent-bound ceremony/date: product owner / 2026-09-01 /
  D-E0006-023, consumed by the D-E0006-024 launcher-gate failure with no retry
- Gate C decision/date: product owner / 2026-09-01 / D-E0006-025,
  deferred/no-go; E0006 is not release-accepted

The combined Gate A/B packet and bounded remediation were approved. E0006-05
removed the observed delivery ambiguity under the frozen contract and passed
independent source/test review. The single authorized E0006-06 ceremony then
stopped at its same-visible-tab attachment boundary, so E0006-04 and Gate C
remain unopened and the standalone UI remains unreleased. E0006-07 subsequently
proved a credential-free launcher-first attachment path. D-E0006-018 activated
exactly one E0006-08 ceremony. Its credential-free launcher preflight passed,
but the product run persisted an unexpected signed approval instead of the
authorized denial. No execution, resume, provider, or external effect followed.
D-E0006-019 consumes that authority and leaves E0006-04 and Gate C unopened.
The owner then directed either a fix or a move; D-E0006-020 selects the
smallest defensible fix: an explicit request-bound Human-intent guard followed
by non-author source/test review. E0006-09 and E0006-10 then passed focused,
host, scoped, formatting, JavaScript, frozen-hash, and distinct review gates.
The final authorized product ceremony stopped before product state because its
sole headed tab remained empty `about:blank`. D-E0006-023 is consumed. The
product owner then deferred Gate C in D-E0006-025: the standalone UI remains
unreleased, terminal approve/deny is the rollback, and no retry or source repair
is authorized.
