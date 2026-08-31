# Release Manager preview dogfood trace

## Scope

This record captures one completed, deterministic Release Manager run against a
local preview workspace on 2026-08-29. The Responses provider returned scripted
responses, so this run validates the runtime, approval, evidence, continuation,
and persistence paths. It does **not** establish live-provider availability or
model quality. The current `release.publish` handler produces a local governed
release result and signed proof; it does not deploy to an external preview
environment.

An earlier fresh `v2` attempt failed because of a bug in the deterministic
response fixture. That run remains terminally sealed in its own workspace
ledger but is not the subject of this record. The successful run below also
supersedes the older legacy trace: its proof used a bare operation name, which
now fails closed after canonical `operation::version` proof enforcement.

## Recorded identities

| Record | ID or value |
| --- | --- |
| Workspace | `/tmp/proof-release-manager-preview-v3.XK6DKX` |
| Agent | `01a04f1a-2441-7b10-968d-0325b2ed9650` |
| Run | `01a04f1a-53a1-7cb1-8a13-715b1d844d00` |
| Step | `01a04f1a-53c3-7cb0-8d39-cf4bf1bb0eae` |
| Approval request | `01a04f1a-53f2-7d72-9891-a8bb78e143e7` |
| Approval decision | `01a04f1a-65ae-7730-9dd3-f4f9606c3789` |
| Proof | `01a04f1a-820b-78c0-a7b4-5397f900b8ec` |
| Operation | `release.publish::v1` |

The approved tool arguments were:

```json
{
  "environment": "preview",
  "version_label": "2026.08.29-rc1"
}
```

## Trace

The append-only event sequence was:

```text
started
model_requested
model_responded
tool_requested
approval_required
approval_resumed
tool_succeeded
model_requested
model_responded
completed
```

The run made two sequential model calls and one governed tool call. The first
model response requested `release.publish`; the runtime checkpointed that call,
created the signed approval request, and stopped. After the signed decision was
recorded, the same run and step resumed, produced proof
`01a04f1a-820b-78c0-a7b4-5397f900b8ec`, passed the tool result back to the
provider, and completed.

The terminal model output reported the actual release ID, edition ID,
environment, version label, and proof ID returned by the governed tool.

The result contained:

| Field | Value |
| --- | --- |
| Release ID | `01a04f1a-820b-78c0-a7b4-53852260a45a` |
| Edition ID | `01a04f1a-820b-78c0-a7b4-537d2acd0431` |
| Environment | `preview` |
| Version label | `2026.08.29-rc1` |
| Proof ID | `01a04f1a-820b-78c0-a7b4-5397f900b8ec` |
| Model calls | 2 |
| Tool attempts | 1 |
| Total tokens | 111 |
| Duration | 11 seconds |
| Cost | Unknown; the provider did not report cost usage |

The deterministic exchange also verified the implemented Responses
continuation shape: the second request used the first response's
`previous_response_id` and supplied the tool result as a
`function_call_output` with the matching call ID. This is the sequential
function-calling flow documented in the
[official OpenAI function-calling guide](https://developers.openai.com/api/docs/guides/function-calling).
That protocol check does not substitute for a live API run.

## Hardened replay invariants

Current runtime and storage recovery rules are stricter than the happy-path
sequence alone demonstrates. Resuming a run already sealed by a terminal
`failed` or `budget_exceeded` event returns the persisted terminal outcome
without adding a checkpoint or event. Approval request expiration is the
earlier of the configured approval TTL and the run duration deadline, and
resume checks that deadline before validating, reconciling, or executing an
approval, so a late decision cannot dispatch `release.publish`.

The terminal seal also protects the approval request, decision, and execution
evidence bound to the step: exact retries remain idempotent, but missing or
conflicting evidence cannot be inserted after sealing. Persisted principal IDs,
kinds, and public keys are likewise immutable.

## Task-correctness evaluation

The persisted run was evaluated against
[`evals/release-manager-preview-v1.json`](../../evals/release-manager-preview-v1.json):

```bash
proof agent evaluate 01a04f1a-53a1-7cb1-8a13-715b1d844d00 \
  --evaluator release-manager-preview/v1 \
  --policy-file evals/release-manager-preview-v1.json
```

Evaluation `01a04f2f-60e4-79b0-a202-f6b564061f51`, created at
`2026-08-29T20:21:40.169154290Z`, passed all 10 checks with a score of 10,000
basis points. Policy loading rejects unknown top-level and nested fields rather
than silently ignoring them. The evaluation verified the exact ordered call
and arguments, agent allowlist, run actor, the `release.publish::v1` proof
signature and time window, signed human approval and execution evidence, event
digests, run/step topology and timestamp windows, retry lineage, approval
chronology, the terminal output's references to the release, edition,
environment, version, and proof, and the absence of failure events. The
persisted evaluation binds canonical policy digest
`1e33747b44100727056c00407103deedf2b0c852349fd6489aa71d4246569f33` to
trace snapshot digest
`bb649a378c97a2d842e3055a5cd149e1225cff09fdfa6d725f98255b49ad4cc9`, so
later policy reuse or drift in the evidence covered by that snapshot is
detectable. A second read of the same
sealed trace produced the same trace digest, confirming that non-durable
principal read timestamps are excluded from the binding.

The first policy evaluation of this same sealed trace passed 9 of 10 checks but
failed its terminal-output-reference check because the policy used incorrect
JSON pointers. Evaluations are append-only historical assertions, so that
failed record remains in the ledger; correcting the policy and evaluating
again produced later passing records. Earlier digest-stability evaluations
`01a04f1d-ab29-71a0-aacd-2cf836c5e662` and
`01a04f1d-abbb-7af3-be17-7d83dd3b4561` also remain as historical assertions;
the canonical `release-manager-preview/v1` result is the record above. The task
evaluations are stored beside the runtime's separate budget-completion evaluation
`01a04f1a-8263-7350-a062-bd2a4b1e7a4d`.

## Repeatable deterministic fixture verification

The checked-in deterministic fixture is the repeatable acceptance path for
this record. It constructs two independent Release Manager traces from the
same fixed clock, UUIDs, signing-key bytes, call/response IDs, and terminal
evaluation ID, then evaluates each directly with the checked-in
`evals/release-manager-preview-v1.json` policy:

```bash
rtk cargo test -p proof-agent-runtime two_fixed_release_manager -- --nocapture
```

The test requires both traces to pass all **10 checks** (10,000 basis points),
contain `approval_resumed`, contain no failed events, and produce the same
canonical **trace digest**. It verifies the exact `release.publish::v1` call,
the signed approval/resume chronology, signed proof, final report references,
and policy binding without weakening the production trace snapshot.

This is fixture evidence only. It proves deterministic runtime and evaluator
behavior against scripted data; it does not use a provider credential or
network and does not establish live-provider availability or model quality.

## Operator console verification

A second deterministic run exercised the browser approval console against a
fresh pending request for `2026.08.29-rc2`:

| Record | ID or value |
| --- | --- |
| Workspace | `/tmp/proof-release-manager-ui.jcFgpa` |
| Run | `01a04eef-43c2-7a11-941d-a449aad4f4e8` |
| Step | `01a04eef-43e3-7202-934d-ba5be971b6ea` |
| Approval request | `01a04eef-4425-7143-9c21-54e96b1b8552` |
| Approval decision | `01a04ef0-a725-7492-abfb-521179fffe8c` |

The loopback-only console loaded without browser or console errors, displayed
the exact `preview` / `2026.08.29-rc2` arguments, and signed the decision with
the enrolled human identity. After signing, the durable execution record was
still absent, the run remained `waiting_for_input`, the step remained
`waiting_for_approval`, and no `approval_resumed` or `tool_succeeded` event had
been appended. This confirms the console decides only; an explicit
`proof agent resume` remains necessary to execute.

## Ledger and key handling

The complete durable ledger is in:

```text
/tmp/proof-release-manager-preview-v3.XK6DKX/.proof/storage/storage.db
```

It contains the agent definition, run, step, checkpoints, events, approval
request and decision, execution result, evaluation, proof, and execution
context. The workspace also contains private signing material. Do not copy
`.proof/keypair.json`, `.proof/approvers/`, or the entire workspace when sharing
this trace. If the ledger must be archived, copy and protect `storage.db`
separately and review its operation arguments and outputs for sensitive data.
The operator-console workspace listed above has the same private-key handling
requirements. On Unix, the CLI enforces mode `0700` on `.proof` and its rotated
key directory and mode `0600` on current, rotated, and approver private-key
files; opening an older workspace repairs broader modes. The private session
URL was intentionally not recorded.

## Live-provider gate

> **Superseded for AXP-E0001 and later live-v2 execution.** The commands in
> this historical section create a different legacy v1 journey and are not
> authorized for the frozen Release Manager live run. Use
> [`release-manager-live.md`](release-manager-live.md) and only its
> independently verified persisted argv/recovery procedure.

Run the following from the repository root with a fresh workspace. This uses
the real OpenAI endpoint when `OPENAI_BASE_URL` is unset. This environment had
neither an `OPENAI_API_KEY` nor `jq`, so the live gate was not run here; copy the
IDs from each JSON response into the placeholders manually.

```bash
rtk cargo build -p proof-transport-cli --bin proof

LIVE_PREVIEW_WS="$(rtk mktemp -d /tmp/proof-release-manager-live.XXXXXX)"
rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" init
rtk cp -R registry/. "$LIVE_PREVIEW_WS/.proof/registry/"
rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" registry inspect release.publish

export OPENAI_API_KEY='<OPENAI_API_KEY>'
export OPENAI_MODEL='<OPENAI_MODEL>'
unset OPENAI_BASE_URL

rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" approval approver-init

rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" agent create \
  --name release-manager-live \
  --instructions 'Publish only the requested version to preview. Call release.publish exactly once. Do not finish until it returns signed proof. Report the release ID, edition ID, environment, version label, and proof ID.' \
  --model "$OPENAI_MODEL" \
  --tool release.publish::v1 \
  --max-steps 2 \
  --max-model-calls 4 \
  --max-total-tokens 10000 \
  --max-duration-seconds 300 \
  --max-output-tokens-per-call 1024

rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" agent start '<AGENT_ID>' \
  --goal 'Publish version 2026.08.29-rc1 to preview using release.publish, then report the release ID, edition ID, environment, version label, and proof ID.'

rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" agent watch '<RUN_ID>'
rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" approval approve '<REQUEST_ID>' \
  --approver '<APPROVER_ID>' \
  --reason 'Approved for live preview dogfood'
rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" agent resume '<RUN_ID>'
rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" agent watch '<RUN_ID>'
rtk ./target/debug/proof -w "$LIVE_PREVIEW_WS" agent evaluate '<RUN_ID>' \
  --evaluator release-manager-preview/v1 \
  --policy-file evals/release-manager-preview-v1.json
```

The gate passes only if the live model requests `release.publish` with the exact
preview arguments, the run pauses for the signed approval, resume succeeds on
the same run and step, the step contains a valid `release.publish::v1` proof, and
the terminal trace contains no `tool_failed`, `failed`, or `budget_exceeded`
event. A terminal model message or automatic passing evaluation alone is not
sufficient evidence of a successful release.
