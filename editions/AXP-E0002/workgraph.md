# Workgraph

Edition: `AXP-E0002`

| Task | Wave | Owner/model | Owned paths | Depends on | Acceptance | Status |
|---|---|---|---|---|---|---|
| E0002-01 | W1 | orchestrator / `gpt-5.6-sol` | edition scaffold and backlog rows | D-E0006-025 | Owner-ready Gate A packet; may become done only on dated owner Gate A acceptance | review; current authority ends here |
| E0002-13 | W2 | orchestrator / `gpt-5.6-sol` | operator contract, schemas, evaluator, W2 decision/status records | E0002-01 done = owner Gate A accepted | Exact independent auth/control/evaluation artifacts; may become done only on digest-bound owner Gate B | blocked; non-dispatchable |
| E0002-05 | W3 | e0002-kernel / `gpt-5.6-sol` | `proof-kernel`, unique handoff | E0002-13 + Gate B | Strict operator commands, cursors, audit, fences, budgets, tests | pending; non-dispatchable |
| E0002-08 | W3 | e0002-auth / `gpt-5.6-sol` | root manifest/lock plus new auth crate, unique handoff | E0002-13 + Gate B | Independent scoped Human session and auth-first race suite | pending; non-dispatchable |
| E0002-12 | W3 | e0002-fixtures / `gpt-5.6-luna` | frozen mechanical fixtures, unique handoff | E0002-13 + Gate B | Deterministic valid/rejection fixture replay | pending; non-dispatchable |
| E0002-06 | W4 | e0002-storage / `gpt-5.6-sol` | `proof-storage`, next migration, unique handoff | E0002-05 | Projection, command/audit ledger, lease/fence, budget race round trips | pending; non-dispatchable |
| E0002-07 | W5 | e0002-runtime / `gpt-5.6-sol` | `proof-agent-runtime`, unique handoff | E0002-06 | Cancel/resume/recovery/budget barriers with zero duplicate effect | pending; non-dispatchable |
| E0002-11 | W6 | e0002-control / `gpt-5.6-sol` | root manifest/lock plus dedicated operator-control crate, unique handoff | E0002-07, E0002-08 | Loopback launcher, signed-challenge adapter, synthetic assembly interface, revoke/shutdown/restart tests | pending; non-dispatchable |
| E0002-02 | W7 | e0002-http / `gpt-5.6-terra` | first sequential `proof-transport-http` wave, unique handoff | E0002-06, E0002-08, E0002-11 | Authenticated/redacted inbox, detail, approval, audit reads | pending; non-dispatchable |
| E0002-09 | W8 | e0002-http / `gpt-5.6-sol` | second sequential `proof-transport-http` wave, unique handoff | E0002-02, E0002-07, E0002-08, E0002-11 | Race-safe decision/cancel/explicit-resume APIs | pending; non-dispatchable |
| E0002-03 | W9 | e0002-ui / `gpt-5.6-terra` | operator console app, unique handoff | E0002-02, E0002-09, E0002-11 | Accessible multi-run journey and credential-hygiene browser evidence | pending; non-dispatchable |
| E0002-04 | W10 | e0002-verifier / `gpt-5.6-sol` | redacted dogfood record, unique handoff | E0002-03,07,08,09,11,12 | Distinct non-author all-required API/browser/race evaluation | pending; non-dispatchable |
| E0002-10 | W11 | orchestrator / `gpt-5.6-sol` | root/public/release integration | every prior task | Quiescent scoped/reverse-impact/final gates and owner Gate C | pending; non-dispatchable |

## Dependency flow

```text
D-E0006-025 planning exception
              |
              v
W1 E0002-01 Gate A scaffold (review; current authority ends)
              |
        owner Gate A
              |
              v
W2 E0002-13 contract/schema/evaluator -> owner Gate B
              |
              v
W3 E0002-05 kernel -> W4 E0002-06 storage -> W5 E0002-07 runtime --+
                                                                     |
W3 E0002-08 independent auth ----------------------------------------+
                                                                     v
W6 E0002-11 loopback control plane / synthetic assembly interface
                              |
                              v
W7 E0002-02 authenticated reads -> W8 E0002-09 protected mutations
                                                     |
                                                     v
W9 E0002-03 operator UI ------------------------------+
                                                     |
W3 E0002-12 fixtures ---------------------------------+
                                                     v
W10 E0002-04 independent verification
                              |
                              v
W11 E0002-10 quiescent integration -> owner Gate C
```

## Wave gates

- W1 may write planning records only. It ends in `review`, with no active
  product writer and an explicit owner Gate A accept/revise/reject request.
  E0002-01 cannot become `done` without dated Gate A acceptance.
- W2 cannot start until the product owner records Gate A. W2 may draft and
  freeze only the approved public/security contract, schemas, and evaluator;
  implementation remains closed until an exact Gate B decision records their
  digests and any dependency/migration authority. E0002-13 remains `review`
  after drafting and cannot become `done` without that dated Gate B acceptance.
- W3 starts only after Gate B. Its three owners have disjoint paths. The auth
  owner is the sole W3 owner of `Cargo.toml` and `Cargo.lock`; the fixture owner
  performs mechanical replay only and stops on policy judgment.
- W4 appends only the next sequential migration and stops on any kernel/public
  shape need. W5 starts after its storage dependency is done and quiescent.
- W6 owns the dedicated loopback control-plane process and root manifest delta.
  It freezes the signed-challenge delivery, a synthetic injected-router/static-
  source assembly interface, listener, session revoke, shutdown, and distinct
  worker/control-plane restart behavior. Actual route/app composition is W10
  verification and W11 integration evidence.
- W7 and W8 use the same named HTTP owner in strictly sequential waves. W7 has
  no mutation authority. W8 cannot weaken auth-first disclosure control or
  make approval auto-resume a run.
- W9 uses only frozen schemas and protected APIs. It persists no credential and
  gives decision, cancel, resume, and session revoke distinct confirmation and
  recovery states.
- W10 is a distinct non-author. It may create deterministic disposable fixtures
  but no provider call or external effect; sensitive runtime material is never
  retained in evidence.
- W11 starts only after every writer stops. The orchestrator reproduces scoped
  and reverse-impact checks, runs the quiescent verifier, documents rollback
  and limitations, and presents Gate C without self-approval or any E0006
  release/evidence claim.

Cross-owner requests and dependency changes are recorded in `decisions.md`.
