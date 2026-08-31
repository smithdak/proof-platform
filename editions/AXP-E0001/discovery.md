# E0001 Read-only Discovery

Three `gpt-5.6-luna` agents independently inspected the runtime/provider path,
Content preview path, and live dogfood/evidence path. They made no repository
edits, accessed no `.proof` content, invoked no provider, and caused no external
effect.

## Converged findings

- The runtime already persists `previous_response_id`, pending tool state,
  signed approval requests/decisions, and same-run resume state. The OpenAI
  gateway resends instructions and uses sequential custom function calls,
  matching the current official Responses contract that continuation uses
  `previous_response_id`, custom functions expose application code, and prior
  instructions are not implicitly carried to the next response:
  <https://developers.openai.com/api/reference/cli/resources/responses/methods/create>.
- Provider/model errors currently terminal-fail. A crash or network ambiguity
  after provider completion but before the local response checkpoint can repeat
  a paid model decision unless E0001 freezes a fail-closed attempt protocol.
- The gateway records token counts but not `cost_microusd`; a monetary cap must
  not interpret unknown cost as zero.
- `registry/content/release-publish.json` declares required UUIDv7 idempotency,
  while the v1 input schema has no idempotency key and the handler selects no
  exact-replay policy.
- The current handler generates random release/edition IDs and returns a local
  JSON result without a durable artifact. `ReleasePipeline` constructs a
  separate proof and in-memory manifest rather than exposing one persisted
  artifact bound to the engine's original proof.
- Existing dogfood is a strong deterministic 10/10 fixture but explicitly does
  not establish live provider availability/model quality or an external preview.

## Recommended product slice

Keep E0001 reversible: use a fresh workspace and synthetic content; build one
immutable local preview artifact only after signed approval; verify it against
the requested edition/version/environment and original proof; perform no
external deployment. Treat public contract/replay changes and paid live use as
separate Gate B decisions.

## Discovery path evidence

- Runtime/provider: `crates/proof-agent-runtime/src/{model,openai,runtime}.rs`,
  `crates/proof-kernel/src/agent.rs`, and
  `evals/release-manager-preview-v1.json`.
- Preview/content: `crates/proof-content/src/{handlers,release}.rs`,
  `crates/proof-content/tests/release_pipeline.rs`, and
  `registry/content/release-publish*.json`.
- Dogfood/evaluation: `docs/dogfood/release-manager-preview.md`,
  `crates/proof-agent-runtime/src/trace_eval.rs`, and
  `crates/proof-transport-cli/src/commands/agent.rs`.

The discovery reports recommend `gpt-5.6-sol` for contract/live verification,
`gpt-5.6-terra` for disjoint runtime and Content implementation, and
`gpt-5.6-luna` only for read-only/mechanical evidence work.
