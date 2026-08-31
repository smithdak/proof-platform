# E0006 Retrospective

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
- Release/rejection date: pending
- Facilitator: orchestrator

## Outcome

Gate A/B, E0006-02 implementation, scoped validation, the direct Human product
journey, and headed current-session browser inspection completed across
separate runs. E0006 remains blocked before E0006-04 and Gate C because the
five-minute run's product path passed but its automation attachment exposed a
different fresh tab; separate runs or tabs cannot be combined into one frozen
14/14 result. The corrected attachment technique is now proven without a
credential, but its final authorized product use ended at a generic current-tab
rejection with no decision or execution. The one-run authority is consumed.

## Keep / change / stop

- Keep: exact approval actionability, loopback binding, Host/Origin checks,
  no-store/referrer/frame defenses, and lower-cost read-only discovery.
- Change: replace the reusable URL fragment and Web Storage bearer with the
  approved one-use local bootstrap and distinct memory-only session.
- Stop: treating a URL fragment as safe merely because fragments are normally
  absent from HTTP requests and referrers.

## Swarm metrics

- Tasks completed / escalated / reworked: E0006-01 and E0006-02 done;
  E0006-03 blocked after an automation-only expiry, one successful Human
  normal-browser ceremony, one headed expiry, and one successful five-minute
  product path whose Human-visible tab was not exposed to automation; E0006-04
  not started.
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
  screenshot, decision, or execution followed. All product failure paths
  remained closed.
- Ownership conflicts: none; writers remained exclusive and are quiescent.
- Next backlog candidates: E0002 only after E0006 Gate C.
