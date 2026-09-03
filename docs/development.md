# Development guide

This guide covers the repository workflow for contributors. Product work that
belongs to an AXP Edition also follows its task packet, path ownership, and
owner gates.

## Prerequisites

- Stable Rust and Cargo.
- Git.
- SQLite tooling is optional but useful for local inspection.
- `jq` is optional for JSON examples and fixture work.

## Build a crate

Work from the repository root and keep checks scoped to the package you
changed:

```bash
cargo build -p proof-kernel
cargo fmt --check -p proof-kernel
cargo test -p proof-kernel
```

Do not use a full workspace test as the default development loop. The
repository may contain concurrent edition work, and scoped failures are easier
to attribute.

## Test reverse dependents

Use the repository helper when a changed crate's workspace dependents must also
compile and pass:

```bash
scripts/test-scoped.sh proof-kernel --list
scripts/test-scoped.sh proof-kernel
```

Inspect the impact set before executing it. Transport changes generally test
that transport only; shared kernel or storage changes can have a wider set.

Repository agents must prefix shell commands with `rtk` and follow the exact
rules in [`AGENTS.md`](../AGENTS.md). The unprefixed commands in this guide are
for human shells.

## Common package checks

| Changed package | Minimum local checks |
|---|---|
| `proof-kernel` | `cargo fmt --check -p proof-kernel`; `cargo test -p proof-kernel` |
| `proof-agent-runtime` | `cargo fmt --check -p proof-agent-runtime`; `cargo test -p proof-agent-runtime` |
| `proof-storage` | `cargo fmt --check -p proof-storage`; `cargo test -p proof-storage` |
| A transport crate | Format and test that transport package |
| Edition records only | `scripts/swarm.sh validate <AXP-E####>`; `git diff --check` |

Every new public function needs a unit test. Every new storage function needs a
round-trip integration test. Every new HTTP endpoint needs a Tower `oneshot`
integration test.

## Repository boundaries

| Area | Change when… |
|---|---|
| `crates/proof-kernel` | A shared identity, registry, authority, execution, evidence, or durable contract changes |
| Domain crate | Domain-owned models, validation, or handlers change |
| `crates/proof-storage` | Persistence or migrations change |
| `crates/proof-agent-runtime` | Model/tool orchestration, budgets, checkpoints, or recovery changes |
| Transport crate | Protocol adaptation or transport-owned behavior changes |
| `contracts` | A public or cross-crate normative contract changes |
| `registry` / `schemas` | Discoverable operations or wire validation changes |
| `evals` | Acceptance policy or deterministic fixtures change |
| `editions` | Product authority, ownership, evidence, or status changes |

Read [`contracts/kernel-api.md`](../contracts/kernel-api.md) before changing a
shared type, and [`contracts/domain-definitions.md`](../contracts/domain-definitions.md)
before changing a domain operation.

## Migrations

SQLite migrations are append-only. Add the next sequential version to the
migration list; never edit or reuse an existing migration number. Include a
round-trip test for fresh, upgraded, and reopened paths as applicable.

Public rollback policy and any destructive effect require explicit product
authority. Tests must use disposable databases.

## Error handling

- Use typed errors and `thiserror` for error enums.
- Keep messages specific enough for operators to act on.
- Map every new public execution error in each affected transport.
- Fail closed when authority, identity, evidence, or replay state is ambiguous.

## Documentation standards

Documentation changes should:

- lead with the reader's outcome;
- distinguish implemented, experimental, planned, and released behavior;
- keep one concept per section and use descriptive link text;
- show copyable commands with placeholders clearly marked;
- state security boundaries beside the feature they qualify;
- link to normative contracts instead of duplicating field-level requirements;
- avoid claims that are not backed by source, tests, or an edition decision;
- update navigation when adding a durable guide; and
- pass `git diff --check`.

Historical edition decisions and handoffs are durable evidence. Correct current
status in the active summary documents; do not rewrite history to make it read
more cleanly.

## AXP Edition work

An edition task is not ordinary free-form development. Before changing it,
read the edition charter, `assignments.tsv`, exact task packet, and
[`contracts/axp-experience.md`](../contracts/axp-experience.md).

Useful commands:

```bash
scripts/swarm.sh status AXP-E0002
scripts/swarm.sh validate AXP-E0002
scripts/swarm.sh packet AXP-E0002 E0002-14
```

Only a dated owner decision dispatches a pending task. A passing worker report
is not acceptance; the task's declared evidence and independent review gates
must pass.

## Before handing off

1. Review the scoped diff and remove unrelated changes.
2. Run formatting and the changed package's tests.
3. Run the declared reverse-impact set when required and when peer writers are
   quiescent.
4. Run `git diff --check`.
5. Record changed files, commands, results, assumptions, risks, and cross-owner
   requests.
6. Do not claim release status without the edition's Gate C decision.
