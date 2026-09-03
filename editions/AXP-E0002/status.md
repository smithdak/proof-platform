# Edition Status

- Edition: `AXP-E0002 — One Human Oversees Many Runs`
- Last updated: 2026-09-02
- Overall: `shifted W9 accepted under D-E0002-095; W10 dependency-ready but
  not dispatched`
- Current wave/tasks: E0002-06/E0002-07/E0002-11 are done; E0002-17 remains
  blocked historical evidence; no task is active
- Dispatch boundary: E0002-14 and E0002-02 are dependency-ready but require a
  separate product-owner W10 dispatch. Every later task remains
  non-dispatchable.

## Gates

- [x] Gate A — direction approved in D-E0002-011
- [x] Gate B historical — exact D-E0002-012 packet accepted in D-E0002-013
- [x] Gate B repair — D-E0002-077 v3 packet accepted in D-E0002-086
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
- [x] twenty tasks have one owner, bounded paths, dependencies, model
  routing, acceptance evidence, and unique handoffs
- [x] W3 contains three disjoint fan-out lanes, including one Luna mechanical
  fixture lane
- [x] additive W4-W8 serialize historical repair, exact semantic repair,
  derived fixtures, reviews, and later kernel alignment; shifted W9 retains
  disjoint storage, runtime-core, and control-shell lanes
- [x] shifted W10 has one lockfile reconciliation owner after two disjoint
  package-manifest freezes; W13 has one later lockfile/product-assembly owner
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
- [x] E0002-17 expanded fixture and byte-custody repair — strict corpus and
  exact evaluator, index, packet, validator, and repaired-file custody pass in
  D-E0002-067
- [x] D-E0002-069 read-only diagnostic — exact schema/runtime/kernel/digest
  closure and a validator-safe additive graph are proposed in D-E0002-070
- [x] product owner authorizes D-E0002-070's exact additive graph and serialized
  E0002-18/E0002-19 planning repair in D-E0002-071
- [x] E0002-18 exact semantic artifacts and Gate-B v3 packet freeze
- [x] E0002-19 exact fixture closure and three fresh independent reviews
- [x] explicit repaired Gate-B acceptance
- [x] separate E0002-20 dispatch
- [x] E0002-20 completion — exact source, scoped/reverse tests, custody, and
  fresh independent review pass under D-E0002-088
- [x] separate shifted-W9 dispatch — E0002-06/E0002-07/E0002-11 only under
  D-E0002-089
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
- [x] shifted W9 storage, runtime, and control lanes complete acceptance under
  D-E0002-095
- [ ] implementation, independent evaluation, integration, and Gate C

## Risks and next actions

| Risk/blocker | Impact | Owner | Next action/date |
|---|---|---|---|
| E0002-05's D-E0002-026 seams passed formatting, 153 scoped tests, focused review, all 11 artifact hashes, five semantic digests, and the Gate-B packet digest | Kernel prerequisite is accepted; its later final scoped/impact/review evidence is already recorded | orchestrator | preserve accepted kernel source custody for downstream work |
| E0002-17 expanded fixture and byte-custody repair passes strict corpus validation and exact custody under D-E0002-067 | Its fresh review set stopped on the command/audit failure in D-E0002-068, so local acceptance cannot complete the task | E0002-17 reviewers / orchestrator | preserve it as blocked historical evidence; do not reuse either passing review |
| D-E0002-071 authorizes the exact D-E0002-070 additive graph and serialized repair | E0002-18 must not edit artifacts until the staged 20-task graph validates; implementation remains closed | orchestrator | validate the administrative graph and diff, then activate E0002-18 alone on PASS |
| D-E0002-072 administrative graph gate passes | E0002-18 was the sole active writer and had to reproduce every D-E0002-070 predicted digest exactly | E0002-18 / orchestrator | stopped before artifact edit under D-E0002-073 |
| E0002-18's generic reads-schema renderer expands pre-existing compact inline objects | Rewriting the whole parsed document would alter unrelated bytes; D-E0002-071 forbids continuing after the failed assumption | product owner / orchestrator | authorize or reject a continuation limited to surgical replacement of only the four parsed branch refs, then the unchanged E0002-18 sequence |
| D-E0002-074 authorizes the surgical reads continuation | Only four exact line replacements may occur before the unchanged predicted-digest gates | E0002-18 / orchestrator | build the byte-preserving candidate, stop on any path or hash mismatch, then complete the unchanged E0002-18 sequence |
| The schema promotion patch matched branch 11 instead of branch 16 | Current reads bytes do not match the certified candidate; the manifest already names the predicted candidate hash | product owner / orchestrator | authorize or reject promotion of the already certified pinned reads candidate, then resume unchanged E0002-18 only after exact comparison |
| D-E0002-076 authorizes pinned reads promotion | The current two-line mismatch must close to byte equality before evaluator promotion | E0002-18 / orchestrator | reverify candidate and deltas, promote without context matching, and stop on any mismatch |
| E0002-18 comprehensive validation rejected `SchemaManifest` | The temporary harness required every logical pointer to begin `#/$defs/`, although the manifest schema explicitly permits root `#`; repaired artifact hashes remain exact, but the task's stop-on-mismatch rule fired before freeze | product owner / orchestrator | authorize or reject correcting only the temporary root-pointer assertion, then rerunning the unchanged full validation sequence |
| D-E0002-079 authorizes the root-pointer harness continuation | Only the temporary `#` resolution assertion may change before the full unchanged validation barrier reruns | E0002-18 / orchestrator | correct that assertion, run every E0002-18 closure check, and stop again on any actual mismatch |
| D-E0002-080 freezes E0002-18 exactly | All artifact, semantic, packet, projection, custody, edition, and diff checks pass; the exact freeze signal releases only E0002-19 | E0002-19 / orchestrator | promote only the pinned 158-file fixture candidate, validate exact custody, then enter the three-review Gate-B hold |
| D-E0002-081 records E0002-19 local PASS | Exact 36-recipe/121-wrapper/index promotion, strict 590-file corpus, terminal-byte profile, secrets scan, hashes, edition validation, and diff hygiene pass | three fresh reviewers / orchestrator | independently review schema/packet, evaluator/fixtures/bytes, and command/audit/source-alignment; stop on any failure |
| D-E0002-082 stops the E0002-19 review set | Schema/packet review passes, but command/audit/source-alignment review finds ordinal 078 absent from E0002-06 ownership and ordinal 067 absent from E0002-07 ownership; the third review is stopped without a verdict | product owner / orchestrator | authorize or reject a planning-only task-packet/handoff correction followed by a completely fresh three-review set; preserve all artifacts and fixtures |
| D-E0002-083 authorizes the exact planning-only ownership repair | Bind ordinal 078's atomic storage audit path and ordinal 067's runtime recording-store outcome without touching product bytes | orchestrator | validate the four packet/handoff additions and unchanged custody, then return E0002-19 to a wholly fresh three-review set |
| D-E0002-084 validates the ownership repair | Both exact obligations, unchanged artifact/fixture custody, strict corpus, edition structure, and diff hygiene pass | three wholly fresh reviewers / orchestrator | independently recheck schema/packet, evaluator/fixture/bytes, and command/audit/source-alignment; stop on any failure |
| D-E0002-085 records the wholly fresh review set | Schema/packet, evaluator/fixture/byte, and command/audit/planned-source reviews independently pass; post-review custody remains exact | product owner | explicitly accept, revise, or reject Gate-B v3 digest `25f64ff5...6eb2`; no E0002-20 dispatch is implied |
| D-E0002-086 accepts repaired Gate B | Exact v3 digest `25f64ff5...6eb2` and every constituent artifact, semantic digest, and material choice are accepted; E0002-19 completes | product owner | decide separately whether to dispatch only E0002-20; keep all source closed meanwhile |
| D-E0002-087 dispatches only E0002-20 | Exact one-file kernel alignment passes 153 scoped and 762 reverse-impact tests plus custody and independent review | E0002-20 / orchestrator | done under D-E0002-088; preserve the accepted kernel bytes while shifted W9 remains closed |
| E0002-11 post-repair format, scoped/impact 17-test runs, custody, and fresh security review pass | Frozen synthetic control shell is accepted; real composition remains downstream | E0002-11 / orchestrator | done under D-E0002-091; preserve exact root/control bytes |
| E0002-07 passes 143 scoped and 267 reverse-impact tests; ordinal 067 passes review | Fresh review finds the sole concurrent lease-claim one-winner test was replaced; the lane retry is consumed | product owner / E0002-07 / orchestrator | historical stop closed by D-E0002-092/D-E0002-094 and final acceptance in D-E0002-095 |
| D-E0002-089 dispatches shifted W9 after E0002-20 completion | E0002-11 completes, while E0002-06/E0002-07 initially stop on their bounded acceptance gates | E0002-06/E0002-07/E0002-11 / orchestrator | all three lanes are now done under D-E0002-095 |
| E0002-06 retry ceiling is exhausted under D-E0002-090 | Latest focused ordinal-078 observation passes after two source/test corrections, but no authorized post-correction full format/scoped suite exists | product owner / E0002-06 / orchestrator | historical stop closed through D-E0002-092/D-E0002-094 and D-E0002-095 |
| D-E0002-091 closes the W9 review phase | Control PASS; runtime FAIL only on missing separate concurrent claim coverage; no source changed during review | product owner / orchestrator | historical review stop closed by the exact continuations and D-E0002-095 acceptance |
| D-E0002-092 authorizes exact W9 continuations | Storage may only verify its pinned corrected source; runtime may only add the separate concurrent-claim test while preserving ordinal 067 | E0002-06/E0002-07 / orchestrator | stopped under D-E0002-093; later exact continuations complete under D-E0002-095 |
| D-E0002-093 records both no-retry stops | Storage format check reports three mechanical test-file differences; runtime format and new contention test pass but one existing +301-second live-resume setup fails | product owner / orchestrator | exact remedies authorized in D-E0002-094 and accepted in D-E0002-095 |
| D-E0002-094 authorizes exact second continuations | Storage may apply only the three reported rustfmt layouts; runtime may change only the positive margin 301→360 while retaining the 299 negative and both operator tests | E0002-06/E0002-07 / orchestrator | local, impact, custody, and fresh reviews pass; done under D-E0002-095 |
| D-E0002-095 accepts shifted W9 | Storage passes 141 local and 332 impact tests plus independent ordinal-078/schema-14 review; runtime passes 144 local and 268 impact tests plus independent ordinal-067/concurrency/deadline review; control remains accepted | orchestrator / product owner | preserve exact W9 custody; request a separate owner decision before dispatching dependency-ready W10 |
| Historical W4 root/storage manifest and lockfile changes were serialized | A continuation must not reinterpret or rerun the exact `W4 lock stable` evidence without authority | E0002-11 / E0002-06 / orchestrator | preserve the exact historical signals while all shifted W9 source remains quiescent |
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
| Same-wave manifest edits can invalidate concurrent Cargo commands | Disjoint source owners can still observe a transient workspace graph | W3/W9/W10/W13 manifest owners | use the assigned manifest/lock barriers before Cargo commands and quiesce before impact tests |
