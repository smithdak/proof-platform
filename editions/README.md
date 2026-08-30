# AXP Editions

An **Edition** is a versioned product outcome. It is delivered through bounded
**Waves** of parallel work, each split into owned **Tasks**. The orchestrator
keeps the workgraph, resolves cross-owner requests, integrates in dependency
order, and reports to the product owner. The canonical product rules live in
[`contracts/axp-experience.md`](../contracts/axp-experience.md); the ranked
outcomes live in [`BACKLOG.md`](BACKLOG.md).

## Lifecycle

1. **Charter:** define the user outcome, success evaluation, budget, risks, and non-goals.
2. **Gate A — Direction:** product owner approves the charter before fan-out.
3. **Contract and workgraph:** freeze interfaces, acceptance checks, dependencies, and path ownership.
4. **Waves:** assign disjoint tasks to cheap agents; each task has one writer, bounded context, and scoped checks.
5. **Gate B — Material change:** pause for owner approval when scope, public contracts, security, migrations, external effects, or budget materially change.
6. **Quiesce and integrate:** stop writers, merge dependency order, and have the integration owner resolve shared surfaces.
7. **Verify:** independent verification runs the declared evaluation and records reproducible evidence.
8. **Gate C — Release:** product owner accepts, requests a bounded repair wave, or rejects the edition.
9. **Close and recur:** record the retrospective, update the backlog, and start read-only discovery for the next edition.

Two editions may not have concurrent writers. Read-only discovery for the next edition is allowed while the current edition is integrating.

## One-writer rule

Every crate, shared file, contract, migration sequence, generated artifact, and release surface has one named owner per Wave. A task never edits another owner’s scope. Cross-scope needs become an orchestrator request. Root manifests and lockfiles are integration-owned.

## Task and handoff

Copy the template and provide objective, owned paths, dependencies, contract
references, acceptance checks, non-goals, commands, budget, and stop
conditions. Record each dispatch in `assignments.tsv`. A handoff must list
changed files, checks run and results, assumptions, risks, and unresolved
requests. "Agent finished" is not acceptance; the declared evaluation is.

## Dispatch loop

For each wave, the orchestrator repeats this loop:

1. Confirm the edition state and any Gate A/B decision permit the task. A
   `pending` row is backlog state, not authorization.
2. Confirm every dependency is `done`, select the lowest eligible model from
   `MODEL_POLICY.md`, and replace `unassigned` with one worker identity.
3. Validate the edition, then emit the worker context with
   `rtk scripts/swarm.sh packet <AXP-E####> <E####-##>`.
4. Review the worker's unique handoff and reproduce its declared acceptance
   checks. Mark the task `done` only when the evidence passes.
5. Quiesce at wave boundaries, resolve cross-owner requests through the
   orchestrator, and unlock the next dependency-safe wave. At Gate C, close
   the retro and promote the next ranked edition back through Gate A.

Workers receive the emitted packet plus `AGENTS.md`; chat history is optional
context, never a hidden requirement.

## Commands

```bash
rtk scripts/swarm.sh new 1
rtk scripts/swarm.sh validate AXP-E0001
rtk scripts/swarm.sh validate-assignments editions/AXP-E0001/assignments.tsv
rtk scripts/swarm.sh status AXP-E0001
rtk scripts/swarm.sh packet AXP-E0001 E0001-01
rtk scripts/swarm.sh scoped proof-agent-runtime --list
rtk scripts/swarm.sh verify AXP-E0001 --quiescent
```

`verify` is the orchestrator-only final gate and intentionally requires the
explicit `--quiescent` acknowledgement before it runs the workspace suite.
Workers use `scripts/test-scoped.sh` for their assigned package.
