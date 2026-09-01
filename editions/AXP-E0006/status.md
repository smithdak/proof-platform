# E0006 Status

- Edition: `AXP-E0006`
- Last updated: 2026-09-01
- Overall: `blocked and quiescent — Gate C deferred/no-go in D-E0006-025`
- Current wave/task: quiescent after W11 disposition; standalone UI unreleased
- Owner action: none for E0006; E0002 Gate A scaffolding only is separately
  authorized without treating E0006 as released

## Gates

- [x] E0001 dependency — Gate C defer recorded in D-E0001-020; writers quiescent
- [x] Gate A — approved 2026-08-31 in D-E0006-004
- [x] Gate B — approved 2026-08-31 in D-E0006-004, exact digests bound
- [x] Gate B remediation — approved 2026-08-31 in D-E0006-015; frozen
  contract/digests preserved
- [x] Gate B intent remediation — approved 2026-09-01 in D-E0006-020; UI-only
  guard with frozen contract/digests preserved
- [x] Gate C — product owner deferred/no-go on 2026-09-01 in D-E0006-025;
  required 14/14 remains absent and E0006 is not release-accepted

## Readiness

- [x] Three bounded read-only discovery audits completed with no provider,
  secret, private-workspace, browser, or repository mutation
- [x] Current fragment/sessionStorage hazard and retained defenses mapped to
  exact source and test paths
- [x] Four-task, one-writer dependency graph and exclusive writable paths drafted
- [x] Proposed `proof-approval-console-session/v1` state machines and limits
  digest-frozen; independent corrected-packet security review PASS
- [x] Proposed deterministic evaluation frozen at 14 required checks and 26
  rejection vectors; mechanical validation and independent review PASS
- [x] Product owner approved Gate A and Gate B
- [x] E0006-02 assigned to e0006-security; exact packet emitted
- [x] E0006-02 host suite 117/117; independent source/contract review PASS
- [x] Credential-safe Human browser-to-TTY handoff, one exact signed synthetic
  denial, explicit revoke, controlled shutdown, and same-TTY echo restoration
  completed with zero execution/resume and no retained credential
- [x] Final headed named agent-browser run passed current-session post-bootstrap
  URL/history/storage/cookie/referrer/console/resource and credential-free
  screenshot checks
- [x] Five-minute rerun completed one exact signed denial, explicit End Session,
  controlled shutdown, TTY restoration, and durable zero execution/resume
- [x] Credential-free diagnostic proved cross-agent attachment to one visible
  headed tab while a long-poll remained unresolved; direct URL/title/body reads
  reached the live document even though tab-list initialization metadata stayed
  stale
- [ ] E0006-03 exact 14/14 browser/process evaluation — D-E0006-013 reached
  terminal `Local confirmation verified`, but the directly attached current tab
  remained at a cleared bootstrap surface with hidden app controls and generic
  rejection; no screenshot, decision, session-possession claim, or execution
  followed
- [x] E0006-05 deterministic root cause and bounded source/test correction;
  orchestrator host reproduction passed 120/120
- [ ] E0006-06 independent review plus one same-session exact 14/14 ceremony;
  source/test review passed, but the one ceremony failed at the isolated
  same-visible-tab attachment boundary before app, decision, or revoke evidence
- [x] E0006-07 credential-free launcher-first diagnostic; one visible tab
  reached the exact pending target through direct reads while inventory retained
  stale New Tab metadata, then all transient state was removed
- [ ] E0006-08 launcher-first exact 14/14 ceremony; launcher preflight passed,
  but the sole run persisted an unexpected signed approval with reason
  `approve` instead of the required denial. Execution remained null and the
  authority is consumed without retry
- [x] E0006-09 explicit decision-intent confirmation; 7/7 focused UI tests,
  JavaScript parse, formatting, and host/scoped 124/124 passed
- [x] E0006-10 distinct non-author intent-guard review; source PASS, focused
  7/7, host/scoped 124/124, frozen hashes exact, no runtime
- [ ] E0006-11 exact ceremony — launcher never became the current document;
  D-E0006-023 consumed without product state or retry

## Risks and next actions

| Risk/blocker | Impact | Owner | Next action/date |
|---|---|---|---|
| D-E0006-013 terminal verification did not establish current-tab session possession | Split verification/exchange scheduling at the deadline was reachable; historical evidence remains insufficient to prove that exact runtime schedule | E0006-05 security owner | completed atomic correction with deterministic boundary regression and 120/120 host tests |
| D-E0006-015 ceremony split the Human-visible product page from the isolated verifier target | Frozen 14/14 requires one same-visible-tab journey; the isolated target remained `New Tab` and evidence cannot be combined | product owner / orchestrator | credential-free diagnosis completed in E0006-07; require a new explicit decision before any product ceremony |
| `agent-browser open about:blank` cannot initialize an ordinary document, and inventory labels remain stale after navigation | Starting from Chrome New Tab can split the visible Human page from the isolated verifier target | E0006-07 orchestrator | launcher-first direct-document probe passed; bind any future ceremony to that already-visible headed tab |
| E0006-08 persisted an unexpected approved decision instead of the authorized denial | Denial-only acceptance failed; exact 14/14 and Gate C are unavailable even though execution remained null | E0006-08 verifier / orchestrator | D-E0006-019 records containment and consumed authority; no retry, execution, resume, or repair is authorized |
| Browser actions can still sign the wrong Human-selected outcome after one native confirmation | Backend containment prevents auto-execution but cannot infer Human intent | E0006-09 security owner | require exact request-bound `DENY` or `APPROVE` challenge under D-E0006-020, then distinct source/test review |
| E0006-11 launcher request returned HTTP 200 but the authorized headed tab remained empty `about:blank` | Same-session product evidence could not begin; final one-run authority is consumed | product owner / orchestrator | no retry or repair; Gate C remains closed and any roadmap exception needs a new explicit owner decision |
| Terminal handoff must remain non-echoing and fail closed without a controlling TTY | A leaked or scripted bootstrap could weaken local-presence proof | E0006-02 | keep the accepted implementation and 117/117 regression coverage unchanged |
| Browser/session state races could mint more than one session | Duplicate local authority | E0006-02 / E0006-05 | retain the passing atomic state-machine suite and accepted remediation; do not infer live release from deterministic evidence |
| E0002 requires a separate authentication primitive | Reusing an unreleased E0006 session would create an invalid authority boundary | orchestrator | D-E0006-025 permits E0002 Gate A scaffolding only; require independent scoped operator auth and separate Gate A/Gate B before implementation |
