# AXP Experience Contract

**Status:** active planning contract
**Owner:** project owner
**Last updated:** 2026-08-29

This file is the canonical product contract for Proof as an Agent Experience
Platform (AXP). It defines the outcomes that editions build toward and the
approval boundaries for agent-led delivery. Product claims, edition charters,
and implementation waves must remain consistent with this contract.

## Product definition

Proof is the governed runtime and control plane where autonomous software:

1. discovers versioned operations through structured contracts;
2. acts with explicit, bounded authority and budgets;
3. pauses for signed human decisions when policy requires them;
4. resumes without losing durable execution lineage; and
5. returns independently verifiable evidence for consequential transitions.

Content, commerce, workflow, and analytics are domain adapters that prove the
kernel's generality. They are not the product boundary.

## Product invariants

Every AXP Edition MUST preserve these invariants:

- **Evidence:** successful governed transitions produce signed, independently
  verifiable proof. A transport response is never the only record of progress.
- **Authority:** operations are checked against current identity, delegation,
  governance, and approval policy immediately before execution.
- **Composability:** operations are discovered from versioned registry data and
  use stable structured input and output contracts.
- **Durability:** runs, attempts, approvals, checkpoints, retries, failures,
  budgets, and evaluations survive process boundaries.
- **Human control:** an operator can understand the proposed consequence and
  exact arguments before approving, denying, revoking, or stopping work.
- **Task correctness:** model completion alone is not success. The edition's
  declared deterministic evaluation and any required live gate must pass.

## Primary experiences

### Agent experience

Agents need:

- stable operation discovery and schemas;
- explicit tool, authority, time, token, step, output, and cost limits;
- durable continuation across tool calls, approvals, failures, and restarts;
- machine-verifiable results and failure evidence; and
- portable identities and execution evidence across transports and workspaces.

### Operator experience

Operators need:

- a clear view of goals, active runs, pending consequences, arguments, budgets,
  authority, and evidence;
- signed approve and deny decisions with revocation and separation of duties;
- safe pause, cancel, retry, and recovery controls;
- deterministic audit and evaluation of one run or an aggregate task tree; and
- explicit release gates for external effects and production changes.

Product work MUST deepen these experiences before introducing another domain,
unless the project owner records an exception in an edition decision log.

## Runtime boundary

The current native runtime is a durable leaf executor. It owns one agent run,
sequential model decisions, governed tool dispatch, approval suspension,
recovery, budgets, and evaluation.

Development swarms are coordinated by the repository orchestrator until an AXP
Edition adds and validates native team primitives. Edition charters MUST NOT
assume that the current runtime already supplies parent/child runs, a work DAG,
worker leases, isolated sandboxes, aggregate budgets, or a scheduler.

## AXP Product Editions

Use `AXP-E####` for product-delivery editions. This name is deliberately
distinct from the content-domain `Edition` snapshot.

An AXP Product Edition is a versioned, evidence-backed, shippable user outcome
whose scope and acceptance evaluation are fixed before implementation fan-out.
It contains one or more conflict-safe waves. It is not a sprint, a crate batch,
or an open-ended feature list.

### Edition states

`proposed -> approved -> contracted -> building -> integrating -> preview -> released`

An edition may instead become `blocked`, `rejected`, or `abandoned`. These are
recorded outcomes; edition history is never silently rewritten.

Only one edition may have active writers. Read-only discovery for the next
edition may overlap with integration of the current edition.

### Owner gates

- **Gate A — direction:** the project owner approves the charter, user journey,
  outcome metric, budget, acceptance policy, and non-goals before fan-out.
- **Gate B — material risk:** the owner decides public-contract changes,
  migrations, authority or security changes, secret handling, destructive or
  external effects, paid provider gates, and material scope expansion.
- **Gate C — release:** the owner accepts or rejects the candidate after seeing
  the outcome, evaluation evidence, known limitations, cost, and rollback plan.

All reversible implementation decisions inside a frozen charter are delegated
to the orchestrator and assigned workers.

## Edition acceptance contract

An edition cannot close as `released` until it has:

1. an owner-approved charter and frozen work graph;
2. disjoint, recorded ownership for every writable path;
3. scoped formatting and tests for every changed crate;
4. impact tests for transitive workspace dependents and non-Cargo surfaces;
5. contract, migration, transport, security, and recovery checks applicable to
   its changes;
6. a deterministic end-to-end evaluation bound to its policy and evidence;
7. a live-provider or external-effect gate when the charter requires one;
8. an evidence packet, known limitations, and rollback notes; and
9. explicit Gate C approval.

The full workspace verification runs only after all edition writers quiesce and
the orchestrator has completed integration.

## Team and ownership contract

- The orchestrator owns the edition work graph, root integration, and final
  release candidate.
- Every writable crate or shared path has exactly one owner per wave.
- Workers stop and report cross-owner needs instead of editing another scope.
- Workers never commit or push. The orchestrator commits after verification.
- Shared contracts, root manifests, lockfiles, migration numbering, and large
  hotspot modules have one named steward.
- Independent verification is performed by an agent that did not author the
  behavior under evaluation whenever capacity permits.

## Model routing contract

Model selection is based on task risk and measured evaluation performance:

- bounded discovery, documentation, schema, fixture, and mechanical work starts
  on the efficient worker tier;
- normal scoped implementation starts on the balanced tier;
- public contracts, security, migrations, architecture, integration, and
  release decisions use the flagship tier;
- failed or ambiguous work escalates by task, not by upgrading the entire
  edition; and
- a cheaper tier remains eligible only while its handoffs pass the same tests
  and evaluations as a more capable tier.

The repository-specific routing table lives in `editions/MODEL_POLICY.md`.

## Decision log

| # | Date | Decision | Rationale |
|---|---|---|---|
| 1 | 2026-08-29 | Proof's product boundary is AXP rather than CMS/DXP | The kernel, runtime, authority, approval, and evidence system spans all domain adapters. |
| 2 | 2026-08-29 | Agent and operator experience depth precedes new domain breadth | The four current domains already prove generality; usability, live operation, oversight, and production readiness are the limiting product gaps. |
| 3 | 2026-08-29 | Product delivery uses tracked AXP Editions containing conflict-safe waves | Outcome contracts and evidence must persist beyond chat and individual agent sessions. |
| 4 | 2026-08-29 | The current runtime is treated as a leaf executor, not a native swarm scheduler | Parent/child lineage, leases, isolation, aggregate budgets, and scheduling are not yet implemented. |
| 5 | 2026-08-29 | Human oversight is concentrated at direction, material-risk, and release gates | This preserves owner control without turning routine integration into a human bottleneck. |
