# E0006 Decisions

## D-E0006-001 — P0 secure approval-console candidate

Status: adopted for scaffolding · Date: 2026-08-31 · Decision owner: orchestrator

The authoritative backlog ranks E0006 ahead of E0002 because the current
standalone approval UI prints a reusable Human signing-session bearer in a URL
fragment and saves it to `sessionStorage`. D-E0001-020 leaves E0001 blocked,
quiescent, and writer-free, satisfying E0006's prerequisite for a proposed
edition scaffold.

Three `gpt-5.6-luna` workers independently audited the current server/browser
flow, threat surface, dependencies, tests, and workgraph. They performed no
write, provider call, browser launch, secret read, or private-workspace access.
Their converged findings are preserved in `discovery.md`.

This decision authorizes only proposed edition, contract, evaluation, and
backlog records. It does not approve Gate A, Gate B, a CLI edit, credential
creation, server start, browser session, or signing action.

## D-E0006-002 — Gate A direction proposal

Status: approved by D-E0006-004 · Date: 2026-08-31 · Decision owner: product owner · Proposer: orchestrator

Approve E0006 as the smallest secure prerequisite for the operator-control
roadmap: one local Human opens a clean loopback URL, completes a verified
non-URL handoff, receives one memory-only scoped session, and retains the
existing exact approval review/signing behavior. Success is exact 14/14
deterministic security evaluation plus independent browser/process evidence.

Delivery is four sequential tasks, one security writer at a time, zero paid
provider use, and no kernel/runtime/storage/migration work. E0002 multi-run
control-plane behavior, remote access, persistent sessions, and provider work
are non-goals. Approval authorizes W1 contract completion only; W2 remains
blocked until D-E0006-003 is also approved and E0006-01 becomes `done`.

## D-E0006-003 — Gate B bootstrap/session security proposal

Status: approved by D-E0006-004 · Date: 2026-08-31 · Decision owner: product owner · Proposer: orchestrator

Approve the exact `proof-approval-console-session/v1` contract:

- contract file SHA-256:
  `e040b4a0913490b42a0af6143e35da2d25261ccaf8e67a169b06259e26b67463`;
- evaluation file SHA-256:
  `7b91911ad524f5ea303ae939bf5f852e8a83b852814feba834134d298a230634`;
- ordered check-set SHA-256:
  `b5ddad83dc550468dd062ca814313e64fd04cb63ffad00a8a2dc7e5391c01889`;
  and
- ordered rejection-vector-set SHA-256:
  `43ad301d7a86e91760961b3d309ff193515029d6c688ba19d816936f1ef10fe8`.

Approval binds those exact artifacts and sets. Any later contract, policy,
check, assertion, vector, or expected-outcome change requires a new Gate B
decision and new digests.

1. Bind only to `127.0.0.1` and print one clean `http://127.0.0.1:<port>/`
   URL plus non-secret instructions. No query, fragment, credential, automatic
   browser launch, child argument, or child environment secret is allowed.
2. Support Linux only, matching the trusted-workspace boundary, and require an
   interactive controlling terminal. The browser generates a 64-bit random
   code in page memory; the Human enters it once through a non-echoing prompt.
   The pending code expires after 120 seconds. Mismatch, malformed input, a
   duplicate, timeout, or replay revokes it.
3. The browser opens one bounded long-poll exchange immediately after
   registration. It releases no authority before terminal verification and
   permits exactly one atomic winner for a distinct 256-bit random session.
   Bootstrap and session bytes remain process-memory only and bind to one
   server instance plus the opened workspace.
4. Keep the browser session only in a JavaScript closure. Forbid URL state,
   Web Storage, IndexedDB, cookies, referrers, service workers, logs, screenshots,
   test artifacts, and repository files. Page reload loses the session.
5. Expire the session after 15 minutes absolute or 5 minutes idle, whichever
   occurs first; support explicit revoke; erase in-memory authority at expiry,
   revoke, and shutdown. Use monotonic time for enforcement.
6. Parse fixed-length encodings and compare secret bytes in constant work.
   Reject missing, malformed, or duplicate credential headers before any
   approval query, signing-key access, or decision write.
7. Preserve exact loopback Host checks for every API call and exact Origin plus
   `application/json` checks for every state-changing call. Send no CORS
   allowance and retain/add no-store, no-referrer, frame denial, CSP, and MIME
   sniffing defenses.
8. One mutation authority lease spans final session/deadline validation, a
   fresh registry/governance reload, exact signed request/run/step/argument
   validation, signing-key access, signature creation, and durable decision.
   Revoke/expiry races have exactly one winner. Enrolled/sealed Human binding,
   one-decision semantics, signature shape, and no automatic resume remain.
9. Gate B permits adding only the already-locked `rustix` 1.1.4 package with
   its `termios` feature directly to the CLI crate, plus its exact `Cargo.lock`
   dependency delta. No password helper, signal crate, child `stty`, or other
   package is approved. TTY echo restoration is mandatory on every specified
   completion, error, unwind, SIGINT, and SIGTERM path.

Failure is deliberately fail-closed. A lost exchange response, page reload,
bootstrap mismatch, local race, or unavailable controlling terminal requires
restarting the console; it never restores the fragment bearer. A same-UID
attacker able to read process/browser memory, root compromise, a compromised
Human private key, and local denial of service remain out of scope and must be
reported as limitations rather than hidden.

Approval authorizes only E0006-02 within its exclusive CLI and exact lockfile
paths. Any migration, persistent credential, remote bind, signing/authority
relaxation, new external effect, or E0002 scope requires another Gate B
decision.

## D-E0006-004 — Gate A and Gate B approved

Status: approved · Date: 2026-08-31 · Decision owner: product owner

The product owner responded `approve and proceed` to the explicit request to
approve Gate A and Gate B as recorded in D-E0006-002 and D-E0006-003. This
approves the charter, outcome, zero-provider budget, 14-check evaluation,
non-goals, and exact digest-bound `proof-approval-console-session/v1` security
contract.

E0006-01 is accepted. E0006-02 is authorized as the sole active edition writer
within `crates/proof-transport-cli/**`, its unique handoff, and only the exact
approved `rustix` 1.1.4 `termios` lockfile delta. All D-E0006-003 stop
conditions remain binding. This decision does not approve Gate C, public
release, provider work, remote access, persistent credentials, or E0002
implementation.

## D-E0006-005 — E0006-02 accepted for independent verification

Status: adopted · Date: 2026-08-31 · Decision owner: orchestrator

The sole E0006-02 security writer completed the approved CLI-only change. The
first distinct source review rejected six edge cases; the writer corrected the
exchange ordering, revoke-confirmation UX, TTY restoration/shutdown ordering,
generic extractor failures, exact terminal parsing/EOT handling, and actual
decision-versus-revoke/expiry race evidence. A second review found and closed
one pre-bootstrap shutdown hang. The final distinct source/contract review
reports PASS with no remaining release blocker.

The orchestrator reproduced `rtk cargo fmt --check -p proof-transport-cli`,
the CLI-only impact set, scoped diff checks, and 117/117 host-context CLI tests.
E0006-02 is quiescent and accepted. E0006-03 is assigned to a fresh non-author
verifier for the exact frozen browser/process evaluation. This is not Gate C
approval and authorizes no source repair, provider call, private workspace, or
credential-bearing evidence.

## D-E0006-006 — E0006-03 stopped at the Human-presence boundary

Status: blocked · Date: 2026-08-31 · Decision owner: orchestrator

The distinct E0006-03 verifier reproduced the frozen digests, 117/117 host CLI
tests, formatting, CLI-only impact set, clean loopback launch, real pre-session
browser surfaces, response headers, no-CORS behavior, and the deterministic
26-vector suite in a fresh provider-free synthetic workspace. It did not claim
14/14.

The live exchange requires a Human to read the ephemeral browser value visually
and type it directly into the controlling terminal. Available agent interfaces
could expose the value only through retained tool output or a screenshot, or
could relay it programmatically through a prohibited clipboard, file, pipe,
argument, or helper. The verifier correctly refused those workarounds. No
session, decision, execution, resume, credential capture, or retained workspace
was produced; browser/server cleanup and the restoration boundary completed.

E0006-03 is quiescent and blocked. E0006-04, the quiescent final verifier, and
Gate C must not start. The bounded recovery is a new E0006-03 run with one
co-located authorized Human who directly observes the browser and types into
the exact PTY without communicating or retaining the value. This does not
authorize a source change, contract relaxation, or automated secret relay.

## D-E0006-007 — Human ceremony passed; attachable-browser evidence remains blocked

Status: blocked · Date: 2026-08-31 · Decision owner: orchestrator

An authorized co-located Human completed a second fresh provider-free ceremony
without communicating or retaining the bootstrap or session credential. The
browser code cleared after direct entry into the controlling terminal. The
Human reviewed the exact synthetic `release.publish::v1` request, signed one
intentional denial, explicitly ended the session, stopped the server with
Ctrl+C, and confirmed `echo restored` in the same terminal.

The distinct verifier established the valid request and Human signatures,
exact canonical arguments, one denial, null execution proof, no run resume or
tool success, IPv4-loopback-only listener, clean argv, required response
headers, explicit revoke, listener shutdown, and fixture disposal. No provider,
external effect, credential-bearing capture, private key, database copy, or
retained synthetic workspace was used.

This materially closes the Human-handoff, live signing, revoke, shutdown, and
TTY-restoration evidence gaps, but it does not establish 14/14. The successful
page ran in a normal browser that was not attachable to the required named
agent-browser session, so current-session post-bootstrap URL/history/storage/
cookie/referrer/console/resource/screenshot evidence and the browser side of
the actual-session artifact scan remain unavailable. E0006-03 is therefore
blocked and quiescent, E0006-04 remains pending, and Gate C is not requested.
The bounded recovery is one more fresh ceremony on a GUI-enabled same-host
session where the co-located Human can see the headed named agent-browser and
the independent verifier retains attachment only after the code clears.

## D-E0006-008 — Final headed attachable-browser rerun authorized

Status: active · Date: 2026-08-31 · Decision owner: product owner / orchestrator

The product owner responded `approve. proceed` to the bounded recovery in
D-E0006-007. A credential-free probe then proved that the host's real X display
can launch a headed named agent-browser when its daemon runtime is placed in a
private temporary directory outside the restricted sandbox. The blank probe
was closed without opening a product workspace or generating a credential.

E0006-03 is reactivated for one fresh provider-free synthetic ceremony. The
authorized Human may visually read the code and type it directly into the
controlling TTY; the independent verifier may inspect the same browser only
after the code clears. All D-E0006-003 credential, capture, scope, and
fail-closed rules remain binding. This does not authorize E0006-04, Gate C,
provider use, source repair, contract relaxation, commit, or push.

## D-E0006-009 — Final headed attempt failed closed at approval expiry

Status: blocked · Date: 2026-08-31 · Decision owner: orchestrator

The D-E0006-008 run successfully used the same visible headed named
agent-browser through direct Human terminal confirmation and independent
post-bootstrap inspection. The clean URL/history classification, empty browser
persistence/referrer/console/error surfaces, same-origin resources, rendered
request bindings, and permitted credential-free screenshots all passed.

The Human attempted exactly one synthetic denial, but the browser returned the
generic `request rejected` result and the durable decision remained null as the
approval reached its expiry boundary. No second decision was attempted. End
Session then could not confirm explicit revoke and disabled approval actions as
designed. Controlled Ctrl+C shutdown erased in-memory authority, closed the
listener, and the same terminal reported `echo restored`. Final durable state
was one expired request with null decision and execution proof, a waiting run
and step, and no resume, tool success, proof, evaluation, provider call, or
external effect.

This attempt proves the previously missing headed current-session browser
capability, but D-E0006-008 authorized only one fresh ceremony and its required
same-session decision/revoke evidence failed. E0006-03 is blocked and
quiescent; E0006-04 and Gate C remain unopened. No E0006-02 source or contract
repair is authorized or indicated. A further fresh run requires an explicit
owner decision and must be time-boxed so handoff, browser checks, decision,
revoke, and shutdown all complete inside the fixed request lifetime.

## D-E0006-010 — One five-minute E0006-03 rerun authorized

Status: active · Date: 2026-08-31 · Decision owner: product owner

The product owner explicitly authorized `one new time-boxed E0006-03 run`.
This authorizes exactly one additional fresh provider-free synthetic ceremony
under the unchanged D-E0006-003 contract. It does not authorize a source,
contract, lifetime, authority, provider, external-effect, commit, or push
change.

The operational order is bounded to avoid the prior expiry: start a fresh
request, open it in the existing headed named agent-browser pattern, complete
the direct Human browser-to-TTY handoff, and submit one synthetic denial
immediately after source-ordered code clearing. The distinct verifier then
inspects the still-open same browser, durable decision, zero execution/resume,
explicit revoke, shutdown, and TTY restoration. The target is five minutes
from fixture creation through revoke. Any failed boundary stops the run without
retry or repair.

## D-E0006-011 — Five-minute product path passed; same-tab evidence blocked

Status: blocked · Date: 2026-08-31 · Decision owner: orchestrator

The D-E0006-010 run completed the direct Human browser-to-TTY handoff and
persisted exactly one signed synthetic denial inside the approval window. The
decision bound the expected request, enrolled Human, digest, and two canonical
arguments. The Human did not run the displayed resume command, explicitly
ended the browser session, stopped the server, and confirmed `echo restored`.
Independent durable evidence showed null execution proof, no execution record,
a waiting run and step, and events ending at `approval_required`, with no
resume, tool success, proof, evaluation, provider call, or external effect.

The frozen 14/14 result still cannot be claimed. The verifier's attachment to
the named `e0006-manual` automation session exposed a distinct fresh
pre-bootstrap tab; its tab list never exposed the Human-visible post-decision
tab. The verifier therefore did not capture or infer that tab's screenshot,
URL/history, storage, cookie, referrer, console, service-worker, or resource
state and did not combine prior-run browser evidence with this run.

E0006-03 is blocked and quiescent; E0006-04 and Gate C remain unopened. This is
a browser-attachment/evidence-channel blocker, not an E0006-02 product-source
failure. D-E0006-010's one-run authority is consumed. Before any further
product ceremony is proposed, a credential-free probe must establish a
deterministic one-visible-tab/CDP attachment path. No retry, source repair,
contract relaxation, commit, or push is authorized by this decision.

## D-E0006-012 — Credential-free cross-agent single-tab attachment passed

Status: adopted · Date: 2026-08-31 · Decision owner: orchestrator

The required diagnostic used one credential-free loopback page, one visible
headed named agent-browser session, and one tab. The existing blank tab was
navigated asynchronously with `location.assign` while the page held an
unresolved long-poll. The orchestrator and distinct E0006-03 verifier then
independently attached to that same named session. Each direct read returned
the exact live loopback URL, title `E0006 attachment probe`, body marker
`Credential-free single-tab attachment probe`, and state `pending`; inventory
remained exactly one active session and one tab.

The probe established that `tab list` may retain stale initialization metadata
(`about:blank`) after the current document has navigated. Direct URL, title,
body, and evaluation reads are therefore authoritative for same-tab evidence;
the inventory label is not. Initializing one blank headed named tab and then
asynchronously navigating that existing tab is the deterministic pattern for
any later E0006-03 ceremony. It avoids the direct `open <URL>` navigation path
that timed out during the long-poll and diverged from the Human-visible tab in
the previous run.

The probe browser, listener, isolated runtime, and exact temporary files were
closed or removed. It used no product fixture, credential, private workspace,
decision, provider, or external effect. D-E0006-010's product-run authority
remains consumed; E0006-03 stays blocked and quiescent, and E0006-04 and Gate C
remain unopened. A final product ceremony requires a new explicit owner
authorization and must produce the product journey plus same-tab evidence in
one run. This decision authorizes no retry, source change, contract relaxation,
commit, or push.

## D-E0006-013 — One final single-tab E0006-03 run authorized

Status: active · Date: 2026-08-31 · Decision owner: product owner

The product owner explicitly responded `authorize one final single-tab
E0006-03 run`. This authorizes exactly one fresh provider-free synthetic
ceremony under the unchanged frozen contract, policy, and D-E0006-003 security
boundaries. D-E0006-012 supplies the required credential-free attachment
diagnostic; it does not supply product evidence.

The run must initialize exactly one visible blank tab in one headed named
agent-browser session, then asynchronously navigate that existing tab to the
fresh clean loopback URL. Direct current URL, title, body, and evaluation reads
are authoritative; stale `tab list` initialization metadata must not trigger a
second tab or navigation. The verifier may inspect the tab only after the
Human enters the displayed bootstrap directly into the controlling non-echoing
terminal and source-ordered code clearing is visible. No bootstrap or session
value may be extracted, printed, messaged, copied, captured, or persisted.

The Human must sign exactly one synthetic denial immediately, never run the
displayed resume command, and explicitly end the session. The independent
verifier must establish the same-tab browser aggregate and credential-free
screenshot, exact decision binding, zero execution/resume, explicit revoke,
controlled shutdown, listener absence, same-TTY restoration, and exact fixture
disposal in this one run. Any failed boundary stops without retry or repair.

This decision authorizes no approval, execution, provider use, external effect,
source or contract change, lifetime extension, E0006-04, Gate C, commit, or
push. After this single run, further product authority is consumed regardless
of outcome.

## D-E0006-014 — Final current tab failed closed; authority consumed

Status: blocked · Date: 2026-08-31 · Decision owner: orchestrator

The D-E0006-013 run used one fresh provider-free synthetic fixture and one
visible headed named agent-browser session with one asynchronously navigated
tab. The server process argv was credential-free and its actual ephemeral
listener was IPv4 loopback `127.0.0.1:35953`; the planned fixed port `43123`
was unused. The Human visually entered the browser code into the controlling
terminal and reported exact `Local confirmation verified.`

Direct post-confirmation reads by the orchestrator and distinct verifier
reached the same clean loopback URL and title, with one named session and one
tab. The current document nevertheless retained a visible bootstrap panel with
empty code, hidden app/session controls, one same-origin resource, and generic
`request rejected. Restart the console to try again.` Tab-list initialization
metadata remained stale. The verifier correctly took no screenshot, and no
decision was attempted.

Independent source classification establishes that terminal verification means
the pending code matched before the terminal deadline; it does not establish
that a session was minted or delivered to the current tab. The shared browser
failure path cannot distinguish bootstrap registration from exchange rejection.
A same-document deadline/equality failure, a replaced or reloaded document in
the same tab, or a competing exchange can produce this state. The evidence
therefore proves a fail-closed current tab and inconclusive prior exchange
ownership, not a second browser, compromise, or successful session.

The Human stopped the server and confirmed `echo restored`. Final durable state
was exactly one pending signed request with null decision and execution proof,
a `waiting_for_input` run, a `waiting_for_approval` step, and events ending at
`approval_required`, with no resume, tool success, proof, evaluation, provider,
or external effect. Browser/process/listener cleanup completed, and the isolated
runtime and exact disposable fixture were removed.

D-E0006-013's final one-run authority is consumed. E0006-03 is blocked and
quiescent; E0006-04 and Gate C remain unopened. This decision authorizes no
retry, source or contract change, lifetime extension, E0002 work, commit, or
push.

## D-E0006-015 — Bounded delivery remediation and one post-review ceremony authorized

Status: active · Date: 2026-08-31 · Decision owner: product owner

After reviewing the blocked disposition and the orchestrator's exact proposed
next step, the product owner responded `proceed`. This authorizes a bounded
E0006 remediation wave to diagnose and fix the current-tab bootstrap/session
exchange failure, preserve the frozen security contract, add regression
evidence, and permit one final 14/14 verification ceremony after independent
review. It authorizes no provider use or external effect.

E0006-05 is the only source-writing task and retains the named E0006 security
owner's exclusive `proof-transport-cli` boundary. It must establish a
deterministic cause and may correct only the legitimate single-document
delivery path. The one-use state machine, terminal verification, generic error
boundary, reload/lost-response failure, TTLs, loopback/request boundaries,
signing lease, and all frozen contract/evaluation digests remain binding. Any
need to relax one of those rules stops for a new owner decision.

E0006-06 remains a distinct non-author. It must pass source/test review before
starting exactly one fresh provider-free synthetic ceremony. That ceremony
must produce the complete same-tab, same-session 14/14 browser/process/decision/
revoke result; any failed boundary consumes its authority without retry and
prior-run evidence may not be combined into a PASS. This decision does not
authorize E0006-04, Gate C self-approval, E0002 implementation, commit, or push.

## D-E0006-016 — Post-remediation attachment failed; authority consumed

Status: blocked · Date: 2026-08-31 · Decision owner: orchestrator

E0006-05 completed the bounded source remediation and a distinct non-author
accepted its deterministic cause, frozen-contract preservation, and 120/120
host-context scoped result. The one D-E0006-015-authorized ceremony then used
the current binary, a fresh provider-free synthetic fixture, one loopback
listener, and one isolated headed named browser session initialized with one
blank tab. The fixture contained one `release.publish::v1` request for exact
arguments `preview` / `2026.08.29-rc1` and no decision or execution.

Asynchronous navigation to the actual `127.0.0.1:34989` listener was reported
scheduled, and the Human reported exact `Local confirmation verified.` after
entering the displayed code in the controlling terminal. Direct credential-
free reads by the orchestrator and verifier nevertheless found the isolated
target still at `chrome://new-tab-page/`, titled `New Tab`. The Human-visible
product page was in a regular browser, not that isolated target. The required
same-visible-tab product attachment therefore could not be established. The
verifier stopped without a screenshot, decision, reload, second navigation, or
attempt to combine evidence.

The Human ended the product session and confirmed terminal echo restoration.
The listener was absent and the isolated browser was closed. Final durable
inspection showed the sole request expired with null decision and execution,
the run still `waiting_for_input`, the step still `waiting_for_approval`, and
events ending at `approval_required`; there was no resume, tool success,
provider use, or external effect. The exact private fixture and browser runtime
were then permanently removed.

This is an attachment/evidence-channel blocker, not a rejection of the accepted
E0006-05 source/test remediation. D-E0006-015's one ceremony is consumed.
E0006-06 is blocked and quiescent; E0006-04 and Gate C remain unopened. This
decision authorizes no retry, repair, E0002 implementation, commit, or push.

## D-E0006-017 — Credential-free launcher-first attachment diagnostic passed

Status: adopted · Date: 2026-09-01 · Decision owner: orchestrator

After the product owner directed `let proceed`, the orchestrator bounded the
work to a credential-free investigation of the D-E0006-016 attachment blocker.
The scope explicitly excluded a product fixture or ceremony, credential, Human
TTY handoff, source change, provider, execution, external effect, E0002 work,
commit, and push.

Installed `agent-browser 0.19.0` rejects `open about:blank` because its `open`
command requires a hostname. That leaves Chrome's privileged New Tab document
as the isolated target; D-E0006-015 then scheduled navigation from that document
while the Human opened the product in a regular browser. The diagnostic instead
opened one fast credential-free IPv4-loopback launcher in one isolated headed
session. Although `open` itself returned a `Page.navigate` timeout, direct URL,
title, body, console, overlay, snapshot, and visual checks proved the launcher
was live.

The existing launcher document then scheduled `location.assign` to a second
credential-free page holding a pending request. Direct reads from later CLI
processes reached the exact target URL and title, marker `Credential-free
single-tab attachment probe`, and state `pending`, with no console or overlay
error. Inventory remained exactly one tab but retained the stale label `New Tab
- chrome://newtab/`. This reproduces and resolves the evidence-channel class:
direct current-document reads are authoritative; initialization inventory is
not.

The browser closed, the fixed `127.0.0.1:43125` listener stopped, listener
absence was confirmed, and the exact transient server/browser directories and
credential-free screenshots were permanently removed. No product or credential
state existed.

E0006-07 is done and quiescent. E0006-08 is scaffolded as a launcher-first exact
ceremony but remains pending and non-dispatchable. D-E0006-015 stays consumed;
this decision does not authorize E0006-08, E0006-04, Gate C, or E0002.

## D-E0006-018 — One launcher-first E0006-08 ceremony authorized

Status: consumed by D-E0006-019 · Date: 2026-09-01 · Decision owner: product owner

The orchestrator presented the exact authorization: `Authorize exactly one
launcher-first, provider-free E0006-08 synthetic ceremony using one preverified
visible headed tab, one synthetic denial, zero execution/resume/external effect,
and no retry or source repair.` The product owner responded `proceed`.

This activates only E0006-08 under the unchanged frozen contract, evaluation,
14-check digest, and 26-vector digest. The credential-free launcher must be
created, directly verified by orchestrator and distinct verifier, and visibly
confirmed by the Human before any product fixture or approval lifetime begins.
The existing launcher document must then navigate asynchronously to the clean
product URL; the Human uses that same headed window and must not open the URL in
a regular browser.

No bootstrap or session value may be extracted, printed, copied, messaged,
captured, or persisted. Exactly one synthetic denial is permitted after the
source-ordered app checks. No approval, execution, resume, provider use,
external effect, second tab/session, retry, repair, lifetime extension, E0006-04,
Gate C self-approval, E0002 work, commit, or push is authorized. Any failed
boundary consumes this one authority and stops the run.

## D-E0006-019 — E0006-08 denial-only ceremony failed; authority consumed

Status: blocked · Date: 2026-09-01 · Decision owner: orchestrator

The single D-E0006-018-authorized run began with a provider-free,
credential-free launcher in one isolated headed named session. The Human
visually confirmed the launcher, and a distinct non-author independently
verified the same live document through direct URL, title, marker, state,
console, overlay, and one-tab reads. That preflight passed but supplies no
product evidence by itself.

After same-tab navigation to the fresh product listener, durable inspection
found one signed **approved** decision for the synthetic
`release.publish::v1` request, with reason `approve`, instead of the sole
authorized denial. The approval-console source permits that outcome only
through an authenticated decision submission; the available retained evidence
does not establish which same-session interaction initiated it. The denial-only
acceptance contract is therefore violated, and neither the launcher preflight
nor historical runs may be combined into a 14/14 result.

Containment completed without exercising the approved authority. The request
has `execution_proof_id: null`; the run remains `waiting_for_input`; the
step remains `waiting_for_approval`; events end at `approval_required`; and
tokens, cost, resume, tool success, provider use, execution, evaluation, proof,
and external effect are all zero or absent. The controlled browser and exact
product PID stopped, both loopback listeners disappeared, and the Human
confirmed exact same-terminal `echo restored`.

The exact disposable product fixture, isolated browser runtime, and launcher
directory were then permanently removed, and independent absence checks passed.

D-E0006-018 is consumed. E0006-08 is blocked and quiescent. No retry, source
repair, execution, resume, E0006-04, Gate C, E0002 work, commit, or push is
authorized by this decision. A new bounded owner decision is required before
any further product ceremony or remediation.

## D-E0006-020 — Bounded Human-intent confirmation remediation authorized

Status: approved · Date: 2026-09-01 · Decision owner: product owner

After reviewing the contained E0006-08 result, the product owner directed:
`either fix this or we move on`. Because E0002 depends on the secure approval
primitive, the orchestrator selected the bounded fix and explicitly limited it
to the browser's Human-intent confirmation surface.

E0006-09 may change only `proof-transport-cli` UI source and tests. The first
decision click must open an in-document confirmation bound to the exact request
and selected outcome. Final submission stays disabled until the Human enters
the exact case-sensitive phrase `DENY <request-id>` or
`APPROVE <request-id>`. The confirmation freezes request selection,
approver, reason, and outcome; cancellation, expiry, actionability loss, revoke,
reload, or session loss clears it without a POST. Native `confirm()` is
removed, approval receives danger styling, and action labels become explicit.

This is a Human-error guard, not a new authentication or authorization
boundary. It does not change the decision request schema or server handler,
backend actionability, authority lease, signatures, durable outcomes, session
state, credential transport/lifetimes, execution/resume behavior, dependencies,
or provider budget. The four frozen contract/evaluation hashes remain binding
and must not change.

E0006-10 is a distinct non-author source/test review with no product runtime.
E0006-11 is a non-dispatchable future ceremony packet and requires a separate
exact owner authorization after E0006-10 PASS. This decision authorizes no
ceremony, approval, execution/resume, provider, external effect, E0006-04,
Gate C, E0002 implementation, commit, or push.

## D-E0006-021 — E0006-09 accepted for independent source/test review

Status: adopted · Date: 2026-09-01 · Decision owner: orchestrator

The named CLI security owner completed the D-E0006-020 UI-only remediation.
The initial actions are explicit, approval uses danger styling, and the native
confirmation dialog is replaced by an accessible in-document region displaying
the exact outcome, operation/version, request ID, and exact case-sensitive
challenge. Request, outcome, approver, and reason are captured in one frozen
page-memory intent; selection and form inputs remain disabled; a fresh protected
detail read is checked before the unchanged decision POST; and cancellation,
expiry, actionability/session failure, revoke, or reload clears intent without a
known pre-POST write.

The security owner and orchestrator each reproduced the focused 7/7 embedded-UI
tests, JavaScript parsing, formatting, and clean diff checks. The owner also
passed both host CLI and scoped-impact suites at 124/124. Source inspection and
path checks show no backend decision handler/schema, authority, signing,
session, execution/resume, dependency, contract, evaluation, manifest, or
lockfile change.

E0006-09 is done and quiescent. E0006-10 is activated for the distinct
non-author source/test review only. No product fixture, bootstrap/session,
decision, ceremony, provider, external effect, E0006-11, E0006-04, Gate C,
E0002 implementation, commit, or push is authorized.

## D-E0006-022 — Intent guard independently accepted; ceremony remains gated

Status: adopted · Date: 2026-09-01 · Decision owner: orchestrator

The distinct non-author reviewed the exact E0006-09 delta and accepted every
D-E0006-020 boundary. The review verified explicit initial/final action
separation, request/outcome challenge construction, approval danger styling,
accessible labels/focus, frozen request/outcome/operation/approver/reason,
disabled selection/form controls, strict case-sensitive equality, no
form/default submit/Enter/native confirmation/persistence, protected fresh
detail revalidation, pre-POST clearing paths, and conservative uncertainty
after a decision POST begins.

Independent reproduction passed 7/7 focused embedded-UI tests, JavaScript
parsing, formatting, host CLI 124/124, scoped-impact 124/124 with a CLI-only
impact set, persistence/native-confirm scans, and scoped diff checks. The
contract, evaluation, 14-check set, and 26-vector set retain their exact frozen
hashes. No backend handler/schema, authority lease, signing, session,
execution/resume, dependency, manifest, or lockfile drift exists.

E0006-09 and E0006-10 are done and quiescent. The Human-intent defect class is
fixed and independently verified. E0006-11 remains pending and
non-dispatchable: source/test acceptance does not authorize a product fixture,
bootstrap/session, decision, provider, external effect, or ceremony. E0006-04,
Gate C, and E0002 remain closed until a future exact owner decision and
successful E0006-11 result. No commit or push is authorized by this decision.

The orchestrator's final host-context scoped rerun passed 124/124. Formatting,
CLI-only impact listing, edition validation, frozen file hashes, and diff checks
also passed. A preceding restricted-sandbox attempt failed only on known
trusted-ancestor/PTY permission boundaries and is not product evidence.

## D-E0006-023 — One final intent-bound E0006-11 ceremony authorized

Status: active · Date: 2026-09-01 · Decision owner: product owner

After E0006-10 independently accepted the Human-intent guard, the product owner
authorized: `Authorize exactly one intent-bound, launcher-first, provider-free
E0006-11 synthetic ceremony using one preverified visible headed tab, one
explicit denial challenge, one durable denial, zero approval/execution/resume/
external effect, and no retry or source repair.`

This activates only E0006-11 under the unchanged frozen contract, evaluation,
14-check digest, and 26-vector digest. One credential-free launcher must be
created and directly preverified before product state exists. The same one
visible headed tab must then navigate to one fresh disposable synthetic
workspace containing exactly one `release.publish::v1` request for `preview` /
`2026.08.29-rc1`. The Human alone reads the browser-generated code and enters it
directly into the controlling terminal, then reviews the exact request, enters
the exact case-sensitive `DENY <request-id>` challenge, and signs exactly one
denial with reason `E0006 synthetic verification — deny; no execution
authorized.`

No bootstrap or session value may be extracted, printed, copied, messaged,
captured, persisted, or placed in retained evidence. No approval, execution,
resume, provider call, external effect, second tab/session, retry, source
repair, lifetime extension, E0006-04, Gate C self-approval, E0002 work, commit,
or push is authorized. Any failed launcher, Human handoff, session, intent,
decision, revoke, evidence, listener, browser, cleanup, or TTY boundary consumes
this one authority and stops the run.

## D-E0006-024 — E0006-11 launcher gate failed; authority consumed

Status: blocked · Date: 2026-09-01 · Decision owner: orchestrator

The credential-free launcher bound only to `127.0.0.1:43127` and served one
GET 200. The authorized headed `agent-browser` open nevertheless timed out.
The orchestrator and distinct non-author attached read-only to the same named
session and independently found exactly one session and one tab whose
authoritative current URL remained `about:blank`, with empty title/body and no
launcher marker or verified state. Overlay, console, and page-error surfaces
were clean, but they cannot substitute for the missing launcher document. No
screenshot was taken.

The fail-closed launcher gate stopped before any product fixture, port 43128
listener, bootstrap/session, approval request, Human TTY handoff, intent
challenge, decision, provider call, execution/resume, proof, or external effect
existed. The browser closed and launcher stopped. Independent checks found both
ports absent, no active browser session or Proof process, and both exact
transient directories absent after authorized disposal.

D-E0006-023 is consumed. E0006-11 is blocked and quiescent; no retry, source
repair, product ceremony, E0006-04, Gate C self-approval, E0002 implementation,
commit, or push is authorized. Exact 14/14 remains unavailable.

## D-E0006-025 — Gate C deferred; E0002 Gate A scaffolding exception

Status: adopted · Date: 2026-09-01 · Decision owner: product owner

The product owner directed: `Defer AXP-E0006 Gate C, keep the standalone
approval UI unreleased with terminal approve/deny as rollback, and authorize
AXP-E0002 Gate A scaffolding only. E0002 must define independent scoped
operator authentication and may not claim or reuse E0006 as released.`

AXP-E0006 is therefore blocked and quiescent with a dated Gate C defer/no-go,
not release acceptance. `proof approval ui` remains an unreleased path. The
supported rollback is the existing terminal `approval approve` and `approval
deny` commands; the historical fragment/Web Storage bearer remains prohibited.
No E0006 retry, repair, ceremony, release claim, or evidence waiver is approved.

As an explicit roadmap exception, AXP-E0002 may create only its Gate A planning
scaffold: charter, dependency-safe workgraph, disjoint future ownership,
assignments, task/handoff placeholders, status, decisions, evidence plan,
retrospective placeholder, and owner-ready Gate A/Gate B proposal. Its status
must remain proposed/review and every implementation task non-dispatchable.
E0002 must specify an independent Human-bound, workspace/instance-bound,
capability-scoped operator authentication contract. E0006 may be a read-only
source of lessons, but E0002 may not describe it as released, inherit its
acceptance evidence, reuse its session contract as release authority, or widen
its session implicitly.

This decision does not approve E0002 Gate A or Gate B, a public contract,
migration, shared type, dependency, source implementation, provider call,
browser/server runtime, external effect, destructive action, commit, or push.
