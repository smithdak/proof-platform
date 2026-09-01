# Edition Status

- Edition: `AXP-E0002 — One Human Oversees Many Runs`
- Last updated: 2026-09-01
- Overall: `proposed — Gate A packet ready; no product writer authorized`
- Current wave/task: W1 / E0002-01 in review; planning records only
- Owner decision needed: accept, revise, or reject the proposed Gate A charter,
  journey, all-required evaluation policy, zero-live-spend budget, non-goals,
  and frozen workgraph. Approval would activate E0002-13 contract work only;
  implementation would remain blocked pending Gate B.

## Gates

- [ ] Gate A — direction approved
- [ ] Gate B — mandatory for independent auth, public contract/schema,
  evaluator, migrations, authority/races, root manifests, and API mutations
- [ ] Gate C — release accepted

## Readiness

- [x] D-E0006-025 records E0006 Gate C defer/no-go and the E0002 planning-only
  roadmap exception
- [x] E0006 is quiescent and not claimed or reused as released
- [x] independent Human/workspace/server-instance/capability auth is explicit
- [x] exact journey, outcome metric, non-goals, budget, risks, and rollback are
  proposed
- [x] thirteen tasks have one owner, bounded paths, dependencies, model routing,
  acceptance evidence, and unique handoffs
- [x] W3 contains three disjoint fan-out lanes, including one Luna mechanical
  fixture lane
- [x] E0002-11 explicitly owns loopback launch, signed-challenge session
  delivery, same-origin assembly, revoke/shutdown, and restart semantics
- [x] dispatch locks are explicit governance checks: E0002-01 cannot become
  done without dated Gate A; E0002-13 is blocked and cannot become done without
  digest-bound Gate B; W3 depends on E0002-13 done. Because `swarm.sh` validates
  dependency status but not decision contents, the orchestrator must inspect
  the dated decision and Gate B digests before either status transition.
- [x] three distinct read-only reviews pass: security/gates,
  workgraph/ownership/model routing, and journey/evaluation
- [ ] product owner approves Gate A
- [ ] E0002-13 freezes contract/schema/evaluator and product owner approves
  Gate B
- [ ] implementation, independent evaluation, integration, and Gate C

## Risks and next actions

| Risk/blocker | Impact | Owner | Next action/date |
|---|---|---|---|
| Gate A is not approved | No contract or product task may dispatch | product owner | review D-E0002-002; accept, revise, or reject |
| E0006 is unreleased | Its session/evidence cannot authenticate or validate E0002 | orchestrator | preserve the independent-auth prohibition in every downstream packet |
| Existing reads enumerate unscoped run/audit data | An operator endpoint could leak goals, arguments, workspace paths, or identities | E0002-13 contract owner | define auth-first redacted DTOs and generic error ordering for Gate B |
| Current cancel is status-only and recovery lacks durable fences | Cancel/restart races could permit provider/tool work or stale writes | kernel/storage/runtime owners | freeze append-only commands, leases/fences, and barrier evidence before implementation |
| Aggregate budget enforcement is per-run only | Concurrent runs could exceed the operator's declared aggregate limit | kernel/storage/runtime owners | bind aggregate ledger and race semantics in E0002-13 Gate B packet |
| Browser control can conflate decision, cancel, resume, and session revoke | Human intent or recovery could target the wrong authority | UI/security owners | require distinct request-bound controls, no auto-resume, and uncertain-write readback |
| Existing HTTP binds broadly and no task previously owned session delivery | Independent auth could remain a library with no safe product boundary | E0002-11 control owner | implement exact loopback launcher/same-origin adapter and separate worker/control-plane restart vectors after Gate B |
| New auth crate and SQLite migration are material changes | Unreviewed dependencies/schema could become authority | product owner | explicit Gate B with exact artifacts, dependency, and migration number |
