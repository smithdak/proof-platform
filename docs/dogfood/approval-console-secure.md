# Secure approval console verification

**Edition:** AXP-E0006
**Evaluation date:** 2026-08-31
**Result:** **FAIL / blocked — do not release**

The independent verification did not establish the required 14/14 result. The
implementation's deterministic CLI suite passed 117/117 in host context, and a
fresh synthetic console exposed only a clean IPv4-loopback URL with the
required response defenses. The first automated journey stopped at the local handoff:
the verifier had no channel through which a Human could read the displayed
one-use code and type it directly into the console's controlling terminal.

Using browser text output, a screenshot, a clipboard, a file, a pipe, an
argument, or an automated relay would have disclosed the code or weakened the
frozen Human-presence contract. None of those workarounds was used. No
bootstrap code, session credential, private key, browser capture, HAR, copied
database, or complete `.proof` workspace was retained.

## Evidence obtained

- The frozen contract, policy, ordered 14-check set, and ordered 26-vector set
  matched their approved SHA-256 digests.
- `rtk cargo test -p proof-transport-cli` passed 117 tests across three suites
  in host context. `rtk cargo fmt --check -p proof-transport-cli` passed. The
  reverse-impact selector reported only `proof-transport-cli`.
- A provider-free fresh synthetic workspace contained one exact pending
  `release.publish::v1` request for `preview` / `2026.08.29-rc1`.
- The server listened only on an ephemeral `127.0.0.1` port. Ordinary output
  contained one clean URL and non-secret instructions, with no query or
  fragment.
- The required isolated browser loaded meaningful content with no framework
  overlay, browser error, console output, or remote resource. Before exchange,
  the current URL had no query or fragment; referrer, local storage, session
  storage, cookies, IndexedDB, and service-worker registrations were empty.
- The index returned `Cache-Control: no-store`, `X-Content-Type-Options:
  nosniff`, `Referrer-Policy: no-referrer`, frame denial, and the restrictive
  local-only CSP. Real 404 and 405 responses retained no-store/nosniff, and a
  foreign-origin OPTIONS probe returned no CORS permission.
- No screenshot or network export was made while the code was visible. The
  browser was closed without saving state. The bootstrap expired without a
  session, the server was stopped, and the loopback listener closed.
- Immediately before disposal, the request still had no decision or execution
  proof. The run remained `waiting_for_input`, its step remained
  `waiting_for_approval`, and its event stream ended at `approval_required`.
- The explicitly created synthetic workspace was removed. No browser session
  remained active.

## Follow-up Human ceremony

A second fresh provider-free fixture completed the product journey on
2026-08-31 without communicating or retaining either credential. An authorized
co-located Human visually read the one-use code in a normal browser, typed it
directly into the controlling terminal, observed the code clear and the inbox
appear, reviewed the exact synthetic request, and signed one `denied` decision
with reason `E0006 synthetic verification — deny; no execution authorized.`

Independent durable inspection established:

- request `01a058a3-b3f4-76f3-8ebb-2ee04adb8d75` is the only approval;
- it binds `release.publish::v1`, run
  `01a058a3-b3a9-76c1-8f16-82dc1822f3f9`, step
  `01a058a3-b3c5-7e92-9ad0-5d76d8897b14`, exact arguments
  `{"environment":"preview","version_label":"2026.08.29-rc1"}`, and input
  digest `8dbccce517d9d4db2349b31681da8125a918c4e0740f1ff9d868618155cd835c`;
- decision `01a058a7-6907-7c90-8a8b-fc6a51c4645e` was made by the one enrolled
  Human `01a058a3-9a87-7472-bb5d-39c665f2f1f9`, references the exact request,
  and has outcome `denied` with the stated reason;
- the provider-free preparation validator verified the request signature,
  Human decision signature, request digest, request ID, and enrolled Human,
  then failed closed on the intentional denial before reaching runtime resume;
- the request has no execution proof, the run remains `waiting_for_input`, the
  step remains `waiting_for_approval`, and the event stream still ends at
  `approval_required`; and
- the live process argv contains only the executable, disposable workspace,
  `approval ui`, and fixed port. It has no child process and listens only on
  `127.0.0.1`. Current index/404/405 probes retain the required security
  headers and emit no CORS permission; and
- after the durable inspection completed, the Human clicked End Session and
  observed the exact non-secret ended-session state. A post-revoke durable read
  showed the same single denial and no execution proof while the server
  remained loopback-only pending controlled shutdown; and
- the Human then stopped Terminal A with Ctrl+C and ran a same-TTY check that
  returned exactly `echo restored`. Independent `lsof` inspection found no
  remaining listener. A final durable read still showed the same denial and no
  execution proof, after which only the exact disposable fixture was removed.

This is successful Human product-journey evidence, including a valid signed
decision and zero execution/resume. It does **not** change the overall FAIL /
blocked verdict: the successful page was a normal browser that was not
attachable to the named agent-browser session. Therefore the independent
verifier could not capture current-session agent-browser URL/history/storage/
cookie/referrer/console/resource/screenshot evidence. Evidence from another
server instance cannot be combined with this memory-only session to claim the
frozen 14/14 result.

## Final headed agent-browser attempt

D-E0006-008 authorized one final fresh attempt using a real headed, named,
attachable `e0006-manual` agent-browser session. The Human again transferred
the one-use code visually and directly into the controlling terminal. The
terminal reported local confirmation verified, and the verifier attached only
after `#app:not([hidden])` proved source-ordered code clearing.

Current-session post-bootstrap evidence passed:

- the URL was exactly the clean `http://127.0.0.1:<PORT>/`, with empty query,
  fragment, and referrer; the Navigation API exposed one current clean entry;
- local and session storage, cookies, IndexedDB, service-worker registrations,
  and Cache Storage were empty;
- all 13 observed resources were same-origin with empty query/fragment, with
  zero remote-origin resource;
- the bootstrap text was empty and hidden, the app was visible, the credential
  was not global, and browser errors, console output, and overlays were absent;
- the page rendered exactly one `release.publish::v1` request with the expected
  request/run/step/requester/Human, exact `preview` /
  `2026.08.29-rc1` arguments, and enabled decision controls; and
- the permitted credential-free post-clear screenshot was visually clean and
  had SHA-256
  `7478922b399e3da38ed138285823ed07d34c7404693580e7cb49cef7df12a559`.

The decision step failed closed. After the Human attempted the one denial with
the fixed synthetic reason, the same browser displayed generic `request
rejected`, an empty receipt, and a still-pending request. Durable inspection
found `decision: null`, `execution_proof_id: null`, a `waiting_for_input` run,
a `waiting_for_approval` step, and an event stream still ending at
`approval_required`. The credential-free rejection screenshot had SHA-256
`78061d1f0394dd2e1f29666b6ee46988395d8210ca40c0c01102fe8deb9de81a`.
No retry was made. The request subsequently expired with the same null
decision/execution state.

End Session then returned a generic rejection, so the UI retained fail-closed
disabled authority controls rather than claiming revocation. The Human stopped
Terminal A; the same-TTY check returned exactly `echo restored`, no listener
remained, and closing agent-browser left no active named session. A prior
headed bootstrap-timeout fixture also remained expired with null decision and
execution. Both exact fixture directories and both credential-free captures
were removed after final inspection.

This final run resolves the earlier browser-capability gap but still cannot
score 14/14: its required signed decision and confirmed explicit revoke did not
complete. The successful signed denial from the normal-browser fixture and the
successful browser secrecy capture from this different server instance cannot
be combined into one frozen end-to-end result.

## Five-minute decision-first rerun

D-E0006-010 authorized one last fresh provider-free fixture on fixed port
`43121`, with exactly one pending `release.publish::v1` request. The Human
completed the direct visual-to-controlling-TTY bootstrap and, before any
verifier browser inspection, signed exactly one denial with reason `E0006
synthetic verification — deny; no execution authorized.` The Human-visible
headed page showed the exact `preview` / `2026.08.29-rc1` arguments, blocked
reason `approval request was already denied`, the fixed reason, receipt
`Decision signed: denied.`, and a resume command that was not run.

Independent durable reads established:

- exactly one denied request and one immutable decision, bound to the expected
  request and required enrolled Human;
- 64-byte request and decision signatures, an exact request-digest link, and a
  decision time inside the request window. The signing route verifies the
  signed request and enrolled Human key before creating and persisting the
  decision; the deterministic signature/substitution suite was already green;
- exact pending arguments `preview` / `2026.08.29-rc1` and input digest
  `8dbccce517d9d4db2349b31681da8125a918c4e0740f1ff9d868618155cd835c`;
- `execution_proof_id: null`, no execution record, run `waiting_for_input`,
  step `waiting_for_approval`, and events still ending at sequence 4
  `approval_required`, with no resume, tool success, proof, or evaluation; and
- an IPv4-only `127.0.0.1:43121` listener and the required no-store, CSP,
  no-referrer, frame-denial, and nosniff response headers before shutdown.

The Human then clicked End Session and reported the exact `session ended`
state, establishing explicit revocation. Controlled Ctrl+C removed the
listener, the same Terminal A returned exactly `echo restored`, and the named
browser session was closed; the session inventory was empty. Final durable
reads preserved the one denial and zero execution. The exact disposable
fixture was removed after those reads.

This successful product path still does **not** establish 14/14. When the
independent verifier attached to named session `e0006-manual`, its tab inventory
exposed only a distinct fresh pre-bootstrap tab, not the Human-visible
post-decision tab. Aggregate inspection of that exposed tab found a clean URL,
empty code text and persistence surfaces, but the approval app was hidden and
no decision content was present. The verifier therefore did not take a
screenshot or claim post-decision URL/history/storage/resource/console evidence
for the Human session. Prior browser evidence belongs to a different fixture
and cannot be combined with this denial/revoke run.

## Credential-free attachment diagnostic

A later diagnostic isolated the tab-attachment behavior without a product
fixture, approval authority, credential, provider, or governed effect. One
headed named session `e0006-tab-probe` began with one existing `about:blank`
tab. That same tab was asynchronously navigated to
`http://127.0.0.1:43122/` while an intentional `/hold` fetch remained pending.

Independent direct reads from both agents reached the same live document and
returned exact title `E0006 attachment probe`, marker `Credential-free
single-tab attachment probe`, and status `pending`. Session inventory contained
one named session and tab inventory contained one tab. `tab list` continued to
show the tab's initialization metadata as `about:blank`; direct URL, title, and
body reads proved that metadata stale and non-authoritative for the current
document. The verifier did not navigate, screenshot, or close the probe.

The probe browser, listener, isolated runtime directory, and disposable temp
content were subsequently removed; independent checks found no listener on
43122 and no runtime directory. This explains the attachment symptom but does
not supply product evidence: D-E0006-010 authority was already consumed, and
the probe had no bootstrap, session authority, approval, decision, or product
page. A new explicit owner authorization is required for any product rerun.

## D-E0006-013 final single-tab run

D-E0006-013 authorized one final fresh provider-free fixture at
`/tmp/proof-e0006-final-single.ICHLUn`, preparation
`00000000-0000-7000-8000-00000000000c`, request
`01a058f6-fdd8-7312-a98c-f6aca788c2c4`, run
`01a058f6-fd92-77b1-be95-de7e8e797893`, step
`01a058f6-fdad-7bd0-bcd3-2621d703f535`, and enrolled Human
`01a058f6-8ed4-70e1-96d3-9a16a4542472`. The clean process selected its default
IPv4 loopback port `35953`; planned port `43123` was never used.

The Human reported exact terminal status `Local confirmation verified.` Direct
read-only attachment found exactly one named session and one tab and returned
the exact clean URL `http://127.0.0.1:35953/` and title `Proof approvals`.
However, the current DOM showed the bootstrap panel visible with empty code,
the approval app and session controls hidden, one same-origin resource, and
status `request rejected. Restart the console to try again.` Tab-list metadata
remained stale `chrome://newtab/`. Because the code-hidden/app-visible safety
gate did not pass, the verifier did not take a screenshot, inspect any
credential, or claim product-page browser aggregates.

Terminal handoff therefore succeeded, but browser session establishment and
ownership remained inconclusive. The visible tab failed closed. This evidence
does **not** prove a second browser, session theft, or compromise.

The Human stopped the process with Ctrl+C and the same terminal reported
exactly `echo restored`. Final durable state contained exactly one pending
`release.publish::v1` request with `decision: null` and
`execution_proof_id: null`; the run remained `waiting_for_input`, the step
remained `waiting_for_approval`, and events 0–4 still ended at
`approval_required`, with no resume, tool success, proof, or evaluation. No
provider boundary or governed effect occurred.

The browser/session, process, listeners, isolated runtime, and exact fixture
were removed. Independent cleanup checks found no listener on either 35953 or
43123 and neither runtime nor fixture path. D-E0006-013 authority is consumed;
there was no retry. The result remains **FAIL / blocked** because no authorized run
combined the complete product journey with the frozen current-session browser
evidence.

In the earlier normal-browser follow-up, the Human reported taking one
permitted screenshot only after the bootstrap code cleared. It was not
transmitted to, independently inspected by, or retained by the verifier, and
could not substitute for current-session agent-browser evidence in that run.
No credential-bearing capture was created or retained by the verifier.

During the first, expired attempt, the terminal was independently observed with
echo disabled and the process reported expiry only after its verified
restoration boundary; that disposable PTY vanished before an external flag
comparison. The successful follow-up closed this evidence gap: after explicit
browser revocation and controlled Ctrl+C, the same Terminal A reported exactly
`echo restored`.

## Blocked release disposition after D-E0006-014

D-E0006-013 was explicitly final and consumed; E0006-03 stopped blocked and
quiescent. E0006-04 and Gate C remained unopened, and the secure approval
console was not releasable from that evidence. Terminal `proof approval
approve` and `proof approval deny` remained the rollback path; the historical
fragment and Web Storage bearer remained prohibited.

D-E0006-015 subsequently authorized one bounded source remediation and, only
after a distinct non-author source/test PASS, one fresh provider-free ceremony.
It did not convert prior evidence into a release result or open Gate C.

## E0006-06 pre-ceremony remediation review

**Source/test review: PASS — ceremony not started.**

The distinct non-author reviewed the one-file E0006-05 CLI delta against the
unchanged frozen contract and evaluation. The split-linearization diagnosis is
reachable in the old source: an exchange may claim while pending, terminal
verification may commit `Verified` at second 119, and the admitted delivery
task may resume at second 120, where the old loop expires the bootstrap before
session activation. The candidate activates the session under the same
authority lock when in-time verification observes the already-admitted
exchange, while leaving that exchange as the sole secret-delivery path.

The candidate preserves the required boundaries:

- a duplicate exchange cannot claim or receive authority;
- delivery at or after session idle/absolute expiry returns no secret;
- a lost or cancelled response consumes the one-shot authority, and reload or
  replay cannot recover it;
- deadline equality still rejects an unverified bootstrap, and only successful
  authentication advances the idle deadline;
- malformed, unauthenticated, wrong-scope, and replay failures retain generic
  responses and pre-protected-work ordering; and
- TTL constants, routes, response shapes, dependency/lock state, contract,
  evaluation, provider budget, and rollback behavior are unchanged.

The deterministic claimed-exchange regression uses explicit claim/release
barriers rather than task yields or a wall-clock race. It passes on the
candidate; inspection of the pre-remediation loop proves the same controlled
schedule reaches `Expired` and returns no session. Focused delivery, idle,
Tokio-notification, router single-use/revoke, generic-error, and lost-response/
scope tests passed. Host-context CLI and reverse-impact runs each passed
120/120; formatting passed and the impact set contains only
`proof-transport-cli`.

Frozen evidence remained exact: contract
`e040b4a0913490b42a0af6143e35da2d25261ccaf8e67a169b06259e26b67463`,
evaluation `7b91911ad524f5ea303ae939bf5f852e8a83b852814feba834134d298a230634`,
14-check set
`b5ddad83dc550468dd062ca814313e64fd04cb63ffad00a8a2dc7e5391c01889`,
and 26-vector set
`43ad301d7a86e91760961b3d309ff193515029d6c688ba19d816936f1ef10fe8`.
Production-source sentinel scans were empty.

This was readiness evidence only, not 14/14 browser/process evidence or a
release result. No browser, server, product fixture, credential, provider
boundary, external effect, execution, commit, or push was started during the
review.

## D-E0006-015 ceremony result

**E0006-06 result: FAIL / BLOCKED — attachment/tooling blocker.** The
source-remediation review above remains PASS; the single authorized product
ceremony did not establish the frozen same-tab browser evidence.

The current binary was built against a fresh provider-environment-unset
synthetic fixture containing exactly one approval request,
`01a0592b-2223-7743-af05-97b238bcd2d4`, for
`release.publish::v1`, with exact arguments `environment=preview` and
`version_label=2026.08.29-rc1`. One isolated headed session,
`e0006-remediation`, contained one pre-existing New Tab. Async navigation to
the actual listener at `http://127.0.0.1:34989/` was reported scheduled. The
Human used the product page in a regular browser, and the terminal reported
exactly `Local confirmation verified.`

The independent credential-free direct read of the isolated verifier target
remained `chrome://new-tab-page/`, titled `New Tab`. Product selectors and
elements were absent on New Tab, so code clearing, bootstrap hiding, app
visibility, and End Session visibility could not be established. The verifier
therefore could not establish possession of the Human-visible product document
or the required same-tab session, and no 14/14 result can be claimed.
No navigation, reload, click, tab mutation, snapshot, screenshot, credential
read, or credential capture occurred. Evidence from another tab or run was not
combined into a result.

The Human ended the session and confirmed exactly `echo restored`. The
listener was absent and the isolated browser was closed. Pre-disposal CLI
inspection showed that the sole request expired with `decision: null` and
`execution_proof_id: null`; the run remained `waiting_for_input`, the step
remained `waiting_for_approval`, and the event stream ended at
`approval_required`. No resume, tool success, provider access, execution, or
external effect occurred. The exact synthetic-fixture and browser-runtime
directories were deleted after inspection and are not recoverable.

D-E0006-015 ceremony authority is consumed. No retry or source repair is
authorized. E0006-06 is complete and quiescent at **FAIL / BLOCKED
(attachment/tooling blocker)**; E0006-04 and Gate C remain unopened, and
terminal approvals remain the rollback.
