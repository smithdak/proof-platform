# Release Manager live operator runbook

## Status and authority

This is the secure operator and independent-verification procedure for the one
approved AXP-E0001 Release Manager live journey. The procedure remains frozen
for auditability. The final section records the 2026-08-31 execution result:
the authorized journey sealed failed at provider retry exhaustion before an
approval request, publication, artifact, or proof existed.

This runbook supersedes the **Live-provider gate** in
[`release-manager-preview.md`](release-manager-preview.md). Do not follow that
older gate: it constructs a new workspace and agent, targets the legacy v1
journey, and uses generic start/resume/evaluation commands. None of those are
authorized for E0001 live execution.

The paid Gate B decision, the final-source credential-free readiness packet,
and the packet's exact persisted `next_argv` jointly authorize the first start.
This document does not independently authorize a provider call, an approval,
or a consequence.

## E0001-20 final-source replay barrier

E0001-20 changes live-start replay while retaining the existing persisted
live-start argv schema. The runtime derives its durable start-claim key
canonically from the exact existing readiness bindings and setup; an operator
does not supply or reconstruct that claim. Because the implementation source
changes, the retained readiness packet is **not executable until replayed and
revalidated against final source**, even though its historical credential-free
evidence passed. After E0001-20 reaches final source, independently replay its
exact 10/10 credential-free finish check, rerun the required final-source host
gates, and confirm its immutable identifiers, binding digest, ready-record
digest, and persisted `next_argv` remain exact.

Do not patch, translate, retype, reconstruct, or combine any old and new argv.
Execute only the byte-for-byte persisted `next_argv` from the independently
verified post-E0001-20 replay. If that packet or its private workspace is
missing, has drifted, or cannot replay credential-free, stop before reading a
credential and regenerate it through the approved preparation workflow; do not
regenerate merely because the source changed when exact replay still passes.

The retained preparation values below remain the required bindings only if the
post-E0001-20 final-source replay confirms them exactly:

| Record | Retained reference | Post-E0001-20 rule |
| --- | --- | --- |
| Workspace root | `/tmp/proof-e0001-readiness.GRXKx3` | Require this exact private workspace unless loss or drift forces a newly approved preparation. |
| Preparation | `01a053eb-a460-7f21-8b2b-5cebcd872dd0` | Require the exact replayed preparation ID. |
| Preflight evaluation | `01a053ec-51a1-7b13-af4e-2535a8ea551f` | Require the exact replayed 10/10 evaluation ID. |
| Readiness binding | `7efebe8d98ad23f9a122ae64f76887d371804f719bae7fb4a69b434e511b6b71` | Require the independently recomputed exact digest. |
| Ready-record SHA-256 | `8bc2784f8ae20cc108fe50715ef31ec46289ed4bcf81da6a32b66554785cc40b` | Require the exact immutable record digest. |
| `next_argv` | retained but intentionally not reproduced here | Execute only its replay-verified persisted value. |

The final packet must preserve the approved target and authority unless a new
owner gate explicitly changes them:

| Binding | Required value |
| --- | --- |
| Live agent definition | `01a053ec-082a-7a53-b023-0ac263cbb460` |
| Agent/workspace actor | `01a053eb-33b9-7b03-8280-551c2fdb13c5` |
| Sealed Human approver | `01a053eb-7527-7320-b9c1-199817173725` |
| Delegation | `01a053ec-2d67-72e1-8c5d-1f22c7e53180` |
| Synthetic edition | `01a053eb-cc85-7420-b03d-5a1690e1f1d1` |
| Manifest | `sha256:081871f79a710ec4025b5e110f640c49c1ce0fcc0f3dcac44ec5526ad05ff52f` |
| Environment | `preview` |
| Version label | `2026.08.30-rc1` |
| Live policy | `/home/dakota/projects/proof-platform/evals/release-manager-live-v1.json` |
| Provider/model | direct OpenAI Responses / exact `gpt-5.6-sol` |

## Roles and separation of duties

- The live operator executes the persisted start, observes the waiting state,
  reviews it with the emitted workspace-bound watch argv, executes only the
  emitted exact-Human decision argv, and performs recovery with the exact
  policy-bearing live command.
- Only the policy-bound, enrolled Human
  `01a053eb-7527-7320-b9c1-199817173725` may approve or deny the request.
- The independent verifier must be a non-author: not the live-run author and
  not the Human who signed the decision. The verifier never approves, denies,
  starts, or advances an unsealed run.
- One writer controls the private workspace at a time. Runtime enforces this
  with a crash-released workspace execution lease held from before a resume's
  first durable read through its complete outcome. The verifier begins only
  after the operator has stopped and the run is durably terminal.

## Frozen live limits

The final packet and live trace must preserve all of these boundaries:

- one persisted live run and no replacement run;
- one direct OpenAI Responses journey, exact `gpt-5.6-sol`, default service
  tier, `store=true`, and no base-URL override;
- synthetic edition, manifest, version, goal, and tool arguments only;
- exactly one successful `release.publish::v2` mutation and one immutable
  local preview artifact after signed approval;
- at most four provider dispatches, at most three logical model turns, and at
  most one retry; a successful canonical journey has exactly two committed
  model responses;
- exactly one tool attempt for a passing journey, at most 10,000 total tokens,
  at most 1,024 output tokens per call, and at most 300 seconds;
- calculated cost at most 120,000 micro-USD under the frozen pricing schedule
  and owner-authorized spend at most 150,000 micro-USD (USD 0.15);
- provider cost may be explicitly unavailable, but unavailable is never
  recorded or treated as zero; and
- no external deployment, production publication, hosting, CMS mutation,
  unapproved network call, or destructive cleanup.

Every failed or rejected attempt remains evidence. Never discard it by making
a replacement preparation or live run.

## Pre-start checklist

Before making a credential available to the live command, require all of the
following:

1. E0001-20 is committed at final source and its implementation, replay,
   formatting, scoped tests, complete host CLI gate, edition validation, and
   independent audits pass.
2. The retained credential-free readiness packet has passed exact ordered
   10/10 replay and independent digest/argv verification after E0001-20.
3. The final packet workspace is private and unchanged, the ready record is
   immutable, and its recorded modes and SHA-256 still match.
4. The exact Gate B decision still authorizes the direct endpoint, exact model,
   one synthetic run, and both spend ceilings.
5. `OPENAI_BASE_URL` is absent. No credential is written into argv, shell
   history, a file, evidence, or this document.
6. The operator has the replay-verified packet's exact workspace and persisted
   `next_argv`, preparation/evaluation IDs, binding digest, ready-record digest,
   and retained authority bindings in view. The runtime, not the operator,
   allocates the run ID on the first successful start claim.
7. No live-v2 run, provider attempt, preview artifact, failure, or earlier live
   effect already exists for the replay-verified packet.

Any failed item is a stop, not an invitation to repair the argv manually.

## First start: persisted argv only

Execute the replay-verified packet's persisted `next_argv` exactly once as the
first `live-start`. It is intentionally not reproduced in this runbook. Do not
construct a `live-start` command from the IDs above, do not change argument
order or spelling, and do not substitute a different policy path, model,
endpoint, workspace, preparation, evaluation, or delegation.

E0001-20 derives one durable claim from the exact existing bindings and setup.
The first successful claim allocates and returns the run ID. An exact start
replay adopts and returns that already persisted run rather than creating a
replacement, and it must never add another provider dispatch. This is crash
recovery, not authority for a second start:

- only the first exact `next_argv` execution may acquire the claim and create
  the run; it normally performs the initial dispatch, exact start replay never
  dispatches, and only the validated pristine `live-resume` branch below may
  perform the first dispatch after a post-claim/pre-dispatch crash;
- if process outcome is uncertain and the returned run ID is available, execute
  only its emitted workspace-bound watch argv before considering replay;
- replay only the same persisted `next_argv`, never a reconstructed command;
- require the replay to return the exact existing run ID and unchanged provider
  dispatch ledger; and
- after the run reaches the approval boundary, never invoke `live-start`
  again. Use only policy-bearing `live-resume` for that same run.

If a live command reports that the run is already executing in the workspace,
stop that contender. A contending `live-resume` is read-only: it has not
written evidence, constructed a gateway, sent a provider request, or performed
the publication. A first execution of the persisted `live-start` performs its
atomic start claim before lease acquisition. If that command acquired the
claim, it may retain exactly the claim plus its Running run, checkpoint zero,
and Started event zero before reporting contention. It still has not appended
a Prepared or later checkpoint, constructed a gateway, sent a provider
request, or performed the publication.

Do not loop, force-unlock, delete a lock artifact, start another process, or
create a replacement run. Establish whether the original operator process is
still active and let it finish or terminate normally. The operating system
releases the lease when the owning process exits, including after abrupt
process death. After start-time contention, replay only the exact persisted
`next_argv`; do not construct a command. Require `already_started` for the same
retained run ID and an unchanged zero-dispatch ledger, then use only the
emitted workspace-bound watch and state-specific recovery argv. After resume
contention, inspect the same run with its already emitted watch argv and use
only its permitted policy-bearing same-run resume. Never use generic resume or
the approval UI for either branch.

Stop if start replay creates or selects another run, changes the start identity
or goal, adds a dispatch, or returns evidence inconsistent with the first run.

If exact start replay returns `already_started`, it emits only array-form
`review_argv`, `live_resume_argv`, and `watch_argv`; it does not emit a decision
argv. Execute its emitted `review_argv` first and take exactly one of these
branches:

- If watch proves the visible pristine v2 state and ledgers — the same Running
  run, Started event zero, zero attempts, dispatches, turns, tool attempts,
  usage, and cost, and no step, request, decision, execution, evaluation,
  artifact, failure, or terminal evidence — execute the `already_started`
  result's emitted policy-bearing `live_resume_argv` exactly once. Require its
  internal exact run/checkpoint/event-prefix and fresh-epoch validation to
  succeed before dispatch. This is same-run recovery of the interrupted first
  start and is the only allowed recovery path to perform its first provider
  dispatch.
- If watch proves the same run is already waiting for approval but the
  original waiting output was lost, execute that same emitted policy-bearing
  `live_resume_argv` exactly once to re-emit the request's exact decision argv.
  It must not execute the tool or add a provider dispatch before a decision.
- For any other state, stop. In particular, never resume a `dispatching`,
  `response_received`, ambiguous, malformed, mixed-version, substituted, or
  unexpectedly terminal trace through this recovery branch.

Generic `agent resume` remains forbidden in every branch. A second replay or
resume is not authorized merely because an operator lost output; inspect the
same run again and preserve its evidence.

## Workspace-bound watch and exact-Human decision

The waiting result from the first start or authorized recovery `live-resume`
must emit array-form `review_argv`, `approve_argv`, `deny_argv`,
`live_resume_argv`, and `watch_argv` values. Before executing any of them,
verify that every applicable argv contains the exact replay-verified workspace
and returned run or request ID, that the decision argvs contain the exact sealed Human
`01a053eb-7527-7320-b9c1-199817173725`, and that `live_resume_argv` contains the
exact absolute live-policy path. Missing, string-form, ambiguous, or divergent
argv is a stop.

Execute the emitted `review_argv` byte-for-byte. It must be a workspace-bound
`agent watch` for the returned live run. Do not construct a watch command from
the retained identifiers or substitute another workspace or run ID.

The first watch after the model requests the tool must show all of the
following before a Human decision:

- the exact persisted `<LIVE_RUN_ID>` and one waiting step/request;
- `state_kind` exactly `agent_runtime_v2` (not null, v1, mixed, unknown, or
  malformed state);
- run status `waiting_for_input` and step status `waiting_for_approval`;
- exact agent, actor, delegation, edition, manifest, environment, version,
  idempotency key, run, step, request, call, and input-digest bindings;
- the exact `proof_content_v2_release_publish` call with these five and only
  these five arguments: `idempotency_key`, `edition_id`, `environment`,
  `version_label`, and `manifest_digest`;
- one signed Agent request and no decision, execution, tool success, artifact,
  or second mutation; and
- no ambiguous, terminal-failed, budget-exceeded, or duplicate dispatch/effect
  evidence.

The watch must expose the sealed request, its exact required Human, and the
same actionable context. Stop if the required Human is absent or differs, any
additional Human is offered, or any displayed value differs from the start
result and replay-verified packet.

Only after that exact five-argument review, execute one emitted decision argv
byte-for-byte: `approve_argv` to approve or `deny_argv` to deny. The permitted
argv is the workspace-bound `approval approve` or `approval deny` array emitted
for this request with the exact sealed Human; it is not a hand-built generic
approval command. Never retype it, replace its workspace/request/Human, or use
an approval argv copied from another output.

Do not start `approval ui`: it is not part of this evidence path, and its
private session fragment currently prints to stdout. No UI session URL,
fragment, token, or browser state may be created or captured for this run.

For live-v2, a successful decision response must contain no generic resume
command. A non-null recommendation for `proof agent resume` is a stop; only
the emitted policy-bearing `live_resume_argv` may advance the same run.

## Same-run recovery after approve or deny

After the decision process stops, recover only the same run by executing the
emitted `live_resume_argv` byte-for-byte. It must be workspace-bound, name the
returned live run, use `agent live-resume`, and carry the exact absolute policy
path `/home/dakota/projects/proof-platform/evals/release-manager-live-v1.json`.

Never use generic `agent resume`. Never use `live-start` after the approval
boundary. The live-resume command must reload the original run, preflight,
authority, policy, bindings, signed request, decision, step, attempt ledger,
and process-epoch chronology before it can execute or call the provider.

If approval was denied, resume only to durably reconcile that same denial and
terminal state; it must not execute the tool. If approval was granted, require
the exact same run and step, exactly one successful publication mutation,
original proof, one immutable artifact, and the exact terminal report.

## Immediate stop and escalation rules

Stop without creating a replacement run, retrying an ambiguous request, or
cleaning evidence if any of these occurs:

- the final-source or replayed readiness gate is incomplete, drifted, or
  below 10/10;
- the persisted argv cannot be executed exactly, or any operator proposes a
  reconstructed start command;
- a start replay does not return the same existing run or changes the provider
  dispatch count;
- watch reports null, v1, mixed, unsupported, malformed, noncontiguous, or
  digest-invalid runtime state;
- the workspace, run, agent, actor, delegation, step, request, call,
  idempotency key, edition, manifest, version, environment, input digest, or
  exact five arguments differ anywhere;
- the approval review is not actionable, does not expose only the sealed Human,
  or the request is missing, expired, substituted, already decided, or bound
  to another step/run;
- anyone reconstructs or substitutes the emitted exact-Human decision argv,
  uses generic `agent resume`, or invokes a second `live-start` after the
  approval boundary;
- a process dies with a provider attempt in `dispatching` or
  `response_received`, or any attempt is `ambiguous`; never automatically
  retry an attempt whose byte/effect boundary is uncertain;
- the model requests the wrong tool or arguments, requests an additional tool,
  or attempts an unapproved external effect;
- provider/model/endpoint/settings, synthetic boundary, authority, policy,
  request/tool-schema/check-set/tamper-set/pricing digests, or process epochs
  drift;
- dispatch, turn, retry, tool, token, output-token, duration, calculated-cost,
  or owner-spend limits are exceeded or cannot be accounted for as required;
- proof, approval signature/chronology, artifact path/bytes/digest, replay,
  final references, or single-effect evidence is invalid;
- a duplicate mutation, second artifact, second live run, unrecorded failure,
  or missing/rewritten failed attempt is observed; or
- the sealed live evaluation is anything other than exact ordered 17/17,
  10,000 basis points, with no failed or budget-exceeded event.

Preserve the exact state and report the stop. Do not make the evidence look
successful by deleting a run, attempt, artifact, proof, or evaluation.

## Operator evidence capture

Record only the redacted, shareable evidence needed by E0001-04:

- final source revision and final host/readiness gate results;
- replay-verified preparation, preflight evaluation, run, step, approval request,
  approval decision, proof, publication, and live evaluation IDs;
- exact provider/model/settings and synthetic target values;
- ordered event chronology and redacted process epoch/attempt/response IDs;
- policy, bindings, trace, check-set, tamper-set, pricing, request,
  tool-schema, artifact, proof, approval, and replay digests;
- attempts, dispatches, logical turns, retries, tool attempts, successful
  mutations, input/output/total tokens, duration, calculated cost, reported or
  explicitly unavailable provider cost, and both ceilings;
- failed/rejected attempts without removing them; and
- cleanup/rollback disposition and residual risks.

Never record or copy:

- `OPENAI_API_KEY`, its presence check output, or any environment value;
- the approval UI session URL, fragment, bearer token, browser storage, or a
  screenshot containing them;
- `.proof/keypair.json`, `.proof/approvers/*.json`, rotated private keys, or
  other private signing material;
- `.proof/storage/storage.db`, its WAL/sidecars, a database dump, or the full
  `.proof`/workspace tree; or
- raw provider bodies or other material outside the approved synthetic and
  redacted evidence fields.

Do not destructively clean the private workspace before Gate C. If Gate C
defers or rejects, retain the sealed ledger and sole local artifact unchanged;
cleanup requires a separate authorized retention decision.

## Independent terminal verification

The non-author verifier starts only after the operator stops and an initial
credential-free watch proves a sealed terminal run. The verifier must not use
this procedure to advance a waiting, running, failed-in-recovery, or ambiguous
run.

Capture the first terminal watch outside the workspace with provider variables
removed:

```bash
rtk env -u OPENAI_API_KEY -u OPENAI_BASE_URL \
  /home/dakota/projects/proof-platform/target/debug/proof \
  --workspace '<FINAL_WORKSPACE>' agent watch '<LIVE_RUN_ID>' \
  > /tmp/e0001-live-watch-before.json
```

Before replay, require `state_kind=agent_runtime_v2`, run `succeeded`, exactly
one succeeded `release.publish::v2` step, exactly one `completed` event, no
failure/budget event, exact-one `proof-release-manager-live/v1` evaluation,
all 17 ordered checks passed, and score 10,000. If any condition fails, stop;
do not call live-resume.

Recompute and exact-compare the sealed outcome using the policy-bearing live
path with no credential or base URL:

```bash
rtk env -u OPENAI_API_KEY -u OPENAI_BASE_URL \
  /home/dakota/projects/proof-platform/target/debug/proof \
  --workspace '<FINAL_WORKSPACE>' agent live-resume '<LIVE_RUN_ID>' \
  --policy-file /home/dakota/projects/proof-platform/evals/release-manager-live-v1.json \
  > /tmp/e0001-live-terminal-replay.json
```

The replay must return the exact persisted `completed` run, output, and 17/17
evaluation without reading a credential, constructing a provider, sending a
request, advancing the process epoch, appending an event/checkpoint/evaluation,
or executing another effect. Generic `agent evaluate` is not the live
evaluator and must not be used.

Capture the watch again and require byte identity:

```bash
rtk env -u OPENAI_API_KEY -u OPENAI_BASE_URL \
  /home/dakota/projects/proof-platform/target/debug/proof \
  --workspace '<FINAL_WORKSPACE>' agent watch '<LIVE_RUN_ID>' \
  > /tmp/e0001-live-watch-after.json
```

```bash
rtk cmp --silent \
  /tmp/e0001-live-watch-before.json /tmp/e0001-live-watch-after.json
```

```bash
rtk sha256sum \
  /tmp/e0001-live-watch-before.json /tmp/e0001-live-watch-after.json
```

Both SHA-256 values must be identical. Any difference is a stop and failed
verification, not permission to replay again.

Finally, use workspace-bound read-only commands to confirm the one executed
approval and original proof:

```bash
rtk env -u OPENAI_API_KEY -u OPENAI_BASE_URL \
  /home/dakota/projects/proof-platform/target/debug/proof \
  --workspace '<FINAL_WORKSPACE>' approval list
```

```bash
rtk env -u OPENAI_API_KEY -u OPENAI_BASE_URL \
  /home/dakota/projects/proof-platform/target/debug/proof \
  --workspace '<FINAL_WORKSPACE>' verify '<LIVE_PROOF_ID>'
```

The verifier records only the approved redacted evidence summary, commands and
results. The raw `/tmp` captures remain private operator material and are not
committed or copied into edition records.

## 2026-08-31 execution record — stopped at retry exhaustion

The operator satisfied the final-source entry barrier at revision
`357ec1196069ee7c42ee1f55c5e8f2f47e0cf6da`: locked Kernel 106/106, Storage
133/133, Runtime 122/122, and CLI 93/93 passed; scoped formatting passed; the
current CLI binary built; and the exact credential-free preparation replay
remained 10/10 with output SHA-256
`758847ef1a6f7be775b1fb90a86e962cf9eb93ffa9f03d9c754dd58d2b348bd7`.
A distinct non-author independently verified the ready record, its private
modes, both readiness digests, exact bindings, persisted argv, and zero prior
runs for the live agent.

The operator loaded and exact-compared the packet's array-form `next_argv`,
then executed it once without a base-URL override. Direct OpenAI Responses,
exact `gpt-5.6-sol`, default synchronous tier, `store=true`, the frozen
synthetic goal, and the one strict release function were preserved. Run
`01a057fe-0a47-7fe1-a607-5f350f90cd9b` started at
`2026-08-31T13:24:21.700208419Z` and sealed failed at
`2026-08-31T13:24:25.329439675Z`.

The immutable chronology is:

1. Sequence 0: `started`.
2. Sequence 1: `model_requested`; attempt
   `01a057fe-0a5c-7ce3-b67c-a2e1ee5f101b` returned explicit HTTP 429 without
   a response ID and became `rejected_retryable/http_429`.
3. Sequence 2: `model_requested`; the sole retry
   `01a057fe-13e1-74d2-874f-7db04fd0fb8d` was dispatched with the identical
   request digest and sealed `failed_terminal/retry_limit_exhausted`.
4. Sequence 3: `failed`; terminal error `live automatic retry limit exceeded`.

No waiting result or decision argv existed, so the operator did not invoke
review, approve, deny, `live-resume`, generic resume, or a second start. The
terminal ledger contains two provider dispatches, one retry, no committed
model response, no logical model turn, no tool attempt, no approval, no
mutation, no artifact, no proof, and no publication. Committed usage is zero
tokens and calculated committed-usage cost is 0 micro-USD; provider cost is
explicitly unavailable and is not treated as zero. No budget-exceeded or
ambiguous event exists.

Live evaluation `01a057fe-1894-75b0-ad3c-947e70cee86c` correctly sealed failed
at 5/17 and 2,941 basis points. Complete trace digest is
`6200c3570e16b477d6063645f2b80ee0939152d1f1e5cca9a3fee149ead04aeb`;
runtime-state digest is
`ded05e4cd7c7f96d8c312d9f42d1399674850b6db185c0cce02a7d7324b7e9fa`;
live bindings digest is
`0b1c12f1f1b9087bc967de71e01c943b17cb15650a7082337bf0e65c5b9d8d00`.
The redacted start and watch captures remain private, mode `0600`, with
SHA-256 values
`f32fd42758bfb6aa413b277a352cba6189df4f6e7fc6c5dd488bc30c68194fea`
and
`144b02b2a5215e4fd74ac04c96dc5e3ea936923620539ae887145de366d031ba`.
A distinct non-author then performed two credential-free read-only watches;
the results were byte-identical at the latter digest and independently
confirmed the exact failure chronology, counters, evaluator result, zero
effects, and absence of ambiguity or a budget event. The verifier did not
sign or advance the run.

This is an immediate-stop result under the frozen rules, not a recoverable
approval pause. The private workspace and failed evidence remain unchanged;
there is no local consequence to roll back. The current one-run/no-replacement
Gate B authority is exhausted. Do not retry, create a replacement preparation
or run, change model or endpoint, or perform a diagnostic provider request
without a new explicit owner decision.

The two dispatch barriers prove one initial call and one retry. They do not
prove that the retry's original cause was another 429: the terminal record
collapses either an explicit 429 or a certified-no-bytes failure into
`retry_limit_exhausted`. Retain that limitation in every downstream summary.
