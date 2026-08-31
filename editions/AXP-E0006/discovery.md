# E0006 Read-only Discovery

Three `gpt-5.6-luna` agents independently inspected the standalone approval
console, public guidance, current dependencies/tests, and next-edition graph on
2026-08-31. They made no repository edits, opened no private `.proof` content,
started no UI/browser/provider, and created no external effect.

## Current flow and hazard

- `cmd_approval_ui` creates one 32-byte random token, keeps it in
  `ApprovalUiState`, binds `127.0.0.1`, and prints the token in
  `http://127.0.0.1:<port>/#<token>`.
- The inline browser script reads `location.hash`, saves it under
  `sessionStorage["proofApprovalSession"]`, clears the visible hash, and sends
  the reusable token in `X-Proof-Session` for every API request.
- That one token authorizes the complete approval inbox/detail surface and any
  otherwise-actionable Human decision for the server lifetime. It has no
  absolute expiry, idle expiry, one-use exchange, replay detection, explicit
  revocation, or constant-time comparison.
- Fragment omission from ordinary HTTP/referrer transmission does not make the
  bearer safe. It deliberately enters stdout/terminal capture, URL/browser
  state, Web Storage, clipboard/manual-copy flows, and any surrounding logs.

## Defenses to retain

- IPv4 loopback-only bind.
- Exact Host and Origin checks for mutations and JSON content-type enforcement.
- `Cache-Control: no-store`, CSP, `Referrer-Policy: no-referrer`, frame denial,
  and text-only rendering of approval data.
- Server-side reconstruction and exact validation of signed request, native
  run, step, operation/version, canonical arguments/input digest, actionability,
  required enrolled Human, and one-decision semantics.
- No generic resume recommendation for live-v2; signing never executes a tool.
- Private approver-key mode and actor/key consistency enforcement.

## Converged minimal design

Serve a clean URL and use a browser-generated random bootstrap code confirmed
once through a non-echoing controlling-terminal prompt. Atomically exchange
the verified code once for a distinct high-entropy session held only in page
memory. Bind all state to one process instance and workspace; enforce short
bootstrap, absolute-session, and idle-session expiry; compare fixed secret
bytes in constant work; retain exact request-boundary and signing checks.

This needs no kernel, runtime, storage, migration, persistent session, remote
access, or provider work. The existing Axum/Tokio/randomness stack is adequate;
any new dependency used for terminal secrecy must be separately surfaced in
Gate B and integrated by the orchestrator.

## Source and test evidence

- Server/auth/UI: `crates/proof-transport-cli/src/commands/approval_ui.rs` and
  `approval_ui.html`.
- Signing/key boundary: `crates/proof-transport-cli/src/commands/approval.rs`.
- CLI exposure: `crates/proof-transport-cli/src/main.rs`.
- Process-test pattern: `crates/proof-transport-cli/tests/live_prepare_process.rs`.
- Public prohibition: `README.md` and
  `docs/dogfood/release-manager-live.md`.
- Historical non-security browser exercise:
  `docs/dogfood/release-manager-preview.md`.

## Required verification

Tower tests must cover clean launch, state transitions, exact one winner under
concurrency, mismatch/replay/expiry/cross-scope rejection, session expiry and
revocation, unchanged actionability, and pre-signing failure. Process tests
must prove clean output/argv. A distinct non-author must use browser automation
against a disposable workspace and retain only redacted evidence showing no
URL/history/storage/cookie/referrer/console credential, required response
headers, exact decision behavior, and a 14/14 policy result.
