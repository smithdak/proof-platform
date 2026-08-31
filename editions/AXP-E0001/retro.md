# E0001 Retrospective

Prepared for Gate C on 2026-08-31. The owner decision remains pending; update
only the decision line after the product owner explicitly accepts defer or
chooses reject.

## Outcome

- Live journey result: exact persisted start executed once; run
  `01a057fe-0a47-7fe1-a607-5f350f90cd9b` sealed failed at retry exhaustion
  before approval or tool execution.
- Artifact/proof/evaluation result: no artifact, publication, proof, approval,
  or mutation; deterministic preflight 10/10; live evaluation
  `01a057fe-1894-75b0-ad3c-947e70cee86c` failed 5/17 at 2,941 basis points.
- Provider/model, attempts, tokens, duration, and cost: direct OpenAI Responses
  / exact `gpt-5.6-sol`; two dispatches, one retry, zero committed responses,
  turns, tools, and tokens; elapsed 3.629231 seconds; calculated
  committed-usage cost 0 micro-USD; provider cost unavailable and not treated
  as zero; USD 0.15 owner ceiling not exceeded.
- Owner decision: pending. D-E0001-019 recommends Gate C defer; no agent
  self-approval is recorded.

## Swarm performance

- Tasks completed / escalated / reworked: 18 implementation, contract,
  readiness, and repair tasks reached `done`; E0001-04 stopped on its required
  live failure rule; E0001-05 integrated the defer packet. Seven bounded
  late-stage corrections (E0001-14 through E0001-20) converted audit findings
  into independently tested authority, import, filesystem, identity, review,
  one-shot, and concurrency controls.
- Budget planned / used: no explicit per-task agent-token budget was recorded.
  The live budget was at most four dispatches, one retry, 10,000 tokens,
  120,000 calculated micro-USD, and USD 0.15 owner spend; actual durable
  counters were two dispatches, one retry, zero committed tokens, and zero
  calculated committed-usage micro-USD.
- Evaluation failures and preserved evidence: the exact 5/17 live evaluation,
  both provider attempts, four-event trace, state/check/policy digests, and
  private mode-0600 captures were retained. The failed run was not replaced or
  made to look successful.
- Ownership conflicts or prevented collisions: disjoint writable paths and
  single-writer waves prevented cross-crate edits. The final one-writer lease,
  atomic start claim, and expected-tail append work was discovered before paid
  execution and independently verified under contention.

## Product truth

- Runtime/recovery claims established: credential-free validation precedes
  provider construction; exact one-shot start, durable pre-I/O barriers, one
  bounded retry, fail-closed terminal sealing, zero-effect failure accounting,
  stable credential-free watch, and independent replay of the retained
  evidence all worked in the live process.
- Preview/publication claims established: deterministic and integration tests
  establish the strict `release.publish::v2` contract, immutable artifact path,
  approval binding, proof construction, and exact replay behavior. The live
  process established only that no publication occurs before a committed tool
  call and signed approval.
- Claims not established: no live model tool call, Human approval chronology,
  process-boundary continuation, immutable live artifact, original live proof,
  exact replay of a successful effect, terminal report, or 17/17 live
  evaluation exists. E0001 is not release-accepted.
- Remaining risks and next edition: provider capability/quota readiness was
  not established before consuming the sole journey; attempt 2's underlying
  retryable cause is lost when terminalized; the failed private `/tmp`
  workspace awaits an owner retention decision.

## Changes for the next edition

- Assignment/model routing: keep bounded implementation, test, documentation,
  and evidence-index tasks eligible for lower-cost agents; retain frontier
  review for shared authority, concurrency, paid-provider execution, and final
  integration. Give each worker one narrow path set and one durable handoff.
- Acceptance/evidence policy: add an explicitly gated, non-mutating provider
  capability/quota readiness check before allocating a one-shot primary live
  journey. Never infer availability from credential presence. Preserve the
  rule that a failed attempt remains evidence and cannot be replaced under the
  same authority.
- Tooling/process: preserve the original retryable cause when an exhausted
  attempt becomes terminal; add explicit two-429 and mixed retryable-cause
  tests; provide a redacted operator summary that distinguishes dispatched,
  committed, billable-known, and provider-cost-unavailable states without
  exposing raw provider bodies or credentials.
