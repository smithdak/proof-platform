# E0000 Retrospective

Completed after Gate C approval. Detailed command evidence is linked from
[`evidence.md`](evidence.md) and the individual handoffs.

## Outcome

- Shipped outcome: a tracked edition/swarm control plane, ownership-aware
  dispatch validation, Cargo reverse-dependency test selection, one canonical
  eight-operation Content v1 contract, durable exact replay for the frozen
  mutation pair, and governed CLI/HTTP/MCP/WebSocket paths that return the
  engine's original proof.
- Evaluation/conformance result: the final quiescent gate passed 405 tests
  across 46 suites. The seven-package Content closure passed 231 tests across
  31 suites; the runtime closure passed 63 tests across 3 suites. Executable
  conformance exercised all eight operations, and two independent fixed
  Release Manager traces scored 10/10 with identical digest
  `b14a94ed884b758f9503fdb95f85a65bd9d384d56aa13130ffe181fb69deae34`.
- Owner decision: Gate C approved on 2026-08-29 in D-E0000-007, with the
  historical identity still classified as compromised and prohibited for
  production use pending a separate security decision.

## Swarm performance

- Lowest-cost tasks that worked well: `gpt-5.6-luna` handled edition records
  and bounded transport error-mapping work; `gpt-5.6-terra` handled scoped
  crate, transport, conformance, and runtime packets.
- Tasks requiring `gpt-5.6-terra`/`gpt-5.6-sol` escalation: contract/security,
  shared replay design, kernel replay, and final integration were assigned to
  `gpt-5.6-sol` up front because of their blast radius, not after a failed
  low-cost attempt.
- Queue time and integration time: wall-clock and token-cost telemetry were not
  captured; all writers were confirmed quiescent before the final gate.
- Ownership collisions or prevented collisions: no unresolved collision
  reached integration. The launcher fixtures proved overlap and forward-wave
  dependency rejection, and shared transport paths were sequenced under one
  owner per wave.

## Product truth

- Contract drift found: the implementation had an unregistered ninth local
  helper, incomplete replay semantics, inconsistent target output shapes,
  content/kernel digest ambiguity, and a stale full-platform registry count.
  E0000 reconciled all five against the frozen contract.
- Evidence gaps found: no live provider/model run, no one-database
  cross-transport replay claim, no atomic filesystem-plus-ledger transaction,
  and no identity-rotation/old-proof migration exercise.
- Remaining risks and their next edition: E0001 owns live provider evidence;
  database-path compatibility and filesystem/replay reconciliation require
  explicitly gated follow-up packets; compromised-key rotation and history
  disposition must precede any production identity claim and fit the production
  workspace security work no later than E0004.

## Changes for the next edition

- Assignment template: keep exclusive paths, dependency edges, model budget,
  stop conditions, and evidence commands mandatory; add an explicit shared API
  freeze reference to every downstream packet.
- Wave sizing: retain one orchestrator plus at most three writers, with
  dependency-ordered transport waves when crates share paths.
- Acceptance policy: require package-impact closure after dependency changes,
  a quiescent workspace gate, a non-sensitive security audit, and explicit
  owner acceptance of every unresolved critical/high risk.
- Tooling/process: record wall-clock/token cost in future handoffs and generate
  final impact/evidence summaries from machine-readable test output to prevent
  stale intermediate counts.
