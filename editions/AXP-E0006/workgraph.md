# AXP-E0006 Workgraph

Edition: `AXP-E0006`

| Task | Wave | Owner/model | Owned paths | Depends on | Acceptance | Status |
|---|---|---|---|---|---|---|
| E0006-01 | W1 | orchestrator / `gpt-5.6-sol` | proposed session contract, evaluation, backlog, and edition records | E0001 Gate C disposition | Exact threat model, charter, protocol, limits, task packets, and Gate A/B decision packet validate | done; Gate A/B approved by D-E0006-004 |
| E0006-02 | W2 | e0006-security / `gpt-5.6-sol` | `proof-transport-cli`, exact `Cargo.lock` delta, unique handoff | E0006-01 + Gate A/B | Clean URL/output, one long-poll terminal-confirmed bootstrap, scoped session, expiry/revocation versus signing lease, fresh governance, unchanged signatures | done; 117/117 host tests and independent source review PASS |
| E0006-03 | W3 | e0006-verifier / `gpt-5.6-sol` | redacted dogfood evidence plus unique handoff | E0006-02 | Browser/process/router/security evidence passes exact 14/14 policy with no captured credential | blocked; D-E0006-013 reached terminal verification but the current attached tab failed closed before session/app evidence, and final run authority is consumed |
| E0006-05 | W4 | e0006-security / `gpt-5.6-sol` | `proof-transport-cli` plus unique handoff | E0006-02 + D-E0006-015 | Reproduce and remove the current-document bootstrap/session delivery ambiguity without weakening one-use, lost-response, reload, or generic-error rules | done; atomic verification/exchange activation and 120/120 host tests accepted |
| E0006-06 | W5 | e0006-verifier / `gpt-5.6-sol` | redacted dogfood evidence plus unique handoff | E0006-05 | Independent source/test review followed by the one authorized same-tab exact 14/14 ceremony | blocked; source/test review passed, but isolated same-tab attachment failed and authority is consumed |
| E0006-07 | W6 | orchestrator / `gpt-5.6-sol` | credential-free diagnostic plus edition records and unique handoff | E0006-05 + D-E0006-016 failure context | Prove one launcher-first visible headed tab remains directly attachable while a target request is pending; remove all transient state | done; direct target reads passed, inventory staleness classified, cleanup complete |
| E0006-08 | W7 | e0006-verifier / `gpt-5.6-sol` | redacted dogfood evidence plus unique handoff | E0006-07 + D-E0006-018 | One launcher-first same-visible-tab exact 14/14 product journey | blocked and quiescent; launcher passed, but an unexpected approval violated the denial-only contract; zero execution and no retry |
| E0006-09 | W8 | e0006-security / `gpt-5.6-sol` | `proof-transport-cli` UI/tests plus unique handoff | E0006-07 + D-E0006-019 + D-E0006-020 | Exact request/outcome Human-intent challenge with no backend or frozen-contract drift | done; focused 7/7 and host/scoped 124/124 accepted |
| E0006-10 | W9 | e0006-verifier / `gpt-5.6-sol` | redacted dogfood evidence plus unique handoff | E0006-09 | Independent source/test review; no product runtime | done; independent PASS, host/scoped 124/124, frozen hashes exact |
| E0006-11 | W10 | e0006-verifier / `gpt-5.6-sol` | redacted dogfood evidence plus unique handoff | E0006-10 + D-E0006-023 | One intent-bound launcher-first exact 14/14 denial journey | blocked and quiescent; launcher gate failed, authority consumed |
| E0006-04 | W11 | orchestrator / `gpt-5.6-sol` | public guidance, contract reconciliation, release records, root integration | E0006-01,02,05,07,08,09,10,11 | Scoped impact plus quiescent gate, rollback and limitations, dated Gate C decision | blocked and quiescent; Gate C deferred/no-go in D-E0006-025 |

## Dependency flow

```text
E0001 D-E0001-020 defer; writers quiescent
                    |
                    v
E0006-01 threat model + contract + evaluation
                    |
              Gate A + Gate B
                    |
                    v
E0006-02 single-owner CLI security implementation
                    |
                    v
E0006-03 historical independent verification (blocked)
                    |
                    v
E0006-05 bounded source remediation
                    |
                    v
E0006-06 non-author review + one exact ceremony (blocked)
                    |
                    v
E0006-07 launcher-first credential-free diagnostic (done)
                    |
             D-E0006-018
                    |
                    v
E0006-08 launcher-first exact ceremony (blocked; authority consumed)
                    |
             D-E0006-020
                    |
                    v
E0006-09 explicit Human-intent confirmation
                    |
                    v
E0006-10 independent source/test review
                    |
              D-E0006-023
                    |
                    v
E0006-11 intent-bound exact ceremony (blocked; authority consumed)
                    |
              Gate remains closed
E0006-04 disposition integration + Gate C defer/no-go
                    |
                    v
E0002 Gate A scaffold only under D-E0006-025 roadmap exception
```

## Wave gates

- W1 is planning/contract work only. It may not modify CLI behavior, start the
  approval server, create a credential, read private workspace material, or
  claim owner approval. E0006-01 remains `review` until both Gate A and the
  exact security Gate B packet are approved.
- W2 has one writer for the entire CLI crate because authentication state,
  HTML, server routing, process behavior, and tests form one security boundary.
  That writer also owns the exact already-locked `rustix` dependency delta in
  `Cargo.lock`. It starts only after E0006-01 is `done` and the decisions are
  recorded.
- W3 starts only after W2 is quiescent and its scoped gates pass. The verifier
  must not have authored E0006-02 and must use a disposable synthetic workspace.
  No secret-bearing screenshot, log, browser export, or test artifact may be
  retained.
- W4 is the D-E0006-015 bounded repair wave. It has one CLI writer, preserves
  the frozen public contract, and stops before any ceremony. Its handoff must
  name the reproduced cause and prove that reload, replay, duplicate exchange,
  and lost-response paths remain fail closed.
- W5 starts only after W4 is quiescent and independently reviewable. The
  non-author verifier first reviews source and deterministic evidence. Only
  after that review passes may the one D-E0006-015 ceremony begin; any failed
  boundary consumes the authority without retry. That ceremony failed at the
  isolated same-visible-tab attachment boundary, so W5 is blocked and
  quiescent.
- W6 is a credential-free tooling diagnostic only. It may use one loopback
  listener and one isolated headed tab, but no product fixture, credential,
  Human handoff, decision, provider, source edit, or retained capture. It proved
  the launcher-first direct-document path and removed all transient state.
- W7 consumed its one D-E0006-018 run. The launcher-first preflight passed, but
  the product ceremony persisted an unexpected approved decision instead of
  the required denial. Execution remained null, cleanup passed, and W7 is
  blocked and quiescent with no retry or evidence combination permitted.
- W8 is the D-E0006-020 UI-only intent-remediation wave. It has one CLI writer,
  preserves backend decision/session/authority behavior and all frozen hashes,
  and starts no browser or product ceremony.
- W9 starts only after W8 is done and quiescent. The distinct non-author reviews
  source and deterministic evidence, reproduces scoped checks, and creates no
  product fixture, credential, decision, provider call, or external effect.
- W10 consumed D-E0006-023 at the launcher gate. The launcher returned HTTP
  200, but the sole headed tab remained empty `about:blank`; no product state
  began. No failure permits a retry, source repair, execution/resume, provider
  call, or external effect.
- W11 release integration cannot pass because E0006-11 failed. D-E0006-025
  authorizes only disposition reconciliation: keep public guidance unreleased,
  preserve terminal rollback, record the owner Gate C defer/no-go, and do not
  run or claim the release-only final verifier.

Cross-owner requests and dependency changes are recorded in `decisions.md`.
