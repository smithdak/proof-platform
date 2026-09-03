# AXP Editions

An AXP Edition is a versioned, shippable user outcome. Editions turn product
intent into bounded tasks, explicit authority, reproducible evidence, and a
product-owner release decision.

The normative operating rules live in
[`contracts/axp-experience.md`](../contracts/axp-experience.md). The ranked
outcome queue lives in [`BACKLOG.md`](BACKLOG.md).

## Key terms

| Term | Meaning |
|---|---|
| Edition | One independently valuable product outcome |
| Wave | A dependency-safe batch of disjoint tasks |
| Task | One owner, bounded paths, declared budget, and measurable acceptance |
| Handoff | Durable record of changes, checks, assumptions, risks, and requests |
| Gate | Owner decision required before direction, material change, or release |
| Quiescence | All relevant writers have stopped before shared verification |

## Status vocabulary

| Status | Meaning |
|---|---|
| `pending` | Tracked but not authorized to start |
| `active` | Explicitly dispatched and currently owned |
| `review` | Writer stopped; acceptance evidence is being evaluated |
| `blocked` | Work stopped at a recorded boundary requiring a new decision or dependency |
| `done` | Declared acceptance evidence passed |

`pending` is never implicit dispatch authority. Likewise, `done` means the task
is accepted; it does not mean the edition is released.

## The three owner gates

### Gate A — Direction

The product owner approves the charter's outcome, metric, scope, non-goals,
budget, and risk posture before implementation fan-out.

### Gate B — Material change

The edition pauses for explicit approval before public contracts, migrations,
authority or security changes, secrets, destructive or external effects, paid
provider work, or material scope expansion.

### Gate C — Release

After quiescent integration and independent evaluation, the product owner
accepts release, requests a bounded repair wave, or rejects or defers the
edition. No task or agent self-approves Gate C.

## Edition anatomy

Each `AXP-E####/` directory contains:

| File | Purpose |
|---|---|
| `charter.md` | User outcome, journey, metric, scope, budget, and owner gates |
| `assignments.tsv` | Dispatch source of truth: wave, status, owner, model, paths, dependencies |
| `workgraph.md` | Dependency flow, wave gates, and integration order |
| `ownership.md` | One-writer mapping for crates and shared surfaces |
| `tasks/` | Exact worker packets and stop conditions |
| `handoffs/` | Durable worker and reviewer evidence |
| `decisions.md` | Append-only authority and exception history |
| `evidence.md` | Acceptance checks, commands, results, and limitations |
| `status.md` | Current state, blockers, and next action |
| `retro.md` | Outcome and operating lessons after closure |

Historical decisions and handoffs are evidence. Update current summaries when
state changes; do not rewrite prior failures into a cleaner story.

## Lifecycle

1. **Charter the outcome.** Define one user-visible result, its evaluation,
   scope, budget, non-goals, and rollback.
2. **Approve direction.** Gate A authorizes only the work stated in its decision.
3. **Freeze contracts and graph.** Assign paths, dependencies, model tiers,
   tests, and stop conditions.
4. **Dispatch one wave.** Activate only dependency-ready tasks explicitly named
   by the owner decision.
5. **Work within ownership.** Each writer changes only its assigned paths and
   unique handoff.
6. **Stop and hand off.** Record exact commands, results, custody, assumptions,
   and cross-owner requests.
7. **Quiesce and verify.** Stop peer writers before reverse-impact or shared
   integration checks.
8. **Accept or repair.** Mark a task done only after its declared evidence and
   review pass; otherwise preserve the stop and request bounded authority.
9. **Integrate and evaluate.** The integration owner runs the complete declared
   journey without skipped checks.
10. **Decide release.** Gate C belongs to the product owner.
11. **Close and recur.** Record the retrospective, update the backlog, and begin
    read-only discovery for the next edition.

Only one edition may have active writers. Read-only discovery for the next
edition may overlap current-edition integration.

## One-writer rule

Every crate, contract, migration sequence, manifest, lockfile, generated
artifact, and release surface has one named owner per wave. A worker never fixes
another owner's source to make its own task compile. Cross-scope needs become a
request to the orchestrator, which resolves them in dependency order.

Shared manifests and lockfiles require explicit serialization. Reverse-impact
checks wait for source quiescence so a package never compiles a peer's partial
edit.

## Dispatch loop

The orchestrator repeats this loop for each wave:

1. Confirm the current owner decision permits the exact task.
2. Confirm every dependency is `done` and every writable path is disjoint.
3. Select the lowest eligible model from [`MODEL_POLICY.md`](MODEL_POLICY.md).
4. Validate the edition and emit the worker packet.
5. Review the unique handoff and reproduce required evidence.
6. Obtain independent review where the task requires it.
7. Mark the task `done`, or preserve a precise `blocked` state and return the
   smallest safe continuation to the product owner.

## Commands

```bash
rtk scripts/swarm.sh new 2
rtk scripts/swarm.sh validate AXP-E0002
rtk scripts/swarm.sh validate-assignments editions/AXP-E0002/assignments.tsv
rtk scripts/swarm.sh status AXP-E0002
rtk scripts/swarm.sh packet AXP-E0002 E0002-14
rtk scripts/swarm.sh scoped proof-agent-runtime --list
rtk scripts/swarm.sh verify AXP-E0002 --quiescent
```

`verify --quiescent` is the orchestrator-only final gate. Workers run scoped
checks from their task packets and use `scripts/test-scoped.sh` for required
reverse dependents.

## Acceptance standard

“The agent finished” is not evidence. A task closes only when its declared
files, commands, results, invariants, custody, and review requirements are
present and reproducible. An edition releases only when its complete journey
passes and the product owner records Gate C.
