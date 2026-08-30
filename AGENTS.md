# Proof Platform — Agent & Contributor Conventions

This file governs how agents and humans work in this repository. Read it fully before making any change.

## Build & Test Commands

- Always prefix shell commands with `rtk` (token-optimized proxy). Example: `rtk cargo test -p proof-kernel`.
- **Scoped testing is mandatory.** Only run tests for crates you changed plus crates that depend on them:
  - Changed `proof-kernel`: `rtk cargo test -p proof-kernel`
  - Changed `proof-content`: `rtk cargo test -p proof-content`
  - Changed `proof-storage`: `rtk cargo test -p proof-storage`
  - Changed a transport (`http`, `mcp`, `cli`): test that transport only.
  - Never run bare `rtk cargo test` (full workspace) unless explicitly told to. Other agents may be mid-edit on other crates; full-suite failures caused by concurrent work are not yours to fix.
- Formatting: `rtk cargo fmt --check -p <crate>`. Run before reporting done.
- Use `rtk scripts/test-scoped.sh <crate>` when a changed crate's reverse
  workspace dependents must also be tested. Use `--list` to inspect the impact
  set without executing tests.

## Crate Ownership

Each agent is assigned a disjoint set of crates per wave. Only edit crates in your assigned set. If your task requires a change outside your set (e.g. a shared type needs a new field):

1. Stop and report the needed change to the orchestrator instead of editing it yourself.
2. Do NOT "helpfully" fix compile errors in other crates — those belong to the crate owner or the integration step.

Exception: adding a new variant to an enum or a new field to a struct that you own in the kernel is allowed; updating every downstream match/construction site is the orchestrator's integration job.

## AXP Edition Delivery

- Product work is organized as tracked AXP Product Editions under `editions/`.
  An edition is a shippable user outcome; waves are dependency-safe fan-out
  batches; tasks are single-owner assignments.
- Before accepting an edition task, read `contracts/axp-experience.md`, the
  edition charter, `assignments.tsv`, and the exact task packet.
- `assignments.tsv` is the dispatch source of truth. A worker may edit only its
  listed writable paths and its unique handoff file.
- Only one edition may have active writers. Read-only discovery for the next
  edition may overlap with current-edition integration.
- Gate A requires owner approval before product implementation fan-out. Gate B
  is required for public contracts, migrations, authority/security, secrets,
  destructive or external effects, paid provider work, and material scope
  expansion. Gate C is the owner release decision.
- Model routing follows `editions/MODEL_POLICY.md`. Start bounded work on the
  lowest tier that passes its evaluation and escalate only the failed task.
- Workers preserve a durable handoff containing changed files, commands and
  results, assumptions, risks, and cross-owner requests. Completion requires
  the task's acceptance evidence, not an agent's self-report.
- The orchestrator validates an edition with
  `rtk scripts/swarm.sh validate <AXP-E####>`. The quiescent final gate is
  `rtk scripts/swarm.sh verify <AXP-E####> --quiescent` and runs only after all
  writers stop.

## Shared Types & API Contracts

- `contracts/kernel-api.md` defines the canonical shapes of shared types (`RegistryEntry`, `ExecutionContext`, `Proof`, operation naming, error variants). Read it before touching any shared type.
- `contracts/domain-definitions.md` defines the canonical scope, operation set, and decision history for each domain module. Read it before starting any domain work. Wave specs must reference the domain definition — the swarm builds toward what is written there, not toward chat.
- When adding a field to a shared struct, make it backward-compatible: use `Option<T>` with `#[serde(default)]` where possible, and never remove or rename existing fields.
- Operation names follow `domain.action` (e.g. `schema.create`, `object.create`). Version strings follow `v<N>` (e.g. `v1`). Proof storage uses the `operation::version` composite key format. Do not invent new formats.
- New SQLite migrations must be appended to the `MIGRATIONS` list with the next sequential version number. Never edit or duplicate an existing migration.

## Error Handling

- Every new `ExecutionError` variant must be added to the HTTP transport's error-to-status mapping in the same wave (report to orchestrator if the transport is outside your crate set).
- Use `thiserror` for error enums. Error messages are user-facing: be specific.

## Testing

- Every new public function gets at least one unit test.
- Every new storage function gets a round-trip integration test.
- Every new HTTP endpoint gets a tower oneshot integration test (see `crates/proof-transport-http/tests/http_integration.rs` for the pattern).
- Test helpers (like `RecordingStore`) live in the crate that owns the trait, exported via `lib.rs`. Do not define duplicates.

## Git Discipline

- Never commit. The orchestrator commits after integration and full-suite verification.
- Never push.
- Keep changes minimal and scoped to the task description.
