# AXP-E0001 Charter — One Live Agent Does Real Work

- Edition ID: `AXP-E0001`
- Owner: product owner
- Orchestrator: Codex primary agent
- Base revision: `2f22a0c6ce28e70dcc9be89f2961f7e4f7acb9b6`
- Status: `approved`
- Discovery: three read-only `gpt-5.6-luna` audits completed 2026-08-29

## Outcome

User/problem: E0000 proves the governed runtime with deterministic scripted
model turns, but it does not prove that a real provider-backed model can finish
a useful release journey across a signed human approval and process boundary.

North-star journey:

1. In a fresh isolated workspace with newly generated test identities, a live
   OpenAI Responses model receives a synthetic request to publish one exact
   content edition and version label to `preview`.
2. The model discovers and calls only the versioned preview-publication tool
   allowed by its delegation and budget.
3. The runtime persists the pending call and pauses before the consequential
   transition. A distinct enrolled human sees the exact operation and canonical
   arguments and signs an approval decision.
4. After a process boundary, the same run and step resume without a blind
   mutation replay or a second successful publication.
5. The approved call produces one immutable local preview artifact bound to the
   requested edition, environment, version, manifest digest, output, and the
   engine's original signed proof. It does not deploy to an external service.
6. An independent verifier checks the proof and sealed trace, and the declared
   evaluation passes 10/10 with provider/model, token, duration, attempt, and
   cost evidence recorded.

Success metric and declared evaluation:

- Deterministic preflight remains 10/10 with a stable trace digest before any
  paid call.
- One controlled live journey calls the frozen publication operation exactly
  once with exact arguments, pauses for approval, and resumes the same
  run/step after process restart.
- No preview artifact, canonical content mutation, or external effect occurs
  before approval. No external deployment occurs anywhere in E0001.
- The stored artifact's canonical digest, requested identifiers, operation
  output, and original proof all agree; tampering fails independent
  verification.
- The live trace has no `tool_failed`, `failed`, or `budget_exceeded` event and
  passes every sealed policy check.
- Usage stays within the approved Gate B limits and records actual or explicitly
  unavailable provider cost without silently treating unknown cost as zero.

## Scope

In scope:

- Freeze a versioned live Release Manager journey and sealed evaluation policy.
- Resolve the `release.publish` registry/schema/handler idempotency and identity
  mismatch without breaking v1 callers; a new operation version is the default
  recommendation for Gate B.
- Make provider continuation and ambiguous failure recovery explicit,
  persistent, bounded, and fail-closed before any governed mutation.
- Build and verify a durable, disposable local preview artifact using synthetic
  content and newly generated test identities.
- Run one primary live-provider dogfood attempt and, only if the recorded policy
  permits it, one bounded retry; preserve failed evidence.

Non-goals:

- Production publication, hosting, deployment, CMS integration, or any other
  external preview effect.
- Native agent teams, parent/child runs, scheduling, leases, aggregate budgets,
  or an operator-console expansion.
- Provider failover, arbitrary hosted tools, parallel tool calls, or a general
  workflow engine.
- Historical workspace-key rotation/history rewriting, transport database-path
  unification, PostgreSQL, or registry-wide replay rollout.
- Sending repository source, secrets, private keys, or real customer content to
  a model provider.

## Budget

- Delivery: one orchestrator and at most three concurrent workers; W2 has two
  disjoint implementation owners. Use the model tiers in `assignments.tsv`.
- Paid live use: zero before explicit Gate B approval. Gate B must name the
  exact OpenAI model, direct endpoint policy, maximum USD spend, and approver.
- Proposed live ceiling: one primary attempt plus one bounded retry, at most 4
  model calls, 2 tool attempts, 10,000 total tokens, 1,024 output tokens per
  call, and 300 seconds per run.
- Data: synthetic operation arguments only. `OPENAI_BASE_URL` remains unset for
  the live gate unless the owner separately approves a proxy and its data path.
- Retry: no automatic retry after an ambiguous provider completion or governed
  mutation. Such a state stops for operator reconciliation under the frozen
  contract.

## Material-risk triggers requiring Gate B

- A public operation/schema/version, idempotency, evaluator, authority,
  approval, signature, or proof-contract change.
- A kernel/storage API change, SQLite migration, or new durable secret.
- Provider credential handling, paid API use, provider/model selection, proxy
  use, data-retention change, or an attempt beyond the approved live ceiling.
- Any filesystem mutation outside the disposable preview boundary, destructive
  cleanup, or external deployment/publication.
- Any relaxation of exact call, approval, proof, budget, or fail-closed recovery
  requirements.

## Approval

- Gate A approver/date: product owner, 2026-08-30
- Gate B decision/date: pending E0001-01 contract and live-spend packet
- Gate C decision/date: pending verified candidate

Gate A result: approved. This authorizes W1 contract design only. W2
implementation remains blocked until the E0001-01 Gate B packet is approved;
the paid live gate also requires a separate explicit Gate B authorization.
