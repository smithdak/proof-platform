# Edition Status

- Edition: `AXP-E0002 — One Human Oversees Many Runs`
- Last updated: 2026-09-02
- Overall: `planning repair in review — E0002-17 local fixture acceptance passed`
- Current wave/tasks: E0002-16 is done under D-E0002-052; E0002-17 is in review
  under D-E0002-065; shifted W6 E0002-06/E0002-07/E0002-11 remain blocked
- Dispatch boundary: no writer is active. Only the three fresh independent
  E0002-17 reviews may proceed; Gate-B acceptance and implementation remain
  closed.

## Gates

- [x] Gate A — direction approved in D-E0002-011
- [x] Gate B historical — exact D-E0002-012 packet accepted in D-E0002-013
- [ ] Gate B repair — D-E0002-037 superseding packet accepted
- [ ] Gate C — release accepted

## Readiness

- [x] D-E0006-025 records E0006 Gate C defer/no-go and the E0002 planning-only
  roadmap exception
- [x] E0006 is quiescent and not claimed or reused as released
- [x] independent Human/workspace/server-instance/capability auth is explicit
- [x] disposable trusted workspace and fresh identity requirement explicitly
  excludes the compromised repository-root `.proof` identity
- [x] exact journey, outcome metric, non-goals, budget, risks, and rollback are
  proposed
- [x] seventeen tasks have one owner, bounded paths, dependencies, model
  routing, acceptance evidence, and unique handoffs
- [x] W3 contains three disjoint fan-out lanes, including one Luna mechanical
  fixture lane
- [x] additive W4/W5 serialize Gate-B artifact freeze and derived fixtures;
  shifted W6 retains disjoint storage, runtime-core, and control-shell lanes
- [x] shifted W7 has one lockfile reconciliation owner after two disjoint
  package-manifest freezes; W10 has one later lockfile/product-assembly owner
- [x] E0002-11 explicitly owns loopback launch, signed-challenge session
  delivery, same-origin assembly, revoke/shutdown, and restart semantics
- [x] E0002-15 owns the real runnable router/store/runtime/static-app source
  composition before independent verification, with no legacy-router fallback
- [x] dispatch locks are explicit governance checks: E0002-01 cannot become
  done without dated Gate A and is done under D-E0002-011; D-E0002-012 returned
  E0002-13 to review after drafting, and it cannot become done without
  digest-bound Gate B; W3 depends on E0002-13 done. Because `swarm.sh` validates
  dependency status but not decision contents, the orchestrator must inspect
  the dated decision and Gate B digests before either status transition.
- [x] revised packet received three distinct read-only PASS reviews in
  D-E0002-010 for
  security/gates, workgraph/ownership/model routing, and journey/evaluation
- [x] product owner approved Gate A in D-E0002-011
- [x] E0002-13 freezes the contract/schema/evaluator and exact Gate B packet
- [x] product owner accepts D-E0002-012 at Gate B in D-E0002-013
- [x] product owner authorizes the additive D-E0002-040 workgraph in
  D-E0002-041 without accepting the superseding Gate B
- [x] D-E0002-041 administrative graph validates in D-E0002-042 and E0002-16
  alone activates
- [x] E0002-16 artifact freeze — valid exact signal recorded in D-E0002-052;
  superseding Gate B remains unaccepted
- [x] E0002-17 expanded fixture repair — strict corpus and exact evaluator,
  index, packet, validator, and repaired-file custody pass in D-E0002-065
- [ ] D-E0002-037 stale-fence consistency artifacts, derived fixture closure,
  three independent reviews, and superseding Gate-B packet pass
- [x] E0002-12 frozen fixture corpus independently validates and is done
- [x] E0002-05 bounded retry passes local format and 149 scoped kernel tests
- [x] E0002-05 final 151-test scoped, 715-test reverse-impact, and independent
  review acceptance pass
- [x] E0002-08 scoped 26-test, reverse-impact, and independent review
  acceptance pass
- [x] W3 kernel, independent auth, and mechanical fixture lanes complete
- [x] product owner historically dispatches only W4 in D-E0002-021
- [x] historical W4 root/storage-manifest/offline-lock barrier completes before
  source; its exact signal names remain unchanged
- [ ] shifted W6 storage, runtime, and control lanes complete acceptance
- [ ] implementation, independent evaluation, integration, and Gate C

## Risks and next actions

| Risk/blocker | Impact | Owner | Next action/date |
|---|---|---|---|
| E0002-05's D-E0002-026 seams passed formatting, 153 scoped tests, focused review, all 11 artifact hashes, five semantic digests, and the Gate-B packet digest | Kernel prerequisite is accepted; reverse impact remains serialized behind shifted W6 writer quiescence | orchestrator | hold kernel source quiescent and run its impact check only after all shifted W6 lanes are locally green |
| E0002-17 expanded fixture repair passes strict corpus validation and exact custody under D-E0002-065 | Local acceptance does not substitute for the required independent judgments | E0002-17 reviewers / orchestrator | run the three separately authorized fresh read-only reviews; do not mark done or accept Gate B before all pass |
| E0002-11 post-repair format, 17 scoped tests, and its one-package reverse-impact run pass | Fresh independent review and the E0002-17 dependency remain incomplete | product owner / E0002-11 / orchestrator | keep the lane blocked until dependency and review gates are explicitly satisfied |
| E0002-07 now formats and passes 143 scoped tests | Runtime reverse-impact and fresh independent review remain incomplete | product owner / E0002-07 / orchestrator | keep the lane blocked and complete only its already-bounded acceptance sequence after the dependency gate |
| All shifted W6 writers are stopped; storage and control local/impact checks are green, while runtime impact/review is pending | E0002-17 is not done and the superseding Gate B remains unaccepted, so no shifted W6 completion or later fan-out is permitted | product owner / orchestrator | complete E0002-17's three reviews and explicit Gate-B decision before any implementation dispatch |
| Historical W4 root/storage manifest and lockfile changes were serialized | A continuation must not reinterpret or rerun the exact `W4 lock stable` evidence without authority | E0002-11 / E0002-06 / orchestrator | preserve the exact historical signals while all shifted W6 source remains quiescent |
| W3 manifest/lock ordering is serialized | Premature source/Cargo work could observe an invalid workspace graph | E0002-08/E0002-05/orchestrator | auth signals root-ready; kernel freezes its manifest; auth signals lock-stable; only then source work begins |
| E0006 is unreleased | Its session/evidence cannot authenticate or validate E0002 | orchestrator | preserve the independent-auth prohibition in every downstream packet |
| Repository-root `.proof` identity remains historically compromised | Reusing it could invalidate Human authentication and product evidence | E0002-13/auth/verifier owners | require a disposable trusted workspace and fresh enrolled identities; reject root `.proof` use |
| CLI/MCP and HTTP/WS open different SQLite paths and HTTP/WS use ephemeral signers | The operator could inspect the wrong run set or lose proof identity across restart | E0002-13 contract owner | freeze one authoritative database, trusted-open rule, and signer lifecycle at Gate B |
| Existing reads enumerate unscoped run/audit data | An operator endpoint could leak goals, arguments, workspace paths, or identities | E0002-13 contract owner | define auth-first redacted DTOs and generic error ordering for Gate B |
| Current cancel is status-only and recovery lacks durable fences | Cancel/restart races could permit provider/tool work or stale writes | kernel/storage/runtime owners | freeze append-only commands, leases/fences, and barrier evidence before implementation |
| Aggregate budget enforcement is per-run only | Concurrent runs could exceed the operator's declared aggregate limit | kernel/storage/runtime owners | bind aggregate ledger and race semantics in E0002-13 Gate B packet |
| Browser control can conflate decision, cancel, resume, and session revoke | Human intent or recovery could target the wrong authority | UI/security owners | require distinct request-bound controls, no auto-resume, and uncertain-write readback |
| Existing HTTP binds broadly and no current process supplies the E0002 boundary | Independent auth could remain a library with no safe product boundary | E0002-11/E0002-15 control owners | implement the loopback shell, then the real same-origin product assembly and separate worker/control-plane restart vectors after Gate B |
| Existing HTTP router contains unauthenticated legacy surfaces | Reusing the general router could expose data/actions inside the control plane | E0002-13/E0002-02/E0002-15 owners | freeze, implement, assemble, and inventory a dedicated protected route set with no legacy fallback |
| New auth crate and SQLite migration are material changes | Unreviewed dependencies/schema could become authority | product owner | explicit Gate B with exact artifacts, dependency, and migration number |
| Same-wave manifest edits can invalidate concurrent Cargo commands | Disjoint source owners can still observe a transient workspace graph | W3/W6/W7/W10 manifest owners | use the assigned manifest/lock barriers before Cargo commands and quiesce before impact tests |
