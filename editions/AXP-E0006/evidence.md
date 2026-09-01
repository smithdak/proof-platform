# E0006 Evaluation Evidence

- Edition: `AXP-E0006`
- Base revision: `d76f44f960d905398a266fa8858562ad15fb2366`
- Contract: proposed `proof-approval-console-session/v1`; SHA-256
  `e040b4a0913490b42a0af6143e35da2d25261ccaf8e67a169b06259e26b67463`
- Evaluation/policy: proposed `proof-approval-console-security/v1`; SHA-256
  `7b91911ad524f5ea303ae939bf5f852e8a83b852814feba834134d298a230634`
- Frozen ordered set digests: checks
  `b5ddad83dc550468dd062ca814313e64fd04cb63ffad00a8a2dc7e5391c01889`;
  rejection vectors
  `43ad301d7a86e91760961b3d309ff193515029d6c688ba19d816936f1ef10fe8`
- Provider/model configuration: none; zero provider use and zero live spend
- Environment: disposable synthetic workspaces, loopback browser, and real
  controlling TTY; all fixtures removed after redacted inspection
- Evaluator: distinct non-author E0006-03/E0006-06/E0006-08/E0006-10/E0006-11
  verifier plus one authorized co-located Human for the non-communicated
  browser-to-TTY handoff
- Result: `blocked and quiescent — Gate C deferred/no-go in D-E0006-025`

## Acceptance checks

| Check | Command/input | Expected | Actual | Result |
|---|---|---|---|---|
| E0001 dependency | D-E0001-020 and E0001 status | prior edition blocked, quiescent, writer-free | owner defer recorded; host quiescent gate 614/614 | passed |
| Read-only discovery | three bounded audits | exact threat/source/test/workgraph basis; zero effects | converged report in `discovery.md`; no writes/provider/browser/secrets | passed |
| Edition structure | `rtk scripts/swarm.sh validate AXP-E0006` | complete artifact/task/handoff graph with disjoint paths | D-E0006-015 extends the valid graph with single-owner E0006-05 remediation and distinct E0006-06 verification | passed |
| Proposed contract | `contracts/approval-console-session.md` | exact threat model, states, limits, routes, failure ordering, rollback | frozen digest; independent corrected-packet review PASS | passed |
| Proposed evaluation | `evals/approval-console-security-v1.json` | 14 ordered required checks with normative assertions plus 26 ordered rejection vectors and frozen set digests | frozen digest; mechanical validation and independent corrected-packet review PASS | passed |
| Gate A | D-E0006-002/D-E0006-004 | dated owner direction approval | approved 2026-08-31 | passed |
| Gate B | D-E0006-003/D-E0006-004 | dated owner security/public-contract approval | approved 2026-08-31 with exact digests | passed |
| CLI implementation | E0006-02 handoff and scoped impact | exact approved behavior and all focused regressions | 117/117 host tests; formatting/impact/diff clean; independent remediated source review PASS | passed |
| Gate B remediation | D-E0006-015 | bounded source/test correction plus one post-review ceremony under frozen contract | owner responded `proceed` to the exact remediation authorization; no provider or external effect | passed |
| Delivery remediation | E0006-05 handoff and scoped impact | deterministic cause, legitimate single-document correction, reload/replay/lost-response still closed | split deadline linearization corrected atomically; worker and orchestrator host scoped runs each passed 120/120; formatting/impact/diff clean | passed |
| Independent remediation review | E0006-06 handoff and redacted dogfood record | non-author accepts source, deterministic regression, error/expiry boundaries, and frozen digests before ceremony | source/test review PASS; focused and host-context evidence accepted; contract/evaluation digests unchanged | passed |
| Post-remediation ceremony | D-E0006-015 / D-E0006-016 | one same-tab, same-session exact 14/14 result | terminal handoff verified, but isolated verifier target remained `chrome://new-tab-page/` while the Human-visible product was in a regular browser; stopped before app/decision/revoke evidence, with null decision/execution and zero resume/effect | failed / blocked |
| Launcher-first attachment diagnostic | E0006-07 / D-E0006-017 | one credential-free headed tab navigates from an ordinary launcher document to a pending target and remains directly attachable | direct URL/title/body/state reads reached the pending target; console/overlay/visual checks passed; inventory stayed one tab with stale New Tab metadata; exact transient state removed | passed |
| Launcher-first product ceremony | E0006-08 / D-E0006-018 / D-E0006-019 | exact same-visible-tab 14/14 result under one-run owner authority | credential-free launcher and independent direct-read preflight passed; the product run then persisted an unexpected signed approval with reason `approve` instead of the required denial; execution proof remained null and no resume/provider/effect followed | failed / blocked |
| Human-intent remediation | E0006-09 / D-E0006-020 / D-E0006-021 | exact request/outcome challenge prevents accidental final POST until explicitly matched; backend and frozen artifacts unchanged | implementation and root review pass; focused 7/7, JavaScript parse, formatting, host and scoped 124/124; no backend/frozen-artifact diff | passed |
| Independent intent review | E0006-10 / D-E0006-022 | non-author source/test PASS with no product runtime | exact diff review PASS; focused 7/7, JavaScript parse, formatting, host/scoped 124/124, CLI-only impact, frozen hashes exact | passed |
| Intent-bound product ceremony | E0006-11 / D-E0006-023 / D-E0006-024 | one authorized launcher-first exact 14/14 denial journey | launcher served GET 200, but the sole current tab remained empty `about:blank`; stopped before product state and cleaned fully | failed / blocked |
| Independent verification | E0006-03 handoff and redacted dogfood record | exact 14/14 plus browser/process/security PASS | pre-session checks and all 26 deterministic vectors passed; prior runs separately passed headed browser secrecy/capture and the Human denial/revoke path; the final D-E0006-013 run reached terminal verification, but the current attached tab retained a cleared bootstrap surface, hidden app, and generic rejection, so no session-possession, screenshot, decision, or execution evidence could be claimed | failed / blocked |
| Final gate | `rtk scripts/swarm.sh verify AXP-E0006 --quiescent` | edition valid, formatting and workspace suite pass | not run and not claimed; E0006-11 failed at the launcher gate, exact 14/14 is absent, and D-E0006-025 deferred Gate C | blocked |
| Owner release | Gate C decision | dated accept/defer/reject | product owner deferred/no-go on 2026-09-01 in D-E0006-025; E0006 is not released | deferred / no-go |

## Credential-free attachment diagnostic

After the failed five-minute attachment, one credential-free loopback page was
opened in exactly one visible headed named session and held an unresolved
long-poll. The existing blank tab navigated asynchronously with
`location.assign`; no second product tab or credential was created. The
orchestrator and distinct verifier independently attached to the same named
session and each read the exact live URL, title, body marker, and `pending`
state. Inventory remained one active session and one tab.

The diagnostic also established that `tab list` can retain the tab's initial
`about:blank` label after navigation. It is therefore not evidence of the live
document. Direct URL/title/body reads reached the current page and are the
required attachment checks for any later run. The headed browser, loopback
listener, isolated browser runtime, and exact temporary fixture were then
closed or removed. This probe used no product workspace, bootstrap, session,
decision, provider, or external effect and does not authorize or substitute for
another E0006-03 ceremony.

## D-E0006-013 final single-tab run

The owner authorized exactly one final provider-free product ceremony. A fresh
private fixture contained one pending `release.publish::v1` request for exact
arguments `preview` / `2026.08.29-rc1`, null decision, and null execution proof.
One visible headed named session began with one blank tab and asynchronously
navigated that tab to the clean IPv4-loopback server. The server used a
credential-free process argv and listened only on `127.0.0.1:35953`.

The Human visually entered the browser code into the controlling non-echoing
terminal and reported exact `Local confirmation verified.` Direct attachment
by the orchestrator and distinct verifier reached the same clean URL and title,
but the current document retained a visible bootstrap panel with empty code,
hidden approval app/session controls, one same-origin resource, and generic
`request rejected. Restart the console to try again.` The tab-list label stayed
at stale initialization metadata. The verifier took no screenshot and neither
agent claimed approval details, current-tab session possession, or the browser
aggregate required by the frozen evaluation.

Source ordering establishes only that terminal handoff succeeded while
session establishment or ownership remains inconclusive. The observed current
tab failed closed. This outcome does not prove a second browser, competing
exchange, compromise, or successful session; a single replaced/reloaded
document or a deadline/equality failure can also produce the observed generic
rejection.

The Human made no decision, stopped the server, and confirmed exact `echo
restored`. Final durable reads showed one pending request, null decision and
execution, run `waiting_for_input`, step `waiting_for_approval`, and events
ending at `approval_required`, with no resume, tool success, proof, evaluation,
provider, or external effect. The browser, process, listeners, isolated runtime,
and exact disposable fixture were removed. D-E0006-013 authority is consumed.

## D-E0006-015 post-remediation ceremony

The bounded E0006-05 correction passed independent source/test review under the
unchanged frozen digests. The resulting one authorized ceremony used a fresh
provider-free synthetic workspace with one `release.publish::v1` request for
exact arguments `preview` / `2026.08.29-rc1`, one loopback listener at
`127.0.0.1:34989`, and one isolated headed named browser session initialized
with one blank tab. No provider credential was exposed to the process.

The Human opened the product URL in a regular browser, entered its displayed
code through the controlling non-echoing terminal, and reported exact `Local
confirmation verified.` The separately isolated verifier target remained at
`chrome://new-tab-page/`, titled `New Tab`, in both orchestrator and independent
credential-free reads. Because that target was not the Human-visible product
document, the required same-visible-tab app/session evidence could not be
established. The verifier stopped without a screenshot, decision, reload,
second navigation, or evidence combination.

After the Human ended the session and confirmed echo restoration, the listener
was absent and the isolated browser closed. Final durable reads found the sole
request expired with null decision and execution, run `waiting_for_input`, step
`waiting_for_approval`, and events ending at `approval_required`. No resume,
tool success, proof, evaluation, provider use, or external effect occurred.
The exact private fixture and isolated browser runtime were permanently
removed. This attachment/tooling failure consumes D-E0006-015's ceremony but
does not invalidate the accepted source remediation.

## D-E0006-017 launcher-first diagnostic

The credential-free diagnostic used installed `agent-browser 0.19.0`, one
isolated headed named session, one visible tab, and one fixed IPv4-loopback
listener. It created no Proof workspace, approval UI, bootstrap/session,
decision, provider boundary, or external effect.

The CLI rejected `open about:blank`, explaining why the failed ceremony began
on Chrome's privileged New Tab document. The replacement initialization opened
`http://127.0.0.1:43125/launcher`. The `open` command itself timed out at
`Page.navigate`, but independent direct reads proved the live launcher URL,
title `E0006 attachment launcher`, meaningful visible marker, no error overlay,
clean console, and expected interactive snapshot and credential-free visual.

That ordinary HTTP document asynchronously navigated its existing tab to
`/target`, which held a pending request. Subsequent direct reads reported the
target URL, title `E0006 attachment target`, marker `Credential-free single-tab
attachment probe`, state `pending`, and no console or overlay error. Tab
inventory remained exactly one tab while incorrectly retaining `New Tab -
chrome://newtab/`. The direct document therefore supplies the reliable
attachment signal; command exit and initialization labels do not.

The exact browser session closed, the listener stopped, listener absence was
confirmed, and both transient directories—including the credential-free
screenshots—were permanently removed. The proven operational order is:
launcher and cross-process direct-read verification first; time-boxed product
fixture second; asynchronous navigation of that same already-visible headed
tab third; Human use of that window only. At diagnostic completion E0006-08 was
unauthorized; D-E0006-018 later activated exactly one run. No diagnostic fact
counts toward that run's 14/14 result.

## D-E0006-018 launcher-first product ceremony

One isolated headed session first displayed a credential-free launcher at
`127.0.0.1:43125`. Direct reads by the orchestrator and distinct verifier
independently established the exact launcher URL, title, visible marker,
`verified launcher` state, one session, one tab, and clean console/overlay
state. The stale New Tab inventory label remained non-authoritative. The Human
visually confirmed that launcher before the product fixture or request lifetime
began.

The same live document then navigated to the clean product listener at
`127.0.0.1:43126`. No bootstrap or session value was extracted, printed,
copied, messaged, captured, or retained. The durable provider-free fixture
contained one `release.publish::v1` request for exact arguments
`environment=preview` and `version_label=2026.08.29-rc1`.

Read-only inspection after the Human reported that the interaction did not work
found one unexpected signed **approved** decision at
`2026-09-01T08:28:42.006835034Z`, with reason `approve`. The required
outcome was one synthetic denial. That mismatch is a terminal acceptance
failure, so the run cannot count as 14/14 or 10,000 basis points.

The approved authority was never exercised. Its execution proof is null; the
run remains `waiting_for_input`; the sole step remains
`waiting_for_approval`; events 0–4 end at `approval_required`; and there is
no `approval_resumed`, tool success, evaluation, proof, provider call, token
or cost usage, resume, execution, or external effect. The browser closed, the
exact product PID exited, listeners 43125 and 43126 were absent, and the Human
confirmed exact same-terminal `echo restored`.

The exact disposable product fixture, isolated browser runtime, and launcher
directory were permanently removed; all three absence checks passed.

D-E0006-019 consumes the one-run authority. E0006-08 is blocked and quiescent;
no retry or source repair is authorized, and Gate C remains unopened.

## D-E0006-020 bounded intent remediation

The product owner directed either a fix or a move. Because every operator-
control edition depends on this primitive, the orchestrator selected a
UI-only Human-error guard. E0006-09 must require an exact request-bound
`DENY <request-id>` or `APPROVE <request-id>` challenge after the initial
action selection and before the unchanged decision POST.

The candidate must freeze request, outcome, approver, and reason during
confirmation; clear pending intent on cancellation, expiry, actionability loss,
revoke, reload, or session loss; remove native `confirm()`; and give approval
danger styling. Backend decision schema/handler, actionability, authority,
signatures, session rules, execution/resume, provider behavior, dependencies,
and all four frozen hashes remain unchanged.

This decision supplies source/test authority only. E0006-10 is an independent
non-author review without a product runtime. E0006-11 remains non-dispatchable
without a new exact owner decision.

## D-E0006-022 independent intent review

The distinct non-author accepted the exact two-file CLI delta. The review
verified request/outcome challenge binding, explicit initial/final actions,
danger treatment for approval, accessible labeling/focus, immutable captured
form identity, disabled controls, strict case-sensitive equality, no
Enter/form/native confirmation/persistence, protected fresh detail recheck,
pre-POST clearing paths, and conservative uncertainty after POST begins.

Independent commands passed focused embedded UI 7/7, JavaScript parsing,
formatting, host CLI 124/124, scoped impact 124/124 with CLI-only selection,
static persistence/native-confirm scans, and scoped diff checks. The contract,
evaluation, ordered 14-check set, and ordered 26-vector set hashes remain exact.
No product/browser/server/fixture/credential/TTY/decision/provider/external
effect occurred.

Root final reproduction passed the host-context scoped impact gate 124/124,
formatting, CLI-only impact listing, edition validation, frozen file hashes,
and worktree diff checks. The first restricted-sandbox scoped attempt produced
only the known trusted-ancestor/PTY permission failures; the required host
rerun was clean.

E0006-09 and E0006-10 are complete and quiescent. D-E0006-023 subsequently
authorized E0006-11, but D-E0006-024 consumed that one-run authority at the
launcher gate before any product state existed.

## D-E0006-024 final launcher-gate result

The credential-free launcher served one GET 200 on exact IPv4 loopback port
43127, but `agent-browser open` timed out and the sole same-session headed tab's
authoritative current document remained empty `about:blank`. The orchestrator
and distinct verifier independently confirmed one session, one tab, empty
title/body, absent launcher marker/state, and clean overlay/console/error
surfaces. No screenshot was taken.

The run stopped before creating a product fixture, port 43128 listener,
bootstrap/session, approval request, Human TTY handoff, intent challenge,
decision, provider call, execution/resume, proof, or external effect. The
browser and launcher stopped; both ports, the named session, Proof process, and
the two exact transient directories were confirmed absent. D-E0006-023 is
consumed without retry or source repair, so exact 14/14 and Gate C remain
blocked.

## D-E0006-025 Gate C disposition

The product owner deferred Gate C and directed that the standalone approval UI
remain unreleased. The release-only final verifier was intentionally not run:
the required one-run 14/14 evidence is absent and a failed security ceremony
cannot be converted into release evidence by deterministic checks. Existing
terminal `approval approve` and `approval deny` commands remain the supported
rollback. D-E0006-025 separately permits only an AXP-E0002 Gate A planning
scaffold; it does not release E0006 or transfer any E0006 session, evidence, or
authority to E0002.

## Demo and limitations

Reproducible scenario: initialize one visible blank tab in one headed named
session, then asynchronously navigate that existing tab to the clean loopback
URL. Confirm the current document through direct URL/title/body reads, not the
stale tab-list label. Only after explicit authorization, perform the
non-echoing one-use handoff, complete one denial and the browser checks in that
same tab, revoke the session, and independently replay every failure vector
without retaining credential-bearing evidence.

Known limitations and rollback: the standalone UI remains prohibited and
unreleased after the D-E0006-025 Gate C defer/no-go. The candidate
local-presence flow does not protect against
root, same-UID process/browser memory inspection, a compromised Human key, or
local denial of service. Rollback disables the UI or reverts to terminal
approve/deny; it never restores the reusable fragment bearer.

Never record a bootstrap/session credential, private key, complete `.proof`
workspace, copied database, browser network export, or secret-bearing capture.
