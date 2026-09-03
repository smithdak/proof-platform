# Proof Platform documentation

This documentation is organized around reader intent: learn the system, run it
locally, integrate a transport, extend the platform, or inspect delivery
evidence.

## Find your path

| If you want to… | Start with | Then read |
|---|---|---|
| Try Proof locally | [Getting started](getting-started.md) | [CLI reference](cli-reference.md) |
| Understand the model | [Core concepts](concepts.md) | [Architecture](../ARCHITECTURE.md) |
| Connect an MCP client | [MCP server guide](../crates/proof-transport-mcp/README.md) | [Security model](security-model.md) |
| Use the development HTTP API | [HTTP API](http-api.md) | [Security model](security-model.md) |
| Add a domain or operation | [Architecture](../ARCHITECTURE.md#extending-proof) | [Domain contracts](../contracts/domain-definitions.md) |
| Work on the repository | [Development guide](development.md) | [Agent conventions](../AGENTS.md) |
| Follow product delivery | [AXP Editions](../editions/README.md) | [Edition backlog](../editions/BACKLOG.md) |

## Guides and references

### Learn

- [Core concepts](concepts.md) explains operations, authority, evidence,
  approvals, durable runs, and replay.
- [Architecture](../ARCHITECTURE.md) describes component ownership, execution
  flow, persistence, trust boundaries, and extension points.
- [Security model](security-model.md) separates implemented protections from
  development-only and not-yet-released surfaces.

### Use

- [Getting started](getting-started.md) takes a fresh installation through its
  first signed content operation.
- [CLI reference](cli-reference.md) maps command families to common workflows.
- [MCP server guide](../crates/proof-transport-mcp/README.md) covers stdio client
  setup, run metadata, and signed human approval.
- [HTTP API](http-api.md) documents the generic development transport and its
  current safety boundary.

### Build and operate

- [Development guide](development.md) covers scoped builds, tests, formatting,
  repository structure, and documentation standards.
- [AXP Editions](../editions/README.md) explains the gated delivery model used
  for product work.
- [Dogfood evidence](dogfood/) contains durable records from approved
  evaluation journeys. Treat these as evidence, not tutorials.

### Contracts and release history

- [Kernel API](../contracts/kernel-api.md) defines shared runtime types and
  invariants.
- [Domain definitions](../contracts/domain-definitions.md) define the canonical
  operation sets and domain decisions.
- [AXP experience](../contracts/axp-experience.md) defines the product-delivery
  contract.
- [Changelog](../CHANGELOG.md) records user-visible changes by release.

## Sources of truth

Documentation explains the system; it does not redefine contracts. When two
descriptions differ, use this precedence:

1. Public and security contracts under [`contracts/`](../contracts/).
2. Shared Rust types and behavior covered by crate tests.
3. Registry manifests and schemas under [`registry/`](../registry/) and
   [`schemas/`](../schemas/).
4. CLI `--help` output and transport route definitions.
5. Explanatory guides in this directory.

Product status is similarly explicit: an edition task marked `done` is accepted
work, while a released product requires its edition's Gate C decision. Check
the relevant [edition status and decision records](../editions/) before relying
on an experimental surface.

## Documentation conventions

- Commands use placeholders such as `<RUN_ID>` for values you must replace.
- Examples assume the repository root unless a different directory is shown.
- Paths beginning with `.proof/` are relative to the selected Proof workspace.
- Security warnings describe the current implementation, not future intent.
- Examples avoid the repository-root `.proof` directory when durable product
  evidence requires a fresh, disposable workspace.
