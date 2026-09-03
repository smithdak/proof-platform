# Workgraph

Edition: `AXP-E0002`

| Task | Wave | Owner/model | Owned paths | Depends on | Acceptance | Status |
|---|---|---|---|---|---|---|
| E0002-01 | W1 | orchestrator / `gpt-5.6-sol` | edition scaffold and backlog rows | D-E0006-025 | Owner-ready Gate A packet; may become done only on dated owner Gate A acceptance | done; D-E0002-011 |
| E0002-13 | W2 | orchestrator / `gpt-5.6-sol` | operator contract, schemas, evaluator, W2 decision/status records | E0002-01 done = owner Gate A accepted | Exact independent auth/control/evaluation artifacts; may become done only on digest-bound owner Gate B | done; D-E0002-013 |
| E0002-05 | W3 | e0002-kernel / `gpt-5.6-sol` | `proof-kernel`, unique handoff | E0002-13 + Gate B | Strict operator commands, cursors, audit, fences, budgets, tests | done; D-E0002-033 |
| E0002-08 | W3 | e0002-auth / `gpt-5.6-sol` | root manifest/lock plus new auth crate, unique handoff | E0002-13 + Gate B | Independent scoped Human session and auth-first race suite | done; D-E0002-020 |
| E0002-12 | W3 | e0002-fixtures / `gpt-5.6-luna` | frozen mechanical fixtures, unique handoff | E0002-13 + Gate B | Deterministic valid/rejection fixture corpus and digests | done; D-E0002-014 |
| E0002-16 | W4 | orchestrator / `gpt-5.6-sol` | exact stale-fence contract/reads/manifest/evaluator/digest closure plus bounded edition records | E0002-13, E0002-12 | Exact four-profile repair, validation, and artifact freeze signal | done; D-E0002-052 |
| E0002-17 | W5 | e0002-fixtures / `gpt-5.6-luna` | exact derived fixture closure, unique handoff | E0002-16, E0002-12 | 28 setup and 121 wrapper propagation with exact custody | blocked; D-E0002-068 semantic-review stop |
| E0002-18 | W6 | orchestrator / `gpt-5.6-sol` | exact reads/manifest/evaluator/v3-packet repair plus bounded edition records | E0002-12, E0002-16 | Four positive-fence refs, ordinals 067/078/103/105, exact hashes and pending packet | done; D-E0002-080 exact freeze |
| E0002-19 | W7 | e0002-fixtures / `gpt-5.6-luna` | exact 158-file derived fixture closure, unique handoff | E0002-12, E0002-18 | 36 recipes, 121 wrappers, index, exact custody, reviews and Gate B | done; D-E0002-086 repaired Gate B accepted |
| E0002-20 | W8 | e0002-kernel / `gpt-5.6-sol` | `proof-kernel`, unique handoff | E0002-05, E0002-18, E0002-19 | Four correlated stale-fence profiles, tests, impact, independent review | done; D-E0002-088 exact kernel acceptance |
| E0002-06 | W9 | e0002-storage / `gpt-5.6-sol` | `proof-storage`, migration 14 after Gate B, unique handoff | E0002-05, E0002-19, E0002-20 | Projection, command/audit ledger, lease/fence, budget race round trips | done; D-E0002-095 storage acceptance |
| E0002-07 | W9 | e0002-runtime / `gpt-5.6-sol` | `proof-agent-runtime`, unique handoff | E0002-05, E0002-19, E0002-20 | Runtime command/barrier policy against kernel recording stores | done; D-E0002-095 runtime acceptance |
| E0002-11 | W9 | e0002-control / `gpt-5.6-sol` | root manifest/lock plus dedicated operator-control crate, unique handoff | E0002-05, E0002-08, E0002-19, E0002-20 | Loopback launcher, signed-challenge session, protected-router shell, revoke/shutdown tests | done; D-E0002-091 security acceptance |
| E0002-14 | W10 | e0002-backend-verifier / `gpt-5.6-sol` | `conformance`, sole W10 `Cargo.lock`, unique handoff | E0002-06, E0002-07, E0002-11, E0002-12 | Real SQLite/runtime/control integration, harness implementation, and backend-owned evaluator subset | pending; dependency-ready; separate owner dispatch required |
| E0002-02 | W10 | e0002-http / `gpt-5.6-terra` | first sequential `proof-transport-http` wave, unique handoff | E0002-06, E0002-08, E0002-11 | Authenticated/redacted inbox, detail, approval, audit reads | pending; dependency-ready; separate owner dispatch required |
| E0002-09 | W11 | e0002-http / `gpt-5.6-sol` | second sequential `proof-transport-http` wave, unique handoff | E0002-02, E0002-07, E0002-08, E0002-11, E0002-14 | Race-safe decision/cancel/explicit-resume APIs | pending; non-dispatchable |
| E0002-03 | W12 | e0002-ui / `gpt-5.6-terra` | operator console app, unique handoff | E0002-02, E0002-09, E0002-11 | Accessible multi-run journey and credential-hygiene browser evidence | pending; non-dispatchable |
| E0002-15 | W13 | e0002-assembly / `gpt-5.6-sol` | second sequential `proof-operator-control` wave, `Cargo.lock`, generated UI bundle, unique handoff | E0002-02, E0002-03, E0002-09, E0002-11, E0002-14 | Runnable real protected router/app/store/runtime assembly with no legacy fallback | pending; non-dispatchable |
| E0002-04 | W14 | e0002-verifier / `gpt-5.6-sol` | redacted dogfood record, unique handoff | E0002-03,07,08,09,11,12,14,15 | Distinct non-author all-required API/browser/race evaluation | pending; non-dispatchable |
| E0002-10 | W15 | orchestrator / `gpt-5.6-sol` | root/public/release integration | every required task including E0002-18/19/20; excludes superseded E0002-17 | Quiescent scoped/reverse-impact/final gates and owner Gate C | pending; non-dispatchable |

## Dependency flow

```text
D-E0006-025 planning exception
              |
              v
W1 E0002-01 Gate A scaffold (done; D-E0002-011)
              |
        owner Gate A
              |
              v
W2 E0002-13 historical Gate B + W3 E0002-12 historical fixtures
              |
              v
W4 E0002-16 exact Gate-B repair/freeze
              |
              v
W5 E0002-17 historical fixtures (blocked; superseded)
              |
              v
W6 E0002-18 exact semantic artifact repair
              |
              v
W7 E0002-19 exact fixtures + reviews -> owner repaired Gate B
              |
              v
W8 E0002-20 kernel alignment done (D-E0002-088)
              |
              v
W3 kernel/auth + W8 repair gate -> W9 storage/runtime/control
              |
              +--> W10 E0002-14 backend integration
              +--> W10 E0002-02 protected reads
                              |
                              v
                    W11 E0002-09 protected mutations
                              |
                              v
                    W12 E0002-03 operator UI
                              |
                              v
                    W13 E0002-15 real product assembly
                              |
W3 historical fixtures -----+
                              v
                    W14 E0002-04 independent verification
                              |
                              v
                    W15 E0002-10 integration -> Gate C
```

## Wave gates

- W1 wrote planning records only and is done under dated owner Gate A decision
  D-E0002-011. That decision activates no product implementation.
- W2 used D-E0002-011 only to draft and freeze the approved public/security
  contract, schemas, evaluator, and Gate B digest packet. D-E0002-012 records
  the exact packet; D-E0002-013 accepts it and completes E0002-13.
- W3 was dispatched under D-E0002-013 and stopped at D-E0002-014 with E0002-12
  done and both E0002-05 and E0002-08 blocked after their permitted retries
  failed final acceptance. D-E0002-015 authorizes one exceptional final repair
  attempt for only those recorded blockers; D-E0002-016 records that it stopped
  on the first mandated format failure. D-E0002-017 authorizes only the required
  reflow and remaining serialized acceptance sequence; D-E0002-018 records its
  stop at kernel test compilation. D-E0002-019 authorizes only the no-`Debug`
  test assertion rewrite and remaining serialized acceptance; D-E0002-020
  records all three W3 tasks done. Its three W3
  owners had disjoint paths. The auth
  owner is the sole W3 owner of `Cargo.toml` and `Cargo.lock`: it writes the
  root/auth manifests plus the exact inert Cargo target scaffold without Cargo
  and signals root-ready; kernel then freezes
  its SHA-256/subtle/zeroize package-manifest delta without Cargo or source edits;
  auth then
  performs the sole two-package offline lock reconciliation while every source
  tree is quiescent and signals lock-stable before source fan-out. After fan-
  out, auth runs no Cargo command until kernel source is quiescent because its
  package commands compile proof-kernel.
  The fixture owner materializes only the strict corpus/index/documents and
  stops on policy or harness judgment. E0002-14 later owns executable replay.
  All W3 writers quiesce before reverse-impact acceptance runs.
- W4 is the D-E0002-041 additive planning-only Gate-B repair. E0002-16 alone
  becomes active only after the administrative graph validates. It changes
  exactly artifact ordinals 1, 4, 10, and 11, closes their derived semantic and
  packet digests, emits the exact freeze signal, and becomes done as a drafting
  barrier rather than as Gate-B acceptance.
- W5 is the serialized planning-only fixture propagation. E0002-17 starts only
  after the W4 freeze and changes exactly the derived 28 setup documents, 121
  wrapper policy hashes, and index links. Its byte candidate passes, but its
  fresh review set stopped on D-E0002-068 semantic defects; it remains blocked
  historical evidence and is superseded rather than rewritten.
- W6 is D-E0002-071's exact planning-artifact correction. E0002-18 alone may
  activate after the administrative graph validates. It changes only four
  positive-fence references, evaluator ordinals 067/078/103/105, their exact
  derived manifest/evaluator closure, and the pending-owner Gate-B v3 packet.
- W7 is the serialized mechanical fixture propagation. E0002-19 begins only
  after E0002-18 emits `Gate B semantic repair artifacts frozen`; it changes
  exactly 36 recipe documents, 121 wrappers, and the index. It then stops for
  three fresh independent read-only reviews and explicit repaired Gate-B
  acceptance.
- W8 is the post-Gate-B kernel alignment. E0002-20 was separately dispatched
  only after E0002-19 completed through explicit Gate-B acceptance and is done
  under D-E0002-088.
- The storage/runtime/control lanes were historically dispatched as W4 under
  D-E0002-021 with later work closed. D-E0002-041 first assigned their
  unfinished continuations to W6; D-E0002-071 shifts them to W9 without
  renaming or rerunning the historical `W4 root ready`, `W4 storage manifest
  frozen`, or `W4 lock stable` signals. W9 retains three disjoint writers after
  the kernel/auth APIs and E0002-19/E0002-20 are done. Storage
  appends only Gate-B-bound migration 14. Runtime implements against kernel
  recording stores and cannot claim durable SQLite integration. Control owns
  the dedicated loopback shell and root manifest delta: it writes the exact
  inert Cargo target scaffold and signals root-ready before product source work
  and without Cargo; storage then freezes its rustix package-
  manifest delta without source work or Cargo; control alone performs the two-
  package offline lock reconciliation while all source trees are quiescent and
  signals lock-stable before source fan-out or any other historical W4 Cargo command. Its
  synthetic router/static-source
  and store-opener interfaces expose no legacy unauthenticated routes, product
  authority, or dependency on in-progress storage/runtime source.
- D-E0002-095 accepts all three shifted-W9 lanes after their scoped and
  quiescent impact gates, exact custody, and fresh independent reviews pass.
  This satisfies W10 dependencies but does not dispatch either W10 owner.
- W10 has two source-disjoint writers. E0002-14 independently composes the real
  SQLite, runtime, and control APIs and proves restart/fence/budget semantics
  before mutations. Its digest-bound unscored backend subset contains all four
  backend-owned positive scenarios (worker restart, aggregate-budget
  concurrency, runtime dispatch commit, and runtime dispatch failure) and the
  exact ordered frozen vector list. E0002-02 may build protected reads from completed storage/
  auth/control APIs, but cannot claim the E0002-14 mutation/recovery result. Both
  owners first stabilize only their owned package-manifest deltas, before any
  W10 source edit and without running Cargo. While both source trees remain
  quiescent, E0002-14 alone runs the Gate-B-frozen lock-reconciliation command,
  exclusively updates `Cargo.lock`, and signals it stable before either owner
  begins source work or another Cargo command; any later dependency change
  stops both writers, restores quiescence, and repeats that barrier.
  After source fan-out, conformance runs no Cargo command until HTTP source is
  quiescent because its package commands compile proof-transport-http.
  Reverse-impact acceptance waits for W10 writer quiescence.
- W10 and W11 use the same named HTTP owner in strictly sequential waves. W10
  has no mutation authority. W11 cannot weaken auth-first disclosure control or
  make approval auto-resume a run, and begins only after backend integration.
- W12 uses only frozen schemas and protected APIs. It persists no credential and
  gives decision, cancel, resume, and session revoke distinct confirmation and
  recovery states.
- W13 reopens only `proof-operator-control` after the shell, protected routes,
  backend integration, and UI are complete. It wires the runnable real router,
  authoritative store/runtime services, and built static app through the frozen
  interfaces. It is the sole W13 `Cargo.lock` owner and may reproduce only the
  frozen generated bundle under `apps/operator-console/dist/**`; other UI
  files are read-only. It must not fall back to the general legacy HTTP router
  or alter lower-layer behavior.
- W14 is a distinct non-author. It uses a fresh disposable workspace and fresh
  identities, may create deterministic synthetic fixtures, but makes no
  provider call or external effect; sensitive runtime material is never
  retained in evidence.
- W15 starts only after every writer stops. The orchestrator reproduces scoped
  and reverse-impact checks, runs the quiescent verifier, documents rollback
  and limitations, and presents Gate C without self-approval or any E0006
  release/evidence claim.

Cross-owner requests and dependency changes are recorded in `decisions.md`.
