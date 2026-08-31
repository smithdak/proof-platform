# Release Manager Live Preview Contract v1

**Status:** APPROVED and implemented for AXP-E0001 Gate B (B1-B4 and B6);
B5 prerequisites pass and execution is stopped at the unavailable credential
**Contract ID:** `proof-release-manager-live/v1`
**Operation recommendation:** `release.publish::v2`
**Policy:** `evals/release-manager-live-v1.json`
**Owner:** project owner
**Last updated:** 2026-08-30

This is the approved Gate B contract for the AXP-E0001 live Release Manager
preview. It freezes a local-only, synthetic journey. B1-B4 and B6 are
implemented. The recorded deterministic, implementation, host, and readiness
prerequisites activate B5, but no credential is currently available and no
provider attempt has occurred. B5 authorizes only the already frozen direct
request once that credential boundary is satisfied; it never authorizes an
external deployment or production publication.

The design rationale below intentionally preserves its pre-implementation
tense as the immutable Gate B decision record. Current canonical API, registry,
and migration status is reflected in `kernel-api.md` and
`domain-definitions.md`; D-E0001-010 records B5 activation.

## Approved owner decisions

| ID | Decision | Recommendation | Effect of approval |
|---|---|---|---|
| B1 | Public operation contract | Add `release.publish::v2`; leave `v1` active and byte-for-byte compatible | Authorizes the v2 registry/schema/artifact contract below |
| B2 | Kernel and storage prerequisites | Add default-compatible version-aware handler hooks, require explicit delegation IDs for this journey, add SQLite v12 delegation-scope persistence, and implement `SqliteStore::load_delegation` | Lets v1 retain `IdempotencyPolicy::None`, makes v2 exact replay version-specific, and makes live delegation enforcement real |
| B3 | CLI, authority, and recovery | Add strict delegation grant/load/start setup, deterministic preflight before credential access/provider construction, and the provider-attempt state machine below | Authorizes scoped kernel/storage/CLI/runtime work; chain-only or default/unbounded delegation is forbidden |
| B4 | Sealed evaluator | Adopt `proof-release-manager-live-policy/v1`, all declared checks, resolved bindings, digests, and tamper vectors | Authorizes runtime evaluator changes while preserving the deterministic v1 evaluator |
| B5 | Paid live profile | After deterministic preflight, allow one run against the direct OpenAI endpoint using `gpt-5.6-sol`, under the limits below | Separate explicit approval is still required before a credential is read or a request is sent |
| B6 | Migration and external effects | Append SQLite migration v12 for serialized `Delegation.scope`; allow only the separately approved direct provider request and local preview artifact | No preview deployment or other external effect is authorized |

Recommended Gate B result: approve B1-B4 and B6; conditionally approve B5 only
after the deterministic preflight evidence named below is present. Add
the separately owned W2 kernel/storage prerequisites `E0001-06/07`; W3 runtime
and Content `E0001-02/03` start after both. W4 CLI `E0001-08` starts only after
W3 plus `E0001-06/07`, consumes the runtime's lazy-provider seam, and owns the
credential reader. W5 live evidence `E0001-04` follows all implementation; W6
integration `E0001-05` follows W5. If B2 is not approved, select the additive
fallback operation in the compatibility matrix and revise the policy before
implementation; do not improvise in W2.

## Official provider baseline

The contract targets `POST /v1/responses`. The official OpenAI create-response
reference states that `previous_response_id` continues multi-turn state, that
instructions from the previous response are **not** implicitly carried into a
later request, and that custom function tools are supported. It also states
that output item order/type is model-dependent, so parsing MUST scan typed
output items rather than assume item zero is assistant text:
<https://developers.openai.com/api/reference/cli/resources/responses/methods/create>.
The official function-calling guide defines returning a function result as a
`function_call_output` input item associated by `call_id`; the exact structured
template in this contract applies that shape:
<https://developers.openai.com/api/docs/guides/function-calling>.

The selected model is the exact ID `gpt-5.6-sol`, not the moving `gpt-5.6`
alias. The official model page lists Responses and function calling support and,
as of this contract date, text pricing of USD 4.00 per million input tokens and
USD 20.00 per million output tokens, with cache writes billed at 1.25 times
uncached input:
<https://developers.openai.com/api/docs/models/gpt-5.6-sol>.

Provider documentation is evidence about the remote API, not a substitute for
Proof's fail-closed persistence. A provider response ID is an opaque cursor. It
MUST NOT be treated as an idempotency key for a governed Proof operation.

## Compatibility and recommendation

| Option | v1 callers | Exact replay | Artifact/identity binding | Kernel impact | Decision |
|---|---|---|---|---|---|
| Change `release.publish::v1` in place | Breaks callers missing the five new fields; changes output and replay behavior | Possible only by breaking v1 | Strong | None | Reject |
| Keep current v1 | Compatible | Registry claims required UUIDv7 but handler selects no replay | Missing edition identity, durable artifact, and original-proof binding | None | Reject for E0001 |
| Add `release.publish::v2` | Fully compatible; v1 remains active | Strong after B2 | Strong and preserves operation lineage | Kernel dispatch plus storage/CLI delegation prerequisites | **Recommend** |
| Add `release.preview.publish::v1` | Fully compatible | Strong with a separate handler key | Strong | None | Fallback only; adds a ninth Content operation and drifts from the approved `release.publish` journey |

### Why B2 and the storage prerequisite are required

The current `OperationHandler` exposes `operation()` but not a version, and
`ExecutionEngine` stores handlers by operation name. Its
`idempotency_policy()` therefore applies to every version of that operation.
Setting exact replay on the current `release.publish` handler would make v1
require a key and would break v1. Registering a second v2 handler would replace
the v1 handler under the same map key.

B2 is a separate Gate B scope request for a kernel owner. The backward-
compatible API recommendation is:

```rust
fn idempotency_policy_for(&self, version: &str) -> IdempotencyPolicy {
    self.idempotency_policy()
}

fn execute_versioned(
    &self,
    version: &str,
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    self.execute(input, context)
}
```

The engine calls the version-aware methods after registry lookup. Existing
implementors compile and behave exactly as before. The Content handler returns
the old policy/behavior for `v1` and required exact replay/v2 behavior only for
`v2`. Equivalent version-keyed registration is acceptable only if its public
compatibility is demonstrated and the owner records that substitution.

The live SQLite path has a second independent blocker. `ExecutionStore` has a
default `load_delegation` that returns `None`, and `SqliteStore` does not
override it. Migration v1 already created `delegations`, but the table and CLI
save/load helpers persist only legacy actions/resources and reconstruct the
newer `Delegation.scope` as `Default::default()` (unbounded). The engine cannot
load and enforce the intended operation/domain scope from SQLite today.

Gate B therefore freezes migration **v12** as the next sequential migration.
It appends one `scope_json TEXT NOT NULL DEFAULT '{}'` column to `delegations`.
`{}` preserves legacy row behavior and is not sufficient for this live
journey. New/updated E0001 grants MUST round-trip this exact scope:

```json
{
  "allowed_operations": ["release.publish"],
  "allowed_domains": ["content"]
}
```

`resource_scope` is omitted from the structured scope and legacy
`allowed_actions`/`resource_scope` remain backward-compatible columns. The
storage owner MUST deserialize `scope_json` first through a storage-local DTO
equivalent to:

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDelegationScopeV1 {
    allowed_operations: Option<Vec<String>>,
    allowed_domains: Option<Vec<String>>,
    resource_scope: Option<String>,
}
```

Thus the general v12 JSON object permits only `allowed_operations`,
`allowed_domains`, and optional `resource_scope`; any other key or malformed
value is a storage load error, never an ignored field or default. `{}` remains
readable solely for legacy compatibility. The E0001 exact grant requires the
first two keys with the singleton arrays above and forbids the structured
`resource_scope` key entirely. This strictness is local to v12 storage decode
and this journey; it does not add `deny_unknown_fields` to the shared kernel
`DelegationScope` or globally change legacy deserialization.

The storage owner implements `SqliteStore::load_delegation` by ID, decoding all
legacy fields plus that DTO. The CLI owner updates grant/save/load/validate
round trips. The live runtime requires
`ExecutionContext.delegation_id == Some(resolved_id)` and a chain containing
that exact loaded grant. A chain without an explicit ID, a missing row,
`{}`/default scope, `None` operation/domain lists, wildcard, structured
`resource_scope`, additional operation/domain, wrong recipient,
revoked/expired/not-yet-valid grant, or validity shorter than the run deadline
fails before credential read.

### Complete proposed RegistryEntry

If B1 is approved, the new registry row has every canonical field below. No
field is inherited from v1:

```json
{
  "operation": "release.publish",
  "domain": "content",
  "version": "v2",
  "action": "content:release_publish",
  "description": "Publish one existing content edition as an immutable local preview artifact",
  "input_schema": "content/release-publish-v2.input.json",
  "output_schema": "content/release-publish-v2.output.json",
  "required_authority": "delegation-grant",
  "governance": "human-only",
  "idempotency": "required-uuidv7",
  "consequence": "content-release",
  "evidence_contract": "operation-effect-v1",
  "benchmark": "B1",
  "status": "active",
  "deprecated_since": null,
  "replacement_operation": null
}
```

## Immutable deterministic preflight and live bindings

The deterministic assertion is a separate immutable record, not part of the
fresh live binding schema. This avoids circularity: the deterministic trace is
sealed before a live run, and the live trace later binds the already-computed
preflight digest.

The CLI preflight verifier creates this strict record only after independently
recomputing and validating the referenced deterministic policy and trace:

```json
{
  "schema": "proof-release-manager-preflight-evidence/v1",
  "policy_path": "evals/release-manager-preview-v1.json",
  "policy_digest": "blake3-256:<hex>",
  "trace_digest": "blake3-256:<hex>",
  "evaluator": "proof-agent-trace/v1",
  "run_id": "<deterministic run uuid>",
  "evaluation_id": "<deterministic evaluation uuid>",
  "evaluation_created_at": "<RFC3339 UTC>",
  "outcome": "passed",
  "score_bps": 10000,
  "passed_checks": 10,
  "total_checks": 10
}
```

Unknown fields are rejected. The preflight evidence digest is the kernel
Generic digest of
`{"schema":"proof-release-manager-preflight-evidence-digest/v1","evidence":<complete record>}`.
The digest is stored beside, never inside, the record. Neither record nor
digest contains a live run ID, live policy digest, or credential state.

The repository live policy is a strict template because a fresh workspace
produces new UUIDs and a new edition digest. Its template digest is computed
first over the committed policy with `$binding` atoms intact. Deterministic
setup then resolves exactly these live bindings:

| Binding | Type and source | Rule |
|---|---|---|
| `preflight_evidence_digest` | BLAKE3-256 | Exact immutable record above |
| `run_id` | UUIDv7 from the created live `AgentRun` | One live run only |
| `agent_id` | UUIDv7 from immutable `AgentDefinition` | Definition has only `release.publish::v2` |
| `agent_principal_id` | enrolled agent principal | Newly generated synthetic-workspace identity |
| `approver_principal_id` | enrolled human principal | Distinct from agent; exact trusted public key is bound |
| `delegation_id` | UUIDv7 from SQLite | Explicit ID; chain-only authority is forbidden |
| `delegation_digest` | BLAKE3-256 | Complete loaded grant with exact operation/domain scope |
| `edition_id` | UUID from newly created synthetic edition | Loaded from the disposable workspace |
| `manifest_digest` | `sha256:<64 lowercase hex>` | Digest of the canonical preview manifest below |
| `idempotency_key` | newly generated UUIDv7 | Unique to this intended publication |
| `version_label` | string | Exactly `2026.08.30-rc1` for the primary E0001 journey |
| `process_epoch_id` | UUIDv7 | Initial live CLI process; resume records a different epoch |

`proof-release-manager-live-bindings/v1` contains exactly these twelve fields
and rejects unknown fields. It does **not** contain its own digest, the resolved
policy digest, or the live trace/evaluation IDs. Its digest is the kernel
Generic digest of
`{"schema":"proof-release-manager-live-bindings-digest/v1","bindings":<complete bindings>}`.

The resolved policy replaces every `$binding` atom with exactly one scalar and
contains no unresolved `$binding` atom. The declared `$runtime`/
`$canonical_json_string` continuation directives remain policy syntax until a
committed tool outcome exists; they are not live bindings. Its digest is
computed only after binding replacement.
The initial `agent_runtime_v2` checkpoint/event then stores the template,
preflight, binding, resolved-policy, exact-check-set, pricing, instructions,
input, parameter-schema, tool-declaration, tool-set, and exact-tamper-vector-set
digests. The live trace digest and live evaluation ID are computed only after
terminal sealing and are therefore never inputs to the binding/resolved-policy
digests.

Chronology is fixed and testable:

1. finish and seal the deterministic run/evaluation;
2. verify it and persist the immutable preflight record/digest;
3. load/validate the exact SQLite delegation and synthetic edition;
4. create the local live `AgentRun`, allocate its run/process-epoch IDs, and do
   not construct a provider;
5. compute template, binding, resolved-policy, tool, outbound-input, pricing,
   exact-check-set, and exact-tamper-vector-set digests using those allocated
   IDs;
6. durably persist the initial v2 checkpoint/event and re-read it;
7. prepare the exact first request, persist its `prepared` attempt, and re-read
   both before any factory invocation;
8. only now invoke the CLI factory, which reads `OPENAI_API_KEY` and constructs
   the provider client;
9. perform the durable `dispatching` barrier, live journey, and trace seal;
10. compute the trace digest, run the exact 17-check evaluator, and append the
   live evaluation.

Any failure in steps 1-7 exits without reading the credential or constructing
an HTTP client. A retry uses the same resolved policy and bindings.

## Additive lazy-provider API seam

E0001-02 owns this public `proof-agent-runtime` seam; E0001-08 consumes it.
Names, field types, and call order below are frozen so the disjoint owners MUST
NOT invent parallel constructors or transport callbacks. All new value types
are `Clone`, immutable after construction, serializable where they contain
evidence, and reject unknown fields when deserialized.

```rust
pub enum LiveRunIntent {
    Start { agent_id: Uuid, goal: String },
    Resume { run_id: Uuid },
}

pub struct LiveBindingInputs {
    pub preflight_evidence_digest: ContentDigest,
    pub agent_principal_id: PrincipalId,
    pub approver_principal_id: PrincipalId,
    pub delegation_id: Uuid,
    pub delegation_digest: ContentDigest,
    pub edition_id: Uuid,
    pub manifest_digest: String,
    pub idempotency_key: Uuid,
    pub version_label: String,
}

pub struct LiveAuthoritySetup {
    pub delegation: Delegation,
    pub delegation_digest: ContentDigest,
    pub delegation_chain: DelegationChain,
}

pub struct LivePolicyMaterial {
    pub template: Value,
    pub template_policy_digest: ContentDigest,
    pub binding_inputs: LiveBindingInputs,
    pub check_set_digest: ContentDigest,
    pub tamper_vector_set_digest: ContentDigest,
    pub pricing_schedule_digest: ContentDigest,
    pub instructions_digest: ContentDigest,
    pub initial_input_digest: ContentDigest,
    pub parameters_schema_digest: ContentDigest,
    pub tool_declaration_digest: ContentDigest,
    pub tool_set_digest: ContentDigest,
}

pub struct LiveRunSetup {
    pub intent: LiveRunIntent,
    pub process_epoch_id: Uuid,
    pub preflight_evidence: Value,
    pub preflight_evidence_digest: ContentDigest,
    pub authority: LiveAuthoritySetup,
    pub policy: LivePolicyMaterial,
}

pub struct ModelGatewayFactoryContext {
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub process_epoch_id: Uuid,
    pub provider: String,
    pub endpoint: String,
    pub requested_model: String,
    pub service_tier: String,
    pub request_body_digest: ContentDigest,
}

pub trait ModelGatewayFactory: Send + Sync {
    fn create(
        &self,
        context: &ModelGatewayFactoryContext,
    ) -> Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError>;
}

pub enum ModelGatewayFactoryError {
    Configuration(String),
    Construction(String),
}
```

`AgentRuntime::new(...)` retains its existing signature and behavior. It wraps
the supplied `Arc<dyn ModelGateway>` in runtime-owned
`FixedModelGatewayFactory`; no existing caller changes. The additive
constructor is exactly `AgentRuntime::new_with_gateway_factory(...)`: its first
seven arguments are byte-for-byte the existing `new` arguments through
`approval_store`, and its eighth is
`Arc<dyn ModelGatewayFactory>`. The additive entry point is
`AgentRuntime::run_live(&self, setup: LiveRunSetup) ->
Result<AgentRuntimeOutcome, AgentRuntimeError>`. Existing `start` and `resume`
remain available for legacy/deterministic callers.

`LiveRunSetup` contains no credential, base-URL override, HTTP client, gateway,
closure that has read a secret, or live trace/evaluation ID. On start, runtime
strictly validates the complete preflight record/digest, exact loaded grant and
chain, template and all static digests, canonical goal, IDs, and limits. It
requires the duplicated preflight and delegation IDs/digests in
`binding_inputs` to equal the enclosing evidence/authority byte-for-byte and
requires the target agent ID/principal to match the immutable definition.
It
then creates the `AgentRun`, forms the exact twelve-field binding by combining
the new run/agent ID and setup process epoch with `binding_inputs`, resolves
the complete policy, recomputes every digest, and persists **the complete
preflight record, loaded authority, resolved bindings, resolved policy, and all
digests** in the initial v2 checkpoint/event. It successfully re-reads and
compares both before request preparation.

Because `run_id` is itself a frozen binding sourced from the newly created
`AgentRun`, `LiveRunSetup` intentionally carries the complete preflight,
authority, policy template, nine non-run binding inputs, and their claimed
digests—not a caller-forged resolved policy. All that material enters runtime
before run creation. Runtime alone supplies run/agent/process IDs, then resolves
and recomputes the binding/policy before the initial checkpoint/event. This is
the only permitted two-phase resolution.

On resume, E0001-08 reloads the immutable preflight record, SQLite delegation,
chain, committed policy template, and static digest material without reading a
credential and passes them in a new `LiveRunSetup { intent: Resume { ... } }`.
Runtime loads the run and complete v2 checkpoint, requires the new process epoch
to differ, recomputes all supplied material, and compares it to the originally
stored preflight/authority/resolved bindings/policy/digests. The original
binding (including its initial process epoch) stays immutable; each resumed
attempt records the new process epoch. Any missing or changed value seals/fails
before factory invocation.

Runtime does not call `ModelGatewayFactory::create` during constructor,
registration, setup validation, run creation, initial checkpoint/event writes,
resume reload/comparison, tool/schema/request validation, budget validation, or
`prepared` attempt persistence/re-read. It invokes the factory exactly when a
fully sealed `prepared` attempt reaches the network-dispatch boundary, passing
the exact `ModelGatewayFactoryContext`. E0001-08's factory implementation is
the sole code that reads `OPENAI_API_KEY`, verifies `OPENAI_BASE_URL` is unset,
and builds `OpenAiResponsesGateway`; registering that factory captures no key
or client. Factory failure is a terminal certified-zero-byte failure and does
not increment provider dispatches; its persisted class is `terminal`, code is
`gateway_factory_failed`, and detail is redacted.

After factory success, the runtime performs the mandatory durable
`dispatching` checkpoint/event/re-read barrier below, then and only then calls
the gateway. A crash after factory creation but before durable `dispatching`
has certified zero request bytes; the ephemeral gateway is discarded and
resume redoes all validation before a factory may be invoked again. A crash
after durable `dispatching` remains ambiguous. Tests use separate factory-
invocation and gateway-send spies. Every pre-secret failure above asserts both
counts are zero; fixed-gateway legacy tests prove `AgentRuntime::new` remains
compatible.

## `release.publish::v2` request

The input is strict Draft 2020-12 JSON Schema and rejects unknown fields at
every object level:

```json
{
  "idempotency_key": "<uuidv7>",
  "edition_id": "<uuid>",
  "environment": "preview",
  "version_label": "2026.08.30-rc1",
  "manifest_digest": "sha256:<64 lowercase hex>"
}
```

Rules:

- all five fields are required;
- `idempotency_key` MUST be UUIDv7 and is also the `publication_id`;
- `environment` is the literal `preview`; v2 cannot target production;
- `version_label` is 1-64 ASCII characters matching
  `^[0-9A-Za-z][0-9A-Za-z._-]{0,63}$` and is never used as a path component;
- `manifest_digest` MUST equal the digest independently recomputed from the
  persisted edition before approval is requested and again inside the handler;
- the complete canonical input, including the idempotency key, is the approval
  request input and the exact-replay input; and
- the request contains no repository source, secret, private key, provider
  credential, real customer content, absolute path, URL, or free-form notes.

The kernel `OperationInput` digest of this object is the request digest bound
by the signed approval request, step, proof, replay claim, artifact, policy,
and final evaluation.

## Exact outbound prompt and custom function declaration

The only model-visible material is the resolved synthetic goal, the exact
sealed instructions, the exact one-function declaration, and—after approved
execution—the canonical tool result/proof ID. Repository source, arbitrary
files, environment-variable values, credentials, private keys, database
content, real customer content, and approval private material are prohibited. The
machine-readable values under `outbound_data` and `tool.declaration` in
`evals/release-manager-live-v1.json` are canonical. The literals and JSON
values copied below MUST compare byte-for-byte for strings and as identical
values after Proof kernel canonical JSON serialization for objects; prose never
overrides them.

The sealed instructions are exactly:

```text
You are the Proof Release Manager for one synthetic local preview. Use exactly the declared proof_content_v2_release_publish function once with the supplied edition_id, preview environment, version_label, manifest_digest, and idempotency_key. Do not request, infer, quote, or disclose repository files, source code, secrets, credentials, private keys, customer data, or network resources. After the tool result, report only the publication_id, edition_id, environment, version_label, manifest_digest, artifact relative_path, artifact digest, and proof_id from that result. On continuation, these complete instructions still apply.
```

The resolved initial goal is exactly this single line, with brace atoms
replaced from the live bindings:

```text
Publish synthetic edition {edition_id} to preview as {version_label} using manifest {manifest_digest} and idempotency key {idempotency_key}.
```

The braces above are template atoms, not literal outbound braces. Replacement
uses the exact bound scalar values with no escaping or whitespace change. The
only continuation input is the following structured template, also copied
identically from policy:

```json
{
  "type": "function_call_output",
  "call_id": {"$runtime": "committed_function_call_id"},
  "output": {
    "$canonical_json_string": {
      "ok": true,
      "result": {"$runtime": "canonical_release_publish_v2_output"},
      "proof_id": {"$runtime": "proof_id"}
    }
  }
}
```

At request construction, `$runtime` atoms are replaced only from the committed
function call and governed execution outcome. `$canonical_json_string` then
encodes its resolved object as canonical compact JSON, so the actual Responses
input item is exactly
`{"type":"function_call_output","call_id":"<committed call id>","output":"<canonical compact JSON string>"}`.
The input digest and request-body digest cover that resolved item. An error tool
result fails the live gate and produces no continuation request.

The exact Responses custom function is:

```json
{
  "type": "function",
  "name": "proof_content_v2_release_publish",
  "description": "Publish one existing content edition as an immutable local preview artifact",
  "parameters": {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "additionalProperties": false,
    "required": [
      "idempotency_key",
      "edition_id",
      "environment",
      "version_label",
      "manifest_digest"
    ],
    "properties": {
      "idempotency_key": {"type": "string", "format": "uuid"},
      "edition_id": {"type": "string", "format": "uuid"},
      "environment": {"type": "string", "const": "preview"},
      "version_label": {
        "type": "string",
        "minLength": 1,
        "maxLength": 64,
        "pattern": "^[0-9A-Za-z][0-9A-Za-z._-]{0,63}$"
      },
      "manifest_digest": {
        "type": "string",
        "pattern": "^sha256:[0-9a-f]{64}$"
      }
    }
  },
  "strict": true
}
```

The runtime validates UUIDv7 separately because JSON Schema `format: uuid`
does not distinguish versions. The parameter-schema digest is the kernel
Generic digest of
`{"schema":"proof-openai-function-parameters-digest/v1","parameters":<exact parameters object>}`.
The tool-declaration digest similarly wraps the complete declaration under
`proof-openai-function-declaration-digest/v1`; the tool-set digest wraps the
one-element ordered array under `proof-openai-tool-set-digest/v1`.

The request-body digest wraps the complete credential-free JSON body under
`proof-openai-responses-request-digest/v1`. That body includes exact requested
model, instructions, resolved input, previous response ID/null, the complete
one-element tool array, `tool_choice`, `parallel_tool_calls`, `store`,
`stream`, `background`, `service_tier`, and `max_output_tokens`. The bearer
credential and HTTP headers are excluded. Every dispatch stores all four
digests plus the instructions and input digests; schema/name/description/tool
order/settings substitution is therefore detectable.

## Canonical preview manifest and artifact

The handler loads `.proof/data/editions/<edition_id>.json` within the supplied
workspace. The preview manifest is:

```json
{
  "schema": "proof-content-preview-manifest/v1",
  "edition_id": "<uuid>",
  "edition_content_digest": "sha256:<64 lowercase hex>",
  "objects": [
    {
      "object_id": "<uuid>",
      "locale": "<string>",
      "content_digest": "sha256:<64 lowercase hex>"
    }
  ]
}
```

Objects are sorted by UUID byte order. Unknown fields are rejected. Each object
digest is recomputed from the complete canonical persisted object using the
Content SHA-256 contract. `edition_content_digest` is recomputed from the
edition's sorted complete object array; a stored value is never trusted alone.
`manifest_digest` is the Content SHA-256 digest of the complete canonical
manifest and MUST match the request.

After signed approval, the acquired exact-replay claim is the only path that
may create this artifact:

```json
{
  "schema": "proof-content-preview-artifact/v1",
  "publication_id": "<same uuidv7 as idempotency_key>",
  "request": {
    "idempotency_key": "<uuidv7>",
    "edition_id": "<uuid>",
    "environment": "preview",
    "version_label": "2026.08.30-rc1",
    "manifest_digest": "sha256:<64 lowercase hex>"
  },
  "request_digest": "blake3-256:<64 lowercase hex>",
  "manifest": "<complete proof-content-preview-manifest/v1 object>",
  "created_at": "<RFC3339 UTC timestamp>",
  "created_by": "<agent principal id>"
}
```

The canonical path relative to the workspace is
`.proof/data/previews/<edition_id>/<idempotency_key>.json`. Only validated UUID
components form the path. The artifact is canonical compact UTF-8 JSON with no
trailing newline. It is written with no-overwrite semantics through a temporary
file in the same directory and an atomic publication step. An existing equal
file is not rewritten; an existing unequal file is a conflict. Temporary files
are preserved as failed evidence unless an operator explicitly removes them.

The artifact digest is the kernel BLAKE3-256 Generic digest over canonical JSON:

```json
{
  "schema": "proof-content-preview-artifact-digest/v1",
  "artifact": "<the complete artifact object>"
}
```

No digest field appears inside the artifact, avoiding a self-reference. The
operation output carries the digest; the engine's original proof signs the
output digest, which cryptographically binds artifact -> output -> proof
without reconstructing or embedding a second proof.

## Canonical operation output and proof

The strict output is:

```json
{
  "operation": "release.publish",
  "data": {
    "publication_id": "<same uuidv7 as idempotency_key>",
    "edition_id": "<requested uuid>",
    "environment": "preview",
    "version_label": "2026.08.30-rc1",
    "manifest_digest": "sha256:<64 lowercase hex>",
    "artifact": {
      "schema": "proof-content-preview-artifact/v1",
      "relative_path": ".proof/data/previews/<edition_id>/<idempotency_key>.json",
      "digest": "blake3-256:<64 lowercase hex>"
    },
    "published_at": "<same instant as ExecutionContext.timestamp>",
    "published_by": "<agent principal id>"
  }
}
```

All nested objects reject unknown fields. `published_at` and artifact
`created_at` equal the execution context timestamp. The handler returns this
output; the `ExecutionEngine` then returns the one original
`ExecutionOutcome { output, proof }`. The proof MUST have:

- `operation == "release.publish::v2"`;
- actor equal to `published_by` and the requesting agent;
- delegation ID equal to the resolved delegation;
- `input_digest` equal to the canonical v2 request digest;
- `output_digest` equal to the kernel `OperationOutput` digest of the complete
  output above;
- timestamp equal to `published_at`, artifact `created_at`, and
  `ApprovalExecution.executed_at`; and
- a valid Ed25519 signature under the enrolled engine principal.

The artifact does not contain a copied proof. The completed approval execution
and exact-replay ledger preserve the original output and proof. A verifier
loads the artifact from the output's relative path, recomputes manifest,
artifact, input, and output digests, verifies both signatures and approval
chronology, and compares every identifier. Any mismatch fails.

## Exact replay and mutation boundary

The replay tuple is
`("release.publish", "v2", idempotency_key)`. The existing E0000 exact-replay
rules remain unchanged:

```json
{
  "operation": "release.publish",
  "version": "v2",
  "idempotency_key": "<uuidv7>",
  "input_digest": "blake3-256:<hex>",
  "state": "claimed | completed | failed",
  "outcome": "<present only when completed: exact output plus original proof>"
}
```

- `Acquired` alone may enter the handler and create the artifact.
- `Completed` returns byte-equivalent canonical output and the original proof;
  it creates no artifact, proof, approval execution, or domain mutation.
- the same tuple with different canonical input is `Conflict` before mutation;
- `claimed`/`failed` is indeterminate and never expires, lease-steals, deletes,
  or automatically executes;
- handler or proof failure best-effort marks failed; replay-completion failure
  leaves claimed and blocked; and
- the existing edition and canonical content are read-only. The single allowed
  domain mutation is creation of the one preview artifact after approval.

Crash boundaries are fail-closed. A crash after artifact publication but
before proof/replay completion can leave an artifact with a claimed row. It is
not success and MUST NOT be automatically retried. The operator preserves the
row and artifact for reconciliation and uses a new UUIDv7 only after explicitly
abandoning that intended publication. E0001 performs no destructive cleanup.

## Persistent provider-attempt state machine

The runtime writes strict `agent_runtime_v2` checkpoints and MUST continue
reading legacy `agent_runtime_v1` checkpoints. Unknown fields, missing fields,
counter regressions, and unknown enum values are invalid. The complete v2
state is:

```json
{
  "schema": "proof-agent-runtime-state/v2",
  "agent_id": "<uuidv7>",
  "run_id": "<uuidv7>",
  "started_at": "<RFC3339 UTC>",
  "process_epoch_id": "<uuidv7>",
  "previous_response_id": null,
  "next_input": "<complete tagged ModelInput>",
  "pending_tool": null,
  "authority": {
    "delegation_id": "<uuidv7>",
    "delegation_digest": "blake3-256:<hex>",
    "allowed_operations": ["release.publish"],
    "allowed_domains": ["content"],
    "valid_until": "<RFC3339 UTC>"
  },
  "policy_evidence": {
    "preflight_evidence": "<complete proof-release-manager-preflight-evidence/v1>",
    "loaded_delegation": "<complete loaded Delegation>",
    "delegation_chain": "<complete matching DelegationChain>",
    "resolved_bindings": "<complete proof-release-manager-live-bindings/v1>",
    "resolved_policy": "<complete proof-release-manager-live-policy/v1 with no unresolved $binding atoms>"
  },
  "policy_binding": {
    "preflight_evidence_digest": "blake3-256:<hex>",
    "template_policy_digest": "blake3-256:<hex>",
    "bindings_digest": "blake3-256:<hex>",
    "resolved_policy_digest": "blake3-256:<hex>",
    "check_set_digest": "blake3-256:<hex>",
    "tamper_vector_set_digest": "blake3-256:<hex>",
    "pricing_schedule_digest": "blake3-256:<hex>",
    "instructions_digest": "blake3-256:<hex>",
    "initial_input_digest": "blake3-256:<hex>",
    "parameters_schema_digest": "blake3-256:<hex>",
    "tool_declaration_digest": "blake3-256:<hex>",
    "tool_set_digest": "blake3-256:<hex>"
  },
  "provider": {
    "name": "openai",
    "endpoint": "https://api.openai.com/v1/responses",
    "requested_model": "gpt-5.6-sol",
    "service_tier": "default",
    "tool_choice": "auto",
    "max_output_tokens": 1024,
    "store": true,
    "stream": false,
    "background": false,
    "parallel_tool_calls": false
  },
  "attempts": [],
  "counters": {
    "logical_model_turns": 0,
    "provider_dispatches": 0,
    "retries": 0,
    "tool_attempts": 0,
    "successful_publication_mutations": 0
  },
  "cumulative_usage": {
    "input_tokens": 0,
    "output_tokens": 0,
    "total_tokens": 0
  },
  "cumulative_cost": {
    "provider_cost_microusd": null,
    "provider_cost_status": "unavailable",
    "calculated_cost_microusd": 0,
    "pricing_schedule_id": "proof-openai-gpt-5.6-sol-pricing/2026-08-30",
    "pricing_schedule_digest": "blake3-256:<hex>"
  },
  "final_output": null,
  "terminal_error": null
}
```

Every network dispatch appends one durable strict `ProviderAttempt`:

```json
{
  "schema": "proof-provider-attempt/v1",
  "attempt_id": "<uuidv7>",
  "logical_turn": 1,
  "dispatch_ordinal": 1,
  "retry_of": null,
  "state": "prepared",
  "process_epoch_id": "<uuidv7>",
  "prepared_at": "<timestamp>",
  "dispatched_at": null,
  "finished_at": null,
  "request": {
    "endpoint": "https://api.openai.com/v1/responses",
    "requested_model": "gpt-5.6-sol",
    "previous_response_id": null,
    "instructions": "<exact sealed instructions above>",
    "input": "<exact resolved synthetic ModelInput>",
    "instructions_digest": "blake3-256:<hex>",
    "input_digest": "blake3-256:<hex>",
    "parameters_schema_digest": "blake3-256:<hex>",
    "tool_declaration_digest": "blake3-256:<hex>",
    "tool_set_digest": "blake3-256:<hex>",
    "request_body_digest": "blake3-256:<hex>",
    "function_names": ["proof_content_v2_release_publish"],
    "tool_declarations": ["<complete exact function declaration above>"],
    "tool_choice": "auto",
    "max_output_tokens": 1024,
    "service_tier": "default",
    "store": true,
    "stream": false,
    "background": false,
    "parallel_tool_calls": false
  },
  "response": null,
  "failure": null
}
```

For `response_received`/`committed`, `response` is exactly:

```json
{
  "response_id": "resp_<opaque>",
  "returned_model": "gpt-5.6-sol",
  "response_body_digest": "blake3-256:<hex>",
  "decision_digest": "blake3-256:<hex>",
  "usage": {
    "input_tokens": 1,
    "output_tokens": 1,
    "total_tokens": 2
  },
  "provider_cost_microusd": null,
  "provider_cost_status": "unavailable",
  "calculated_cost_microusd": 25,
  "cumulative_input_tokens": 1,
  "cumulative_output_tokens": 1,
  "cumulative_total_tokens": 2,
  "cumulative_provider_cost_microusd": null,
  "cumulative_provider_cost_status": "unavailable",
  "cumulative_calculated_cost_microusd": 25,
  "pricing_schedule_id": "proof-openai-gpt-5.6-sol-pricing/2026-08-30",
  "pricing_schedule_digest": "blake3-256:<hex>"
}
```

For a failed state, `failure` is exactly
`{"class":"<certified_no_bytes|explicit_429|terminal|ambiguous>","code":"<stable code>","detail":"<redacted string>"}`.
Response and failure are mutually exclusive. Secrets and complete request/
response bodies are not stored; their canonical digests and the complete
declarations above make substitutions auditable. The bearer credential is read
only after preflight and never persisted or logged.

State invariants are monotonic: attempts append; ordinals are contiguous;
counters equal the attempt/step ledger; cumulative usage and calculated cost
equal the exact sum of committed responses; cumulative provider cost is the
sum only when every committed response reports it, otherwise it remains null
with status `unavailable`; retry lineage is acyclic; request/tool/policy/pricing
digests and complete request/tool declarations never change;
`previous_response_id` advances only from a committed response;
requested/returned model must match; and a terminal state cannot be cleared. A
resume creates a new process epoch but retains every prior attempt and
cumulative value.

### States and transitions

| State | Meaning | Allowed transition | Automatic network retry? |
|---|---|---|---|
| `prepared` | Budget reserved; exact request digest checkpointed before I/O | `dispatching`, `failed_retryable`, `failed_terminal` | No dispatch has occurred; one bounded retry may be scheduled |
| `dispatching` | Network call may have crossed the process boundary | `response_received`, `rejected_retryable`, `failed_terminal`, `ambiguous` | Never directly |
| `response_received` | Complete 2xx JSON body and response ID are in memory but not yet durably committed | `committed`, `ambiguous` | No |
| `committed` | Validated decision, usage, actual returned model, response ID, and new cursor are atomically represented by checkpoint plus `model_responded` event | terminal for this attempt | Not applicable |
| `rejected_retryable` | Explicit provider rejection proves no response object was created | new `prepared` with `retry_of` | At most once for the entire run |
| `failed_retryable` | Local failure certifies zero request bytes were sent | new `prepared` with `retry_of` | At most once for the entire run |
| `failed_terminal` | Deterministic local/config/auth/schema/budget failure | run failure | Never |
| `ambiguous` | Provider completion or charge may exist but no committed local response exists | operator reconciliation only | **Never** |

The `prepared -> dispatching` transition is a mandatory durable commit barrier.
Before the HTTP layer may open/connect/write any request byte, the runtime MUST:

1. increment `provider_dispatches` and set `dispatched_at`/process epoch;
2. persist the complete attempt with `state=dispatching` in a v2 checkpoint;
3. append the matching immutable `model_requested` event with attempt ID and all
   request/tool digests;
4. successfully re-read both records and verify exact agreement; and only then
5. invoke the HTTP send function.

If any persistence/re-read step fails, zero request bytes are sent and the run
fails closed. Tests use a gateway spy that panics/counts if called before the
barrier and storage fault injection at every step. A crash after the durable
barrier is conservatively ambiguous on restart even if no byte was actually
sent; safety takes precedence over retry availability.

`committed` requires the durable response checkpoint and its immutable event to
agree. If either write fails after a provider response, the attempt is
`ambiguous`; later startup detects the uncommitted `dispatching` or
`response_received` state and seals the run failed without dispatching a tool.

### Failure classification

Safely retryable is deliberately narrow:

- local serialization/validation failure before dispatch is normally terminal;
  a retry is allowed only for an explicitly transient local resource error;
- DNS/connect refusal is retryable only when the HTTP client certifies no
  request bytes were written;
- an explicit HTTP 429 response with no Responses object/ID is retryable;
- HTTP 400/401/403/404 is terminal; and
- timeout, cancellation during I/O, connection reset after possible write,
  HTTP 408, any 5xx, malformed/partial 2xx JSON, missing response ID or usage,
  non-completed Responses status, parse ambiguity, or crash after dispatch is
  ambiguous. It is never automatically retried.

The retry is a new attempt ID with the same logical turn, byte-equivalent
canonical request digest, same previous response cursor, same bindings, and
`retry_of` pointing to the prior attempt. A changed request is terminal policy
failure. There is at most one retry across the entire run, not one per turn.

No tool is parsed or dispatched from `response_received`; only a `committed`
attempt may advance the cursor and expose a model decision. The runtime resends
the complete frozen instructions and exact custom-function declaration on
every request, including requests using `previous_response_id`, because prior
instructions are not carried automatically by the Responses API.

## Live versus deterministic evaluation

Two evaluations remain separate historical assertions:

1. **Deterministic preflight.** The existing
   `evals/release-manager-preview-v1.json` scripted journey must pass all ten
   existing checks with score 10/10 before any credential is read. Its policy
   and stable trace digests, evaluator ID, run ID, and evaluation ID are stored
   in the separate immutable preflight record; the live binding contains only
   that record's digest.
2. **Live gate.** `evals/release-manager-live-v1.json` resolves fresh bindings,
   is digested before provider I/O, and evaluates the sealed live trace. It
   never replaces or weakens the deterministic assertion.

The live policy and every nested object reject unknown fields. Policy schema,
template digest, binding digest, resolved policy digest, trace schema, complete
trace digest, exact-check-set digest, exact-tamper-vector-set digest, run
revision, step/event/approval/provider-attempt counts, and artifact verification
digest are recorded in evaluation metrics. Evaluation rows are append-only. A
later evaluation does not overwrite a failure.

### Declared live checks

The evaluator output MUST contain exactly 17 checks, each exactly once, in the
order below. No implementation-defined, omitted, renamed, duplicated, or extra
check is allowed. `passed_checks == total_checks == 17`, `score_bps == 10000`,
and outcome `passed` are all required. The exact-check-set digest is the kernel
Generic digest of
`{"schema":"proof-release-manager-live-check-set-digest/v1","check_ids":[<the ordered 17 IDs below>]}`
and is bound before credential read and repeated in evaluation metrics.

| # | Check ID | Exact pass condition |
|---:|---|---|
| 1 | `deterministic_preflight` | Named deterministic evaluation passed 10/10 before credential read and its policy/trace digests match the binding |
| 2 | `sealed_policy_and_trace` | Template, bindings, resolved policy, exact check/tamper sets, and complete trace digests recompute; no unknown/unresolved policy field exists |
| 3 | `provider_endpoint_model` | Provider `openai`, direct `https://api.openai.com/v1/responses`, requested and returned model exactly `gpt-5.6-sol`, `store=true`, sequential custom functions, no proxy/hosted/MCP tool |
| 4 | `synthetic_data_boundary` | Outbound bytes equal the sealed instructions, resolved synthetic goal or canonical tool output, and exact tool metadata; their digests match and secrets/source/files/private/customer data are absent |
| 5 | `identity_authority_allowlist` | Enrolled agent, distinct enrolled human, explicit loaded delegation ID/digest, exact `release.publish` + `content` scope through deadline, and one-tool `release.publish::v2` allowlist agree; no chain-only/default/wildcard scope |
| 6 | `exact_tool_call` | Exactly one observed tool step has the fully resolved five-field arguments; no additional call exists |
| 7 | `approval_integrity` | Signed request and signed approve decision bind actor, operation, version, complete input digest, trusted human, validity window, and execution |
| 8 | `approval_restart_chronology` | No artifact/execution precedes approval; a recorded process epoch changes between approval wait and resume; execution is within approval/run windows |
| 9 | `provider_attempt_recovery` | Every attempt follows the state graph, digests/lineage match, retries <=1, and no ambiguous/uncommitted attempt exists |
| 10 | `budgets` | Calls, logical turns, tool attempts, tokens, per-call output, elapsed time, and retry count are within every hard limit |
| 11 | `cost_accounting` | Pricing schedule digest matches; conservative calculated cost is known and <=120000 micro-USD; provider-reported cost is actual or explicitly unavailable, never zero-filled |
| 12 | `artifact_identity_manifest` | Artifact request, edition, preview environment, label, manifest, actor, timestamps, path, and output are exact and all digests recompute |
| 13 | `artifact_file_integrity` | Persisted canonical bytes at the safe relative path equal the verified artifact and its digest; no second preview artifact exists |
| 14 | `proof_integrity` | Original proof signature, composite operation, actor/delegation, input/output digests, timestamp, approval execution, and persisted proof all agree |
| 15 | `exact_replay_single_effect` | One acquired mutation and one completed ledger outcome exist; any replay returns the same output/proof and artifact mutation count remains one |
| 16 | `terminal_report` | Run succeeded and the final model report contains edition ID, environment, version label, manifest digest, artifact digest/path, publication ID, and proof ID |
| 17 | `no_failure_or_unapproved_external_effect` | No tool/runtime/budget failure, canonical-content write, deployment, tool/network side effect, or path outside the preview boundary exists; the one Gate-B-approved direct Responses request is the sole permitted external effect |

### Required tamper vectors

The evaluator MUST use exactly the following ordered 20 IDs, identical to the
policy array. Deletion, addition, rename, duplicate, or reorder is failure:

1. `preflight_record_policy_trace_score_count_or_digest_change`
2. `binding_change_unresolved_binding_or_circular_digest_field`
3. `check_id_cardinality_order_or_exact_set_digest_change`
4. `provider_requested_returned_model_endpoint_or_setting_substitution`
5. `function_name_description_parameters_schema_tool_set_or_request_body_substitution`
6. `provider_attempt_state_request_retry_response_cost_usage_or_epoch_change`
7. `dispatching_checkpoint_event_reread_barrier_missing_or_reordered`
8. `ambiguous_attempt_reclassified_as_retryable_or_committed`
9. `delegation_id_row_digest_scope_chain_recipient_validity_or_revocation_change`
10. `approval_request_argument_version_expiry_or_signature_change`
11. `approver_identity_key_outcome_signature_or_chronology_change`
12. `missing_restart_or_execution_before_approval`
13. `artifact_path_traversal_second_file_bytes_or_digest_change`
14. `artifact_edition_environment_version_manifest_actor_or_timestamp_change`
15. `operation_output_publication_artifact_or_manifest_change`
16. `proof_id_body_signature_actor_delegation_digest_or_timestamp_change`
17. `replay_output_proof_substitution_or_second_mutation`
18. `usage_call_token_duration_retry_price_schedule_or_cost_change`
19. `failure_event_unallowlisted_call_content_mutation_or_unapproved_external_effect`
20. `terminal_output_reference_removed_or_substituted`

`tamper_vector_set.expected_cardinality == 20`, IDs are unique, and exact order
is required. Its digest is the kernel Generic digest of
`{"schema":"proof-release-manager-live-tamper-vector-set-digest/v1","tamper_vector_ids":[<the ordered 20 IDs above>]}`.
The resolved policy contains the array and invariant object; the initial and
every resumed runtime checkpoint/event store `tamper_vector_set_digest`; and
the final evaluation metrics repeat it after recomputation from the exact
array. A check result cannot substitute for a tamper vector or vice versa.

## Exact live limits and cost rules

| Limit | Frozen value | Accounting rule |
|---|---:|---|
| Provider | `openai` | Case-sensitive in sealed evidence |
| Endpoint | `https://api.openai.com/v1/responses` | `OPENAI_BASE_URL` MUST be unset; no proxy |
| Model | `gpt-5.6-sol` | Exact requested and returned string; no alias/failover |
| Service | default synchronous Responses, `store=true` | No background, streaming, conversation, or hosted tools |
| Parallel tool calls | `false` | One custom function only |
| Provider dispatches | 4 | Counts every network attempt, including rejected/retry/ambiguous |
| Logical model turns | 3 | A committed response advances one turn |
| Automatic retries | 1 per run | Only `failed_retryable`/`rejected_retryable`; zero after ambiguity |
| Tool attempts | 2 hard ceiling | Passing trace has exactly one requested/executed step and no failure |
| Successful publication mutations | 1 | Exact replay does not increment |
| Total tokens | 10000 | Sum of provider `usage.total_tokens` across all responses; missing usage is ambiguous |
| Output tokens per request | 1024 | Sent as `max_output_tokens`; includes visible and reasoning tokens per official API reference |
| Wall clock | 300 seconds | From run start through terminal seal, including approval wait/restart |
| Approval TTL | min(15 minutes, run deadline) | Existing rule makes the 300-second deadline authoritative |
| Calculated cost | 120000 micro-USD | Runtime hard budget; unknown calculation fails before tool dispatch |
| Owner live-spend authorization | USD 0.15 | One E0001 primary live run; another run needs new approval |

The sealed pricing schedule is
`proof-openai-gpt-5.6-sol-pricing/2026-08-30`: charge every input token at the
conservative cache-write rate of 5 micro-USD per token (USD 5/M) and every
output token at 20 micro-USD per token (USD 20/M). Do not claim a cache-read
discount. At 10,000 total tokens with the maximum 4,096 output tokens, the
worst calculated text cost is 111,440 micro-USD. The 120,000 micro-USD runtime
limit leaves 8,560 micro-USD rounding/headroom and remains below the owner's
USD 0.15 authorization. No priced built-in tool is allowed.

For each response record raw usage, `provider_cost_microusd` (nullable),
`provider_cost_status` (`reported` or `unavailable`),
`calculated_cost_microusd`, pricing-schedule ID/digest, and cumulative cost.
The Responses result currently supplies token usage but not an authoritative
invoice cost in the consumed contract. `provider_cost_status=unavailable` is
therefore acceptable only when raw token usage is complete and conservative
calculated cost is known. Null never becomes zero. Missing/malformed usage,
unknown model, unknown price schedule, arithmetic overflow, schedule drift, or
calculated cost over the limit stops before tool dispatch and fails the gate.
An organization/project dashboard hard cap is defense in depth, not evidence
that the run stayed within this policy.

## Impact matrix

| Surface | Impact if approved | Gate/owner action |
|---|---|---|
| Public operation | Add active v2 only after implementation; v1 unchanged and active | B1, Content/registry owner |
| Kernel API | Two default-compatible version-aware handler methods (or approved equivalent); explicit-ID delegation path remains authoritative | B2, separate `proof-kernel` owner prerequisite |
| Kernel shared structs/errors/proof/signatures | No field, variant, signature, or proof-format change | None |
| SQLite schema/migrations | Append v12 `delegations.scope_json`; v11 replay remains unchanged | B2/B6, separate `proof-storage` owner prerequisite |
| SQLite execution store | Implement `load_delegation` and strict scope decode; default trait fallback is insufficient | B2, storage owner |
| Runtime API | Add explicit execution-authority/delegation input and durable `agent_runtime_v2` provider attempts; preserve old constructor/checkpoint reads | B3, E0001-02 |
| Evaluator | Add strict live-policy parsing/checks; deterministic preview v1 semantics and digest remain unchanged | B4, E0001-02 |
| CLI/transport | Add scoped delegation round-trip and live-start/resume preflight boundary; consume runtime lazy-provider seam | B3, W4 `E0001-08` after W3 |
| Authority | Persist/load exact operation+domain scope; require `delegation_id`; reject chain-only/default/wildcard/unbounded scope | B2/B3; kernel+storage+CLI+runtime tests |
| Secrets | `OPENAI_API_KEY` environment only, first read after durable preflight/live binding; never persisted/logged; base URL unset | B3/B5; CLI owner controls ordering |
| Provider retention | Direct Responses request uses `store=true` so `previous_response_id` continuation works; synthetic payload only | B5; changing retention/store requires Gate B |
| Filesystem | One immutable artifact under the disposable workspace after approval; edition/content read-only | B1/B6 |
| External effect | One conditionally approved paid provider request is allowed; no other network effect, deployment, hosting, CMS, or production publication | B5/B6 |
| Destructive effect | None; failed/temporary evidence is preserved | None |

## Downstream implementation contract

### Prerequisite K — kernel owner (`E0001-06`)

Before E0001-02/E0001-03, a separately assigned `proof-kernel` owner MUST add
the default-compatible version-aware handler hooks (or owner-approved
equivalent), route replay/execute by requested version, and prove:

- all existing handlers compile and v1 behavior is unchanged;
- `release.publish::v1` selects `None` while v2 selects exact replay;
- an explicit delegation ID loads and enforces operation/domain scope;
- chain-only context is not accepted by the E0001 live entry path; and
- missing/mismatched ID, scope, recipient, validity, and revocation fail before
  handler entry.

Required evidence: `rtk cargo fmt --check -p proof-kernel`,
`rtk cargo test -p proof-kernel`, and
`rtk scripts/test-scoped.sh proof-kernel` after prerequisite owners quiesce.

### Prerequisite S — storage/migration owner (`E0001-07`)

A separately assigned `proof-storage` owner MUST append migration v12, never
edit/reorder an earlier migration, and implement `SqliteStore::load_delegation`.
Tests MUST cover v11->v12 upgrade; empty legacy default; exact new scope
save/load round trip; malformed and unknown-key scope failure; general known
`resource_scope` round trip; E0001 rejection of that key; missing ID;
revoked/expired round trip; operation/domain mismatch; and engine execution
using a loaded grant. The migration down path removes only the v12 column
(using a safe table rebuild if the supported SQLite cannot drop it directly)
and preserves all legacy delegation columns/rows.

Required evidence: `rtk cargo fmt --check -p proof-storage`,
`rtk cargo test -p proof-storage`, and
`rtk scripts/test-scoped.sh proof-storage` after prerequisite owners quiesce.

### W4 C — CLI/setup consumer (`E0001-08`)

Only after `E0001-02/03/06/07`, the separately assigned
`proof-transport-cli` owner MUST:

1. extend delegation `--scope` to accept and persist exact
   `operation_scope.allowed_operations=["release.publish"]` and
   `allowed_domains=["content"]`, retaining legacy actions/resources;
2. add `agent live-start <agent_id> --goal <synthetic-goal> --policy-file
   evals/release-manager-live-v1.json --preflight-evaluation-id <uuid>
   --delegation-id <uuid>`;
3. add `agent live-resume <run_id> --policy-file
   evals/release-manager-live-v1.json` (authority/policy IDs come from the
   checkpoint and are reloaded/revalidated, never supplied anew);
4. drive the runtime successfully through chronology steps 1-7 in the binding
   section before the first
   `std::env::var("OPENAI_API_KEY")`, base-URL lookup, HTTP client creation, or
   provider factory invocation; and
5. implement the CLI-owned `ModelGatewayFactory` without reading a key during
   registration and construct the gateway only when runtime invokes it at the
   frozen prepared-attempt dispatch boundary.

Ordering tests remove `OPENAI_API_KEY` and prove an invalid/missing preflight,
policy, delegation, scope, or initial checkpoint reports that local error—not a
credential error—and a gateway-construction/send spy remains zero. Valid local
preflight with missing key fails only at the credential boundary. Resume tests
revalidate the immutable record/scope before credential read. CLI evaluation
must emit exactly the frozen 17 checks.

Required evidence: `rtk cargo fmt --check -p proof-transport-cli`,
`rtk cargo test -p proof-transport-cli`, and
`rtk scripts/test-scoped.sh proof-transport-cli` after prerequisite owners
quiesce.

### W3 E0001-02 — runtime owner

E0001-02 MUST start only after K/S (`E0001-06/07`):

1. preserve all current deterministic evaluator and legacy checkpoint behavior;
2. implement the complete strict v2 state/attempt/response/failure schemas and
   every monotonic/cumulative invariant above;
3. classify gateway failures at the exact boundary above and expose enough
   transport detail to distinguish certified-zero-byte, explicit rejection,
   terminal, and ambiguous outcomes;
4. enforce the durable dispatching checkpoint+event+reread barrier before any
   request byte and survive restart without resetting calls, tokens, duration,
   retry, usage, or calculated/provider cost;
5. resend complete instructions/tools on each Responses request, use
   `previous_response_id` only after a committed response, scan typed output
   items, and reject multiple function calls;
6. accept only the explicit loaded delegation ID plus matching chain, persist
   its digest, put both in every execution context, and reject chain-only,
   default/wildcard/unbounded/changed authority before credential read;
7. allow only exact `proof_content_v2_release_publish`, strict parameters,
   description, tool set, settings, and exact arguments; bind every digest;
8. stop and seal on ambiguous completion without parsing/dispatching a tool;
9. record requested/returned model, endpoint policy, usage, nullable provider
   cost, conservative calculated cost, attempt/process epochs, and all digests;
10. emit exactly the 17 unique ordered checks and exact-set digest using immutable events,
    steps, approvals, proof, replay evidence, and a safe workspace-relative
    artifact verification record;
11. implement and export the exact additive `LiveRunSetup`, binding/policy
    material, `ModelGatewayFactory`, context, compatible constructor wrapper,
    and `run_live` seam above, including zero-invocation spies;
12. produce dispatch-barrier fault injection, restart, retryable, ambiguous,
    schema/tool substitution, missing-usage, unknown-cost, over-budget,
    changed-request, approval, artifact-tamper, and terminal-seal tests with
    zero live provider calls; and
13. stop with a cross-owner request on any additional kernel/storage/CLI or
    migration need.

Passing evidence is the E0001-02 task's scoped format/tests plus stable 10/10
deterministic preview evaluation. It MUST NOT read a real credential in tests.

### W3 E0001-03 — Content/registry owner

E0001-03 MUST remain blocked until B1/B2 are approved and prerequisites K/S
(`E0001-06/07`) are available. It then MUST:

1. add strict `release.publish::v2` registry/input/output schemas and leave all
   v1 files and behavior unchanged;
2. implement version-aware v1/v2 Content handling with exact replay only for
   v2;
3. load the requested existing edition read-only, recompute every object,
   edition, and manifest digest, and reject any mismatch before artifact write;
4. require preview, validated label, UUID/UUIDv7, and no unknown fields;
5. create exactly the canonical artifact/path/output above only after the
   kernel has acquired replay and the signed approval path invokes the handler;
6. use atomic no-overwrite file publication and surface unequal-existing,
   partial-write, and path-boundary failures without destructive cleanup;
7. return only handler output and let the engine mint/persist the original
   proof; never synthesize a Content-local replacement proof;
8. expose an independent verifier that checks canonical bytes, safe path,
   request/manifest/artifact/output/proof/approval/replay binding;
9. test first execution, completed replay with original proof, changed-input
   conflict, in-progress/failed claim, crash boundaries, mutation count one,
   and every artifact/output/proof tamper vector; and
10. stop on a migration, v1 edit, kernel/transport change, external effect, or
    inability to return the original proof.

E0001-02 and E0001-03 may agree on test fixtures only through this contract and
committed schemas; no hidden chat value may define a field, digest, path, or
recovery choice.

W4 `E0001-08` consumes the public E0001-02 factory seam and may not edit or
replace it. W5 `E0001-04` performs the approved live journey only after
`E0001-02/03/06/07/08`; W6 `E0001-05` performs quiescent integration only after
W5. No earlier wave may read a credential or call the provider.

## Rollback

Before a live run, rollback is removal of unapproved v2 registry/code and the
live policy; v1 remains the serving contract. After a run, quiesce writers and
preserve the exact-replay row, run ledger, approval evidence, provider attempts,
artifact, proof, policy, and trace digests. Disable discovery/use of v2 before
reverting code. Never delete or rewrite completed/claimed/failed replay rows or
signed evidence to make rollback appear clean. The local disposable workspace
may be archived; destructive cleanup is a separate operator action outside
E0001.

Migration v12 is independently reversible only while writers are quiescent.
Its down path removes `delegations.scope_json` and no other field, using the
storage project's supported safe table-rebuild procedure when direct column
drop is unavailable. It MUST preserve every legacy delegation row and column
and restore schema version 11. Before downgrade, export or otherwise preserve
the v12 scope values as operator evidence because v11 cannot enforce or
round-trip them. Downgrade disables the live journey: legacy `{}`/unbounded
scope is never a substitute for the required operation/domain scope.

If the provider model/price/API behavior drifts before the live gate, do not
silently update this v1 policy. Create a new versioned pricing/policy decision
and return to Gate B. A failed or ambiguous live attempt remains failed
evidence; a second run is not covered by the original USD 0.15 approval.

## Non-goals

- production publication, external preview deployment, hosting, CMS adapters,
  web/file/MCP/hosted tools, proxying, provider failover, parallel tools, or
  background/streaming Responses;
- changing, deprecating, or sunsetting `release.publish::v1`;
- changing proof/approval signatures, general authority semantics, migration
  11, or E0000 exact-replay recovery; SQLite change is limited to the approved
  additive v12 delegation-scope column and its rollback;
- native teams, scheduler, leases, aggregate budgets, sandboxing, or a general
  workflow engine;
- real content, repository source, arbitrary files, secrets, private keys, or
  customer data in provider input;
- cost reconciliation against a later invoice or claims about provider data
  retention beyond the exact request configuration; and
- automated cleanup, retry after ambiguity, or history rewriting.

## Approval record

The product owner approved the recommendation on 2026-08-30, recorded as
`D-E0001-002` and `D-E0001-003` in
`editions/AXP-E0001/decisions.md`:

- B1-B4 and B6 are approved exactly as frozen in this contract.
- Migration v12 and the disjoint K (`E0001-06`), S (`E0001-07`), Runtime
  (`E0001-02`), Content (`E0001-03`), and CLI C (`E0001-08`) ownership are
  approved.
- B5 is conditionally approved for one direct `gpt-5.6-sol` Responses run,
  within USD 0.15, only after W2-W4 pass, the fresh deterministic preflight is
  independently 10/10, the distinct trusted human approver and credential are
  available, and all redaction/retention/failure evidence prerequisites pass.
- No credential read, provider request, external deployment, production
  publication, or second live attempt is authorized by implementation
  approval alone.
