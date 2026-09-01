# E0006 Retrospective

## Gate C deferred; operator control planning separated — 2026-09-01

The product owner deferred Gate C in D-E0006-025. E0006 is quiescent and not
released; the standalone approval UI remains unavailable as a supported path,
and terminal approve/deny remains the rollback. The release-only final verifier
was not run because the required one-run 14/14 evidence is absent.

The same decision permits only an AXP-E0002 Gate A planning scaffold. E0002
must define an independent Human-, workspace-, server-instance-, and
capability-scoped operator authentication boundary. It may use E0006 only as
read-only failure/design input and may not inherit its session, evidence,
release status, or authority. Product work remains stopped until separate
E0002 Gate A and security/public-contract Gate B decisions.

## Final intent-bound ceremony stopped at launcher — 2026-09-01

D-E0006-023 authorized one final launcher-first run after the intent guard and
independent source review passed. The credential-free listener served its
launcher, but the one headed named tab never committed that document: direct
reads by the orchestrator and distinct verifier remained empty `about:blank`.
The pre-product safety gate therefore stopped the run without a workspace,
credential, request, Human handoff, decision, provider call, execution, or
external effect. Cleanup removed the browser, listener, and exact transient
directories. The one-run authority is consumed, no retry or repair is
authorized, and Gate C remains blocked.

## Bounded recovery selected — 2026-09-01

The owner directed either a fix or a move. Moving to E0002 would bypass its
declared authentication prerequisite, while changing backend authority would
exceed the evidence: the server persisted the authenticated outcome it
received and still prevented execution. D-E0006-020 therefore selects the
smallest evidence-backed correction—an explicit request/outcome Human-intent
challenge in the browser, followed by non-author source/test review. It does
not authorize another ceremony. E0006-09 and E0006-10 subsequently passed the
focused, host, scoped, JavaScript, formatting, frozen-hash, and independent
review gates; E0006-11 remains separately gated.

## Launcher-first ceremony failed safely — 2026-09-01

The launcher-first procedure solved the attachment problem: one visible headed
tab was directly verified by the orchestrator, the Human, and the distinct
non-author before product state existed. The product ceremony nevertheless
failed its more important denial-only boundary. Durable state contained an
unexpected signed approval with reason `approve` instead of the authorized
denial. Treating the signed outcome—not intent or preflight—as authoritative
prevented a false PASS.

Containment worked. No resume or execution occurred, the run and step remained
at their approval wait states, provider/token/cost/external-effect evidence was
zero, the browser and listeners stopped, and the same terminal restored echo.
The approved authority was disposable and removed after independent evidence.
D-E0006-018 is consumed; a retry would hide the failed safety ceremony rather
than explain it. Future work requires a new bounded owner decision and must
address how the operator can unmistakably bind the intended outcome before
another live signing ceremony.

## Launcher-first diagnostic finding — 2026-09-01

The attachment failure was not resolved by retrying the product. A bounded
credential-free probe showed that `agent-browser 0.19.0` cannot initialize an
ordinary document with `open about:blank`; Chrome leaves the isolated target on
its privileged New Tab page. Starting instead from a fast loopback launcher
made the same existing tab directly attachable before and after asynchronous
navigation to a page holding a pending request. The tab inventory still showed
stale New Tab metadata, while direct URL/title/body/state reads, console,
overlay, snapshot, and visual evidence all reached the live target. Future
ceremonies must initialize and verify this launcher before starting the product
request lifetime, and the Human must use that already-visible headed window.
The diagnostic was removed completely and supplies no product evidence or
ceremony authority.

## Post-remediation blocked finding — 2026-08-31

E0006-05 deterministically reproduced and removed the split deadline
linearization while preserving the frozen contract; independent review and
120/120 host-context tests passed. The single D-E0006-015 ceremony still could
not produce one exact 14/14 journey. The Human completed terminal verification
from a product page in a regular browser, while the isolated verifier session
remained on its pre-existing `New Tab` target. Stopping there preserved the
same-tab evidence rule and avoided manufacturing a PASS from separate browser
contexts. Durable cleanup proved null decision/execution, zero resume/effect,
listener shutdown, echo restoration, browser closure, and exact fixture
disposal. The ceremony authority is consumed; E0006-06, E0006-04, Gate C, and
E0002 remain blocked.

## Interim blocked finding — 2026-08-31

The implementation and deterministic evaluation passed. A co-located Human
also completed the secure handoff, exact signed denial, explicit revoke,
controlled shutdown, and TTY restoration without communicating either
credential. Independent live verification still exposed a delivery-system
boundary: the successful normal-browser page was not attachable for the
required current-session agent-browser evidence. A final headed run then passed
that browser evidence but reached approval expiry before its decision or
explicit revoke completed. This is not an implementation failure to bypass.
Future secure editions must plan a visible, headed, attachable browser plus the
co-located Human ceremony and an explicit execution-time budget as one
verification dependency. The five-minute rerun proved the product path inside
that budget, but also showed that a named automation session is insufficient
unless it deterministically exposes the exact tab the Human is viewing. A
credential-free follow-up isolated the tooling behavior: asynchronously
navigating one pre-existing blank headed tab remains cross-agent attachable
during the long-poll, while `tab list` can retain stale `about:blank` metadata.
Direct live-document reads, not the inventory label, must anchor future
same-tab evidence. The owner-authorized final run then proved that deterministic
attachment alone is insufficient: terminal verification succeeded, but the
current attached document failed closed before exposing session/app evidence.

- Edition: `AXP-E0006`
- Release/rejection date: Gate C deferred/no-go 2026-09-01
- Facilitator: orchestrator

## Outcome

Gate A/B, E0006-02/E0006-05 implementation, scoped validation, the direct Human
product journey, and headed current-session browser inspection completed across
separate runs. E0006-07 supplied a deterministic launcher-first procedure, and
E0006-08 proved that procedure's attachment preflight. E0006 remains blocked
and unreleased because the sole product run then signed an approval
instead of the authorized denial. Execution remained null, but separate runs
or launcher evidence cannot repair that acceptance failure. D-E0006-018 is
consumed without retry under D-E0006-019, the final E0006-11 launcher gate also
failed, and D-E0006-025 records the owner's Gate C defer/no-go.

## Keep / change / stop

- Keep: exact approval actionability, loopback binding, Host/Origin checks,
  no-store/referrer/frame defenses, and lower-cost read-only discovery.
- Change: replace the reusable URL fragment and Web Storage bearer with the
  approved one-use local bootstrap and distinct memory-only session.
- Stop: treating a URL fragment as safe merely because fragments are normally
  absent from HTTP requests and referrers.

## Swarm metrics

- Tasks completed / escalated / reworked: E0006-01, E0006-02, E0006-05, and
  E0006-07 done;
  E0006-03 blocked after an automation-only expiry, one successful Human
  normal-browser ceremony, one headed expiry, and one successful five-minute
  product path whose Human-visible tab was not exposed to automation; E0006-06
  passed source/test review but blocked when its one ceremony again split the
  regular-browser product page from the isolated verifier target; E0006-07
  proved the launcher-first path without product state; E0006-08 then passed
  launcher preflight but blocked when its sole run persisted an unexpected
  approval instead of the authorized denial; D-E0006-020 activated bounded
  E0006-09 intent remediation; E0006-09 and E0006-10 passed; E0006-11 then
  failed its launcher gate under the sole D-E0006-023 authority, and E0006-04
  remains blocked.
- Budget planned / used: three `gpt-5.6-luna` read-only audits plus bounded
  `gpt-5.6-sol` contract, implementation, review, and verification work; zero
  provider or external spend.
- Evaluation failures and causes: exact 14/14 not established; the normal
  browser run passed signing/revoke but lacked attachment, while the final
  headed run passed browser secrecy/capture but reached expiry before a durable
  decision, while the five-minute run passed decision/revoke but attached to a
  distinct fresh tab. A later credential-free cross-agent probe passed with one
  asynchronously navigated visible tab and identified stale tab-list metadata;
  it did not authorize or replace a product run. The final product run then
  reached terminal verification but not current-tab session/app evidence; no
  screenshot, decision, or execution followed. E0006-05 then corrected that
  source race, but the post-remediation product page was not attached to the
  isolated verifier target. E0006-08 later corrected attachment but produced
  the wrong signed outcome. That approved authority was never resumed or
  executed; all external-effect paths remained closed. E0006-11 stopped even
  earlier when the credential-free launcher never became the current document.
- Ownership conflicts: none; writers remained exclusive and are quiescent.
- Next backlog candidate: E0002 Gate A planning scaffold only under
  D-E0006-025; implementation requires its own Gate A and Gate B.
