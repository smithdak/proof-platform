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
- Formatting: `cargo fmt --check -p <crate>`. Run before reporting done.

## Crate Ownership

Each agent is assigned a disjoint set of crates per wave. Only edit crates in your assigned set. If your task requires a change outside your set (e.g. a shared type needs a new field):

1. Stop and report the needed change to the orchestrator instead of editing it yourself.
2. Do NOT "helpfully" fix compile errors in other crates — those belong to the crate owner or the integration step.

Exception: adding a new variant to an enum or a new field to a struct that you own in the kernel is allowed; updating every downstream match/construction site is the orchestrator's integration job.

## Shared Types & API Contracts

- `contracts/kernel-api.md` defines the canonical shapes of shared types (`RegistryEntry`, `ExecutionContext`, `Proof`, operation naming, error variants). Read it before touching any shared type.
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
