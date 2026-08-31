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
- Evaluator: distinct non-author E0006-03 verifier plus one authorized
  co-located Human for the non-communicated browser-to-TTY handoff
- Result: `FAIL / blocked — final current tab failed closed before session/app evidence`

## Acceptance checks

| Check | Command/input | Expected | Actual | Result |
|---|---|---|---|---|
| E0001 dependency | D-E0001-020 and E0001 status | prior edition blocked, quiescent, writer-free | owner defer recorded; host quiescent gate 614/614 | passed |
| Read-only discovery | three bounded audits | exact threat/source/test/workgraph basis; zero effects | converged report in `discovery.md`; no writes/provider/browser/secrets | passed |
| Edition structure | `rtk scripts/swarm.sh validate AXP-E0006` | complete artifact/task/handoff graph with disjoint paths | valid four-task W1-W4 graph; assignment validation and all four packet emissions pass | passed |
| Proposed contract | `contracts/approval-console-session.md` | exact threat model, states, limits, routes, failure ordering, rollback | frozen digest; independent corrected-packet review PASS | passed |
| Proposed evaluation | `evals/approval-console-security-v1.json` | 14 ordered required checks with normative assertions plus 26 ordered rejection vectors and frozen set digests | frozen digest; mechanical validation and independent corrected-packet review PASS | passed |
| Gate A | D-E0006-002/D-E0006-004 | dated owner direction approval | approved 2026-08-31 | passed |
| Gate B | D-E0006-003/D-E0006-004 | dated owner security/public-contract approval | approved 2026-08-31 with exact digests | passed |
| CLI implementation | E0006-02 handoff and scoped impact | exact approved behavior and all focused regressions | 117/117 host tests; formatting/impact/diff clean; independent remediated source review PASS | passed |
| Independent verification | E0006-03 handoff and redacted dogfood record | exact 14/14 plus browser/process/security PASS | pre-session checks and all 26 deterministic vectors passed; prior runs separately passed headed browser secrecy/capture and the Human denial/revoke path; the final D-E0006-013 run reached terminal verification, but the current attached tab retained a cleared bootstrap surface, hidden app, and generic rejection, so no session-possession, screenshot, decision, or execution evidence could be claimed | failed / blocked |
| Final gate | `rtk scripts/swarm.sh verify AXP-E0006 --quiescent` | edition valid, formatting and workspace suite pass | not run | pending |
| Owner release | Gate C decision | dated accept/defer/reject | not reached | pending |

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

## Demo and limitations

Reproducible scenario: initialize one visible blank tab in one headed named
session, then asynchronously navigate that existing tab to the clean loopback
URL. Confirm the current document through direct URL/title/body reads, not the
stale tab-list label. Only after explicit authorization, perform the
non-echoing one-use handoff, complete one denial and the browser checks in that
same tab, revoke the session, and independently replay every failure vector
without retaining credential-bearing evidence.

Known limitations and rollback: the current fragment UI remains prohibited
until E0006 passes. The proposed local-presence flow does not protect against
root, same-UID process/browser memory inspection, a compromised Human key, or
local denial of service. Rollback disables the UI or reverts to terminal
approve/deny; it never restores the reusable fragment bearer.

Never record a bootstrap/session credential, private key, complete `.proof`
workspace, copied database, browser network export, or secret-bearing capture.
